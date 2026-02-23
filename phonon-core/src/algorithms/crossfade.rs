use crate::{AudioBuffer, PhononError, Result};
use rayon::prelude::*;

/// Concatenates two audio buffers with a crossfade transition.
///
/// The end of `buffer1` is faded out while the beginning of `buffer2` is faded in,
/// and they are mixed together in the overlap region.
///
/// # Arguments
///
/// * `buffer1` - The first audio buffer.
/// * `buffer2` - The second audio buffer.
/// * `crossfade_secs` - The duration of the crossfade overlap in seconds.
///   If this exceeds the length of either buffer, it is clamped to the shorter one.
///
/// # Returns
///
/// A new `AudioBuffer` with the crossfaded result. The total length is
/// `len(buffer1) + len(buffer2) - crossfade_samples`.
///
/// # Errors
///
/// Returns an error if sample rates or channel counts do not match.
pub fn crossfade(
    buffer1: &AudioBuffer,
    buffer2: &AudioBuffer,
    crossfade_secs: f64,
) -> Result<AudioBuffer> {
    if buffer1.sample_rate != buffer2.sample_rate {
        return Err(PhononError::InvalidSampleRate(buffer2.sample_rate));
    }
    if buffer1.channels != buffer2.channels {
        return Err(PhononError::ChannelMismatch {
            expected: buffer1.channels,
            actual: buffer2.channels,
        });
    }

    let sample_rate = buffer1.sample_rate;
    let channels = buffer1.channels;

    // If crossfade is zero or negative, just concatenate
    if crossfade_secs <= 0.0 {
        let mut samples = Vec::with_capacity(buffer1.samples.len() + buffer2.samples.len());
        samples.extend_from_slice(&buffer1.samples);
        samples.extend_from_slice(&buffer2.samples);
        return Ok(AudioBuffer {
            samples,
            sample_rate,
            channels,
        });
    }

    let samples_per_sec = sample_rate as f64 * channels as f64;
    let mut fade_len = (crossfade_secs * samples_per_sec) as usize;

    // Align to frame boundary
    let ch = channels as usize;
    if ch > 0 {
        fade_len -= fade_len % ch;
    }

    // Clamp to the shorter buffer
    fade_len = fade_len.min(buffer1.samples.len()).min(buffer2.samples.len());

    if fade_len == 0 {
        let mut samples = Vec::with_capacity(buffer1.samples.len() + buffer2.samples.len());
        samples.extend_from_slice(&buffer1.samples);
        samples.extend_from_slice(&buffer2.samples);
        return Ok(AudioBuffer {
            samples,
            sample_rate,
            channels,
        });
    }

    let b1_len = buffer1.samples.len();
    let b2_len = buffer2.samples.len();
    let total_len = b1_len + b2_len - fade_len;

    let mut samples = Vec::with_capacity(total_len);

    // Part 1: Non-overlapping head of buffer1
    let b1_head_end = b1_len - fade_len;
    samples.extend_from_slice(&buffer1.samples[..b1_head_end]);

    // Part 2: Crossfade overlap region
    /// Minimum sample count before engaging parallel processing.
    const PAR_THRESHOLD: usize = 131072;

    let overlap: Vec<f32> = if fade_len >= PAR_THRESHOLD {
        let b1_tail = &buffer1.samples[b1_head_end..b1_head_end + fade_len];
        let b2_head = &buffer2.samples[..fade_len];
        let inv_fade = 1.0 / fade_len as f32;
        (0..fade_len)
            .into_par_iter()
            .map(|i| {
                let t = i as f32 * inv_fade;
                (b1_tail[i] * (1.0 - t) + b2_head[i] * t).clamp(-1.0, 1.0)
            })
            .collect()
    } else {
        let mut out = Vec::with_capacity(fade_len);
        for i in 0..fade_len {
            let t = i as f32 / fade_len as f32;
            let s1 = buffer1.samples[b1_head_end + i] * (1.0 - t);
            let s2 = buffer2.samples[i] * t;
            out.push((s1 + s2).clamp(-1.0, 1.0));
        }
        out
    };
    samples.extend_from_slice(&overlap);

    // Part 3: Non-overlapping tail of buffer2
    samples.extend_from_slice(&buffer2.samples[fade_len..]);

    Ok(AudioBuffer {
        samples,
        sample_rate,
        channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crossfade_basic() {
        // 10 samples each, crossfade 4 samples => total 16
        let b1 = AudioBuffer {
            samples: vec![1.0; 10],
            sample_rate: 10, // 10 samples/sec, 1 channel
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5; 10],
            sample_rate: 10,
            channels: 1,
        };

        // 0.4s * 10Hz * 1ch = 4 samples crossfade
        let result = crossfade(&b1, &b2, 0.4).unwrap();

        // Total: 10 + 10 - 4 = 16
        assert_eq!(result.samples.len(), 16);

        // Head (0..6): pure buffer1
        for &s in &result.samples[..6] {
            assert!((s - 1.0).abs() < 1e-6);
        }

        // Crossfade region (6..10): mix of fading b1 and rising b2
        // At i=0 (idx 6): t=0.0 → 1.0*(1.0) + 0.5*(0.0) = 1.0
        // At i=1 (idx 7): t=0.25 → 1.0*(0.75) + 0.5*(0.25) = 0.875
        // At i=2 (idx 8): t=0.5 → 1.0*(0.5) + 0.5*(0.5) = 0.75
        // At i=3 (idx 9): t=0.75 → 1.0*(0.25) + 0.5*(0.75) = 0.625
        assert!((result.samples[6] - 1.0).abs() < 1e-6);
        assert!((result.samples[7] - 0.875).abs() < 1e-6);
        assert!((result.samples[8] - 0.75).abs() < 1e-6);
        assert!((result.samples[9] - 0.625).abs() < 1e-6);

        // Tail (10..16): pure buffer2
        for &s in &result.samples[10..] {
            assert!((s - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn test_crossfade_zero_duration() {
        let b1 = AudioBuffer {
            samples: vec![1.0; 5],
            sample_rate: 44100,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5; 5],
            sample_rate: 44100,
            channels: 1,
        };

        let result = crossfade(&b1, &b2, 0.0).unwrap();
        assert_eq!(result.samples.len(), 10);
        assert!((result.samples[4] - 1.0).abs() < 1e-6);
        assert!((result.samples[5] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_crossfade_negative_duration() {
        let b1 = AudioBuffer {
            samples: vec![1.0; 5],
            sample_rate: 44100,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5; 5],
            sample_rate: 44100,
            channels: 1,
        };

        let result = crossfade(&b1, &b2, -1.0).unwrap();
        // Should behave like simple concatenation
        assert_eq!(result.samples.len(), 10);
    }

    #[test]
    fn test_crossfade_exceeding_duration_clamped() {
        let b1 = AudioBuffer {
            samples: vec![1.0; 4],
            sample_rate: 4,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5; 10],
            sample_rate: 4,
            channels: 1,
        };

        // Request 10s crossfade but b1 only has 4 samples → clamped to 4
        let result = crossfade(&b1, &b2, 10.0).unwrap();
        // Total: 4 + 10 - 4 = 10
        assert_eq!(result.samples.len(), 10);
    }

    #[test]
    fn test_crossfade_sample_rate_mismatch() {
        let b1 = AudioBuffer {
            samples: vec![1.0; 10],
            sample_rate: 44100,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5; 10],
            sample_rate: 48000,
            channels: 1,
        };
        assert!(crossfade(&b1, &b2, 0.1).is_err());
    }

    #[test]
    fn test_crossfade_channel_mismatch() {
        let b1 = AudioBuffer {
            samples: vec![1.0; 10],
            sample_rate: 44100,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5; 10],
            sample_rate: 44100,
            channels: 2,
        };
        assert!(crossfade(&b1, &b2, 0.1).is_err());
    }

    #[test]
    fn test_crossfade_empty_first_buffer() {
        let b1 = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 1,
        };
        let b2 = AudioBuffer {
            samples: vec![0.5; 10],
            sample_rate: 44100,
            channels: 1,
        };

        let result = crossfade(&b1, &b2, 0.1).unwrap();
        // fade_len clamped to 0 → pure concat
        assert_eq!(result.samples.len(), 10);
    }

    #[test]
    fn test_crossfade_stereo() {
        // 2 channels, 10 frames = 20 samples each
        let b1 = AudioBuffer {
            samples: vec![0.8; 20],
            sample_rate: 10,
            channels: 2,
        };
        let b2 = AudioBuffer {
            samples: vec![0.4; 20],
            sample_rate: 10,
            channels: 2,
        };

        // 0.2s * 10Hz * 2ch = 4 samples crossfade
        let result = crossfade(&b1, &b2, 0.2).unwrap();
        assert_eq!(result.samples.len(), 20 + 20 - 4);
        assert_eq!(result.sample_rate, 10);
        assert_eq!(result.channels, 2);
    }
}
