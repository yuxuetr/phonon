use phonon_core::{AudioBuffer, PhononError, Result};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tracing::{info, instrument};

use fdk_aac::enc::{BitRate, ChannelMode, EncodeInfo, Encoder, EncoderParams, Transport};

/// AAC encoding configuration.
#[derive(Debug, Clone)]
pub struct AacConfig {
    /// Bitrate in kbps (e.g. 64, 96, 128, 192, 256). Default: 128.
    pub bitrate_kbps: u32,
}

impl Default for AacConfig {
    fn default() -> Self {
        Self { bitrate_kbps: 128 }
    }
}

/// Writes an `AudioBuffer` to an AAC file (ADTS transport).
///
/// Supports mono (1 channel) and stereo (2 channels).
/// Samples are converted from f32 [-1.0, 1.0] to i16 before encoding.
#[instrument(skip(buffer, path), fields(path = %path.as_ref().display(), duration = buffer.duration_secs()))]
pub fn write_aac<P: AsRef<Path>>(
    path: P,
    buffer: &AudioBuffer,
    config: &AacConfig,
) -> Result<()> {
    info!("Encoding AAC");

    if buffer.channels == 0 || buffer.channels > 2 {
        return Err(PhononError::Wav(format!(
            "AAC encoding supports 1 or 2 channels, got {}",
            buffer.channels
        )));
    }

    let channel_mode = if buffer.channels == 1 {
        ChannelMode::Mono
    } else {
        ChannelMode::Stereo
    };

    let params = EncoderParams {
        bit_rate: BitRate::Cbr(config.bitrate_kbps * 1000),
        sample_rate: buffer.sample_rate,
        transport: Transport::Adts,
        channels: channel_mode,
    };

    let encoder = Encoder::new(params)
        .map_err(|e| PhononError::Wav(format!("AAC encoder init error: {e:?}")))?;

    // Convert f32 samples to i16
    let samples_i16: Vec<i16> = buffer
        .samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();

    // Encode in chunks — AAC-LC uses 1024 samples per frame per channel
    let frame_size = 1024 * buffer.channels as usize;
    let mut aac_data: Vec<u8> = Vec::new();
    // Output buffer large enough for one AAC frame (max ~768 bytes per frame per channel)
    let mut out_buf = vec![0u8; 8192];
    let mut offset = 0;

    while offset < samples_i16.len() {
        let end = (offset + frame_size).min(samples_i16.len());
        let chunk = &samples_i16[offset..end];

        // Pad the last chunk if it's shorter than a full frame
        let input = if chunk.len() < frame_size {
            let mut padded = vec![0i16; frame_size];
            padded[..chunk.len()].copy_from_slice(chunk);
            padded
        } else {
            chunk.to_vec()
        };

        let info: EncodeInfo = encoder
            .encode(&input, &mut out_buf)
            .map_err(|e| PhononError::Wav(format!("AAC encode error: {e:?}")))?;

        if info.output_size > 0 {
            aac_data.extend_from_slice(&out_buf[..info.output_size]);
        }

        offset += frame_size;
    }

    // Flush: encode with empty input to get any remaining data
    let empty: Vec<i16> = Vec::new();
    loop {
        let info = encoder
            .encode(&empty, &mut out_buf)
            .map_err(|e| PhononError::Wav(format!("AAC flush error: {e:?}")))?;
        if info.output_size == 0 {
            break;
        }
        aac_data.extend_from_slice(&out_buf[..info.output_size]);
    }

    // Write to file
    let mut file = File::create(path.as_ref()).map_err(PhononError::Io)?;
    file.write_all(&aac_data).map_err(PhononError::Io)?;

    info!(bytes = aac_data.len(), "AAC encoding complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_aac_mono() -> Result<()> {
        let sample_rate = 44100_u32;
        let total_samples = 44100_usize;

        let mut samples = Vec::with_capacity(total_samples);
        for i in 0..total_samples {
            let t = i as f64 / sample_rate as f64;
            let s = (t * 440.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.8;
            samples.push(s);
        }

        let buffer = AudioBuffer {
            samples,
            sample_rate,
            channels: 1,
        };

        let tmp = std::env::temp_dir().join("phonon_test_mono.aac");
        write_aac(&tmp, &buffer, &AacConfig::default())?;

        let metadata = std::fs::metadata(&tmp).map_err(PhononError::Io)?;
        assert!(metadata.len() > 0, "AAC file should not be empty");

        // Verify we can decode it back via Symphonia
        let decoded = crate::read_audio(&tmp)?;
        assert_eq!(decoded.sample_rate, sample_rate);
        assert!(decoded.samples.len() > 0);

        let _ = std::fs::remove_file(&tmp);
        Ok(())
    }

    #[test]
    fn test_write_aac_stereo() -> Result<()> {
        let sample_rate = 44100_u32;
        let frames = 44100_usize;
        let mut samples = Vec::with_capacity(frames * 2);

        for i in 0..frames {
            let t = i as f64 / sample_rate as f64;
            let left = (t * 440.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.5;
            let right = (t * 880.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.5;
            samples.push(left);
            samples.push(right);
        }

        let buffer = AudioBuffer {
            samples,
            sample_rate,
            channels: 2,
        };

        let tmp = std::env::temp_dir().join("phonon_test_stereo.aac");
        write_aac(&tmp, &buffer, &AacConfig::default())?;

        let metadata = std::fs::metadata(&tmp).map_err(PhononError::Io)?;
        assert!(metadata.len() > 0);

        let decoded = crate::read_audio(&tmp)?;
        assert_eq!(decoded.channels, 2);

        let _ = std::fs::remove_file(&tmp);
        Ok(())
    }

    #[test]
    fn test_write_aac_custom_bitrate() -> Result<()> {
        let buffer = AudioBuffer {
            samples: vec![0.0; 44100],
            sample_rate: 44100,
            channels: 1,
        };

        let config_64 = AacConfig { bitrate_kbps: 64 };
        let config_256 = AacConfig { bitrate_kbps: 256 };

        let tmp_64 = std::env::temp_dir().join("phonon_test_64k.aac");
        let tmp_256 = std::env::temp_dir().join("phonon_test_256k.aac");

        write_aac(&tmp_64, &buffer, &config_64)?;
        write_aac(&tmp_256, &buffer, &config_256)?;

        let size_64 = std::fs::metadata(&tmp_64).map_err(PhononError::Io)?.len();
        let size_256 = std::fs::metadata(&tmp_256).map_err(PhononError::Io)?.len();

        assert!(
            size_256 > size_64,
            "256k ({size_256}) should be larger than 64k ({size_64})"
        );

        let _ = std::fs::remove_file(&tmp_64);
        let _ = std::fs::remove_file(&tmp_256);
        Ok(())
    }

    #[test]
    fn test_write_aac_invalid_channels() {
        let buffer = AudioBuffer {
            samples: vec![0.0; 300],
            sample_rate: 44100,
            channels: 3,
        };
        assert!(write_aac("/tmp/bad.aac", &buffer, &AacConfig::default()).is_err());
    }
}
