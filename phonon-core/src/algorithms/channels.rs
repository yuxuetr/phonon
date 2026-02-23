use crate::{AudioBuffer, PhononError, Result};

/// Converts the audio buffer to a different channel count.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer.
/// * `target_channels` - The desired number of channels.
///
/// # Returns
///
/// A new `AudioBuffer` with the target channel count.
///
/// # Behavior
///
/// * **Mono (1) -> Stereo (2)**: Duplicates the mono sample to both left and right channels.
/// * **Stereo (2) -> Mono (1)**: Averages left and right channels: `(L + R) / 2`.
/// * **N -> M**: Currently not fully implemented.
///   * If M < N: Truncates extra channels (keeps first M).
///   * If M > N: Fills extra channels with silence (0.0).
///   * Exception: 1 -> M (Mono broadcast) fills all M channels with the mono sample.
pub fn ensure_channels(buffer: &AudioBuffer, target_channels: u16) -> Result<AudioBuffer> {
    if target_channels == 0 {
        return Err(PhononError::ChannelMismatch { expected: 1, actual: 0 }); // Cannot target 0 channels
    }
    if buffer.channels == target_channels {
        return Ok(buffer.clone());
    }
    if buffer.channels == 0 {
         return Err(PhononError::ChannelMismatch { expected: target_channels, actual: 0 });
    }

    let frame_count = buffer.samples.len() / buffer.channels as usize;
    let mut new_samples = Vec::with_capacity(frame_count * target_channels as usize);

    for i in 0..frame_count {
        let frame_start = i * buffer.channels as usize;
        
        // Strategy pattern could be cleaner, but simple branching is fine for MVP
        if buffer.channels == 1 {
            // Mono broadcast to all channels
            let sample = buffer.samples[frame_start];
            for _ in 0..target_channels {
                new_samples.push(sample);
            }
        } else if buffer.channels == 2 && target_channels == 1 {
            // Stereo downmix
            let l = buffer.samples[frame_start];
            let r = buffer.samples[frame_start + 1];
            new_samples.push((l + r) * 0.5);
        } else {
            // General case: Truncate or Zero-pad
            for ch in 0..target_channels as usize {
                if ch < buffer.channels as usize {
                    new_samples.push(buffer.samples[frame_start + ch]);
                } else {
                    new_samples.push(0.0);
                }
            }
        }
    }

    Ok(AudioBuffer {
        samples: new_samples,
        sample_rate: buffer.sample_rate,
        channels: target_channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mono_to_stereo() {
        let buffer = AudioBuffer {
            samples: vec![0.5, 0.8], // 2 frames
            sample_rate: 44100,
            channels: 1,
        };
        let converted = ensure_channels(&buffer, 2).unwrap();
        
        assert_eq!(converted.channels, 2);
        assert_eq!(converted.samples.len(), 4);
        assert_eq!(converted.samples, vec![0.5, 0.5, 0.8, 0.8]);
    }

    #[test]
    fn test_stereo_to_mono() {
        let buffer = AudioBuffer {
            samples: vec![0.5, 0.5, 1.0, 0.0], // 2 frames. Frame 1: 0.5/0.5 -> 0.5. Frame 2: 1.0/0.0 -> 0.5
            sample_rate: 44100,
            channels: 2,
        };
        let converted = ensure_channels(&buffer, 1).unwrap();
        
        assert_eq!(converted.channels, 1);
        assert_eq!(converted.samples.len(), 2);
        assert_eq!(converted.samples, vec![0.5, 0.5]);
    }

    #[test]
    fn test_ensure_channels_identity() {
        let buffer = AudioBuffer {
            samples: vec![0.5, 0.3],
            sample_rate: 44100,
            channels: 1,
        };
        let converted = ensure_channels(&buffer, 1).unwrap();
        assert_eq!(converted.samples, vec![0.5, 0.3]);
    }

    #[test]
    fn test_ensure_channels_zero_target() {
        let buffer = AudioBuffer {
            samples: vec![0.5],
            sample_rate: 44100,
            channels: 1,
        };
        assert!(ensure_channels(&buffer, 0).is_err());
    }

    #[test]
    fn test_ensure_channels_zero_source() {
        let buffer = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 0,
        };
        assert!(ensure_channels(&buffer, 2).is_err());
    }

    #[test]
    fn test_ensure_channels_empty_buffer() {
        let buffer = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 1,
        };
        let converted = ensure_channels(&buffer, 2).unwrap();
        assert!(converted.samples.is_empty());
        assert_eq!(converted.channels, 2);
    }
}
