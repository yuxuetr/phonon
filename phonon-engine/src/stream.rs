//! Streaming playback helpers.
//!
//! Bridges a stream of [`AudioBuffer`] chunks (e.g. produced by
//! [`phonon_ai::TtsProvider::synthesize_streaming`]) into an [`AudioPlayer`],
//! handling sample-rate and channel conversion so chunks from a 24 kHz mono
//! TTS endpoint can be played live on a 48 kHz stereo device.
//!
//! # Example
//!
//! ```rust,no_run
//! use phonon_engine::{AudioPlayer, stream::play_stream};
//! use phonon_core::AudioBuffer;
//! use tokio::sync::mpsc;
//!
//! # async fn doc() -> phonon_engine::Result<()> {
//! let (tx, rx) = mpsc::channel::<AudioBuffer>(8);
//! let player = AudioPlayer::new()?;
//!
//! tokio::spawn(async move {
//!   // …push TTS chunks into `tx` here…
//!   drop(tx);
//! });
//!
//! play_stream(&player, rx).await?;
//! # Ok(())
//! # }
//! ```

use crate::{AudioPlayer, EngineError, Result};
use phonon_core::{AudioBuffer, ensure_channels, resample};
use tokio::sync::mpsc;
use tracing::{debug, info};

/// Convert an incoming chunk to the device's sample-rate and channel layout
/// and return the interleaved samples ready to be appended to the player.
pub fn convert_for_device(
  buffer: &AudioBuffer,
  device_sample_rate: u32,
  device_channels: u16,
) -> Result<Vec<f32>> {
  if buffer.samples.is_empty() {
    return Ok(Vec::new());
  }

  let resampled = if buffer.sample_rate == device_sample_rate {
    buffer.clone()
  } else {
    resample(buffer, device_sample_rate).map_err(|e| EngineError::Config(e.to_string()))?
  };

  let converted = if resampled.channels == device_channels {
    resampled
  } else {
    ensure_channels(&resampled, device_channels).map_err(|e| EngineError::Config(e.to_string()))?
  };

  Ok(converted.samples)
}

/// Pipe a receiver of [`AudioBuffer`] chunks into an [`AudioPlayer`].
///
/// Each chunk is resampled and re-channeled to match the player's device
/// format and appended via [`AudioPlayer::append_samples`]. When the sender
/// half is dropped, [`AudioPlayer::finish_streaming`] is called so the
/// player auto-stops once playback drains the buffer.
pub async fn play_stream(player: &AudioPlayer, mut rx: mpsc::Receiver<AudioBuffer>) -> Result<()> {
  let device_sample_rate = player.device_sample_rate();
  let device_channels = player.device_channels();
  let mut chunks = 0usize;
  let mut total_samples = 0usize;

  while let Some(buffer) = rx.recv().await {
    if buffer.samples.is_empty() {
      continue;
    }
    let samples = convert_for_device(&buffer, device_sample_rate, device_channels)?;
    if samples.is_empty() {
      continue;
    }
    total_samples += samples.len();
    chunks += 1;
    player.append_samples(&samples)?;
    debug!(chunks, total_samples, "Appended streamed chunk");
  }

  player.finish_streaming();
  info!(chunks, total_samples, "Stream finished");
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn convert_passthrough_when_format_matches() {
    let buf = AudioBuffer {
      samples: vec![0.1, -0.1, 0.2, -0.2],
      sample_rate: 48_000,
      channels: 2,
    };
    let out = convert_for_device(&buf, 48_000, 2).expect("convert");
    assert_eq!(out, buf.samples);
  }

  #[test]
  fn convert_mono_to_stereo_keeps_sample_rate() {
    let buf = AudioBuffer {
      samples: vec![0.5, -0.5, 0.25],
      sample_rate: 48_000,
      channels: 1,
    };
    let out = convert_for_device(&buf, 48_000, 2).expect("convert");
    assert_eq!(out, vec![0.5, 0.5, -0.5, -0.5, 0.25, 0.25]);
  }

  #[test]
  fn convert_resamples_24k_to_48k_doubles_frame_count() {
    // 24 kHz mono → 48 kHz mono should roughly double the frame count.
    let n = 240; // 10 ms @ 24 kHz
    let buf = AudioBuffer {
      samples: (0..n).map(|i| ((i as f32) / n as f32) - 0.5).collect(),
      sample_rate: 24_000,
      channels: 1,
    };
    let out = convert_for_device(&buf, 48_000, 1).expect("convert");
    // Allow ±1 frame tolerance from ceiling/sinc edge handling.
    assert!(
      (out.len() as i64 - 2 * n as i64).abs() <= 2,
      "expected ~{} samples, got {}",
      2 * n,
      out.len()
    );
  }

  #[test]
  fn convert_24k_mono_to_48k_stereo() {
    let n = 120;
    let buf = AudioBuffer {
      samples: (0..n).map(|i| (i as f32) * 0.001).collect(),
      sample_rate: 24_000,
      channels: 1,
    };
    let out = convert_for_device(&buf, 48_000, 2).expect("convert");
    // Each output frame holds 2 samples (stereo).
    assert_eq!(out.len() % 2, 0);
    assert!(out.len() >= 2 * n); // doubled frames × 2 channels minus edge tolerance
  }

  #[test]
  fn convert_empty_buffer_yields_empty() {
    let buf = AudioBuffer {
      samples: vec![],
      sample_rate: 24_000,
      channels: 1,
    };
    let out = convert_for_device(&buf, 48_000, 2).expect("convert");
    assert!(out.is_empty());
  }
}
