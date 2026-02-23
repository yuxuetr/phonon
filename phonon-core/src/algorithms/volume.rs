use crate::AudioBuffer;
use rayon::prelude::*;

/// Minimum sample count before engaging parallel processing.
const PAR_THRESHOLD: usize = 131072;

/// Adjusts the volume of an audio buffer by a gain factor.
///
/// # Arguments
///
/// * `buffer` - The audio buffer to modify in place.
/// * `gain` - The gain factor. 1.0 = no change, 0.5 = half volume, 2.0 = double volume.
///   Values are clamped to `[0.0, f32::MAX]`. A gain of 0.0 silences the audio.
///
/// Samples are clamped to `[-1.0, 1.0]` after applying the gain to prevent clipping.
pub fn adjust_volume(buffer: &mut AudioBuffer, gain: f32) {
    let gain = gain.max(0.0);

    if (gain - 1.0).abs() < f32::EPSILON {
        return; // No change needed
    }

    if buffer.samples.len() >= PAR_THRESHOLD {
        buffer.samples.par_iter_mut().for_each(|s| *s = (*s * gain).clamp(-1.0, 1.0));
    } else {
        for sample in &mut buffer.samples {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
}

/// Creates a copy of the audio buffer with adjusted volume.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer (not modified).
/// * `gain` - The gain factor.
///
/// # Returns
///
/// A new `AudioBuffer` with volume adjusted.
pub fn with_volume(buffer: &AudioBuffer, gain: f32) -> AudioBuffer {
    let mut result = buffer.clone();
    adjust_volume(&mut result, gain);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adjust_volume_half() {
        let mut buffer = AudioBuffer {
            samples: vec![1.0, -1.0, 0.5, -0.5],
            sample_rate: 44100,
            channels: 1,
        };
        adjust_volume(&mut buffer, 0.5);
        assert!((buffer.samples[0] - 0.5).abs() < 1e-6);
        assert!((buffer.samples[1] - (-0.5)).abs() < 1e-6);
        assert!((buffer.samples[2] - 0.25).abs() < 1e-6);
        assert!((buffer.samples[3] - (-0.25)).abs() < 1e-6);
    }

    #[test]
    fn test_adjust_volume_double_with_clamp() {
        let mut buffer = AudioBuffer {
            samples: vec![0.6, -0.7],
            sample_rate: 44100,
            channels: 1,
        };
        adjust_volume(&mut buffer, 2.0);
        // 0.6 * 2.0 = 1.2 -> clamped to 1.0
        assert!((buffer.samples[0] - 1.0).abs() < 1e-6);
        // -0.7 * 2.0 = -1.4 -> clamped to -1.0
        assert!((buffer.samples[1] - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn test_adjust_volume_zero_silences() {
        let mut buffer = AudioBuffer {
            samples: vec![0.5, -0.3, 0.9],
            sample_rate: 44100,
            channels: 1,
        };
        adjust_volume(&mut buffer, 0.0);
        assert!(buffer.samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_adjust_volume_unity_no_change() {
        let original = vec![0.1, 0.2, 0.3];
        let mut buffer = AudioBuffer {
            samples: original.clone(),
            sample_rate: 44100,
            channels: 1,
        };
        adjust_volume(&mut buffer, 1.0);
        assert_eq!(buffer.samples, original);
    }

    #[test]
    fn test_adjust_volume_negative_gain_clamped_to_zero() {
        let mut buffer = AudioBuffer {
            samples: vec![0.5, -0.5],
            sample_rate: 44100,
            channels: 1,
        };
        // Negative gain is clamped to 0.0
        adjust_volume(&mut buffer, -1.0);
        assert!(buffer.samples.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn test_adjust_volume_empty_buffer() {
        let mut buffer = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 1,
        };
        adjust_volume(&mut buffer, 2.0);
        assert!(buffer.samples.is_empty());
    }

    #[test]
    fn test_with_volume_does_not_modify_original() {
        let buffer = AudioBuffer {
            samples: vec![0.5, -0.5],
            sample_rate: 44100,
            channels: 1,
        };
        let result = with_volume(&buffer, 0.5);
        // Original unchanged
        assert!((buffer.samples[0] - 0.5).abs() < 1e-6);
        // Result changed
        assert!((result.samples[0] - 0.25).abs() < 1e-6);
    }
}
