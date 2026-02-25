use crate::{AudioBuffer, PhononError, Result};

/// Generates a silent `AudioBuffer` of the specified duration.
///
/// # Arguments
///
/// * `duration_secs` - Duration of silence in seconds. Must be non-negative.
/// * `sample_rate` - Sample rate in Hz (e.g. 44100).
/// * `channels` - Number of channels (1 = Mono, 2 = Stereo).
///
/// # Returns
///
/// An `AudioBuffer` filled with zeros (silence).
///
/// # Errors
///
/// Returns an error if `sample_rate` is 0 or `channels` is 0.
pub fn silence(duration_secs: f64, sample_rate: u32, channels: u16) -> Result<AudioBuffer> {
  if sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  if channels == 0 {
    return Err(PhononError::ChannelMismatch {
      expected: 1,
      actual: 0,
    });
  }
  if duration_secs <= 0.0 {
    return Ok(AudioBuffer {
      samples: Vec::new(),
      sample_rate,
      channels,
    });
  }

  let total_samples = (duration_secs * sample_rate as f64 * channels as f64) as usize;

  Ok(AudioBuffer {
    samples: vec![0.0; total_samples],
    sample_rate,
    channels,
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_silence_basic() {
    let buf = silence(1.0, 44100, 1).unwrap();
    assert_eq!(buf.samples.len(), 44100);
    assert_eq!(buf.sample_rate, 44100);
    assert_eq!(buf.channels, 1);
    assert!(buf.samples.iter().all(|&s| s == 0.0));
  }

  #[test]
  fn test_silence_stereo() {
    let buf = silence(0.5, 44100, 2).unwrap();
    // 0.5s * 44100 * 2 channels = 44100
    assert_eq!(buf.samples.len(), 44100);
    assert!((buf.duration_secs() - 0.5).abs() < 0.001);
  }

  #[test]
  fn test_silence_zero_duration() {
    let buf = silence(0.0, 44100, 1).unwrap();
    assert!(buf.samples.is_empty());
  }

  #[test]
  fn test_silence_negative_duration() {
    let buf = silence(-1.0, 44100, 1).unwrap();
    assert!(buf.samples.is_empty());
  }

  #[test]
  fn test_silence_zero_sample_rate() {
    assert!(silence(1.0, 0, 1).is_err());
  }

  #[test]
  fn test_silence_zero_channels() {
    assert!(silence(1.0, 44100, 0).is_err());
  }
}
