use crate::{AudioBuffer, PhononError, Result};
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
    const PAR_THRESHOLD: usize = 131072;

    let mixed_samples = if max_len >= PAR_THRESHOLD {
        let s1 = &buffer1.samples;
        let s2 = &buffer2.samples;
        (0..max_len)
            .into_par_iter()
            .map(|i| {
                let v1 = if i < len1 { s1[i] } else { 0.0 };
                let v2 = if i < len2 { s2[i] } else { 0.0 };
                (v1 + v2).clamp(-1.0, 1.0)
            })
            .collect()
    } else {
        let mut out = Vec::with_capacity(max_len);
        for i in 0..max_len {
            let s1 = if i < len1 { buffer1.samples[i] } else { 0.0 };
            let s2 = if i < len2 { buffer2.samples[i] } else { 0.0 };
            out.push((s1 + s2).clamp(-1.0, 1.0));
        }
        out
    };

    Ok(AudioBuffer {
        samples: mixed_samples,
        sample_rate: buffer1.sample_rate,
        channels: buffer1.channels,
    })
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
