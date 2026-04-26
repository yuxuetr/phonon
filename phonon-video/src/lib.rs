//! # phonon-video
//!
//! Video audio track extraction and replacement using ffmpeg CLI.
//!
//! This crate wraps the `ffmpeg` command-line tool to provide:
//!
//! - **Audio extraction**: Extract audio tracks from video files into `AudioBuffer`.
//! - **Audio replacement**: Replace a video's audio track with a new one.
//! - **Probe**: Inspect video/audio stream metadata.
//!
//! ## Requirements
//!
//! The `ffmpeg` and `ffprobe` commands must be available on `$PATH`.
//!
//! ## Example
//!
//! ```rust,no_run
//! use phonon_video::{extract_audio, replace_audio, probe_streams};
//!
//! # async fn example() -> Result<(), phonon_video::VideoError> {
//! // Extract audio from a video file
//! let buffer = extract_audio("input.mp4", None).await?;
//! println!("Extracted {} seconds of audio", buffer.duration_secs());
//!
//! // Replace the audio track
//! replace_audio("input.mp4", "new_audio.wav", "output.mp4").await?;
//!
//! // Inspect streams
//! let streams = probe_streams("input.mp4").await?;
//! for s in &streams {
//!     println!("{}: {}", s.codec_type, s.codec_name);
//! }
//! # Ok(())
//! # }
//! ```

use phonon_core::AudioBuffer;
use serde::Deserialize;
use std::path::Path;
use std::process::Stdio;
use thiserror::Error;
use tracing::{info, instrument};

/// Errors from video processing operations.
#[derive(Error, Debug)]
pub enum VideoError {
  /// ffmpeg/ffprobe not found or failed to execute.
  #[error("ffmpeg error: {0}")]
  Ffmpeg(String),

  /// Audio decoding/encoding error.
  #[error("Audio error: {0}")]
  Audio(String),

  /// I/O error.
  #[error("IO error: {0}")]
  Io(String),

  /// Invalid input or unsupported format.
  #[error("Invalid input: {0}")]
  InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, VideoError>;

// ---------------------------------------------------------------------------
// Stream probe
// ---------------------------------------------------------------------------

/// Metadata for a single stream in a media file.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamInfo {
  /// Stream index.
  pub index: usize,
  /// Codec type: "video", "audio", "subtitle", etc.
  pub codec_type: String,
  /// Codec name (e.g. "h264", "aac", "mp3").
  pub codec_name: String,
  /// Sample rate (audio only).
  #[serde(default)]
  pub sample_rate: Option<String>,
  /// Number of channels (audio only).
  #[serde(default)]
  pub channels: Option<u16>,
  /// Duration in seconds (may be missing for some containers).
  #[serde(default)]
  pub duration: Option<String>,
  /// Bit rate (if available).
  #[serde(default)]
  pub bit_rate: Option<String>,
}

/// ffprobe JSON output structure.
#[derive(Deserialize)]
struct FfprobeOutput {
  streams: Vec<StreamInfo>,
}

/// Check that ffmpeg is available on PATH.
pub async fn check_ffmpeg() -> Result<()> {
  let output = tokio::process::Command::new("ffmpeg")
    .arg("-version")
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .await
    .map_err(|e| VideoError::Ffmpeg(format!("ffmpeg not found: {e}")))?;

  if !output.status.success() {
    return Err(VideoError::Ffmpeg("ffmpeg -version failed".into()));
  }

  Ok(())
}

/// Probe the streams in a media file using `ffprobe`.
#[instrument(skip_all, fields(path = %path.as_ref().display()))]
pub async fn probe_streams(path: impl AsRef<Path>) -> Result<Vec<StreamInfo>> {
  let path = path.as_ref();
  if !path.exists() {
    return Err(VideoError::InvalidInput(format!(
      "File does not exist: {}",
      path.display()
    )));
  }

  let output = tokio::process::Command::new("ffprobe")
    .args(["-v", "quiet", "-print_format", "json", "-show_streams"])
    .arg(path)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .await
    .map_err(|e| VideoError::Ffmpeg(format!("ffprobe failed: {e}")))?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!("ffprobe error: {stderr}")));
  }

  let result: FfprobeOutput = serde_json::from_slice(&output.stdout)
    .map_err(|e| VideoError::Ffmpeg(format!("Failed to parse ffprobe output: {e}")))?;

  info!(streams = result.streams.len(), "Probed media streams");

  Ok(result.streams)
}

// ---------------------------------------------------------------------------
// Audio extraction
// ---------------------------------------------------------------------------

/// Extract the audio track from a video file into an `AudioBuffer`.
///
/// Uses ffmpeg to extract audio as WAV, then reads it via phonon-io.
///
/// # Arguments
///
/// * `video_path` - Path to the input video file.
/// * `sample_rate` - Optional target sample rate. If `None`, uses the original.
#[instrument(skip_all, fields(video = %video_path.as_ref().display()))]
pub async fn extract_audio(
  video_path: impl AsRef<Path>,
  sample_rate: Option<u32>,
) -> Result<AudioBuffer> {
  let video_path = video_path.as_ref();
  if !video_path.exists() {
    return Err(VideoError::InvalidInput(format!(
      "Video file does not exist: {}",
      video_path.display()
    )));
  }

  let tmp = tempfile::Builder::new()
    .suffix(".wav")
    .tempfile()
    .map_err(|e| VideoError::Io(e.to_string()))?;

  let tmp_path = tmp.path().to_path_buf();

  let mut args = vec![
    "-i".to_string(),
    video_path.display().to_string(),
    "-vn".to_string(), // No video
    "-acodec".to_string(),
    "pcm_s16le".to_string(), // 16-bit PCM WAV
  ];

  if let Some(sr) = sample_rate {
    args.push("-ar".to_string());
    args.push(sr.to_string());
  }

  args.push("-y".to_string()); // Overwrite
  args.push(tmp_path.display().to_string());

  info!("Extracting audio from video via ffmpeg");

  let output = tokio::process::Command::new("ffmpeg")
    .args(&args)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .await
    .map_err(|e| VideoError::Ffmpeg(format!("ffmpeg failed: {e}")))?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!(
      "ffmpeg extraction error: {stderr}"
    )));
  }

  let buffer = phonon_io::read_audio(&tmp_path).map_err(|e| VideoError::Audio(e.to_string()))?;

  info!(
    duration = buffer.duration_secs(),
    sample_rate = buffer.sample_rate,
    channels = buffer.channels,
    "Audio extraction complete"
  );

  Ok(buffer)
}

/// Extract the audio track and save directly to a file.
#[instrument(skip_all, fields(video = %video_path.as_ref().display(), output = %output_path.as_ref().display()))]
pub async fn extract_audio_to_file(
  video_path: impl AsRef<Path>,
  output_path: impl AsRef<Path>,
) -> Result<()> {
  let video_path = video_path.as_ref();
  let output_path = output_path.as_ref();

  if !video_path.exists() {
    return Err(VideoError::InvalidInput(format!(
      "Video file does not exist: {}",
      video_path.display()
    )));
  }

  let output = tokio::process::Command::new("ffmpeg")
    .args([
      "-i",
      &video_path.display().to_string(),
      "-vn",
      "-y",
      &output_path.display().to_string(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .await
    .map_err(|e| VideoError::Ffmpeg(format!("ffmpeg failed: {e}")))?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!("ffmpeg error: {stderr}")));
  }

  info!("Audio extracted to file");
  Ok(())
}

// ---------------------------------------------------------------------------
// Audio replacement
// ---------------------------------------------------------------------------

/// Replace the audio track of a video file with a new audio file.
///
/// The video stream is copied without re-encoding. The audio is re-encoded
/// using AAC by default.
///
/// # Arguments
///
/// * `video_path` - Input video file.
/// * `audio_path` - New audio file (WAV, MP3, etc.).
/// * `output_path` - Output video file path.
#[instrument(skip_all, fields(
    video = %video_path.as_ref().display(),
    audio = %audio_path.as_ref().display(),
    output = %output_path.as_ref().display()
))]
pub async fn replace_audio(
  video_path: impl AsRef<Path>,
  audio_path: impl AsRef<Path>,
  output_path: impl AsRef<Path>,
) -> Result<()> {
  let video_path = video_path.as_ref();
  let audio_path = audio_path.as_ref();
  let output_path = output_path.as_ref();

  if !video_path.exists() {
    return Err(VideoError::InvalidInput(format!(
      "Video file does not exist: {}",
      video_path.display()
    )));
  }
  if !audio_path.exists() {
    return Err(VideoError::InvalidInput(format!(
      "Audio file does not exist: {}",
      audio_path.display()
    )));
  }

  info!("Replacing audio track in video");

  let output = tokio::process::Command::new("ffmpeg")
    .args([
      "-i",
      &video_path.display().to_string(),
      "-i",
      &audio_path.display().to_string(),
      "-c:v",
      "copy", // Copy video stream without re-encoding
      "-map",
      "0:v:0", // Take video from first input
      "-map",
      "1:a:0",     // Take audio from second input
      "-shortest", // End when shorter stream ends
      "-y",
      &output_path.display().to_string(),
    ])
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .output()
    .await
    .map_err(|e| VideoError::Ffmpeg(format!("ffmpeg failed: {e}")))?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    return Err(VideoError::Ffmpeg(format!(
      "ffmpeg replace error: {stderr}"
    )));
  }

  info!("Audio track replaced successfully");
  Ok(())
}

/// Replace the audio track using an `AudioBuffer` instead of a file.
///
/// The buffer is first written to a temporary WAV file, then used for replacement.
#[instrument(skip_all, fields(
    video = %video_path.as_ref().display(),
    output = %output_path.as_ref().display(),
    buffer_duration = buffer.duration_secs()
))]
pub async fn replace_audio_with_buffer(
  video_path: impl AsRef<Path>,
  buffer: &AudioBuffer,
  output_path: impl AsRef<Path>,
) -> Result<()> {
  let tmp = tempfile::Builder::new()
    .suffix(".wav")
    .tempfile()
    .map_err(|e| VideoError::Io(e.to_string()))?;

  phonon_io::write_audio(tmp.path(), buffer).map_err(|e| VideoError::Audio(e.to_string()))?;

  replace_audio(video_path, tmp.path(), output_path).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_stream_info_deserialization() {
    let json = r#"{
            "index": 0,
            "codec_type": "video",
            "codec_name": "h264",
            "sample_rate": null,
            "channels": null,
            "duration": "120.5",
            "bit_rate": "5000000"
        }"#;
    let info: StreamInfo = serde_json::from_str(json).expect("parse stream info");
    assert_eq!(info.index, 0);
    assert_eq!(info.codec_type, "video");
    assert_eq!(info.codec_name, "h264");
    assert!(info.sample_rate.is_none());
    assert!(info.channels.is_none());
    assert_eq!(info.duration.as_deref(), Some("120.5"));
  }

  #[test]
  fn test_stream_info_audio() {
    let json = r#"{
            "index": 1,
            "codec_type": "audio",
            "codec_name": "aac",
            "sample_rate": "44100",
            "channels": 2,
            "duration": "120.5",
            "bit_rate": "128000"
        }"#;
    let info: StreamInfo = serde_json::from_str(json).expect("parse audio stream");
    assert_eq!(info.codec_type, "audio");
    assert_eq!(info.sample_rate.as_deref(), Some("44100"));
    assert_eq!(info.channels, Some(2));
  }

  #[test]
  fn test_ffprobe_output_deserialization() {
    let json = r#"{
            "streams": [
                {"index": 0, "codec_type": "video", "codec_name": "h264"},
                {"index": 1, "codec_type": "audio", "codec_name": "aac", "sample_rate": "48000", "channels": 2}
            ]
        }"#;
    let output: FfprobeOutput = serde_json::from_str(json).expect("parse ffprobe output");
    assert_eq!(output.streams.len(), 2);
    assert_eq!(output.streams[0].codec_type, "video");
    assert_eq!(output.streams[1].codec_type, "audio");
  }

  #[tokio::test]
  async fn test_extract_audio_nonexistent_file() {
    let result = extract_audio("/nonexistent/video.mp4", None).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("does not exist"));
  }

  #[tokio::test]
  async fn test_replace_audio_nonexistent_video() {
    let result = replace_audio(
      "/nonexistent/video.mp4",
      "/nonexistent/audio.wav",
      "/tmp/output.mp4",
    )
    .await;
    assert!(result.is_err());
  }

  #[tokio::test]
  async fn test_probe_nonexistent_file() {
    let result = probe_streams("/nonexistent/video.mp4").await;
    assert!(result.is_err());
  }
}
