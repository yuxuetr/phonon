//! # phonon-podcast
//!
//! Podcast generation pipeline for the Phonon audio engine.
//!
//! Converts a JSON podcast script (with multi-speaker text) into a complete
//! podcast audio file, using TTS synthesis and audio processing.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use phonon_ai::OpenAiTts;
//! use phonon_podcast::{PodcastPipeline, PodcastScript};
//!
//! # async fn example() -> Result<(), phonon_podcast::PodcastError> {
//! let tts = OpenAiTts::new().map_err(|e| phonon_podcast::PodcastError::Tts(e.to_string()))?;
//! let script = PodcastScript::from_file("episode.json")?;
//! let pipeline = PodcastPipeline::new(tts);
//! pipeline.generate_to_file(&script, "output.wav").await?;
//! # Ok(())
//! # }
//! ```

pub mod chapters;
pub mod config;
pub mod pipeline;
pub mod script;
pub mod script_gen;
pub mod subtitle;

pub use chapters::{
  Chapter, assign_chapter_times, chapters_from_silence, chapters_from_speakers,
  format_chapters_text,
};
pub use config::PipelineConfig;
pub use pipeline::PodcastPipeline;
pub use script::{PodcastScript, Segment};
pub use script_gen::{OpenAiScriptGenerator, ScriptGenerator, ScriptRequest};
pub use subtitle::{SubtitleEntry, generate_srt, segments_to_srt_file, write_srt_file};

use thiserror::Error;

/// Errors that can occur during podcast generation.
#[derive(Error, Debug)]
pub enum PodcastError {
  /// Script parsing or validation error.
  #[error("Script error: {0}")]
  Script(String),

  /// TTS synthesis error.
  #[error("TTS error: {0}")]
  Tts(String),

  /// Audio processing error.
  #[error("Processing error: {0}")]
  Processing(String),

  /// I/O error (file read/write).
  #[error("IO error: {0}")]
  Io(String),
}
