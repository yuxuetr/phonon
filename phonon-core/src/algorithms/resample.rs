use crate::{AudioBuffer, PhononError, Result};

/// Resamples the audio buffer to a new sample rate using linear interpolation.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer.
/// * `target_sample_rate` - The desired sample rate in Hz.
///
/// # Returns
///
/// A new `AudioBuffer` with the target sample rate.
///
/// # Note
///
/// This implementation uses simple linear interpolation, which is fast but may introduce
/// aliasing artifacts for high-frequency content or significant downsampling.
/// For professional quality, a windowed sinc filter (Polyphase) should be used in future versions.
pub fn resample(buffer: &AudioBuffer, target_sample_rate: u32) -> Result<AudioBuffer> {
    if target_sample_rate == 0 {
        return Err(PhononError::InvalidSampleRate(0));
    }
    if buffer.sample_rate == target_sample_rate {
        return Ok(buffer.clone());
    }
    if buffer.samples.is_empty() {
        return Ok(AudioBuffer::new(target_sample_rate, buffer.channels, 0));
    }

    let ratio = buffer.sample_rate as f64 / target_sample_rate as f64;
    let old_len = buffer.samples.len() / buffer.channels as usize;
    let new_len = (old_len as f64 / ratio).ceil() as usize;
    
    let mut new_samples = Vec::with_capacity(new_len * buffer.channels as usize);

    for i in 0..new_len {
        let old_idx_float = i as f64 * ratio;
        let old_idx_floor = old_idx_float.floor() as usize;
        let old_idx_ceil = (old_idx_floor + 1).min(old_len - 1);
        let t = (old_idx_float - old_idx_floor as f64) as f32;

        for ch in 0..buffer.channels as usize {
            let s1 = buffer.samples[old_idx_floor * buffer.channels as usize + ch];
            let s2 = buffer.samples[old_idx_ceil * buffer.channels as usize + ch];
            
            // Linear interpolation: y = y1 + (y2 - y1) * t
            let sample = s1 + (s2 - s1) * t;
            new_samples.push(sample);
        }
    }

    Ok(AudioBuffer {
        samples: new_samples,
        sample_rate: target_sample_rate,
        channels: buffer.channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resample_identity() {
        let buffer = AudioBuffer {
            samples: vec![0.1, 0.2, 0.3],
            sample_rate: 44100,
            channels: 1,
        };
        let resampled = resample(&buffer, 44100).unwrap();
        assert_eq!(resampled.samples, buffer.samples);
    }

    #[test]
    fn test_resample_upsample() {
        // Double the sample rate: 100Hz -> 200Hz
        // Input: 0.0, 1.0 (2 samples)
        // Output should have approx 4 samples: 0.0, 0.5, 1.0, 1.0(clamped)
        let buffer = AudioBuffer {
            samples: vec![0.0, 1.0],
            sample_rate: 100,
            channels: 1,
        };
        let resampled = resample(&buffer, 200).unwrap();
        
        assert_eq!(resampled.sample_rate, 200);
        assert_eq!(resampled.samples.len(), 4);
        
        assert!((resampled.samples[0] - 0.0).abs() < 1e-6); // idx 0.0 -> 0.0
        assert!((resampled.samples[1] - 0.5).abs() < 1e-6); // idx 0.5 -> 0.5
        assert!((resampled.samples[2] - 1.0).abs() < 1e-6); // idx 1.0 -> 1.0
        assert!((resampled.samples[3] - 1.0).abs() < 1e-6); // idx 1.5 -> 1.0 (clamped to end)
    }

    #[test]
    fn test_resample_stereo() {
        // 1 frame of stereo: L=0.0, R=1.0
        // Upsample 2x
        let buffer = AudioBuffer {
            samples: vec![0.0, 1.0], 
            sample_rate: 100,
            channels: 2,
        };
        let resampled = resample(&buffer, 200).unwrap();
        
        // Should have 2 frames (4 samples)
        // Frame 0: L=0.0, R=1.0
        // Frame 1: L=0.0, R=1.0 (interpolated between frame 0 and frame 0-end)
        
        assert_eq!(resampled.samples.len(), 4);
        assert!((resampled.samples[0] - 0.0).abs() < 1e-6); // L0
        assert!((resampled.samples[1] - 1.0).abs() < 1e-6); // R0
        assert!((resampled.samples[2] - 0.0).abs() < 1e-6); // L1 (interp)
        assert!((resampled.samples[3] - 1.0).abs() < 1e-6); // R1 (interp)
    }
}
