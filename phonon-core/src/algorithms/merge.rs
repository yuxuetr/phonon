use crate::{AudioBuffer, PhononError, Result};

/// Merges multiple audio buffers sequentially.
///
/// # Arguments
///
/// * `buffers` - A slice of audio buffers to merge.
///
/// # Returns
///
/// A new `AudioBuffer` containing the concatenated audio.
///
/// # Errors
///
/// Returns an error if buffers have different sample rates or channel counts.
/// Returns an empty buffer if the input list is empty.
pub fn merge(buffers: &[AudioBuffer]) -> Result<AudioBuffer> {
    if buffers.is_empty() {
        return Ok(AudioBuffer::new(0, 0, 0));
    }

    let first = &buffers[0];
    let sample_rate = first.sample_rate;
    let channels = first.channels;

    // Verify all buffers match sample rate and channels
    for buf in buffers.iter().skip(1) {
        if buf.sample_rate != sample_rate {
            return Err(PhononError::InvalidSampleRate(buf.sample_rate)); // Could be more descriptive
        }
        if buf.channels != channels {
            return Err(PhononError::ChannelMismatch {
                expected: channels,
                actual: buf.channels,
            });
        }
    }

    // Calculate total size to allocate once
    let total_samples: usize = buffers.iter().map(|b| b.samples.len()).sum();
    let mut merged_samples = Vec::with_capacity(total_samples);

    for buf in buffers {
        merged_samples.extend_from_slice(&buf.samples);
    }

    Ok(AudioBuffer {
        samples: merged_samples,
        sample_rate,
        channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge() {
        let b1 = AudioBuffer {
            samples: vec![1.0, 1.0],
            sample_rate: 44100,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5, 0.5],
            sample_rate: 44100,
            channels: 1,
        };
        let b3 = AudioBuffer {
            samples: vec![0.0],
            sample_rate: 44100,
            channels: 1,
        };

        let merged = merge(&[b1, b2, b3]).unwrap();

        assert_eq!(merged.samples.len(), 5);
        assert_eq!(merged.samples, vec![1.0, 1.0, 0.5, 0.5, 0.0]);
        assert_eq!(merged.sample_rate, 44100);
    }

    #[test]
    fn test_merge_mismatch_error() {
        let b1 = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![],
            sample_rate: 48000,
            channels: 1,
        };
        assert!(merge(&[b1, b2]).is_err());
    }

    #[test]
    fn test_merge_empty_list() {
        let merged = merge(&[]).unwrap();
        assert!(merged.samples.is_empty());
        assert_eq!(merged.sample_rate, 0);
    }

    #[test]
    fn test_merge_single_buffer() {
        let b1 = AudioBuffer {
            samples: vec![0.5, 0.3],
            sample_rate: 44100,
            channels: 1,
        };
        let merged = merge(&[b1]).unwrap();
        assert_eq!(merged.samples, vec![0.5, 0.3]);
    }

    #[test]
    fn test_merge_channel_mismatch() {
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
        assert!(merge(&[b1, b2]).is_err());
    }
}
