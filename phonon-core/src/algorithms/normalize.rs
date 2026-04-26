use crate::AudioBuffer;
#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Minimum sample count before engaging parallel processing.
#[cfg(feature = "parallel")]
const PAR_THRESHOLD: usize = 131072;

/// Normalizes the audio buffer to a target peak amplitude.
///
/// # Arguments
///
/// * `buffer` - The audio buffer to normalize. The buffer is modified in place.
/// * `target_peak` - The target peak amplitude (usually 1.0 or less).
///
/// # Example
///
/// ```
/// use phonon_core::AudioBuffer;
/// use phonon_core::algorithms::normalize::normalize;
///
/// let mut buffer = AudioBuffer {
///     samples: vec![0.5, -0.2, 0.1],
///     sample_rate: 44100,
///     channels: 1,
/// };
/// normalize(&mut buffer, 1.0);
/// assert!((buffer.samples[0] - 1.0).abs() < 1e-6);
/// ```
pub fn normalize(buffer: &mut AudioBuffer, target_peak: f32) {
  if target_peak <= 0.0 {
    return; // No-op for invalid target
  }

  #[cfg(feature = "parallel")]
  let use_par = buffer.samples.len() >= PAR_THRESHOLD;
  #[cfg(not(feature = "parallel"))]
  let use_par = false;

  // Find the current peak amplitude
  let current_peak = if use_par {
    #[cfg(feature = "parallel")]
    {
      buffer
        .samples
        .par_iter()
        .map(|s| s.abs())
        .reduce(|| 0.0f32, f32::max)
    }
    #[cfg(not(feature = "parallel"))]
    {
      unreachable!()
    }
  } else {
    #[cfg(feature = "simd")]
    {
      super::simd_ops::simd_max_abs(&buffer.samples)
    }
    #[cfg(not(feature = "simd"))]
    {
      buffer
        .samples
        .iter()
        .map(|s| s.abs())
        .fold(0.0f32, f32::max)
    }
  };

  // Avoid division by zero and unnecessary processing
  if current_peak > 0.0 && (current_peak - target_peak).abs() > 1e-6 {
    let gain = target_peak / current_peak;
    if use_par {
      #[cfg(feature = "parallel")]
      buffer.samples.par_iter_mut().for_each(|s| *s *= gain);
    } else {
      #[cfg(feature = "simd")]
      super::simd_ops::simd_mul(&mut buffer.samples, gain);
      #[cfg(not(feature = "simd"))]
      for sample in &mut buffer.samples {
        *sample *= gain;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_normalize() {
    let mut buffer = AudioBuffer {
      samples: vec![0.5, -0.2, 0.1],
      sample_rate: 44100,
      channels: 1,
    };

    normalize(&mut buffer, 1.0);

    // Peak was 0.5, target is 1.0, so gain is 2.0
    assert!((buffer.samples[0] - 1.0).abs() < 1e-6);
    assert!((buffer.samples[1] - (-0.4)).abs() < 1e-6);
    assert!((buffer.samples[2] - 0.2).abs() < 1e-6);
  }

  #[test]
  fn test_normalize_empty_buffer() {
    let mut buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    normalize(&mut buffer, 1.0);
    assert!(buffer.samples.is_empty());
  }

  #[test]
  fn test_normalize_silent_buffer() {
    let mut buffer = AudioBuffer {
      samples: vec![0.0, 0.0, 0.0],
      sample_rate: 44100,
      channels: 1,
    };
    normalize(&mut buffer, 1.0);
    // All zeros, peak is 0.0, no change
    assert!(buffer.samples.iter().all(|&s| s == 0.0));
  }

  #[test]
  fn test_normalize_already_at_target() {
    let mut buffer = AudioBuffer {
      samples: vec![1.0, -0.5],
      sample_rate: 44100,
      channels: 1,
    };
    normalize(&mut buffer, 1.0);
    assert!((buffer.samples[0] - 1.0).abs() < 1e-6);
    assert!((buffer.samples[1] - (-0.5)).abs() < 1e-6);
  }

  #[test]
  fn test_normalize_zero_target() {
    let mut buffer = AudioBuffer {
      samples: vec![0.5, -0.5],
      sample_rate: 44100,
      channels: 1,
    };
    normalize(&mut buffer, 0.0);
    // No-op for invalid target
    assert!((buffer.samples[0] - 0.5).abs() < 1e-6);
  }

  #[test]
  fn test_normalize_negative_target() {
    let mut buffer = AudioBuffer {
      samples: vec![0.5, -0.5],
      sample_rate: 44100,
      channels: 1,
    };
    normalize(&mut buffer, -1.0);
    // No-op for invalid target
    assert!((buffer.samples[0] - 0.5).abs() < 1e-6);
  }
}
