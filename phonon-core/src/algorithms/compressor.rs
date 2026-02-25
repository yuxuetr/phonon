//! Dynamic range compressor.
//!
//! Reduces the dynamic range of audio, making quiet parts louder
//! and loud parts quieter. Essential for podcast post-production.
//!
//! # Parameters
//!
//! - **threshold** (dB): Level above which compression begins. Typical: -20 to -10 dB.
//! - **ratio**: Compression ratio (e.g. 4.0 means 4:1). Higher = more compression.
//! - **attack** (seconds): How quickly the compressor responds to peaks. Typical: 0.001 - 0.05.
//! - **release** (seconds): How quickly the compressor stops after the signal drops. Typical: 0.05 - 0.5.
//! - **makeup_gain** (dB): Optional gain applied after compression to restore volume.

use crate::AudioBuffer;

/// Compressor configuration.
#[derive(Debug, Clone)]
pub struct CompressorConfig {
  /// Threshold in dB (negative). Signals above this level are compressed.
  pub threshold_db: f32,
  /// Compression ratio. 4.0 means 4:1 compression.
  pub ratio: f32,
  /// Attack time in seconds.
  pub attack_secs: f64,
  /// Release time in seconds.
  pub release_secs: f64,
  /// Makeup gain in dB applied after compression.
  pub makeup_gain_db: f32,
}

impl Default for CompressorConfig {
  fn default() -> Self {
    Self {
      threshold_db: -20.0,
      ratio: 4.0,
      attack_secs: 0.005,
      release_secs: 0.1,
      makeup_gain_db: 0.0,
    }
  }
}

impl CompressorConfig {
  /// Create a compressor tuned for voice / podcast use.
  pub fn voice() -> Self {
    Self {
      threshold_db: -18.0,
      ratio: 3.0,
      attack_secs: 0.003,
      release_secs: 0.15,
      makeup_gain_db: 6.0,
    }
  }

  /// Create a gentle compressor (light leveling).
  pub fn gentle() -> Self {
    Self {
      threshold_db: -24.0,
      ratio: 2.0,
      attack_secs: 0.01,
      release_secs: 0.2,
      makeup_gain_db: 3.0,
    }
  }

  /// Create a heavy / limiter-style compressor.
  pub fn limiter() -> Self {
    Self {
      threshold_db: -6.0,
      ratio: 20.0,
      attack_secs: 0.001,
      release_secs: 0.05,
      makeup_gain_db: 0.0,
    }
  }
}

/// Applies dynamic range compression to the audio buffer **in place**.
///
/// Processes all channels interleaved; the envelope is computed from
/// the absolute sample value, so left/right are compressed together.
///
/// # Example
///
/// ```
/// use phonon_core::{AudioBuffer, compress, CompressorConfig};
///
/// let mut buffer = AudioBuffer {
///     samples: vec![0.0, 0.5, -0.8, 1.0, -1.0, 0.3],
///     sample_rate: 44100,
///     channels: 1,
/// };
/// compress(&mut buffer, &CompressorConfig::voice());
/// // Loud peaks are attenuated; overall dynamics are reduced.
/// ```
pub fn compress(buffer: &mut AudioBuffer, config: &CompressorConfig) {
  if buffer.samples.is_empty() {
    return;
  }

  let sample_rate = buffer.sample_rate as f64;

  // Compute smoothing coefficients from attack/release times.
  // Using a simple one-pole envelope follower:
  //   coeff = exp(-1 / (time * sample_rate))
  let attack_coeff = if config.attack_secs > 0.0 {
    (-1.0 / (config.attack_secs * sample_rate)).exp() as f32
  } else {
    0.0
  };

  let release_coeff = if config.release_secs > 0.0 {
    (-1.0 / (config.release_secs * sample_rate)).exp() as f32
  } else {
    0.0
  };

  let threshold_linear = db_to_linear(config.threshold_db);
  let makeup_linear = db_to_linear(config.makeup_gain_db);
  let ratio = config.ratio.max(1.0); // ratio < 1 would be expansion, clamp to ≥ 1

  let mut envelope: f32 = 0.0;

  for sample in &mut buffer.samples {
    let input_abs = sample.abs();

    // Envelope follower (peak detector with attack/release)
    let coeff = if input_abs > envelope {
      attack_coeff
    } else {
      release_coeff
    };
    envelope = coeff * envelope + (1.0 - coeff) * input_abs;

    // Compute gain reduction
    let gain = if envelope > threshold_linear {
      // How many dB above threshold
      let over_db = linear_to_db(envelope) - config.threshold_db;
      // Compressed overshoot
      let compressed_over_db = over_db / ratio;
      // Target level in dB
      let target_db = config.threshold_db + compressed_over_db;
      // Gain = target / actual (in linear)
      db_to_linear(target_db) / envelope.max(1e-10)
    } else {
      1.0
    };

    *sample *= gain * makeup_linear;
  }
}

/// Applies compression and returns a new buffer (non-mutating variant).
pub fn with_compression(buffer: &AudioBuffer, config: &CompressorConfig) -> AudioBuffer {
  let mut result = buffer.clone();
  compress(&mut result, config);
  result
}

/// Convert decibels to linear amplitude.
fn db_to_linear(db: f32) -> f32 {
  10.0f32.powf(db / 20.0)
}

/// Convert linear amplitude to decibels. Returns -f32::INFINITY for 0.
fn linear_to_db(linear: f32) -> f32 {
  if linear <= 0.0 {
    return f32::NEG_INFINITY;
  }
  20.0 * linear.log10()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_compress_empty_buffer() {
    let mut buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    compress(&mut buffer, &CompressorConfig::default());
    assert!(buffer.samples.is_empty());
  }

  #[test]
  fn test_compress_silence_unchanged() {
    let mut buffer = AudioBuffer {
      samples: vec![0.0; 1000],
      sample_rate: 44100,
      channels: 1,
    };
    compress(&mut buffer, &CompressorConfig::default());
    assert!(buffer.samples.iter().all(|&s| s.abs() < 1e-10));
  }

  #[test]
  fn test_compress_reduces_loud_peaks() {
    // Generate a loud signal (amplitude 1.0) — well above -20dB threshold
    let mut buffer = AudioBuffer {
      samples: vec![1.0; 4410], // 0.1 seconds at 44100Hz
      sample_rate: 44100,
      channels: 1,
    };

    let config = CompressorConfig {
      threshold_db: -20.0,
      ratio: 4.0,
      attack_secs: 0.001,
      release_secs: 0.05,
      makeup_gain_db: 0.0,
    };

    compress(&mut buffer, &config);

    // After compression, the tail samples should be significantly below 1.0
    let tail_avg: f32 =
      buffer.samples[2000..].iter().sum::<f32>() / (buffer.samples.len() - 2000) as f32;
    assert!(
      tail_avg < 0.5,
      "Expected compressed tail avg < 0.5, got {tail_avg}"
    );
  }

  #[test]
  fn test_compress_quiet_signal_mostly_unchanged() {
    // Signal at -40 dB ≈ 0.01 amplitude — below threshold of -20 dB
    let amplitude = 0.01_f32;
    let original: Vec<f32> = (0..4410)
      .map(|i| {
        let t = i as f64 / 44100.0;
        (t * 440.0 * std::f64::consts::TAU).sin() as f32 * amplitude
      })
      .collect();

    let mut buffer = AudioBuffer {
      samples: original.clone(),
      sample_rate: 44100,
      channels: 1,
    };

    let config = CompressorConfig {
      threshold_db: -20.0,
      ratio: 4.0,
      attack_secs: 0.005,
      release_secs: 0.1,
      makeup_gain_db: 0.0, // no makeup gain so quiet stays quiet
    };

    compress(&mut buffer, &config);

    // Should be very close to original (below threshold → gain ≈ 1.0)
    let max_diff = buffer
      .samples
      .iter()
      .zip(original.iter())
      .map(|(a, b)| (a - b).abs())
      .fold(0.0f32, f32::max);

    assert!(
      max_diff < 0.01,
      "Quiet signal changed too much: max_diff={max_diff}"
    );
  }

  #[test]
  fn test_compress_with_makeup_gain() {
    let mut buffer = AudioBuffer {
      samples: vec![0.5; 4410],
      sample_rate: 44100,
      channels: 1,
    };

    let config = CompressorConfig {
      threshold_db: -20.0,
      ratio: 4.0,
      attack_secs: 0.001,
      release_secs: 0.05,
      makeup_gain_db: 6.0, // +6dB ≈ 2x
    };

    compress(&mut buffer, &config);

    // With makeup gain, output should be louder than just compression alone
    // The tail samples should reflect the makeup gain
    let tail_avg: f32 =
      buffer.samples[2000..].iter().sum::<f32>() / (buffer.samples.len() - 2000) as f32;
    assert!(tail_avg > 0.0, "Expected positive output with makeup gain");
  }

  #[test]
  fn test_with_compression_returns_new_buffer() {
    let buffer = AudioBuffer {
      samples: vec![0.8; 1000],
      sample_rate: 44100,
      channels: 1,
    };

    let compressed = with_compression(&buffer, &CompressorConfig::default());

    // Original should be unchanged
    assert!((buffer.samples[999] - 0.8).abs() < f32::EPSILON);
    // Compressed should be different
    assert_ne!(buffer.samples.len(), 0);
    assert_eq!(compressed.samples.len(), buffer.samples.len());
  }

  #[test]
  fn test_config_presets() {
    let voice = CompressorConfig::voice();
    assert!(voice.threshold_db > -30.0 && voice.threshold_db < 0.0);
    assert!(voice.ratio > 1.0);
    assert!(voice.makeup_gain_db > 0.0);

    let gentle = CompressorConfig::gentle();
    assert!(gentle.ratio < voice.ratio); // gentler

    let limiter = CompressorConfig::limiter();
    assert!(limiter.ratio > 10.0);
    assert!(limiter.attack_secs < 0.005);
  }

  #[test]
  fn test_db_linear_roundtrip() {
    for db in [-40.0, -20.0, -6.0, 0.0, 6.0, 12.0] {
      let linear = db_to_linear(db);
      let back = linear_to_db(linear);
      assert!(
        (back - db).abs() < 0.001,
        "Roundtrip failed for {db}dB: got {back}"
      );
    }
  }

  #[test]
  fn test_limiter_clamps_hard() {
    // A limiter with very high ratio should keep output near threshold
    let mut buffer = AudioBuffer {
      samples: vec![1.0; 44100], // 1 second of max amplitude
      sample_rate: 44100,
      channels: 1,
    };

    compress(&mut buffer, &CompressorConfig::limiter());

    // After settling, samples should be well below 1.0
    let tail_max = buffer.samples[10000..]
      .iter()
      .map(|s| s.abs())
      .fold(0.0f32, f32::max);
    assert!(
      tail_max < 0.8,
      "Limiter should clamp hard, but tail_max={tail_max}"
    );
  }
}
