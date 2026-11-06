use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::Engine as _;
use clap::ValueEnum;
use serde::Deserialize;

/// A generated image: raw bytes plus the file extension matching their format.
#[derive(Debug)]
pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// An image generation backend. Add new providers by implementing this trait
/// and selecting them in [`from_kind`].
#[async_trait]
pub trait ImageProvider: Send + Sync {
    /// Human-readable provider name, used in logs.
    fn name(&self) -> &'static str;

    /// Generate a single 16:9 image for `prompt`. The returned extension
    /// reflects the format the backend actually sent (no transcoding).
    async fn generate(&self, prompt: &str) -> Result<GeneratedImage>;
}

/// Selectable image generation backends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum ProviderKind {
    /// Google Gemini 3 Pro Image (nano banana pro).
    #[default]
    #[value(alias = "nano-banana")]
    NanoBananaPro,
    /// OpenAI GPT Image 2.
    #[value(name = "gpt-image-2")]
    GptImage2,
}

pub fn from_kind(kind: ProviderKind) -> Result<Arc<dyn ImageProvider>> {
    match kind {
        ProviderKind::NanoBananaPro => Ok(Arc::new(NanoBanana::from_env()?)),
        ProviderKind::GptImage2 => Ok(Arc::new(GptImage2::from_env()?)),
    }
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?)
}

fn api_base(default: &str) -> String {
    std::env::var("NAILGUN_API_BASE").unwrap_or_else(|_| default.into())
}

fn api_error(status: reqwest::StatusCode, text: String) -> anyhow::Error {
    let message = serde_json::from_str::<serde_json::Value>(&text)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(String::from))
        .unwrap_or(text);
    anyhow::anyhow!("API error {status}: {message}")
}

/// Google's nano banana pro (Gemini 3 Pro Image) via the Gemini API.
pub struct NanoBanana {
    client: reqwest::Client,
    api_key: String,
}

impl NanoBanana {
    const MODEL: &'static str = "gemini-3-pro-image";
    const API_BASE: &'static str = "https://generativelanguage.googleapis.com";

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("GEMINI_API_KEY")
            .or_else(|_| std::env::var("GOOGLE_API_KEY"))
            .context("set GEMINI_API_KEY to use the nano banana pro provider")?;
        Ok(Self {
            client: http_client()?,
            api_key,
        })
    }

    fn url() -> String {
        // Overridable to point at a mock server in tests.
        format!(
            "{}/v1beta/models/{}:generateContent",
            api_base(Self::API_BASE),
            Self::MODEL
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GenerateContentResponse {
    #[serde(default)]
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct CandidateContent {
    #[serde(default)]
    parts: Vec<Part>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Part {
    text: Option<String>,
    inline_data: Option<InlineData>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InlineData {
    mime_type: String,
    data: String,
}

fn extension_for(mime_type: &str) -> Result<&'static str> {
    match mime_type {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        other => bail!("unsupported image format from API: {other}"),
    }
}

#[async_trait]
impl ImageProvider for NanoBanana {
    fn name(&self) -> &'static str {
        "nano-banana-pro"
    }

    async fn generate(&self, prompt: &str) -> Result<GeneratedImage> {
        let body = serde_json::json!({
            "contents": [{"parts": [{"text": prompt}]}],
            "generationConfig": {
                "responseModalities": ["TEXT", "IMAGE"],
                "imageConfig": {"aspectRatio": "16:9"}
            }
        });
        let response = self
            .client
            .post(Self::url())
            .header("x-goog-api-key", &self.api_key)
            .json(&body)
            .send()
            .await
            .context("sending generate request")?;
        let status = response.status();
        let text = response.text().await.context("reading response body")?;
        if !status.is_success() {
            return Err(api_error(status, text));
        }

        let parsed: GenerateContentResponse =
            serde_json::from_str(&text).context("parsing generate response")?;
        let mut model_text = None;
        for candidate in parsed.candidates {
            for part in candidate.content.map(|c| c.parts).unwrap_or_default() {
                if let Some(inline) = part.inline_data {
                    let bytes = base64::engine::general_purpose::STANDARD.decode(inline.data)?;
                    return Ok(GeneratedImage {
                        bytes,
                        extension: extension_for(&inline.mime_type)?,
                    });
                }
                if let Some(t) = part.text {
                    model_text = Some(t);
                }
            }
        }
        match model_text {
            Some(t) => bail!("no image returned; model said: {t}"),
            None => bail!("no image returned"),
        }
    }
}

/// OpenAI GPT Image 2 via the Images API.
pub struct GptImage2 {
    client: reqwest::Client,
    api_key: String,
}

impl GptImage2 {
    const MODEL: &'static str = "gpt-image-2";
    const API_BASE: &'static str = "https://api.openai.com";
    /// 16:9 2K landscape — YouTube thumbnail aspect, listed as a popular GPT Image 2 size.
    const SIZE: &'static str = "2048x1152";

    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY")
            .context("set OPENAI_API_KEY to use the GPT Image 2 provider")?;
        Ok(Self {
            client: http_client()?,
            api_key,
        })
    }

    fn url() -> String {
        format!("{}/v1/images/generations", api_base(Self::API_BASE))
    }
}

#[derive(Deserialize)]
struct ImagesResponse {
    #[serde(default)]
    data: Vec<ImageData>,
    output_format: Option<String>,
}

#[derive(Deserialize)]
struct ImageData {
    b64_json: Option<String>,
}

fn parse_gpt_image_response(text: &str) -> Result<GeneratedImage> {
    let parsed: ImagesResponse = serde_json::from_str(text).context("parsing generate response")?;
    let image = parsed
        .data
        .into_iter()
        .next()
        .context("no image returned")?;
    let Some(b64) = image.b64_json else {
        bail!("no image data returned");
    };
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
    let extension = match parsed.output_format.as_deref() {
        Some("png") => "png",
        Some("jpeg") => "jpg",
        Some("webp") => "webp",
        Some(other) => bail!("unsupported image format from API: {other}"),
        None => extension_for_bytes(&bytes)?,
    };
    Ok(GeneratedImage { bytes, extension })
}

fn extension_for_bytes(bytes: &[u8]) -> Result<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("png")
    } else if bytes.len() >= 3 && bytes[0] == 0xff && bytes[1] == 0xd8 && bytes[2] == 0xff {
        Ok("jpg")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok("webp")
    } else {
        bail!("unrecognized image bytes from API")
    }
}

#[async_trait]
impl ImageProvider for GptImage2 {
    fn name(&self) -> &'static str {
        "gpt-image-2"
    }

    async fn generate(&self, prompt: &str) -> Result<GeneratedImage> {
        let body = serde_json::json!({
            "model": Self::MODEL,
            "prompt": prompt,
            "size": Self::SIZE,
            "output_format": "png",
        });
        let response = self
            .client
            .post(Self::url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("sending generate request")?;
        let status = response.status();
        let text = response.text().await.context("reading response body")?;
        if !status.is_success() {
            return Err(api_error(status, text));
        }
        parse_gpt_image_response(&text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";

    #[test]
    fn extension_mapping() {
        assert_eq!(extension_for("image/png").unwrap(), "png");
        assert_eq!(extension_for("image/jpeg").unwrap(), "jpg");
        assert_eq!(extension_for("image/webp").unwrap(), "webp");
        assert!(extension_for("image/tiff").is_err());
    }

    #[test]
    fn gpt_image_parses_png_with_output_format() {
        let body =
            format!(r#"{{"created":0,"output_format":"png","data":[{{"b64_json":"{PNG_B64}"}}]}}"#);
        let image = parse_gpt_image_response(&body).unwrap();
        assert_eq!(image.extension, "png");
        assert!(image.bytes.starts_with(b"\x89PNG"));
    }

    #[test]
    fn gpt_image_maps_jpeg_output_format() {
        let body = r#"{"created":0,"output_format":"jpeg","data":[{"b64_json":"QQ=="}]}"#;
        let image = parse_gpt_image_response(body).unwrap();
        assert_eq!(image.extension, "jpg");
    }

    #[test]
    fn gpt_image_sniffs_png_when_output_format_missing() {
        let body = format!(r#"{{"created":0,"data":[{{"b64_json":"{PNG_B64}"}}]}}"#);
        let image = parse_gpt_image_response(&body).unwrap();
        assert_eq!(image.extension, "png");
    }

    #[test]
    fn gpt_image_errors_when_data_empty() {
        let err = parse_gpt_image_response(r#"{"created":0,"data":[]}"#).unwrap_err();
        assert!(err.to_string().contains("no image returned"));
    }
}
