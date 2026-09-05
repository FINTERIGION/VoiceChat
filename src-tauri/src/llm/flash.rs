use serde::Deserialize;
use serde_json::json;

const MODEL: &str = "qwen3.8-flash";

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
        let client = reqwest::Client::new();
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
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": MODEL,
                "messages": [
                    {"role": "system", "content": system},
                    {"role": "user", "content": user}
                ]
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| e.to_string())?;
        if !status.is_success() {
            return Err(format!("HTTP {status}: {}", crate::dashscope::snippet(&text)));
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

        let parsed: ChatResponse =
            serde_json::from_str(&text).map_err(|e| {
                crate::tr!(
                    format!("Could not parse the response: {e}; raw: {}", crate::dashscope::snippet(&text)),
                    format!("解析响应失败: {e}; 原始: {}", crate::dashscope::snippet(&text)),
                )
            })?;
        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| crate::tr!("The response contained no content", "响应中没有内容").to_string())
    }
}
