use crate::{algorithms::resample, AudioBuffer, PhononError, Result};

/// changes the speed (and pitch) of the audio buffer by resampling it.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer.
/// * `speed_factor` - The factor to change speed by (e.g. 2.0 for double speed/pitch, 0.5 for half speed/pitch).
///
/// # Returns
///
/// A new `AudioBuffer` with the speed changed.
///
/// # Behavior
///
/// This effectively simulates playing a vinyl record or tape at a different speed.
/// * Speed > 1.0: Faster and Higher Pitch. Duration decreases.
/// * Speed < 1.0: Slower and Lower Pitch. Duration increases.
pub fn change_speed(buffer: &AudioBuffer, speed_factor: f64) -> Result<AudioBuffer> {
    if speed_factor <= 0.0 {
        return Err(PhononError::Wav("Speed factor must be positive".to_string()));
    }
    if (speed_factor - 1.0).abs() < 1e-6 {
        return Ok(buffer.clone());
    }

    // We want to resample such that the new data, when played at the original sample rate,
    // sounds like it's played at `speed_factor`.
    
    // Example: 2.0x speed.
    // Original: 1 second (N samples) at 44100 Hz.
    // Result should be: 0.5 seconds (N/2 samples) at 44100 Hz.
    // To get N/2 samples from N samples, we need to resample *down*.
    // Target "logical" rate for resampling = Original Rate / Speed Factor.
    // Resampling from 44100 to 22050 (factor 0.5) gives us half the samples.
    
    let target_rate = (buffer.sample_rate as f64 / speed_factor).round() as u32;
    
    // Resample to the target rate (this changes the number of samples)
    let mut resampled_buffer = resample(buffer, target_rate)?;
    
    // Now, pretend this buffer is actually at the original sample rate.
    resampled_buffer.sample_rate = buffer.sample_rate;
    
    Ok(resampled_buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_speed() {
        let buffer = AudioBuffer {
            samples: vec![0.0, 0.5, 1.0, 0.5, 0.0], // 5 samples
            sample_rate: 100,
            channels: 1,
        };
        
        // 2x speed -> should have approx half samples (2 or 3)
        // target rate = 100 / 2 = 50.
        // resample 100->50 gives half samples.
        
        let sped_up = change_speed(&buffer, 2.0).unwrap();
        
        assert_eq!(sped_up.sample_rate, 100);
        // Resample implementation: 5 samples * (50/100) = 2.5 -> ceil -> 3 samples?
        assert!(sped_up.samples.len() <= 3);
    }

    #[test]
    fn test_half_speed() {
        let buffer = AudioBuffer {
            samples: vec![0.0, 1.0],
            sample_rate: 100,
            channels: 1,
        };
        
        // 0.5x speed -> double samples
        // target rate = 100 / 0.5 = 200.
        
        let slowed = change_speed(&buffer, 0.5).unwrap();
        
        assert_eq!(slowed.sample_rate, 100);
        assert!(slowed.samples.len() >= 4);
    }
}
