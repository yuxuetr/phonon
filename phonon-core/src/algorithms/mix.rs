use crate::{AudioBuffer, PhononError, Result};
#[cfg(feature = "parallel")]
use rayon::prelude::*;
use tracing::instrument;

/// Mixes two audio buffers together.
///
/// # Arguments
///
/// * `buffer1` - The first audio buffer.
/// * `buffer2` - The second audio buffer.
///
/// # Returns
///
/// A new `AudioBuffer` containing the mixed audio.
/// The length of the result will be equal to the longer of the two inputs.
///
/// # Errors
///
/// Returns an error if sample rates or channel counts do not match.
///
/// # Example
///
/// ```
/// use phonon_core::AudioBuffer;
/// use phonon_core::algorithms::mix::mix;
///
/// let b1 = AudioBuffer { samples: vec![0.5, 0.3], sample_rate: 44100, channels: 1 };
/// let b2 = AudioBuffer { samples: vec![0.4, 0.2], sample_rate: 44100, channels: 1 };
/// let mixed = mix(&b1, &b2).unwrap();
/// assert!((mixed.samples[0] - 0.9).abs() < 1e-6);
/// ```
#[instrument(skip(buffer1, buffer2))]
pub fn mix(buffer1: &AudioBuffer, buffer2: &AudioBuffer) -> Result<AudioBuffer> {
  if buffer1.sample_rate != buffer2.sample_rate {
    return Err(PhononError::InvalidSampleRate(buffer2.sample_rate)); // Simplify for now
  }
  if buffer1.channels != buffer2.channels {
    return Err(PhononError::ChannelMismatch {
      expected: buffer1.channels,
      actual: buffer2.channels,
    });
  }

  let len1 = buffer1.samples.len();
  let len2 = buffer2.samples.len();
  let max_len = len1.max(len2);

  /// Minimum sample count before engaging parallel processing.
  #[cfg(feature = "parallel")]
  const PAR_THRESHOLD: usize = 131072;

  let mixed_samples = {
    #[cfg(feature = "parallel")]
    if max_len >= PAR_THRESHOLD {
      let s1 = &buffer1.samples;
      let s2 = &buffer2.samples;
      (0..max_len)
        .into_par_iter()
        .map(|i| {
          let v1 = if i < len1 { s1[i] } else { 0.0f32 };
          let v2 = if i < len2 { s2[i] } else { 0.0f32 };
          (v1 + v2).clamp(-1.0, 1.0)
        })
        .collect()
    } else {
      mix_scalar(buffer1, buffer2, len1, len2, max_len)
    }
    #[cfg(not(feature = "parallel"))]
    {
      mix_scalar(buffer1, buffer2, len1, len2, max_len)
    }
  };

  Ok(AudioBuffer {
    samples: mixed_samples,
    sample_rate: buffer1.sample_rate,
    channels: buffer1.channels,
  })
}

/// Scalar (+ optional SIMD) mix of two buffers.
fn mix_scalar(
  buffer1: &AudioBuffer,
  buffer2: &AudioBuffer,
  len1: usize,
  len2: usize,
  max_len: usize,
) -> Vec<f32> {
  let min_len = len1.min(len2);
  let mut out = vec![0.0f32; max_len];

  // Overlapping region: both buffers have data — use SIMD if available.
  #[cfg(feature = "simd")]
  {
    super::simd_ops::simd_add_clamp(
      &buffer1.samples[..min_len],
      &buffer2.samples[..min_len],
      &mut out[..min_len],
    );
  }
  #[cfg(not(feature = "simd"))]
  {
    for i in 0..min_len {
      out[i] = (buffer1.samples[i] + buffer2.samples[i]).clamp(-1.0, 1.0);
    }
  }

  // Tail from the longer buffer.
  if len1 > min_len {
    out[min_len..max_len].copy_from_slice(&buffer1.samples[min_len..len1]);
  } else if len2 > min_len {
    out[min_len..max_len].copy_from_slice(&buffer2.samples[min_len..len2]);
  }

  out
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_mix() {
    let b1 = AudioBuffer {
      samples: vec![0.5, 0.5, 0.5],
      sample_rate: 44100,
      channels: 1,
    };
    let b2 = AudioBuffer {
      samples: vec![0.4, 0.6, 0.0, 0.2], // Longer than b1
      sample_rate: 44100,
      channels: 1,
    };

    let mixed = mix(&b1, &b2).unwrap();

    assert_eq!(mixed.samples.len(), 4);
    assert!((mixed.samples[0] - 0.9).abs() < 1e-6); // 0.5 + 0.4
    assert!((mixed.samples[1] - 1.0).abs() < 1e-6); // 0.5 + 0.6 = 1.1 -> clamped to 1.0
    assert!((mixed.samples[2] - 0.5).abs() < 1e-6); // 0.5 + 0.0
    assert!((mixed.samples[3] - 0.2).abs() < 1e-6); // 0.0 + 0.2 (b1 ended)
  }

  #[test]
  fn test_mix_empty_buffers() {
    let b1 = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    let b2 = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    let mixed = mix(&b1, &b2).unwrap();
    assert!(mixed.samples.is_empty());
  }

  #[test]
  fn test_mix_one_empty() {
    let b1 = AudioBuffer {
      samples: vec![0.5, 0.3],
      sample_rate: 44100,
      channels: 1,
    };
    let b2 = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    let mixed = mix(&b1, &b2).unwrap();
    assert_eq!(mixed.samples.len(), 2);
    assert!((mixed.samples[0] - 0.5).abs() < 1e-6);
  }

  #[test]
  fn test_mix_sample_rate_mismatch() {
    let b1 = AudioBuffer {
      samples: vec![0.5],
      sample_rate: 44100,
      channels: 1,
    };
    let b2 = AudioBuffer {
      samples: vec![0.5],
      sample_rate: 48000,
      channels: 1,
    };
    assert!(mix(&b1, &b2).is_err());
  }

  #[test]
  fn test_mix_channel_mismatch() {
    let b1 = AudioBuffer {
      samples: vec![0.5],
      sample_rate: 44100,
      channels: 1,
    };
    let b2 = AudioBuffer {
      samples: vec![0.5, 0.5],
      sample_rate: 44100,
      channels: 2,
    };
    assert!(mix(&b1, &b2).is_err());
  }
}
