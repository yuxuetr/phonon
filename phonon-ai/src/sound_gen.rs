//! AI sound/music generation.
//!
//! Provides the [`SoundGenProvider`] trait and a generic HTTP implementation
//! that can call music/SFX generation APIs.
//!
//! ## Supported APIs
//!
//! The [`HttpSoundGen`] provider works with any API that accepts a JSON prompt
//! and returns audio bytes (e.g. Replicate, Suno, or a local model endpoint).
//!
//! ## Example
//!
//! ```rust,no_run
//! use phonon_ai::sound_gen::{HttpSoundGen, SoundGenProvider, SoundPrompt};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let provider = HttpSoundGen::new(
//!     "https://api.example.com/v1/music",
//!     "my-api-key",
//! )?;
//! let prompt = SoundPrompt::new("calm ambient background music, 30 seconds")
//!     .with_duration(30.0);
//! let audio = provider.generate(&prompt).await?;
//! # Ok(())
//! # }
//! ```

use crate::AiError;
use phonon_core::AudioBuffer;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::io::Cursor;
use tracing::{info, instrument};

/// Prompt / request for sound generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundPrompt {
    /// Text description of the desired sound/music.
    pub prompt: String,
    /// Desired duration in seconds (provider may ignore).
    #[serde(default)]
    pub duration_secs: Option<f64>,
    /// Style hint (e.g. "ambient", "electronic", "orchestral").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Negative prompt — what to avoid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub negative_prompt: Option<String>,
}

impl SoundPrompt {
    /// Create a new sound generation prompt.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            duration_secs: None,
            style: None,
            negative_prompt: None,
        }
    }

    /// Set the desired duration.
    pub fn with_duration(mut self, secs: f64) -> Self {
        self.duration_secs = Some(secs);
        self
    }

    /// Set the style hint.
    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Set a negative prompt.
    pub fn with_negative_prompt(mut self, neg: impl Into<String>) -> Self {
        self.negative_prompt = Some(neg.into());
        self
    }
}

/// Trait for AI sound/music generation providers.
pub trait SoundGenProvider: Send + Sync {
    /// Generate audio from a text prompt.
    fn generate(
        &self,
        prompt: &SoundPrompt,
    ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send;
}

/// Generic HTTP-based sound generation provider.
///
/// Sends a JSON prompt to a configurable endpoint and expects audio bytes
/// in the response. The response audio is decoded via phonon-io.
#[derive(Debug, Clone)]
pub struct HttpSoundGen {
    client: reqwest::Client,
    endpoint: String,
    api_key: String,
    response_format: String,
}

impl HttpSoundGen {
    /// Create a new `HttpSoundGen` provider.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The full URL of the generation API.
    /// * `api_key` - Bearer token for authentication.
    pub fn new(
        endpoint: impl Into<String>,
        api_key: impl Into<String>,
    ) -> Result<Self, AiError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| AiError::Http(e.to_string()))?;

        Ok(Self {
            client,
            endpoint: endpoint.into(),
            api_key: api_key.into(),
            response_format: "wav".to_string(),
        })
    }

    /// Set the expected response audio format (e.g. "wav", "mp3").
    pub fn with_response_format(mut self, fmt: impl Into<String>) -> Self {
        self.response_format = fmt.into();
        self
    }

    /// Decode audio bytes using phonon-io via a temp file.
    fn decode_audio_bytes(data: &[u8], suffix: &str) -> Result<AudioBuffer, AiError> {
        let ext = format!(".{suffix}");
        let tmp = tempfile::Builder::new()
            .suffix(&ext)
            .tempfile()
            .map_err(|e| AiError::Io(e.to_string()))?;

        std::io::copy(
            &mut Cursor::new(data),
            &mut std::fs::File::create(tmp.path()).map_err(|e| AiError::Io(e.to_string()))?,
        )
        .map_err(|e| AiError::Io(e.to_string()))?;

        phonon_io::read_audio(tmp.path()).map_err(|e| AiError::Decode(e.to_string()))
    }
}

impl SoundGenProvider for HttpSoundGen {
    #[instrument(skip(self, prompt), fields(prompt_len = prompt.prompt.len()))]
    async fn generate(&self, prompt: &SoundPrompt) -> Result<AudioBuffer, AiError> {
        info!(prompt = %prompt.prompt, "Generating sound via API");

        let response = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(prompt)
            .send()
            .await
            .map_err(|e| AiError::Http(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());
            return Err(AiError::Api {
                status: status.as_u16(),
                message: body,
            });
        }

        let audio_bytes = response
            .bytes()
            .await
            .map_err(|e| AiError::Http(e.to_string()))?;

        info!(bytes = audio_bytes.len(), "Received audio from generation API");

        let buffer = Self::decode_audio_bytes(&audio_bytes, &self.response_format)?;

        info!(
            duration = buffer.duration_secs(),
            samples = buffer.samples.len(),
            "Sound generation complete"
        );

        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sound_prompt_new() {
        let prompt = SoundPrompt::new("ambient music");
        assert_eq!(prompt.prompt, "ambient music");
        assert!(prompt.duration_secs.is_none());
        assert!(prompt.style.is_none());
        assert!(prompt.negative_prompt.is_none());
    }

    #[test]
    fn test_sound_prompt_builder() {
        let prompt = SoundPrompt::new("epic orchestral")
            .with_duration(60.0)
            .with_style("cinematic")
            .with_negative_prompt("vocals");
        assert_eq!(prompt.prompt, "epic orchestral");
        assert!((prompt.duration_secs.unwrap_or(0.0) - 60.0).abs() < f64::EPSILON);
        assert_eq!(prompt.style.as_deref(), Some("cinematic"));
        assert_eq!(prompt.negative_prompt.as_deref(), Some("vocals"));
    }

    #[test]
    fn test_sound_prompt_serialization() {
        let prompt = SoundPrompt::new("test")
            .with_duration(10.0)
            .with_style("ambient");
        let json = serde_json::to_value(&prompt).expect("serialize");
        assert_eq!(json["prompt"], "test");
        assert_eq!(json["duration_secs"], 10.0);
        assert_eq!(json["style"], "ambient");
        // negative_prompt should be absent (None → skip)
        assert!(json.get("negative_prompt").is_none());
    }

    #[test]
    fn test_sound_prompt_deserialization() {
        let json = r#"{"prompt": "piano melody", "duration_secs": 30.0}"#;
        let prompt: SoundPrompt = serde_json::from_str(json).expect("deserialize");
        assert_eq!(prompt.prompt, "piano melody");
        assert!((prompt.duration_secs.unwrap_or(0.0) - 30.0).abs() < f64::EPSILON);
        assert!(prompt.style.is_none());
    }

    #[test]
    fn test_http_sound_gen_creation() {
        let provider = HttpSoundGen::new("https://api.example.com/generate", "test-key");
        assert!(provider.is_ok());
        let provider = provider.unwrap();
        assert_eq!(provider.endpoint, "https://api.example.com/generate");
        assert_eq!(provider.response_format, "wav");
    }

    #[test]
    fn test_http_sound_gen_response_format() {
        let provider = HttpSoundGen::new("https://api.example.com/generate", "key")
            .unwrap()
            .with_response_format("mp3");
        assert_eq!(provider.response_format, "mp3");
    }
}
