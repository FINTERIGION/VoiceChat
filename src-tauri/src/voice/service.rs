use serde::Deserialize;
use serde_json::json;

pub const PRESET_VOICES: &[&str] = &[
    "longanqian",
    "longanlingxin",
    "longanlingxi",
    "longanxiaoxin",
    "longanlufeng",
];

pub const REALTIME_TARGET_MODEL: &str = "qwen-audio-3.0-realtime-flash";
// qwen-audio-3.0-tts-flash (used for REALTIME_TARGET_MODEL's TTS-series
// sibling) silently fails Voice Design: the API accepts the request but
// comes back with an empty `preview_audio` and a realtime-flash voice_id
// instead of a TTS preview. cosyvoice-v3.5-plus is what Alibaba's own Voice
// Design examples use and actually returns preview_audio.data.
pub const DESIGN_TARGET_MODEL: &str = "cosyvoice-v3.5-plus";

#[derive(Deserialize)]
struct EnrollmentResponse {
    output: EnrollmentOutput,
}

#[derive(Deserialize, Default)]
struct EnrollmentOutput {
    #[serde(default)]
    voice_id: Option<String>,
    #[serde(default)]
    voice: Option<String>,
    #[serde(default)]
    preview_audio: Option<PreviewAudio>,
    #[serde(default)]
    voice_list: Option<Vec<VoiceListEntry>>,
}

#[derive(Deserialize, Default)]
struct PreviewAudio {
    #[serde(default)]
    data: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct VoiceListEntry {
    pub voice_id: String,
    /// `OK` (usable), `DEPLOYING` (still being reviewed) or `UNDEPLOYED`
    /// (rejected). Only `OK` voices can actually be synthesized with.
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub gmt_create: Option<String>,
    /// Only the Qwen series reports this back from `list_voice`.
    #[serde(default)]
    pub target_model: Option<String>,
}

impl VoiceListEntry {
    /// Whether this voice can drive the live session. An account accumulates
    /// TTS-series voices too — the Voice Design flow enrolls one on
    /// `DESIGN_TARGET_MODEL` before cloning its preview for realtime use —
    /// and handing one of those to the realtime model fails at connect time,
    /// so they are told apart here instead of at the point of failure.
    ///
    /// When the API doesn't report `target_model` the id still carries it:
    /// `create_voice` builds ids as `{target_model}-{prefix}-{hash}`.
    pub fn is_realtime(&self) -> bool {
        match self.target_model.as_deref().filter(|m| !m.is_empty()) {
            Some(model) => model == REALTIME_TARGET_MODEL,
            None => self.voice_id.starts_with(REALTIME_TARGET_MODEL),
        }
    }
}

pub struct DesignedVoicePreview {
    /// The TTS-series voice the design enrolled. Of no use once the preview
    /// audio is in hand, so callers delete it (see
    /// `app::commands::discard_design_voice`).
    pub tts_voice: Option<String>,
    /// Base64 WAV audio — playable directly and usable as the clone source.
    pub preview_audio_b64: String,
}

pub struct VoiceService {
    api_key: String,
    base_url: String,
}

impl VoiceService {
    pub fn new(api_key: String, workspace_id: Option<String>, region: Option<&str>) -> Self {
        let base_url = format!(
            "https://{}",
            crate::dashscope::host(workspace_id.as_deref(), region)
        );
        Self { api_key, base_url }
    }

    fn endpoint(&self) -> String {
        format!("{}/api/v1/services/audio/tts/customization", self.base_url)
    }

    async fn call(&self, body: serde_json::Value) -> Result<EnrollmentOutput, String> {
        let client = reqwest::Client::new();
        let resp = client
            .post(self.endpoint())
            .bearer_auth(&self.api_key)
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
        let parsed: EnrollmentResponse = serde_json::from_str(&text).map_err(|e| {
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
        Ok(parsed.output)
    }

    /// Clones a voice from a source audio (`url` may be a `data:` URI or a
    /// public URL — whether the API accepts data URIs is exactly the Phase 2
    /// spike; both are attempted the same way here, so if data URIs don't
    /// work, the caller sees the API's own error and can fall back to
    /// prompting the user for a public URL instead).
    pub async fn clone_voice(
        &self,
        target_model: &str,
        prefix: &str,
        url: &str,
    ) -> Result<String, String> {
        let output = self
            .call(json!({
                "model": "voice-enrollment",
                "input": {
                    "action": "create_voice",
                    "target_model": target_model,
                    "prefix": prefix,
                    "url": url,
                }
            }))
            .await?;
        output.voice_id.ok_or_else(|| {
            crate::tr!("The response had no voice_id", "响应中缺少 voice_id").to_string()
        })
    }

    /// Step 1 of the text-design bridge: describe a voice in words and get a
    /// TTS-series preview back (realtime series doesn't support this
    /// directly, see prompt::builder module docs for why).
    pub async fn design_voice(
        &self,
        voice_prompt: &str,
        preview_text: &str,
        prefix: &str,
    ) -> Result<DesignedVoicePreview, String> {
        let output = self
            .call(json!({
                "model": "voice-enrollment",
                "input": {
                    "action": "create_voice",
                    "target_model": DESIGN_TARGET_MODEL,
                    "voice_prompt": voice_prompt,
                    "preview_text": preview_text,
                    "prefix": prefix,
                },
                "parameters": {
                    "sample_rate": 24000,
                    "response_format": "wav"
                }
            }))
            .await?;
        let preview_audio_b64 = output
            .preview_audio
            .and_then(|p| p.data)
            .filter(|d| !d.is_empty())
            .ok_or_else(|| {
                crate::tr!("The response had no preview audio", "响应中缺少试听音频",).to_string()
            })?;
        Ok(DesignedVoicePreview {
            tts_voice: output.voice,
            preview_audio_b64,
        })
    }

    /// Lists custom voices (cloned or designed) created under this account,
    /// across every target model — see `VoiceListEntry::is_realtime` for
    /// which of them the live session can actually use. Doesn't include the
    /// built-in preset voices — those aren't account-scoped resources, just
    /// fixed names the realtime API accepts.
    ///
    /// Reads page after page until one comes back short: an account can pass
    /// a single page's worth sooner than its owner might think, since Voice
    /// Design left a TTS-series voice behind for every preview before the
    /// app started deleting them.
    pub async fn list_voices(&self) -> Result<Vec<VoiceListEntry>, String> {
        const PAGE_SIZE: usize = 100;
        /// Far past any real account. Only there so an API that ignored
        /// `page_index` and kept sending the same full page couldn't keep
        /// this asking forever.
        const MAX_PAGES: usize = 50;

        let mut voices: Vec<VoiceListEntry> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for page_index in 0..MAX_PAGES {
            let page = self
                .call(json!({
                    "model": "voice-enrollment",
                    "input": {
                        "action": "list_voice",
                        "page_index": page_index,
                        "page_size": PAGE_SIZE,
                    }
                }))
                .await?
                .voice_list
                .unwrap_or_default();
            let full = page.len() >= PAGE_SIZE;
            let before = voices.len();
            // A voice created while paging shifts the rest along by one, so
            // the next page can repeat the last entry of this one.
            voices.extend(page.into_iter().filter(|v| seen.insert(v.voice_id.clone())));
            if !full || voices.len() == before {
                break;
            }
        }
        Ok(voices)
    }

    pub async fn delete_voice(&self, voice_id: &str) -> Result<(), String> {
        self.call(json!({
            "model": "voice-enrollment",
            "input": {
                "action": "delete_voice",
                "voice_id": voice_id,
            }
        }))
        .await?;
        Ok(())
    }
}
