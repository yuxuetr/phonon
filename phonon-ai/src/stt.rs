//! Speech-to-Text (STT) integration.
//!
//! Provides the [`SttProvider`] trait and an OpenAI Whisper implementation
//! ([`WhisperStt`]) that calls the `/audio/transcriptions` API.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use phonon_ai::{WhisperStt, SttProvider};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let stt = WhisperStt::new()?;
//! let result = stt.transcribe_file("recording.wav", None).await?;
//! println!("Text: {}", result.text);
//! for seg in &result.segments {
//!     println!("[{:.1}s - {:.1}s] {}", seg.start, seg.end, seg.text);
//! }
//! # Ok(())
//! # }
//! ```

use crate::client::HttpClient;
use crate::AiError;
use serde::Deserialize;
use std::future::Future;
use std::path::Path;
use tracing::{info, instrument};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A single timed segment returned by transcription.
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptSegment {
    /// Segment index (0-based).
    #[serde(default)]
    pub id: usize,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Transcribed text for this segment.
    pub text: String,
}

/// Full transcription result.
#[derive(Debug, Clone)]
pub struct TranscriptResult {
    /// Complete transcribed text.
    pub text: String,
    /// Optional language detected (ISO-639-1, e.g. "en").
    pub language: Option<String>,
    /// Duration of the audio in seconds.
    pub duration: Option<f64>,
    /// Timed segments (only available when timestamps are requested).
    pub segments: Vec<TranscriptSegment>,
}

/// Options for transcription requests.
#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    /// Language hint (ISO-639-1 code, e.g. "en", "zh").
    /// If `None`, the model auto-detects.
    pub language: Option<String>,
    /// Optional prompt to guide the model (e.g. proper nouns, style).
    pub prompt: Option<String>,
    /// Whether to request timed segments. Defaults to `true`.
    pub timestamps: bool,
    /// Temperature for sampling (0.0 = deterministic). Default 0.0.
    pub temperature: f64,
}

impl TranscribeOptions {
    /// Create default options with timestamps enabled.
    pub fn new() -> Self {
        Self {
            timestamps: true,
            temperature: 0.0,
            ..Default::default()
        }
    }

    /// Set the language hint.
    pub fn with_language(mut self, lang: &str) -> Self {
        self.language = Some(lang.to_string());
        self
    }

    /// Set a guiding prompt.
    pub fn with_prompt(mut self, prompt: &str) -> Self {
        self.prompt = Some(prompt.to_string());
        self
    }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Trait for Speech-to-Text providers.
///
/// Implementors convert audio files or raw bytes into text transcriptions.
pub trait SttProvider: Send + Sync {
    /// Transcribe an audio file at the given path.
    fn transcribe_file(
        &self,
        path: &str,
        options: Option<TranscribeOptions>,
    ) -> impl Future<Output = Result<TranscriptResult, AiError>> + Send;

    /// Transcribe raw audio bytes.
    ///
    /// `filename` is the hint for the API (e.g. "audio.wav").
    fn transcribe_bytes(
        &self,
        data: Vec<u8>,
        filename: &str,
        options: Option<TranscribeOptions>,
    ) -> impl Future<Output = Result<TranscriptResult, AiError>> + Send;
}

// ---------------------------------------------------------------------------
// OpenAI Whisper implementation
// ---------------------------------------------------------------------------

/// OpenAI Whisper STT provider.
///
/// Calls the OpenAI `/audio/transcriptions` endpoint (compatible with the
/// Whisper model).
#[derive(Debug, Clone)]
pub struct WhisperStt {
    client: HttpClient,
    model: String,
}

/// Raw verbose_json response from the Whisper API.
#[derive(Deserialize)]
struct WhisperVerboseResponse {
    text: String,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    segments: Vec<TranscriptSegment>,
}

/// Simple json response (no segments).
#[derive(Deserialize)]
struct WhisperSimpleResponse {
    text: String,
}

impl WhisperStt {
    /// Creates a new `WhisperStt` using the default OpenAI endpoint.
    ///
    /// Reads the API key from `OPENAI_API_KEY`.
    pub fn new() -> Result<Self, AiError> {
        let client = HttpClient::openai()?;
        Ok(Self {
            client,
            model: "whisper-1".to_string(),
        })
    }

    /// Creates a `WhisperStt` with a custom HTTP client and model.
    pub fn with_client(client: HttpClient, model: impl Into<String>) -> Self {
        Self {
            client,
            model: model.into(),
        }
    }

    /// Set the model name (e.g. "whisper-1").
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Build the multipart form for the Whisper API.
    fn build_form(
        &self,
        file_part: reqwest::multipart::Part,
        options: &TranscribeOptions,
    ) -> reqwest::multipart::Form {
        let response_format = if options.timestamps {
            "verbose_json"
        } else {
            "json"
        };

        let mut form = reqwest::multipart::Form::new()
            .part("file", file_part)
            .text("model", self.model.clone())
            .text("response_format", response_format.to_string())
            .text("temperature", options.temperature.to_string());

        if let Some(ref lang) = options.language {
            form = form.text("language", lang.clone());
        }
        if let Some(ref prompt) = options.prompt {
            form = form.text("prompt", prompt.clone());
        }

        // Request word-level timestamps when segments are requested.
        if options.timestamps {
            form = form.text("timestamp_granularities[]", "segment".to_string());
        }

        form
    }

    /// Send the form and parse the response.
    async fn send_request(
        &self,
        form: reqwest::multipart::Form,
        timestamps: bool,
    ) -> Result<TranscriptResult, AiError> {
        let url = format!("{}/audio/transcriptions", self.client.base_url());

        let response = self
            .client
            .inner()
            .post(&url)
            .header(
                "Authorization",
                format!("Bearer {}", self.client.api_key()),
            )
            .multipart(form)
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

        let body = response
            .text()
            .await
            .map_err(|e| AiError::Http(e.to_string()))?;

        if timestamps {
            let resp: WhisperVerboseResponse =
                serde_json::from_str(&body).map_err(|e| AiError::Decode(e.to_string()))?;
            Ok(TranscriptResult {
                text: resp.text,
                language: resp.language,
                duration: resp.duration,
                segments: resp.segments,
            })
        } else {
            let resp: WhisperSimpleResponse =
                serde_json::from_str(&body).map_err(|e| AiError::Decode(e.to_string()))?;
            Ok(TranscriptResult {
                text: resp.text,
                language: None,
                duration: None,
                segments: vec![],
            })
        }
    }
}

impl SttProvider for WhisperStt {
    #[instrument(skip(self), fields(model = %self.model))]
    async fn transcribe_file(
        &self,
        path: &str,
        options: Option<TranscribeOptions>,
    ) -> Result<TranscriptResult, AiError> {
        let opts = options.unwrap_or_default();
        info!(path = %path, "Transcribing audio file via Whisper");

        let file_bytes = tokio::fs::read(path)
            .await
            .map_err(|e| AiError::Io(e.to_string()))?;

        let filename = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio.wav")
            .to_string();

        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(filename)
            .mime_str("application/octet-stream")
            .map_err(|e| AiError::Http(e.to_string()))?;

        let form = self.build_form(part, &opts);
        let result = self.send_request(form, opts.timestamps).await?;

        info!(
            text_len = result.text.len(),
            segments = result.segments.len(),
            "Transcription complete"
        );

        Ok(result)
    }

    #[instrument(skip(self, data), fields(model = %self.model, size = data.len()))]
    async fn transcribe_bytes(
        &self,
        data: Vec<u8>,
        filename: &str,
        options: Option<TranscribeOptions>,
    ) -> Result<TranscriptResult, AiError> {
        let opts = options.unwrap_or_default();
        info!(filename = %filename, "Transcribing audio bytes via Whisper");

        let part = reqwest::multipart::Part::bytes(data)
            .file_name(filename.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| AiError::Http(e.to_string()))?;

        let form = self.build_form(part, &opts);
        let result = self.send_request(form, opts.timestamps).await?;

        info!(
            text_len = result.text.len(),
            segments = result.segments.len(),
            "Transcription complete"
        );

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcribe_options_defaults() {
        let opts = TranscribeOptions::new();
        assert!(opts.timestamps);
        assert!((opts.temperature - 0.0).abs() < f64::EPSILON);
        assert!(opts.language.is_none());
        assert!(opts.prompt.is_none());
    }

    #[test]
    fn test_transcribe_options_builder() {
        let opts = TranscribeOptions::new()
            .with_language("zh")
            .with_prompt("Phonon audio engine");
        assert_eq!(opts.language.as_deref(), Some("zh"));
        assert_eq!(opts.prompt.as_deref(), Some("Phonon audio engine"));
    }

    #[test]
    fn test_verbose_json_deserialization() {
        let json = r#"{
            "text": "Hello world",
            "language": "en",
            "duration": 2.5,
            "segments": [
                {"id": 0, "start": 0.0, "end": 1.2, "text": "Hello"},
                {"id": 1, "start": 1.2, "end": 2.5, "text": " world"}
            ]
        }"#;
        let resp: WhisperVerboseResponse = serde_json::from_str(json)
            .expect("Should deserialize verbose response");
        assert_eq!(resp.text, "Hello world");
        assert_eq!(resp.language.as_deref(), Some("en"));
        assert!((resp.duration.unwrap_or(0.0) - 2.5).abs() < f64::EPSILON);
        assert_eq!(resp.segments.len(), 2);
        assert_eq!(resp.segments[0].text, "Hello");
        assert!((resp.segments[1].start - 1.2).abs() < f64::EPSILON);
    }

    #[test]
    fn test_simple_json_deserialization() {
        let json = r#"{"text": "Hello world"}"#;
        let resp: WhisperSimpleResponse = serde_json::from_str(json)
            .expect("Should deserialize simple response");
        assert_eq!(resp.text, "Hello world");
    }

    #[test]
    fn test_transcript_segment_fields() {
        let seg = TranscriptSegment {
            id: 0,
            start: 1.5,
            end: 3.0,
            text: "Test segment".to_string(),
        };
        assert_eq!(seg.id, 0);
        assert!((seg.start - 1.5).abs() < f64::EPSILON);
        assert!((seg.end - 3.0).abs() < f64::EPSILON);
        assert_eq!(seg.text, "Test segment");
    }

    #[test]
    fn test_verbose_json_missing_optional_fields() {
        // Whisper API may omit language/duration in some edge cases
        let json = r#"{
            "text": "Some text",
            "segments": []
        }"#;
        let resp: WhisperVerboseResponse = serde_json::from_str(json)
            .expect("Should handle missing optional fields");
        assert_eq!(resp.text, "Some text");
        assert!(resp.language.is_none());
        assert!(resp.duration.is_none());
        assert!(resp.segments.is_empty());
    }

    #[test]
    fn test_whisper_stt_model_setter() {
        // Can't call new() without OPENAI_API_KEY, but we test the setter on a mock instance
        let client = HttpClient::new("test-key", "http://localhost:8080/v1", None)
            .expect("Should create client");
        let mut stt = WhisperStt::with_client(client, "whisper-1");
        assert_eq!(stt.model, "whisper-1");
        stt.set_model("whisper-large-v3");
        assert_eq!(stt.model, "whisper-large-v3");
    }
}
