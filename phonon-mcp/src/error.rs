use thiserror::Error;

/// Errors surfaced by phonon-mcp tool handlers.
#[derive(Error, Debug)]
pub enum McpError {
  /// Invalid input arguments (bad JSON shape, out-of-range values, etc.).
  #[error("Invalid arguments: {0}")]
  InvalidArgs(String),

  /// Asset handle was not found in the registry.
  #[error("Audio handle not found: {0}")]
  HandleNotFound(String),

  /// Underlying audio operation failed.
  #[error("Audio operation failed: {0}")]
  Audio(String),

  /// I/O error.
  #[error("I/O error: {0}")]
  Io(String),

  /// Tool not implemented or disabled.
  #[error("Tool not found: {0}")]
  ToolNotFound(String),

  /// JSON serialization / deserialization error.
  #[error("Serde error: {0}")]
  Serde(String),
}

impl From<serde_json::Error> for McpError {
  fn from(value: serde_json::Error) -> Self {
    Self::Serde(value.to_string())
  }
}

impl From<phonon_core::PhononError> for McpError {
  fn from(value: phonon_core::PhononError) -> Self {
    Self::Audio(value.to_string())
  }
}

impl From<phonon_ai::AiError> for McpError {
  fn from(value: phonon_ai::AiError) -> Self {
    Self::Audio(value.to_string())
  }
}

impl From<phonon_podcast::PodcastError> for McpError {
  fn from(value: phonon_podcast::PodcastError) -> Self {
    Self::Audio(value.to_string())
  }
}
