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

pub mod client;
pub mod edge_tts;
pub mod elevenlabs;
pub mod tts;
pub mod voice;

pub use client::HttpClient;
pub use edge_tts::EdgeTts;
pub use elevenlabs::ElevenLabsTts;
pub use tts::{OpenAiTts, TtsProvider};
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

/// Result type for phonon-ai operations.
pub type Result<T> = std::result::Result<T, AiError>;
