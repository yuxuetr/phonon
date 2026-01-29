use phonon_core::{AudioBuffer, PhononError, Result};
use std::path::Path;

/// Read a WAV file into an AudioBuffer.
///
/// Converts all audio data to `f32` in the range `[-1.0, 1.0]`.
/// Currently supports:
/// - 16-bit Integer PCM
/// - 32-bit Float PCM
pub fn read_wav<P: AsRef<Path>>(path: P) -> Result<AudioBuffer> {
    let mut reader = hound::WavReader::open(path)
        .map_err(|e| PhononError::Wav(format!("Failed to open file: {}", e)))?;
    let spec = reader.spec();

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader
                .samples::<f32>()
                .map(|s| s.map_err(|e| PhononError::Wav(format!("Decode error: {}", e))))
                .collect::<Result<Vec<f32>>>()?
        }
        hound::SampleFormat::Int => {
            match spec.bits_per_sample {
                16 => reader
                    .samples::<i16>()
                    .map(|s| {
                        s.map_err(|e| PhononError::Wav(format!("Decode error: {}", e)))
                            .map(|x| x as f32 / 32768.0)
                    })
                    .collect::<Result<Vec<f32>>>()?,
                _ => {
                    return Err(PhononError::Wav(format!(
                        "Unsupported bit depth: {}",
                        spec.bits_per_sample
                    )))
                }
            }
        }
    };

    Ok(AudioBuffer {
        samples,
        sample_rate: spec.sample_rate,
        channels: spec.channels,
    })
}

/// Write an AudioBuffer to a WAV file.
///
/// Currently writes as 16-bit Integer PCM.
/// Samples are clamped to `[-1.0, 1.0]` before conversion to prevent wrapping.
pub fn write_wav<P: AsRef<Path>>(path: P, buffer: &AudioBuffer) -> Result<()> {
    let spec = hound::WavSpec {
        channels: buffer.channels,
        sample_rate: buffer.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| PhononError::Wav(format!("Failed to create file: {}", e)))?;

    for &sample in &buffer.samples {
        // Clamp to avoid integer overflow/wrapping which sounds terrible
        let clamped = sample.max(-1.0).min(1.0);
        let scaled = (clamped * 32767.0) as i16;
        writer
            .write_sample(scaled)
            .map_err(|e| PhononError::Wav(format!("Write error: {}", e)))?;
    }

    writer
        .finalize()
        .map_err(|e| PhononError::Wav(format!("Finalize error: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    
    // We need a way to test without external files if possible, or create temp ones.
    // Since we can write, we can write then read.
    
    #[test]
    fn test_wav_write_read_roundtrip() -> Result<()> {
        let temp_dir = std::env::temp_dir();
        let file_path = temp_dir.join("test_phonon_roundtrip.wav");
        
        // Create a buffer with known data (e.g., a simple sine wave or DC offset)
        // Let's use a DC offset for simplicity of checking, and some ramp
        let mut samples = Vec::new();
        for i in 0..100 {
            samples.push((i as f32 / 100.0) * 0.5); // 0.0 to 0.5
        }
        
        let original_buffer = AudioBuffer {
            samples: samples.clone(),
            sample_rate: 44100,
            channels: 1,
        };
        
        write_wav(&file_path, &original_buffer)?;
        
        let loaded_buffer = read_wav(&file_path)?;
        
        assert_eq!(loaded_buffer.sample_rate, 44100);
        assert_eq!(loaded_buffer.channels, 1);
        assert_eq!(loaded_buffer.samples.len(), 100);
        
        // Check content with some tolerance due to 16-bit quantization
        for (i, &loaded_sample) in loaded_buffer.samples.iter().enumerate() {
            let original_sample = samples[i];
            let diff = (loaded_sample - original_sample).abs();
            // 16-bit quantization error is roughly 1/32768 ~= 0.00003
            assert!(diff < 0.0001, "Sample {} mismatch: expected {}, got {}", i, original_sample, loaded_sample);
        }
        
        // Clean up
        let _ = fs::remove_file(file_path);
        
        Ok(())
    }
}
