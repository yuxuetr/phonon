use crate::{AudioBuffer, PhononError, Result};

/// Trims the audio buffer to the specified time range.
///
/// # Arguments
///
/// * `buffer` - The source audio buffer.
/// * `start_secs` - Start time in seconds.
/// * `end_secs` - End time in seconds.
///
/// # Returns
///
/// A new `AudioBuffer` containing the audio within the specified range.
/// If `start_secs` is beyond the end of the audio, returns an empty buffer.
/// If `end_secs` is beyond the end, clamps to the end.
pub fn trim(buffer: &AudioBuffer, start_secs: f64, end_secs: f64) -> Result<AudioBuffer> {
    if start_secs < 0.0 || end_secs < 0.0 {
        return Err(PhononError::Wav("Time cannot be negative".to_string()));
    }
    if start_secs >= end_secs {
        return Ok(AudioBuffer::new(buffer.sample_rate, buffer.channels, 0));
    }

    let samples_per_sec = buffer.sample_rate as f64 * buffer.channels as f64;
    
    // Calculate start index aligned to channels
    let mut start_idx = (start_secs * samples_per_sec) as usize;
    // Align to frame boundary to avoid channel swapping
    start_idx -= start_idx % buffer.channels as usize;

    // Calculate end index aligned to channels
    let mut end_idx = (end_secs * samples_per_sec) as usize;
    end_idx -= end_idx % buffer.channels as usize;

    // Clamp to buffer bounds
    let len = buffer.samples.len();
    if start_idx >= len {
        return Ok(AudioBuffer::new(buffer.sample_rate, buffer.channels, 0));
    }
    if end_idx > len {
        end_idx = len;
    }

    let new_samples = buffer.samples[start_idx..end_idx].to_vec();

    Ok(AudioBuffer {
        samples: new_samples,
        sample_rate: buffer.sample_rate,
        channels: buffer.channels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trim() {
        // Create 1 second of stereo audio (channel 1: 0.5, channel 2: -0.5)
        let sample_rate = 100;
        let channels = 2;
        let mut samples = Vec::new();
        for _ in 0..sample_rate {
            samples.push(0.5);
            samples.push(-0.5);
        }
        let buffer = AudioBuffer {
            samples,
            sample_rate,
            channels,
        };

        // Trim 0.1s to 0.4s (should be 0.3s long)
        // 0.3s * 100Hz * 2ch = 60 samples
        let trimmed = trim(&buffer, 0.1, 0.4).unwrap();
        
        assert_eq!(trimmed.samples.len(), 60);
        assert_eq!(trimmed.sample_rate, 100);
        assert_eq!(trimmed.channels, 2);
        assert_eq!(trimmed.samples[0], 0.5);
        assert_eq!(trimmed.samples[1], -0.5);
    }
}
