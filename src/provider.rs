use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::Engine as _;
use serde::Deserialize;

/// A generated image: raw bytes plus the file extension matching their format.
pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub extension: &'static str,
}

/// An image generation backend. Add new providers by implementing this trait
/// and selecting them in [`default_provider`].
#[async_trait]
pub trait ImageProvider: Send + Sync {
    /// Human-readable provider name, used in logs.
    fn name(&self) -> &'static str;

    /// Generate a single 16:9 image for `prompt`. The backend chooses the
    /// output format (the Gemini API does not support requesting one);
    /// the returned extension reflects what was actually sent.
    async fn generate(&self, prompt: &str) -> Result<GeneratedImage>;
}

pub fn default_provider() -> Result<Arc<dyn ImageProvider>> {
    Ok(Arc::new(NanoBanana::from_env()?))
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
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(300))
            .build()?;
        Ok(Self { client, api_key })
    }

    fn url() -> String {
        // Overridable to point at a mock server in tests.
        let base = std::env::var("NAILGUN_API_BASE").unwrap_or_else(|_| Self::API_BASE.into());
        format!("{base}/v1beta/models/{}:generateContent", Self::MODEL)
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
            let message = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or(text);
            bail!("API error {status}: {message}");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_mapping() {
        assert_eq!(extension_for("image/png").unwrap(), "png");
        assert_eq!(extension_for("image/jpeg").unwrap(), "jpg");
        assert_eq!(extension_for("image/webp").unwrap(), "webp");
        assert!(extension_for("image/tiff").is_err());
    }
}
