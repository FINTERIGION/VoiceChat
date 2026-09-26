use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

const MODEL: &str = "qwen3.8-flash";
/// Ceiling on a single request's round trip. `complete` is called on the
/// hot path of a live conversation (subtitle translation, barge-in-adjacent
/// memory injection) — a call that hangs with no timeout at all would just
/// sit forever with nothing to show for it, worse than one that fails
/// promptly and lets the caller degrade (the subtitle simply stays
/// single-language; see `subtitle::spawn_translation`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// Ceiling for `complete_long`. Summarizing an 8-message conversation took
/// 7.6-9.3s with thinking off — and 16-20s with it on, which is how every
/// summary used to die at `REQUEST_TIMEOUT`. Double the worst measured run
/// leaves room for a long transcript, while still bounding the stall: the
/// session actor awaits the summary before a rolling reconnect, and before
/// it takes the next command after a stop.
const LONG_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One `reqwest::Client` for every call this process ever makes here,
/// rather than a fresh one per request. A `Client` owns a connection pool
/// keyed by host; building a new one each time throws that pool away, so
/// every single call — including back-to-back translations of consecutive
/// subtitle lines, all going to the same host — pays a full TCP+TLS
/// handshake it didn't need to. Cloning a `Client` is cheap (it's an `Arc`
/// internally), so every call site gets its own handle to the same pool.
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> reqwest::Client {
    HTTP_CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_default()
        })
        .clone()
}

pub struct FlashClient {
    api_key: String,
    base_url: String,
}

impl FlashClient {
    pub fn new(api_key: String, workspace_id: Option<String>, region: Option<&str>) -> Self {
        let base_url = format!(
            "https://{}/compatible-mode/v1",
            crate::dashscope::host(workspace_id.as_deref(), region)
        );
        Self { api_key, base_url }
    }

    pub async fn test_connectivity(&self) -> Result<(), String> {
        let client = http_client();
        let resp = client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": MODEL,
                "messages": [{"role": "user", "content": "ping"}],
                "max_tokens": 1
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let body = crate::dashscope::snippet(&resp.text().await.unwrap_or_default());
            Err(format!("HTTP {status}: {body}"))
        }
    }

    /// One-shot chat completion; returns the first choice's message content.
    pub async fn complete(&self, system: &str, user: &str) -> Result<String, String> {
        self.chat(system, user, None, REQUEST_TIMEOUT).await
    }

    /// `complete` with the model's thinking step explicitly switched off, for
    /// calls someone is watching the clock on (subtitle translation): a
    /// reasoning pass costs seconds before the first output token and buys
    /// nothing for a one-line translation.
    pub async fn complete_fast(&self, system: &str, user: &str) -> Result<String, String> {
        self.chat(system, user, Some(false), REQUEST_TIMEOUT).await
    }

    /// `complete` for a call that reads a whole transcript and writes a
    /// structured answer (memory summarization): thinking off, since it
    /// spent ~1000 tokens reasoning over a few lines of small talk without
    /// a better answer to show for it, and `LONG_REQUEST_TIMEOUT` rather
    /// than `REQUEST_TIMEOUT`, since a timeout here loses the whole
    /// conversation's worth of memory, not one subtitle line.
    pub async fn complete_long(&self, system: &str, user: &str) -> Result<String, String> {
        self.chat(system, user, Some(false), LONG_REQUEST_TIMEOUT)
            .await
    }

    /// `thinking: None` leaves the model on its own default. `timeout`
    /// overrides the shared client's for this request alone.
    async fn chat(
        &self,
        system: &str,
        user: &str,
        thinking: Option<bool>,
        timeout: Duration,
    ) -> Result<String, String> {
        let mut body = json!({
            "model": MODEL,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user}
            ]
        });
        if let Some(on) = thinking {
            body["enable_thinking"] = json!(on);
        }

        let client = http_client();
        let resp = client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!(
                "HTTP {status}: {}",
                crate::dashscope::snippet(&text)
            ));
        }

        #[derive(Deserialize)]
        struct ChatResponse {
            choices: Vec<Choice>,
        }
        #[derive(Deserialize)]
        struct Choice {
            message: Message,
        }
        #[derive(Deserialize)]
        struct Message {
            content: String,
        }

        let parsed: ChatResponse = serde_json::from_str(&text).map_err(|e| {
            crate::tr!(
                format!(
                    "Could not parse the response: {e}; raw: {}",
                    crate::dashscope::snippet(&text)
                ),
                format!(
                    "解析响应失败: {e}; 原始: {}",
                    crate::dashscope::snippet(&text)
                ),
            )
        })?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| {
                crate::tr!("The response contained no content", "响应中没有内容").to_string()
            })
    }
}
