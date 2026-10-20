//! Edge TTS provider using Microsoft Edge's Read Aloud API.
//!
//! This provider is **free** and requires no API key.
//! It supports many voices across multiple languages.
//!
//! # Example
//!
//! ```rust,no_run
//! use phonon_ai::{EdgeTts, TtsProvider, VoiceConfig};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let tts = EdgeTts::new();
//! let voice = VoiceConfig::new("zh-CN-XiaoxiaoNeural")
//!     .with_language("zh-CN");
//! let audio = tts.synthesize("你好世界", &voice).await?;
//! # Ok(())
//! # }
//! ```

use crate::AiError;
use crate::client::retry_transient;
use crate::voice::VoiceConfig;
use phonon_core::AudioBuffer;
use std::time::Duration;
use tracing::{info, instrument};

/// Upper bound for one connect + synthesize round trip.
const EDGE_TIMEOUT: Duration = Duration::from_secs(60);

/// Edge TTS provider.
///
/// Uses the Microsoft Edge Read Aloud API via WebSocket.
/// Free to use, no API key required.
///
/// ## Common Voice Names
///
/// | Language | Voice Name |
/// |----------|-----------|
/// | English (US) | `en-US-JennyNeural`, `en-US-GuyNeural`, `en-US-AriaNeural` |
/// | Chinese (CN) | `zh-CN-XiaoxiaoNeural`, `zh-CN-YunyangNeural`, `zh-CN-XiaoyiNeural` |
/// | Japanese | `ja-JP-NanamiNeural`, `ja-JP-KeitaNeural` |
#[derive(Debug, Clone)]
pub struct EdgeTts;

impl EdgeTts {
  /// Creates a new Edge TTS provider.
  ///
  /// No configuration needed — the Edge API is free and keyless.
  pub fn new() -> Self {
    Self
  }
}

impl Default for EdgeTts {
  fn default() -> Self {
    Self::new()
  }
}

impl crate::tts::TtsProvider for EdgeTts {
  #[instrument(skip(self, text), fields(text_len = text.len(), voice = %voice.voice_id))]
  async fn synthesize(&self, text: &str, voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
    info!("Synthesizing text via Edge TTS");

    // Map VoiceConfig speed (0.25–4.0, default 1.0) to Edge rate percentage
    let rate = ((voice.speed - 1.0) * 100.0) as i32;

    let config = msedge_tts::tts::SpeechConfig {
      voice_name: voice.voice_id.clone(),
      audio_format: "audio-24khz-48kbitrate-mono-mp3".to_string(),
      pitch: 0,
      rate,
      volume: 0,
    };

    // The async client can be dropped mid-request, so the timeout really
    // cancels a stalled connection instead of leaking a blocking thread.
    let audio_bytes = retry_transient(|| async {
      let synthesis = async {
        let mut client = msedge_tts::tts::client::tokio_runtime::connect_async()
          .await
          .map_err(|e| AiError::Http(format!("Edge TTS connection failed: {e}")))?;
        client
          .synthesize(text, &config)
          .await
          .map_err(|e| AiError::Http(format!("Edge TTS synthesis failed: {e}")))
      };
      let audio = tokio::time::timeout(EDGE_TIMEOUT, synthesis)
        .await
        .map_err(|_| AiError::Http(format!("Edge TTS timed out after {EDGE_TIMEOUT:?}")))??;
      Ok(audio.audio_bytes)
    })
    .await?;

    info!(bytes = audio_bytes.len(), "Received Edge TTS response");

    let buffer = crate::tts::decode(&audio_bytes, "mp3")?;

    info!(
      duration = buffer.duration_secs(),
      samples = buffer.samples.len(),
      "Edge TTS synthesis complete"
    );

    Ok(buffer)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_edge_tts_creation() {
    let tts = EdgeTts::new();
    let _default = EdgeTts;
    // Just verify construction works
    let _ = format!("{tts:?}");
  }

  // Integration test — requires network access.
  // Run with: cargo test -p phonon-ai -- --ignored
  #[tokio::test]
  #[ignore]
  async fn test_edge_tts_synthesize_english() {
    use crate::tts::TtsProvider;

    let tts = EdgeTts::new();
    let voice = VoiceConfig::new("en-US-JennyNeural");
    let buffer = tts
      .synthesize("Hello, this is a test.", &voice)
      .await
      .unwrap();

    assert!(!buffer.samples.is_empty());
    assert!(buffer.duration_secs() > 0.5);
    assert_eq!(buffer.channels, 1); // mono output
  }

  #[tokio::test]
  #[ignore]
  async fn test_edge_tts_synthesize_chinese() {
    use crate::tts::TtsProvider;

    let tts = EdgeTts::new();
    let voice = VoiceConfig::new("zh-CN-XiaoxiaoNeural").with_language("zh-CN");
    let buffer = tts
      .synthesize("你好世界，今天天气真不错。", &voice)
      .await
      .unwrap();

    assert!(!buffer.samples.is_empty());
    assert!(buffer.duration_secs() > 0.5);
  }
}
