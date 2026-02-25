use crate::AudioBuffer;

/// Repeats the audio buffer the specified number of times.
///
/// Equivalent to pydub's `sound * count`.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer (not modified).
/// * `count` - Number of times to repeat. 0 returns an empty buffer, 1 returns a clone.
///
/// # Returns
///
/// A new `AudioBuffer` containing the audio repeated `count` times.
pub fn repeat(buffer: &AudioBuffer, count: usize) -> AudioBuffer {
  if count == 0 || buffer.samples.is_empty() {
    return AudioBuffer {
      samples: Vec::new(),
      sample_rate: buffer.sample_rate,
      channels: buffer.channels,
    };
  }

  if count == 1 {
    return buffer.clone();
  }

  let total_len = buffer.samples.len() * count;
  let mut samples = Vec::with_capacity(total_len);

  for _ in 0..count {
    samples.extend_from_slice(&buffer.samples);
  }

  AudioBuffer {
    samples,
    sample_rate: buffer.sample_rate,
    channels: buffer.channels,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_repeat_twice() {
    let buffer = AudioBuffer {
      samples: vec![0.1, 0.2, 0.3],
      sample_rate: 44100,
      channels: 1,
    };
    let repeated = repeat(&buffer, 2);
    assert_eq!(repeated.samples, vec![0.1, 0.2, 0.3, 0.1, 0.2, 0.3]);
  }

  #[test]
  fn test_repeat_three_times() {
    let buffer = AudioBuffer {
      samples: vec![0.5, -0.5],
      sample_rate: 44100,
      channels: 2,
    };
    let repeated = repeat(&buffer, 3);
    assert_eq!(repeated.samples.len(), 6);
    assert_eq!(repeated.samples, vec![0.5, -0.5, 0.5, -0.5, 0.5, -0.5]);
  }

  #[test]
  fn test_repeat_zero() {
    let buffer = AudioBuffer {
      samples: vec![0.5; 10],
      sample_rate: 44100,
      channels: 1,
    };
    let repeated = repeat(&buffer, 0);
    assert!(repeated.samples.is_empty());
  }

  #[test]
  fn test_repeat_one_is_clone() {
    let buffer = AudioBuffer {
      samples: vec![0.1, 0.2],
      sample_rate: 44100,
      channels: 1,
    };
    let repeated = repeat(&buffer, 1);
    assert_eq!(repeated.samples, buffer.samples);
  }

  #[test]
  fn test_repeat_empty_buffer() {
    let buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    let repeated = repeat(&buffer, 5);
    assert!(repeated.samples.is_empty());
  }

  #[test]
  fn test_repeat_preserves_metadata() {
    let buffer = AudioBuffer {
      samples: vec![0.5],
      sample_rate: 48000,
      channels: 2,
    };
    let repeated = repeat(&buffer, 3);
    assert_eq!(repeated.sample_rate, 48000);
    assert_eq!(repeated.channels, 2);
  }
}
