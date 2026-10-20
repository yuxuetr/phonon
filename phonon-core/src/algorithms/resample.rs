use crate::{AudioBuffer, PhononError, Result};
#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Resamples the audio buffer to a new sample rate using windowed sinc interpolation.
///
/// Uses a Lanczos-windowed sinc filter for high-quality resampling with minimal
/// aliasing artifacts. The `quality` parameter controls the filter half-width
/// (higher = better quality but slower).
///
/// # Arguments
///
/// * `buffer` - The source audio buffer.
/// * `target_sample_rate` - The desired sample rate in Hz.
///
/// # Returns
///
/// A new `AudioBuffer` with the target sample rate.
pub fn resample(buffer: &AudioBuffer, target_sample_rate: u32) -> Result<AudioBuffer> {
  resample_sinc(buffer, target_sample_rate, 16)
}

/// Resamples using windowed sinc (Lanczos) interpolation with configurable quality.
///
/// * `quality` — sinc filter half-width in samples (typical range 4–64, default 16).
///   Higher values produce better results but are slower.
pub fn resample_sinc(
  buffer: &AudioBuffer,
  target_sample_rate: u32,
  quality: usize,
) -> Result<AudioBuffer> {
  let ch = buffer.frame_stride()?;
  if target_sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  if buffer.sample_rate == target_sample_rate {
    return Ok(buffer.clone());
  }
  if buffer.samples.is_empty() {
    return Ok(AudioBuffer::new(target_sample_rate, buffer.channels, 0));
  }

  let ratio = buffer.sample_rate as f64 / target_sample_rate as f64;
  let old_frames = buffer.samples.len() / ch;
  let new_frames = (old_frames as f64 / ratio).ceil() as usize;
  let a = quality.max(1);

  // When downsampling, the sinc is stretched by `ratio` so its cutoff sits
  // at the new Nyquist; its support grows by the same factor, so the window
  // must cover ±a·ratio source frames or the filter is truncated.
  let filter_scale = if ratio > 1.0 { 1.0 / ratio } else { 1.0 };
  let half_width = a as f64 / filter_scale;

  /// Minimum output frame count before engaging parallel processing.
  #[cfg(feature = "parallel")]
  const PAR_THRESHOLD: usize = 2048;

  let samples = &buffer.samples;

  // Compute one output frame (ch samples) into `out`.
  let compute_frame = |i: usize, out: &mut [f32]| {
    let src_pos = i as f64 * ratio;
    let j_start = (src_pos - half_width).ceil() as i64;
    let j_end = (src_pos + half_width).floor() as i64;

    for c in 0..ch {
      let mut sum = 0.0f64;
      let mut weight_sum = 0.0f64;

      for j in j_start..=j_end {
        if j < 0 || j >= old_frames as i64 {
          continue;
        }
        let x = (src_pos - j as f64) * filter_scale;
        let w = lanczos_kernel(x, a);
        sum += samples[j as usize * ch + c] as f64 * w;
        weight_sum += w;
      }

      out[c] = if weight_sum.abs() > 1e-12 {
        ((sum / weight_sum) as f32).clamp(-1.0, 1.0)
      } else {
        0.0
      };
    }
  };

  let mut new_samples = vec![0.0f32; new_frames * ch];

  #[cfg(feature = "parallel")]
  if new_frames >= PAR_THRESHOLD {
    new_samples
      .par_chunks_mut(ch)
      .enumerate()
      .for_each(|(i, out)| compute_frame(i, out));
  } else {
    for i in 0..new_frames {
      compute_frame(i, &mut new_samples[i * ch..(i + 1) * ch]);
    }
  }
  #[cfg(not(feature = "parallel"))]
  for i in 0..new_frames {
    compute_frame(i, &mut new_samples[i * ch..(i + 1) * ch]);
  }

  Ok(AudioBuffer {
    samples: new_samples,
    sample_rate: target_sample_rate,
    channels: buffer.channels,
  })
}

/// Resamples using simple linear interpolation (fast, lower quality).
///
/// Kept for backward compatibility and for cases where speed matters more than quality.
pub fn resample_linear(buffer: &AudioBuffer, target_sample_rate: u32) -> Result<AudioBuffer> {
  let ch = buffer.frame_stride()?;
  if target_sample_rate == 0 {
    return Err(PhononError::InvalidSampleRate(0));
  }
  if buffer.sample_rate == target_sample_rate {
    return Ok(buffer.clone());
  }
  if buffer.samples.is_empty() {
    return Ok(AudioBuffer::new(target_sample_rate, buffer.channels, 0));
  }

  let ratio = buffer.sample_rate as f64 / target_sample_rate as f64;
  let old_len = buffer.samples.len() / ch;
  let new_len = (old_len as f64 / ratio).ceil() as usize;

  let mut new_samples = Vec::with_capacity(new_len * buffer.channels as usize);

  for i in 0..new_len {
    let old_idx_float = i as f64 * ratio;
    let old_idx_floor = old_idx_float.floor() as usize;
    let old_idx_ceil = (old_idx_floor + 1).min(old_len - 1);
    let t = (old_idx_float - old_idx_floor as f64) as f32;

    for ch in 0..buffer.channels as usize {
      let s1 = buffer.samples[old_idx_floor * buffer.channels as usize + ch];
      let s2 = buffer.samples[old_idx_ceil * buffer.channels as usize + ch];
      let sample = s1 + (s2 - s1) * t;
      new_samples.push(sample);
    }
  }

  Ok(AudioBuffer {
    samples: new_samples,
    sample_rate: target_sample_rate,
    channels: buffer.channels,
  })
}

/// Normalized sinc function: sinc(x) = sin(πx) / (πx), sinc(0) = 1.
fn sinc(x: f64) -> f64 {
  if x.abs() < 1e-12 {
    1.0
  } else {
    let px = std::f64::consts::PI * x;
    px.sin() / px
  }
}

/// Lanczos window kernel: L(x, a) = sinc(x) * sinc(x/a) for |x| < a, else 0.
fn lanczos_kernel(x: f64, a: usize) -> f64 {
  let a_f = a as f64;
  if x.abs() < a_f {
    sinc(x) * sinc(x / a_f)
  } else {
    0.0
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_resample_identity() {
    let buffer = AudioBuffer {
      samples: vec![0.1, 0.2, 0.3],
      sample_rate: 44100,
      channels: 1,
    };
    let resampled = resample(&buffer, 44100).unwrap();
    assert_eq!(resampled.samples, buffer.samples);
  }

  #[test]
  fn test_resample_upsample() {
    // Double the sample rate: 100Hz -> 200Hz with linear (predictable)
    let buffer = AudioBuffer {
      samples: vec![0.0, 1.0],
      sample_rate: 100,
      channels: 1,
    };
    let resampled = resample_linear(&buffer, 200).unwrap();

    assert_eq!(resampled.sample_rate, 200);
    assert_eq!(resampled.samples.len(), 4);

    assert!((resampled.samples[0] - 0.0).abs() < 1e-6);
    assert!((resampled.samples[1] - 0.5).abs() < 1e-6);
    assert!((resampled.samples[2] - 1.0).abs() < 1e-6);
    assert!((resampled.samples[3] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_resample_stereo() {
    let buffer = AudioBuffer {
      samples: vec![0.0, 1.0],
      sample_rate: 100,
      channels: 2,
    };
    let resampled = resample_linear(&buffer, 200).unwrap();

    assert_eq!(resampled.samples.len(), 4);
    assert!((resampled.samples[0] - 0.0).abs() < 1e-6);
    assert!((resampled.samples[1] - 1.0).abs() < 1e-6);
    assert!((resampled.samples[2] - 0.0).abs() < 1e-6);
    assert!((resampled.samples[3] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_sinc_preserves_frequency() {
    // Generate a 440Hz sine wave at 44100Hz, resample to 48000Hz,
    // then resample back. The result should closely match the original.
    let sr = 44100_u32;
    let n = 4410; // 0.1 seconds
    let samples: Vec<f32> = (0..n)
      .map(|i| {
        let t = i as f64 / sr as f64;
        (t * 440.0 * 2.0 * std::f64::consts::PI).sin() as f32
      })
      .collect();

    let original = AudioBuffer {
      samples: samples.clone(),
      sample_rate: sr,
      channels: 1,
    };
    let up = resample_sinc(&original, 48000, 16).unwrap();
    let back = resample_sinc(&up, sr, 16).unwrap();

    // Compare mid-section to avoid edge effects
    let start = 500;
    let end = n.min(back.samples.len()) - 500;
    let max_err = back.samples[start..end]
      .iter()
      .zip(&samples[start..end])
      .map(|(a, b)| (a - b).abs())
      .fold(0.0f32, f32::max);
    assert!(
      max_err < 0.02,
      "Sinc roundtrip max error too high: {max_err}"
    );
  }

  #[test]
  fn test_sinc_vs_linear_quality() {
    // Sinc should have lower error than linear for a pure tone roundtrip
    let sr = 44100_u32;
    let n = 4410;
    let samples: Vec<f32> = (0..n)
      .map(|i| {
        let t = i as f64 / sr as f64;
        (t * 1000.0 * 2.0 * std::f64::consts::PI).sin() as f32
      })
      .collect();
    let original = AudioBuffer {
      samples: samples.clone(),
      sample_rate: sr,
      channels: 1,
    };

    let sinc_up = resample_sinc(&original, 48000, 16).unwrap();
    let sinc_back = resample_sinc(&sinc_up, sr, 16).unwrap();

    let lin_up = resample_linear(&original, 48000).unwrap();
    let lin_back = resample_linear(&lin_up, sr).unwrap();

    let calc_rms_err = |resampled: &[f32]| -> f64 {
      let start = 500;
      let end = n.min(resampled.len()) - 500;
      let (sum, count) = resampled[start..end].iter().zip(&samples[start..end]).fold(
        (0.0f64, 0usize),
        |(sum, count), (a, b)| {
          let err = (*a - *b) as f64;
          (sum + err * err, count + 1)
        },
      );
      (sum / count as f64).sqrt()
    };

    let sinc_err = calc_rms_err(&sinc_back.samples);
    let lin_err = calc_rms_err(&lin_back.samples);

    assert!(
      sinc_err < lin_err,
      "Sinc RMS error ({sinc_err:.6}) should be lower than linear ({lin_err:.6})"
    );
  }

  fn tone(freq: f64, sample_rate: u32, secs: f64) -> AudioBuffer {
    let n = (sample_rate as f64 * secs) as usize;
    AudioBuffer {
      samples: (0..n)
        .map(|i| {
          (2.0 * std::f64::consts::PI * freq * i as f64 / sample_rate as f64).sin() as f32 * 0.5
        })
        .collect(),
      sample_rate,
      channels: 1,
    }
  }

  /// RMS of the middle half, skipping edge transients.
  fn mid_rms(samples: &[f32]) -> f64 {
    let mid = &samples[samples.len() / 4..samples.len() * 3 / 4];
    (mid.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / mid.len() as f64).sqrt()
  }

  #[test]
  fn test_downsample_rejects_content_above_new_nyquist() {
    // 10 kHz is above the 8 kHz Nyquist of the 16 kHz target and aliases
    // to 6 kHz. A filter truncated to ±a source frames leaked ≈ -26 dB here.
    let input = tone(10_000.0, 48_000, 0.5);
    let out = resample(&input, 16_000).unwrap();
    let ratio_db = 20.0 * (mid_rms(&out.samples) / mid_rms(&input.samples)).log10();
    assert!(ratio_db < -60.0, "stopband leakage {ratio_db:.1} dB");
  }

  #[test]
  fn test_downsample_preserves_passband() {
    let input = tone(1_000.0, 48_000, 0.5);
    let out = resample(&input, 16_000).unwrap();
    let ratio_db = 20.0 * (mid_rms(&out.samples) / mid_rms(&input.samples)).log10();
    assert!(ratio_db.abs() < 0.1, "passband gain {ratio_db:.3} dB");
  }

  #[test]
  fn test_resample_zero_channels_errors() {
    let buffer = AudioBuffer {
      samples: vec![0.1; 8],
      sample_rate: 8_000,
      channels: 0,
    };
    assert!(resample(&buffer, 16_000).is_err());
    assert!(resample_linear(&buffer, 16_000).is_err());
  }

  #[test]
  fn test_resample_empty_buffer() {
    let buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    let resampled = resample(&buffer, 48000).unwrap();
    assert!(resampled.samples.is_empty());
    assert_eq!(resampled.sample_rate, 48000);
  }

  #[test]
  fn test_resample_zero_target() {
    let buffer = AudioBuffer {
      samples: vec![1.0],
      sample_rate: 44100,
      channels: 1,
    };
    assert!(resample(&buffer, 0).is_err());
  }
}
