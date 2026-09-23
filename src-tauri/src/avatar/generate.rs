//! Draws an avatar with Qwen-Image through Model Studio's synchronous
//! multimodal-generation endpoint. See
//! https://help.aliyun.com/zh/model-studio/qwen-image-generation-and-editing-api-reference.
//!
//! The API answers with a link to the picture rather than the picture, and
//! the link expires after 24 hours — so it is downloaded straight away and
//! handed to the editor as bytes, where it is cropped and stored like any
//! other picture the user picked.

use std::time::Duration;

use serde::Deserialize;
use serde_json::json;

use super::Format;

const MODEL: &str = "qwen-image-3.0";

/// Square, and twice the size the editor crops to, which leaves room to zoom
/// in on the face without it going soft.
const SIZE: &str = "1024*1024";

/// What an avatar must not have: the model otherwise likes to letter a
/// caption or the character's name across the picture.
const NEGATIVE_PROMPT: &str =
    "文字, 字母, 水印, 签名, 边框, 多个人物, 拼贴, 模糊, 低清晰度, 畸形, 多余的手指";

/// A generation takes anywhere from ten seconds to over a minute, and the
/// synchronous endpoint holds the request open for all of it.
const GENERATE_TIMEOUT: Duration = Duration::from_secs(180);

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// A 1024² PNG comes to a few megabytes; this only guards against a response
/// that is something else entirely.
const MAX_DOWNLOAD_BYTES: usize = 20 * 1024 * 1024;

pub struct ImageClient {
    api_key: String,
    base_url: String,
}

#[derive(Deserialize)]
struct GenerationResponse {
    #[serde(default)]
    output: Option<GenerationOutput>,
}

#[derive(Deserialize)]
struct GenerationOutput {
    #[serde(default)]
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    #[serde(default)]
    content: Vec<ContentPart>,
}

#[derive(Deserialize)]
struct ContentPart {
    #[serde(default)]
    image: Option<String>,
}

impl ImageClient {
    pub fn new(api_key: String, workspace_id: Option<String>, region: Option<&str>) -> Self {
        let base_url = format!(
            "https://{}",
            crate::dashscope::host(workspace_id.as_deref(), region)
        );
        Self { api_key, base_url }
    }

    /// Generates one square picture for `prompt` and returns its bytes.
    pub async fn generate(&self, prompt: &str) -> Result<(Vec<u8>, Format), String> {
        let client = reqwest::Client::builder()
            .timeout(GENERATE_TIMEOUT)
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client
            .post(format!(
                "{}/api/v1/services/aigc/multimodal-generation/generation",
                self.base_url
            ))
            .bearer_auth(&self.api_key)
            .json(&json!({
                "model": MODEL,
                "input": {
                    "messages": [{
                        "role": "user",
                        "content": [{ "text": prompt }],
                    }],
                },
                "parameters": {
                    "size": SIZE,
                    "n": 1,
                    "negative_prompt": NEGATIVE_PROMPT,
                    // Lets the model flesh out a one-line description into
                    // a full scene; most people describe a character, not a
                    // composition.
                    "prompt_extend": true,
                    "watermark": false,
                },
            }))
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
        let url = image_url(&text).ok_or_else(|| {
            crate::tr!(
                format!(
                    "The response had no image in it: {}",
                    crate::dashscope::snippet(&text)
                ),
                format!("响应中没有图片：{}", crate::dashscope::snippet(&text)),
            )
        })?;
        download(&url).await
    }
}

fn image_url(body: &str) -> Option<String> {
    serde_json::from_str::<GenerationResponse>(body)
        .ok()?
        .output?
        .choices
        .into_iter()
        .flat_map(|c| c.message.content)
        .find_map(|part| part.image)
        .filter(|url| !url.is_empty())
}

/// Fetches the generated picture. The link is a pre-signed object-storage
/// URL, so it is fetched without the API key — which has no business going
/// anywhere but the Model Studio host.
async fn download(url: &str) -> Result<(Vec<u8>, Format), String> {
    if !url.starts_with("https://") {
        return Err(crate::tr!(
            format!("Refusing to download the image from a non-HTTPS link: {url}"),
            format!("图片链接不是 HTTPS，已拒绝下载：{url}"),
        ));
    }
    let client = reqwest::Client::builder()
        .timeout(DOWNLOAD_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(crate::tr!(
            format!("Couldn't download the generated image (HTTP {status})"),
            format!("下载生成的图片失败（HTTP {status}）"),
        ));
    }
    if resp
        .content_length()
        .is_some_and(|len| len > MAX_DOWNLOAD_BYTES as u64)
    {
        return Err(too_large());
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_DOWNLOAD_BYTES {
        return Err(too_large());
    }
    let format = Format::sniff(&bytes).ok_or_else(|| {
        crate::tr!(
            "The generated file isn't an image this app can read",
            "生成的文件不是可识别的图片",
        )
        .to_string()
    })?;
    Ok((bytes.to_vec(), format))
}

fn too_large() -> String {
    crate::tr!(
        "The generated image is unexpectedly large",
        "生成的图片大小异常",
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_image_in_a_response() {
        let body = r#"{
            "output": {"choices": [{"finish_reason": "stop", "message": {
                "role": "assistant",
                "content": [{"image": "https://dashscope-result.oss.aliyuncs.com/x.png?Expires=1"}]
            }}]},
            "usage": {"output_width": 1024, "output_height": 1024},
            "request_id": "r"
        }"#;
        assert_eq!(
            image_url(body).as_deref(),
            Some("https://dashscope-result.oss.aliyuncs.com/x.png?Expires=1")
        );
    }

    #[test]
    fn a_response_without_an_image_yields_none() {
        assert_eq!(image_url(r#"{"output": {"choices": []}}"#), None);
        assert_eq!(
            image_url(r#"{"output": {"choices": [{"message": {"content": [{"text": "no"}]}}]}}"#),
            None
        );
        assert_eq!(image_url(r#"{"code": "InvalidParameter"}"#), None);
        assert_eq!(image_url("not json"), None);
    }
}
