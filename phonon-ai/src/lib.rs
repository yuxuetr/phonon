//! # phonon-ai
//!
//! AI-powered audio generation for the Phonon audio engine.
//!
//! This crate provides Text-to-Speech (TTS) integration, converting text into
//! `AudioBuffer` for use with other Phonon crates.
//!
//! ## Supported Providers
//!
//! - **OpenAI TTS** (`OpenAiTts`) — Uses the OpenAI `/audio/speech` API
//! - Extensible via the `TtsProvider` trait for custom providers
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use phonon_ai::{OpenAiTts, TtsProvider, VoiceConfig};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let tts = OpenAiTts::new()?;
//! let voice = VoiceConfig::new("alloy").with_model("tts-1-hd");
//! let audio = tts.synthesize("Hello, welcome to our podcast!", &voice).await?;
//! # Ok(())
//! # }
//! ```

pub mod cache;
pub mod client;
pub mod edge_tts;
pub mod elevenlabs;
pub mod minimax;
pub mod qwen;
pub mod sound_gen;
pub mod stt;
pub mod tts;
pub mod voice;

pub use cache::CachingTts;
pub use client::{HttpClient, retry_transient};
pub use edge_tts::EdgeTts;
pub use elevenlabs::ElevenLabsTts;
pub use minimax::MiniMaxTts;
pub use qwen::QwenTts;
pub use sound_gen::{HttpSoundGen, ReplicateSoundGen, SoundGenProvider, SoundPreset, SoundPrompt};
pub use stt::{
  SegmentMode, SttProvider, TranscribeOptions, TranscriptResult, TranscriptSegment, WhisperStt,
};
pub use tts::{OpenAiTts, TtsProvider, pcm_i16le_to_f32};
pub use voice::VoiceConfig;

use thiserror::Error;

/// Errors that can occur in phonon-ai operations.
#[derive(Error, Debug)]
pub enum AiError {
  /// HTTP request or network error.
  #[error("HTTP error: {0}")]
  Http(String),

  /// API returned an error response.
  #[error("API error (status {status}): {message}")]
  Api { status: u16, message: String },

  /// Configuration error (e.g. missing API key).
  #[error("Configuration error: {0}")]
  Config(String),

  /// Audio decoding error.
  #[error("Audio decode error: {0}")]
  Decode(String),

  /// I/O error.
  #[error("IO error: {0}")]
  Io(String),
}

impl AiError {
  /// Whether retrying the same request may succeed: network failures,
  /// rate limiting (429) and server errors (5xx).
  pub fn is_transient(&self) -> bool {
    match self {
      AiError::Http(_) => true,
      AiError::Api { status, .. } => *status == 429 || (500..600).contains(status),
      AiError::Config(_) | AiError::Decode(_) | AiError::Io(_) => false,
    }
  }
}

/// Result type for phonon-ai operations.
pub type Result<T> = std::result::Result<T, AiError>;
