//! Signal generators for test tones, beeps, and audio effects.
//!
//! Generates common waveforms as `AudioBuffer`:
//! - Sine wave
//! - Square wave
//! - Sawtooth wave
//! - White noise

use crate::{AudioBuffer, PhononError};

/// Waveform type for signal generation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Waveform {
  /// Pure sine wave.
  Sine,
  /// Square wave (alternating +1 / -1).
  Square,
  /// Sawtooth wave (linear ramp -1 → +1).
  Sawtooth,
  /// White noise (uniform random in [-1, 1]).
  WhiteNoise,
}

/// Generates a mono audio signal with the specified waveform.
///
/// # Arguments
///
/// * `waveform` - The type of waveform to generate.
/// * `frequency` - Frequency in Hz (ignored for `WhiteNoise`).
/// * `duration_secs` - Duration in seconds.
/// * `sample_rate` - Sample rate in Hz.
/// * `amplitude` - Peak amplitude in [0.0, 1.0].
///
/// # Example
///
/// ```
/// use phonon_core::{generate_signal, Waveform};
///
/// let beep = generate_signal(Waveform::Sine, 440.0, 0.5, 44100, 0.8).unwrap();
/// assert_eq!(beep.sample_rate, 44100);
/// assert_eq!(beep.channels, 1);
/// ```
pub fn generate_signal(
  waveform: Waveform,
  frequency: f64,
  duration_secs: f64,
  sample_rate: u32,
  amplitude: f32,
) -> crate::Result<AudioBuffer> {
  if sample_rate == 0 {
    return Err(PhononError::InvalidParam(
      "Sample rate must be > 0".to_string(),
    ));
  }
  if duration_secs < 0.0 {
    return Err(PhononError::InvalidParam(
      "Duration must be >= 0".to_string(),
    ));
  }
  if frequency < 0.0 {
    return Err(PhononError::InvalidParam(
      "Frequency must be >= 0".to_string(),
    ));
  }

  let total_samples = (duration_secs * sample_rate as f64) as usize;
  let amplitude = amplitude.clamp(0.0, 1.0);

  let samples = match waveform {
    Waveform::Sine => generate_sine(frequency, sample_rate, total_samples, amplitude),
    Waveform::Square => generate_square(frequency, sample_rate, total_samples, amplitude),
    Waveform::Sawtooth => generate_sawtooth(frequency, sample_rate, total_samples, amplitude),
    Waveform::WhiteNoise => generate_white_noise(total_samples, amplitude),
  };

  Ok(AudioBuffer {
    samples,
    sample_rate,
    channels: 1,
  })
}

/// Generates a sine wave tone (convenience wrapper).
///
/// 440 Hz = A4 (concert pitch).
pub fn generate_tone(
  frequency: f64,
  duration_secs: f64,
  sample_rate: u32,
) -> crate::Result<AudioBuffer> {
  generate_signal(Waveform::Sine, frequency, duration_secs, sample_rate, 0.8)
}

/// Generates a short beep sound (sine wave at 1000 Hz, 0.2 seconds).
pub fn generate_beep(sample_rate: u32) -> crate::Result<AudioBuffer> {
  generate_signal(Waveform::Sine, 1000.0, 0.2, sample_rate, 0.6)
}

fn generate_sine(freq: f64, sr: u32, len: usize, amp: f32) -> Vec<f32> {
  let sr = sr as f64;
  (0..len)
    .map(|i| {
      let t = i as f64 / sr;
      (t * freq * std::f64::consts::TAU).sin() as f32 * amp
    })
    .collect()
}

fn generate_square(freq: f64, sr: u32, len: usize, amp: f32) -> Vec<f32> {
  let sr = sr as f64;
  (0..len)
    .map(|i| {
      let t = i as f64 / sr;
      let phase = (t * freq).fract();
      if phase < 0.5 { amp } else { -amp }
    })
    .collect()
}

fn generate_sawtooth(freq: f64, sr: u32, len: usize, amp: f32) -> Vec<f32> {
  let sr = sr as f64;
  (0..len)
    .map(|i| {
      let t = i as f64 / sr;
      let phase = (t * freq).fract();
      // Ramp from -1 to +1 across each period
      (2.0 * phase as f32 - 1.0) * amp
    })
    .collect()
}

fn generate_white_noise(len: usize, amp: f32) -> Vec<f32> {
  // Simple xorshift64 PRNG — fast, decent distribution, deterministic.
  let mut state: u64 = 0xDEAD_BEEF_CAFE_BABE;
  (0..len)
    .map(|_| {
      state ^= state << 13;
      state ^= state >> 7;
      state ^= state << 17;
      // Map upper 32 bits to [-1.0, 1.0]
      let u = (state >> 32) as f32 / u32::MAX as f32; // [0.0, 1.0)
      (u * 2.0 - 1.0) * amp
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_generate_sine() {
    let buf =
      generate_signal(Waveform::Sine, 440.0, 1.0, 44100, 0.8).expect("Should generate sine wave");
    assert_eq!(buf.sample_rate, 44100);
    assert_eq!(buf.channels, 1);
    assert_eq!(buf.samples.len(), 44100);
    // Peak should be close to amplitude
    let peak = buf.samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    assert!((peak - 0.8).abs() < 0.01, "Peak={peak}, expected ~0.8");
  }

  #[test]
  fn test_generate_square() {
    let buf = generate_signal(Waveform::Square, 100.0, 0.1, 44100, 1.0)
      .expect("Should generate square wave");
    // All samples should be either +1 or -1
    assert!(
      buf
        .samples
        .iter()
        .all(|&s| (s - 1.0).abs() < f32::EPSILON || (s + 1.0).abs() < f32::EPSILON)
    );
  }

  #[test]
  fn test_generate_sawtooth() {
    let buf = generate_signal(Waveform::Sawtooth, 100.0, 0.01, 44100, 1.0)
      .expect("Should generate sawtooth wave");
    assert!(!buf.samples.is_empty());
    // Should have values across the range [-1, 1]
    let min = buf.samples.iter().cloned().fold(f32::INFINITY, f32::min);
    let max = buf
      .samples
      .iter()
      .cloned()
      .fold(f32::NEG_INFINITY, f32::max);
    assert!(min < -0.5);
    assert!(max > 0.5);
  }

  #[test]
  fn test_generate_white_noise() {
    let buf =
      generate_signal(Waveform::WhiteNoise, 0.0, 1.0, 44100, 1.0).expect("Should generate noise");
    assert_eq!(buf.samples.len(), 44100);
    // Should not be all zeros
    let sum: f32 = buf.samples.iter().map(|s| s.abs()).sum();
    assert!(sum > 0.0);
    // Mean should be roughly near 0 (random)
    let mean: f32 = buf.samples.iter().sum::<f32>() / buf.samples.len() as f32;
    assert!(mean.abs() < 0.1, "Noise mean should be ~0, got {mean}");
  }

  #[test]
  fn test_generate_tone_convenience() {
    let buf = generate_tone(440.0, 0.5, 44100).expect("Should generate tone");
    assert_eq!(buf.samples.len(), 22050);
    assert_eq!(buf.channels, 1);
  }

  #[test]
  fn test_generate_beep() {
    let buf = generate_beep(44100).expect("Should generate beep");
    assert_eq!(buf.samples.len(), 44100 / 5); // 0.2 seconds
  }

  #[test]
  fn test_zero_duration() {
    let buf = generate_signal(Waveform::Sine, 440.0, 0.0, 44100, 1.0)
      .expect("Zero duration should be valid");
    assert!(buf.samples.is_empty());
  }

  #[test]
  fn test_invalid_sample_rate() {
    let result = generate_signal(Waveform::Sine, 440.0, 1.0, 0, 1.0);
    assert!(result.is_err());
  }

  #[test]
  fn test_invalid_negative_duration() {
    let result = generate_signal(Waveform::Sine, 440.0, -1.0, 44100, 1.0);
    assert!(result.is_err());
  }

  #[test]
  fn test_amplitude_clamping() {
    let buf =
      generate_signal(Waveform::Sine, 440.0, 0.01, 44100, 5.0).expect("Should clamp amplitude");
    let peak = buf.samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    // Clamped to 1.0
    assert!(peak <= 1.001, "Peak={peak}, should be <= 1.0");
  }

  #[test]
  fn test_sine_zero_crossings() {
    // A 10Hz sine at 44100 samples/sec for 1 second should cross zero 20 times
    let buf = generate_signal(Waveform::Sine, 10.0, 1.0, 44100, 1.0).expect("Should generate");
    let mut crossings = 0;
    for i in 1..buf.samples.len() {
      if (buf.samples[i] > 0.0) != (buf.samples[i - 1] > 0.0) {
        crossings += 1;
      }
    }
    // 10Hz = 10 cycles = 20 zero crossings per second
    assert_eq!(
      crossings, 20,
      "10Hz sine should cross zero 20 times in 1s, got {crossings}"
    );
  }
}
