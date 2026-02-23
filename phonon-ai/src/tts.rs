use crate::client::HttpClient;
use crate::voice::VoiceConfig;
use crate::AiError;
use phonon_core::AudioBuffer;
use serde::Serialize;
use std::future::Future;
use std::io::Cursor;
use tracing::{info, instrument};

/// Trait for Text-to-Speech providers.
///
/// Implementors convert text into an `AudioBuffer` using a specified voice.
/// The trait uses associated future types to remain object-safe while supporting async.
pub trait TtsProvider: Send + Sync {
    /// Synthesizes the given text into an `AudioBuffer`.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to synthesize.
    /// * `voice` - The voice configuration to use.
    ///
    /// # Returns
    ///
    /// An `AudioBuffer` containing the synthesized speech.
    fn synthesize(
        &self,
        text: &str,
        voice: &VoiceConfig,
    ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send;

    /// Synthesizes text and sends audio chunks to a channel as they become available.
    ///
    /// This enables streaming playback — the caller can start processing/playing
    /// the first chunks before synthesis is complete.
    ///
    /// The default implementation calls `synthesize()` and sends the entire buffer
    /// as a single chunk. Providers that support true streaming (like OpenAI)
    /// may override this for lower latency.
    ///
    /// # Arguments
    ///
    /// * `text` - The text to synthesize.
    /// * `voice` - The voice configuration to use.
    /// * `tx` - Channel sender for audio buffer chunks.
    fn synthesize_streaming(
        &self,
        text: &str,
        voice: &VoiceConfig,
        tx: tokio::sync::mpsc::Sender<Result<AudioBuffer, AiError>>,
    ) -> impl Future<Output = Result<(), AiError>> + Send {
        async move {
            let buffer = self.synthesize(text, voice).await;
            let _ = tx.send(buffer).await;
            Ok(())
        }
    }
}

/// OpenAI TTS provider.
///
/// Uses the OpenAI `/audio/speech` API to synthesize text to speech.
/// Returns audio as WAV (via Symphonia decoding of the MP3 response).
#[derive(Debug, Clone)]
pub struct OpenAiTts {
    client: HttpClient,
}

/// OpenAI TTS API request body.
#[derive(Serialize)]
struct TtsRequest<'a> {
    model: &'a str,
    input: &'a str,
    voice: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    speed: Option<f64>,
    response_format: &'a str,
}

impl OpenAiTts {
    /// Creates a new `OpenAiTts` using the default OpenAI endpoint.
    ///
    /// Reads the API key from `OPENAI_API_KEY` environment variable.
    pub fn new() -> Result<Self, AiError> {
        let client = HttpClient::openai()?;
        Ok(Self { client })
    }

    /// Creates a new `OpenAiTts` with a custom HTTP client.
    ///
    /// Useful for OpenAI-compatible APIs (e.g. local TTS servers).
    pub fn with_client(client: HttpClient) -> Self {
        Self { client }
    }

    /// Decodes audio bytes (MP3/WAV etc.) into an AudioBuffer using Symphonia via phonon-io.
    fn decode_audio_bytes(data: &[u8]) -> Result<AudioBuffer, AiError> {
        // Write bytes to a temp file, then use phonon-io to decode.
        // This is not ideal but avoids duplicating Symphonia logic.
        let tmp = tempfile::Builder::new()
            .suffix(".mp3")
            .tempfile()
            .map_err(|e| AiError::Io(e.to_string()))?;

        std::io::copy(&mut Cursor::new(data), &mut std::fs::File::create(tmp.path())
            .map_err(|e| AiError::Io(e.to_string()))?)
            .map_err(|e| AiError::Io(e.to_string()))?;

        let buffer = phonon_io::read_audio(tmp.path())
            .map_err(|e| AiError::Decode(e.to_string()))?;

        Ok(buffer)
    }
}

impl TtsProvider for OpenAiTts {
    #[instrument(skip(self, text), fields(text_len = text.len(), voice = %voice.voice_id))]
    async fn synthesize(
        &self,
        text: &str,
        voice: &VoiceConfig,
    ) -> Result<AudioBuffer, AiError> {
        info!("Synthesizing text via OpenAI TTS");

        let url = format!("{}/audio/speech", self.client.base_url());

        let speed = if (voice.speed - 1.0).abs() > f64::EPSILON {
            Some(voice.speed)
        } else {
            None
        };

        let request_body = TtsRequest {
            model: &voice.model,
            input: text,
            voice: &voice.voice_id,
            speed,
            response_format: "mp3",
        };

        let response = self
            .client
            .inner()
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.client.api_key()))
            .header("Content-Type", "application/json")
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

        info!(bytes = audio_bytes.len(), "Received audio response");

        // Decode the audio response (MP3) into an AudioBuffer
        let buffer = Self::decode_audio_bytes(&audio_bytes)?;

        info!(
            duration = buffer.duration_secs(),
            samples = buffer.samples.len(),
            "TTS synthesis complete"
        );

        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Integration tests with actual OpenAI API require OPENAI_API_KEY
    // and should be run with `cargo test -- --ignored`

    #[test]
    fn test_tts_request_serialization() {
        let req = TtsRequest {
            model: "tts-1",
            input: "Hello world",
            voice: "alloy",
            speed: Some(1.5),
            response_format: "mp3",
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "tts-1");
        assert_eq!(json["input"], "Hello world");
        assert_eq!(json["voice"], "alloy");
        assert_eq!(json["speed"], 1.5);
        assert_eq!(json["response_format"], "mp3");
    }

    #[test]
    fn test_tts_request_no_speed() {
        let req = TtsRequest {
            model: "tts-1",
            input: "Hello",
            voice: "alloy",
            speed: None,
            response_format: "mp3",
        };
        let json = serde_json::to_value(&req).unwrap();
        assert!(json.get("speed").is_none());
    }
}
