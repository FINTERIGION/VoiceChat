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
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub gmt_create: Option<String>,
}

pub struct DesignedVoicePreview {
    /// The TTS-series voice id, kept around in case the preview needs
    /// re-synthesizing to a longer clip before cloning it for realtime use.
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
            return Err(format!("HTTP {status}: {text}"));
        }
        let parsed: EnrollmentResponse =
            serde_json::from_str(&text).map_err(|e| format!("解析响应失败: {e}; 原始: {text}"))?;
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
        output.voice_id.ok_or_else(|| "响应中缺少 voice_id".to_string())
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
            .ok_or_else(|| "响应中缺少试听音频".to_string())?;
        Ok(DesignedVoicePreview {
            tts_voice: output.voice,
            preview_audio_b64,
        })
    }

    /// Lists custom voices (cloned or designed) created under this account.
    /// Doesn't include the built-in preset voices — those aren't
    /// account-scoped resources, just fixed names the realtime API accepts.
    pub async fn list_voices(&self) -> Result<Vec<VoiceListEntry>, String> {
        let output = self
            .call(json!({
                "model": "voice-enrollment",
                "input": {
                    "action": "list_voice",
                    "page_index": 0,
                    "page_size": 100,
                }
            }))
            .await?;
        Ok(output.voice_list.unwrap_or_default())
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
