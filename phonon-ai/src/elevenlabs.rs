//! ElevenLabs TTS provider.
//!
//! Uses the ElevenLabs Text-to-Speech API for high-quality, expressive voice synthesis.
//! Supports multiple voices, voice cloning, and emotional expression.
//!
//! # Setup
//!
//! Set the `ELEVENLABS_API_KEY` environment variable.
//!
//! # Example
//!
//! ```rust,no_run
//! use phonon_ai::{ElevenLabsTts, TtsProvider, VoiceConfig};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let tts = ElevenLabsTts::new()?;
//! let voice = VoiceConfig::new("21m00Tcm4TlvDq8ikWAM") // Rachel
//!     .with_style("conversational");
//! let audio = tts.synthesize("Hello from ElevenLabs!", &voice).await?;
//! # Ok(())
//! # }
//! ```

use crate::client::HttpClient;
use crate::voice::VoiceConfig;
use crate::AiError;
use phonon_core::AudioBuffer;
use serde::Serialize;
use std::io::Cursor;
use tracing::{info, instrument};

const ELEVENLABS_BASE_URL: &str = "https://api.elevenlabs.io/v1";

/// ElevenLabs TTS provider.
///
/// High-quality neural TTS with expressive voice control.
/// Requires an `ELEVENLABS_API_KEY` environment variable.
///
/// ## Common Voice IDs
///
/// - `21m00Tcm4TlvDq8ikWAM` — Rachel (calm, narration)
/// - `29vD33N1CtxCmqQRPOHJ` — Drew (well-rounded)
/// - `EXAVITQu4vr4xnSDxMaL` — Bella (soft)
/// - `ErXwobaYiN019PkySvjV` — Antoni (well-rounded)
/// - `MF3mGyEYCl7XYWbV9V6O` — Elli (young female)
#[derive(Debug, Clone)]
pub struct ElevenLabsTts {
    client: HttpClient,
    /// Model ID to use. Default: "eleven_multilingual_v2".
    model_id: String,
}

/// Voice settings sent to the ElevenLabs API.
#[derive(Debug, Clone, Serialize)]
struct VoiceSettings {
    stability: f64,
    similarity_boost: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    use_speaker_boost: Option<bool>,
}

/// Request body for the ElevenLabs TTS API.
#[derive(Debug, Serialize)]
struct ElevenLabsTtsRequest<'a> {
    text: &'a str,
    model_id: &'a str,
    voice_settings: VoiceSettings,
}

impl ElevenLabsTts {
    /// Creates a new ElevenLabs TTS provider.
    ///
    /// Reads the API key from the `ELEVENLABS_API_KEY` environment variable.
    pub fn new() -> Result<Self, AiError> {
        let api_key = std::env::var("ELEVENLABS_API_KEY").map_err(|_| {
            AiError::Config("ELEVENLABS_API_KEY environment variable not set".to_string())
        })?;

        let client = HttpClient::new(api_key, ELEVENLABS_BASE_URL, Some(180))?;

        Ok(Self {
            client,
            model_id: "eleven_multilingual_v2".to_string(),
        })
    }

    /// Creates a new ElevenLabs TTS provider with a custom API key and model.
    pub fn with_config(
        api_key: impl Into<String>,
        model_id: impl Into<String>,
    ) -> Result<Self, AiError> {
        let client = HttpClient::new(api_key, ELEVENLABS_BASE_URL, Some(180))?;
        Ok(Self {
            client,
            model_id: model_id.into(),
        })
    }

    /// Sets the model ID (e.g. "eleven_monolingual_v1", "eleven_multilingual_v2").
    pub fn with_model(mut self, model_id: impl Into<String>) -> Self {
        self.model_id = model_id.into();
        self
    }

    /// Decodes audio bytes into an AudioBuffer via phonon-io.
    fn decode_audio_bytes(data: &[u8]) -> Result<AudioBuffer, AiError> {
        let tmp = tempfile::Builder::new()
            .suffix(".mp3")
            .tempfile()
            .map_err(|e| AiError::Io(e.to_string()))?;

        std::io::copy(
            &mut Cursor::new(data),
            &mut std::fs::File::create(tmp.path()).map_err(|e| AiError::Io(e.to_string()))?,
        )
        .map_err(|e| AiError::Io(e.to_string()))?;

        let buffer =
            phonon_io::read_audio(tmp.path()).map_err(|e| AiError::Decode(e.to_string()))?;

        Ok(buffer)
    }

    /// Derive voice settings from VoiceConfig's style/emotion fields.
    fn voice_settings_from_config(voice: &VoiceConfig) -> VoiceSettings {
        // Map emotion/style hints to ElevenLabs voice settings.
        // Default: balanced stability + similarity.
        let mut stability = 0.5;
        let mut similarity_boost = 0.75;
        let mut style_val = None;

        if let Some(ref emotion) = voice.emotion {
            match emotion.to_lowercase().as_str() {
                "calm" | "neutral" => {
                    stability = 0.75;
                    similarity_boost = 0.5;
                }
                "excited" | "happy" | "energetic" => {
                    stability = 0.3;
                    similarity_boost = 0.8;
                    style_val = Some(0.7);
                }
                "sad" | "melancholy" => {
                    stability = 0.6;
                    similarity_boost = 0.6;
                    style_val = Some(0.5);
                }
                "angry" | "intense" => {
                    stability = 0.25;
                    similarity_boost = 0.85;
                    style_val = Some(0.8);
                }
                _ => {}
            }
        }

        if let Some(ref style) = voice.style {
            match style.to_lowercase().as_str() {
                "conversational" => {
                    stability = 0.4;
                    style_val = Some(0.5);
                }
                "narration" | "narrative" => {
                    stability = 0.65;
                    similarity_boost = 0.7;
                }
                "newscast" | "formal" => {
                    stability = 0.7;
                    similarity_boost = 0.8;
                }
                _ => {}
            }
        }

        VoiceSettings {
            stability,
            similarity_boost,
            style: style_val,
            use_speaker_boost: Some(true),
        }
    }
}

impl crate::tts::TtsProvider for ElevenLabsTts {
    #[instrument(skip(self, text), fields(text_len = text.len(), voice = %voice.voice_id))]
    async fn synthesize(
        &self,
        text: &str,
        voice: &VoiceConfig,
    ) -> Result<AudioBuffer, AiError> {
        info!("Synthesizing text via ElevenLabs TTS");

        let url = format!(
            "{}/text-to-speech/{}",
            self.client.base_url(),
            voice.voice_id
        );

        let voice_settings = Self::voice_settings_from_config(voice);
        let request_body = ElevenLabsTtsRequest {
            text,
            model_id: &self.model_id,
            voice_settings,
        };

        let response = self
            .client
            .inner()
            .post(&url)
            .header("xi-api-key", self.client.api_key())
            .header("Content-Type", "application/json")
            .header("Accept", "audio/mpeg")
            .json(&request_body)
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

        info!(bytes = audio_bytes.len(), "Received ElevenLabs audio response");

        let buffer = Self::decode_audio_bytes(&audio_bytes)?;

        info!(
            duration = buffer.duration_secs(),
            samples = buffer.samples.len(),
            "ElevenLabs TTS synthesis complete"
        );

        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elevenlabs_config_error_without_env() {
        // Clear env to test error path
        // SAFETY: test runs single-threaded; no other thread reads this var concurrently.
        unsafe { std::env::remove_var("ELEVENLABS_API_KEY") };
        let result = ElevenLabsTts::new();
        assert!(result.is_err());
    }

    #[test]
    fn test_elevenlabs_with_config() {
        let tts = ElevenLabsTts::with_config("test-key", "eleven_monolingual_v1")
            .expect("Should create with explicit config");
        assert_eq!(tts.model_id, "eleven_monolingual_v1");
    }

    #[test]
    fn test_elevenlabs_with_model() {
        let tts = ElevenLabsTts::with_config("test-key", "eleven_multilingual_v2")
            .expect("Should create")
            .with_model("eleven_turbo_v2");
        assert_eq!(tts.model_id, "eleven_turbo_v2");
    }

    #[test]
    fn test_voice_settings_default() {
        let voice = VoiceConfig::new("some_voice");
        let settings = ElevenLabsTts::voice_settings_from_config(&voice);
        assert!((settings.stability - 0.5).abs() < f64::EPSILON);
        assert!((settings.similarity_boost - 0.75).abs() < f64::EPSILON);
        assert!(settings.style.is_none());
    }

    #[test]
    fn test_voice_settings_excited_emotion() {
        let voice = VoiceConfig::new("some_voice").with_emotion("excited");
        let settings = ElevenLabsTts::voice_settings_from_config(&voice);
        assert!(settings.stability < 0.5); // lower stability for excited
        assert!(settings.style.is_some());
    }

    #[test]
    fn test_voice_settings_narration_style() {
        let voice = VoiceConfig::new("some_voice").with_style("narration");
        let settings = ElevenLabsTts::voice_settings_from_config(&voice);
        assert!(settings.stability > 0.6); // higher stability for narration
    }

    #[test]
    fn test_request_serialization() {
        let settings = VoiceSettings {
            stability: 0.5,
            similarity_boost: 0.75,
            style: None,
            use_speaker_boost: Some(true),
        };
        let req = ElevenLabsTtsRequest {
            text: "Hello",
            model_id: "eleven_multilingual_v2",
            voice_settings: settings,
        };
        let json = serde_json::to_value(&req).expect("Should serialize");
        assert_eq!(json["text"], "Hello");
        assert_eq!(json["model_id"], "eleven_multilingual_v2");
        assert!(json["voice_settings"]["style"].is_null()); // skipped
    }

    // Integration test — requires ELEVENLABS_API_KEY.
    // Run with: cargo test -p phonon-ai -- --ignored
    #[tokio::test]
    #[ignore]
    async fn test_elevenlabs_synthesize() {
        use crate::tts::TtsProvider;

        let tts = ElevenLabsTts::new().expect("Need ELEVENLABS_API_KEY");
        let voice = VoiceConfig::new("21m00Tcm4TlvDq8ikWAM"); // Rachel
        let buffer = tts
            .synthesize("Hello, this is an ElevenLabs test.", &voice)
            .await
            .expect("Synthesis should succeed");

        assert!(!buffer.samples.is_empty());
        assert!(buffer.duration_secs() > 0.5);
    }
}
