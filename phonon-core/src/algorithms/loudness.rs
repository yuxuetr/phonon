use crate::AudioBuffer;
use rayon::prelude::*;

/// Minimum sample count before engaging parallel processing.
const PAR_THRESHOLD: usize = 131072;

/// Computes the loudness of the audio buffer in dBFS (decibels relative to full scale).
///
/// A square wave at max amplitude is ~0 dBFS, a sine wave at max amplitude is ~-3 dBFS.
/// Returns `f32::NEG_INFINITY` for silent buffers.
///
/// Equivalent to pydub's `.dBFS` property.
pub fn dbfs(buffer: &AudioBuffer) -> f32 {
    if buffer.samples.is_empty() {
        return f32::NEG_INFINITY;
    }

    // dBFS = 20 * log10(rms / 1.0), where 1.0 is the maximum possible amplitude
    let rms_val = rms(buffer);
    if rms_val <= 0.0 {
        return f32::NEG_INFINITY;
    }

    20.0 * rms_val.log10()
}

/// Computes the RMS (Root Mean Square) of the audio buffer.
///
/// Returns 0.0 for empty or silent buffers.
///
/// Equivalent to pydub's `.rms` property (but as float, not integer).
pub fn rms(buffer: &AudioBuffer) -> f32 {
    if buffer.samples.is_empty() {
        return 0.0;
    }

    let sum_squares: f64 = if buffer.samples.len() >= PAR_THRESHOLD {
        buffer.samples.par_iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum()
    } else {
        buffer.samples.iter()
            .map(|&s| (s as f64) * (s as f64))
            .sum()
    };

    (sum_squares / buffer.samples.len() as f64).sqrt() as f32
}

/// Computes the peak amplitude of the audio buffer.
///
/// Returns the maximum absolute sample value. 0.0 for empty buffers.
///
/// Equivalent to pydub's `.max` property (normalized to float).
pub fn peak(buffer: &AudioBuffer) -> f32 {
    if buffer.samples.len() >= PAR_THRESHOLD {
        buffer.samples.par_iter()
            .map(|s| s.abs())
            .reduce(|| 0.0_f32, f32::max)
    } else {
        buffer.samples.iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max)
    }
}

/// Computes the peak amplitude in dBFS.
///
/// Returns `f32::NEG_INFINITY` for silent buffers.
///
/// Equivalent to pydub's `.max_dBFS` property.
pub fn peak_dbfs(buffer: &AudioBuffer) -> f32 {
    let p = peak(buffer);
    if p <= 0.0 {
        f32::NEG_INFINITY
    } else {
        20.0 * p.log10()
    }
}

/// Adjusts the volume of an audio buffer by a dB amount.
///
/// Positive values increase volume, negative values decrease it.
/// This is equivalent to pydub's `sound + 6` or `sound - 3` operators.
///
/// Samples are clamped to `[-1.0, 1.0]` to prevent clipping.
///
/// # Arguments
///
/// * `buffer` - The audio buffer to modify in place.
/// * `db` - The gain in decibels (e.g. 6.0 for +6dB, -3.0 for -3dB).
pub fn adjust_volume_db(buffer: &mut AudioBuffer, db: f32) {
    if db.abs() < f32::EPSILON {
        return; // No change needed
    }

    // Convert dB to linear gain: gain = 10^(dB/20)
    let gain = 10.0_f32.powf(db / 20.0);

    if buffer.samples.len() >= PAR_THRESHOLD {
        buffer.samples.par_iter_mut().for_each(|s| *s = (*s * gain).clamp(-1.0, 1.0));
    } else {
        for sample in &mut buffer.samples {
            *sample = (*sample * gain).clamp(-1.0, 1.0);
        }
    }
}

/// Creates a copy of the audio buffer with dB-adjusted volume.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer (not modified).
/// * `db` - The gain in decibels.
///
/// # Returns
///
/// A new `AudioBuffer` with adjusted volume.
pub fn with_volume_db(buffer: &AudioBuffer, db: f32) -> AudioBuffer {
    let mut result = buffer.clone();
    adjust_volume_db(&mut result, db);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rms_silence() {
        let buffer = AudioBuffer {
            samples: vec![0.0; 100],
            sample_rate: 44100,
            channels: 1,
        };
        assert_eq!(rms(&buffer), 0.0);
    }

    #[test]
    fn test_rms_dc_offset() {
        let buffer = AudioBuffer {
            samples: vec![0.5; 100],
            sample_rate: 44100,
            channels: 1,
        };
        assert!((rms(&buffer) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_rms_square_wave() {
        // Square wave at amplitude 1.0 has RMS = 1.0
        let mut samples = Vec::new();
        for i in 0..1000 {
            samples.push(if i % 2 == 0 { 1.0 } else { -1.0 });
        }
        let buffer = AudioBuffer {
            samples,
            sample_rate: 44100,
            channels: 1,
        };
        assert!((rms(&buffer) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_rms_empty() {
        let buffer = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 1,
        };
        assert_eq!(rms(&buffer), 0.0);
    }

    #[test]
    fn test_dbfs_silence() {
        let buffer = AudioBuffer {
            samples: vec![0.0; 100],
            sample_rate: 44100,
            channels: 1,
        };
        assert!(dbfs(&buffer).is_infinite() && dbfs(&buffer) < 0.0);
    }

    #[test]
    fn test_dbfs_square_wave_max() {
        // Square wave at amplitude 1.0: RMS = 1.0, dBFS = 0.0
        let samples: Vec<f32> = (0..1000).map(|i| if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let buffer = AudioBuffer {
            samples,
            sample_rate: 44100,
            channels: 1,
        };
        assert!((dbfs(&buffer) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_dbfs_half_amplitude() {
        // DC at 0.5 → RMS = 0.5 → dBFS = 20*log10(0.5) ≈ -6.02
        let buffer = AudioBuffer {
            samples: vec![0.5; 1000],
            sample_rate: 44100,
            channels: 1,
        };
        assert!((dbfs(&buffer) - (-6.02)).abs() < 0.1);
    }

    #[test]
    fn test_peak() {
        let buffer = AudioBuffer {
            samples: vec![0.3, -0.8, 0.5, -0.2],
            sample_rate: 44100,
            channels: 1,
        };
        assert!((peak(&buffer) - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_peak_dbfs() {
        // Peak = 1.0 → peak_dBFS = 0.0
        let buffer = AudioBuffer {
            samples: vec![0.5, -1.0, 0.3],
            sample_rate: 44100,
            channels: 1,
        };
        assert!((peak_dbfs(&buffer) - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_adjust_volume_db_plus_6() {
        let mut buffer = AudioBuffer {
            samples: vec![0.25],
            sample_rate: 44100,
            channels: 1,
        };
        // +6dB ≈ 2x gain → 0.25 * 2.0 = 0.5
        adjust_volume_db(&mut buffer, 6.0);
        assert!((buffer.samples[0] - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_adjust_volume_db_minus_6() {
        let mut buffer = AudioBuffer {
            samples: vec![1.0],
            sample_rate: 44100,
            channels: 1,
        };
        // -6dB ≈ 0.5x gain → 1.0 * 0.5 = 0.5
        adjust_volume_db(&mut buffer, -6.0);
        assert!((buffer.samples[0] - 0.5).abs() < 0.02);
    }

    #[test]
    fn test_adjust_volume_db_clipping() {
        let mut buffer = AudioBuffer {
            samples: vec![0.8],
            sample_rate: 44100,
            channels: 1,
        };
        // +20dB = 10x gain → 0.8 * 10 = 8.0 → clamped to 1.0
        adjust_volume_db(&mut buffer, 20.0);
        assert!((buffer.samples[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_adjust_volume_db_zero() {
        let original = vec![0.5, -0.3];
        let mut buffer = AudioBuffer {
            samples: original.clone(),
            sample_rate: 44100,
            channels: 1,
        };
        adjust_volume_db(&mut buffer, 0.0);
        assert_eq!(buffer.samples, original);
    }

    #[test]
    fn test_with_volume_db_immutable() {
        let buffer = AudioBuffer {
            samples: vec![0.5],
            sample_rate: 44100,
            channels: 1,
        };
        let louder = with_volume_db(&buffer, 6.0);
        // Original unchanged
        assert!((buffer.samples[0] - 0.5).abs() < 1e-6);
        // New one is louder
        assert!(louder.samples[0] > 0.5);
    }
}
