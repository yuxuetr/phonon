use crate::AudioBuffer;

/// Reverses the audio buffer.
///
/// For multi-channel audio, frames are reversed while maintaining
/// the channel order within each frame.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer (not modified).
///
/// # Returns
///
/// A new `AudioBuffer` with the audio reversed.
pub fn reverse(buffer: &AudioBuffer) -> AudioBuffer {
  if buffer.samples.is_empty() || buffer.channels == 0 {
    return buffer.clone();
  }

  let ch = buffer.channels as usize;
  let frame_count = buffer.samples.len() / ch;
  let mut reversed = Vec::with_capacity(buffer.samples.len());

  // Reverse frame order, keep channel order within each frame
  for frame_idx in (0..frame_count).rev() {
    let start = frame_idx * ch;
    for c in 0..ch {
      reversed.push(buffer.samples[start + c]);
    }
  }

  AudioBuffer {
    samples: reversed,
    sample_rate: buffer.sample_rate,
    channels: buffer.channels,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_reverse_mono() {
    let buffer = AudioBuffer {
      samples: vec![0.1, 0.2, 0.3, 0.4, 0.5],
      sample_rate: 44100,
      channels: 1,
    };
    let rev = reverse(&buffer);
    assert_eq!(rev.samples, vec![0.5, 0.4, 0.3, 0.2, 0.1]);
  }

  #[test]
  fn test_reverse_stereo() {
    // Frames: [L1,R1], [L2,R2], [L3,R3]
    let buffer = AudioBuffer {
      samples: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
      sample_rate: 44100,
      channels: 2,
    };
    let rev = reverse(&buffer);
    // Reversed frames: [L3,R3], [L2,R2], [L1,R1]
    assert_eq!(rev.samples, vec![0.5, 0.6, 0.3, 0.4, 0.1, 0.2]);
  }

  #[test]
  fn test_reverse_empty() {
    let buffer = AudioBuffer {
      samples: vec![],
      sample_rate: 44100,
      channels: 1,
    };
    let rev = reverse(&buffer);
    assert!(rev.samples.is_empty());
  }

  #[test]
  fn test_reverse_single_sample() {
    let buffer = AudioBuffer {
      samples: vec![0.5],
      sample_rate: 44100,
      channels: 1,
    };
    let rev = reverse(&buffer);
    assert_eq!(rev.samples, vec![0.5]);
  }

  #[test]
  fn test_reverse_double_reverse_identity() {
    let buffer = AudioBuffer {
      samples: vec![0.1, 0.2, 0.3, 0.4],
      sample_rate: 44100,
      channels: 2,
    };
    let double_rev = reverse(&reverse(&buffer));
    assert_eq!(double_rev.samples, buffer.samples);
  }
}
