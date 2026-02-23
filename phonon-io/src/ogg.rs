use phonon_core::{AudioBuffer, PhononError, Result};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tracing::{info, instrument};

use std::num::NonZero;
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

/// OGG Vorbis encoding configuration.
#[derive(Debug, Clone)]
pub struct OggConfig {
    /// Target quality level from -0.1 (lowest) to 1.0 (highest). Default: 0.5.
    pub quality: f32,
}

impl Default for OggConfig {
    fn default() -> Self {
        Self { quality: 0.5 }
    }
}

/// Writes an `AudioBuffer` to an OGG Vorbis file.
///
/// Supports mono (1 channel) and stereo (2 channels).
/// Samples are deinterleaved into planar f32 format for the Vorbis encoder.
#[instrument(skip(buffer, path), fields(path = %path.as_ref().display(), duration = buffer.duration_secs()))]
pub fn write_ogg<P: AsRef<Path>>(
    path: P,
    buffer: &AudioBuffer,
    config: &OggConfig,
) -> Result<()> {
    info!("Encoding OGG Vorbis");

    let ch = buffer.channels as usize;
    if ch == 0 || ch > 2 {
        return Err(PhononError::Wav(format!(
            "OGG Vorbis encoding supports 1 or 2 channels, got {}",
            buffer.channels
        )));
    }

    // Encode into an in-memory buffer first
    let mut ogg_data: Vec<u8> = Vec::new();

    let sample_rate_nz = NonZero::new(buffer.sample_rate)
        .ok_or_else(|| PhononError::Wav("Sample rate must be non-zero".to_string()))?;
    let channels_nz = NonZero::new(buffer.channels as u8)
        .ok_or_else(|| PhononError::Wav("Channels must be non-zero".to_string()))?;

    let mut encoder = VorbisEncoderBuilder::new(
        sample_rate_nz,
        channels_nz,
        &mut ogg_data,
    )
    .map_err(|e| PhononError::Wav(format!("OGG encoder init error: {e}")))?;

    encoder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
        target_quality: config.quality,
    });

    let mut encoder = encoder
        .build()
        .map_err(|e| PhononError::Wav(format!("OGG encoder build error: {e}")))?;

    // Deinterleave samples into planar layout: Vec<Vec<f32>>
    let frame_count = buffer.samples.len() / ch;
    let mut planar: Vec<Vec<f32>> = vec![Vec::with_capacity(frame_count); ch];

    for frame in 0..frame_count {
        for (c, channel) in planar.iter_mut().enumerate() {
            channel.push(buffer.samples[frame * ch + c]);
        }
    }

    // Encode in blocks (Vorbis works best with ~1024-sample blocks)
    let block_size = 1024;
    let mut offset = 0;

    while offset < frame_count {
        let end = (offset + block_size).min(frame_count);
        let block_slices: Vec<&[f32]> = planar
            .iter()
            .map(|channel| &channel[offset..end])
            .collect();

        encoder
            .encode_audio_block(block_slices.as_slice())
            .map_err(|e| PhononError::Wav(format!("OGG encode block error: {e}")))?;

        offset = end;
    }

    // Finalize the stream
    encoder
        .finish()
        .map_err(|e| PhononError::Wav(format!("OGG finish error: {e}")))?;

    // Write to file
    let mut file = File::create(path.as_ref()).map_err(PhononError::Io)?;
    file.write_all(&ogg_data).map_err(PhononError::Io)?;

    info!(bytes = ogg_data.len(), "OGG Vorbis encoding complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_ogg_mono() -> Result<()> {
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

        let tmp = std::env::temp_dir().join("phonon_test_mono.ogg");
        write_ogg(&tmp, &buffer, &OggConfig::default())?;

        let metadata = std::fs::metadata(&tmp).map_err(PhononError::Io)?;
        assert!(metadata.len() > 0, "OGG file should not be empty");

        // Verify we can decode it back via Symphonia
        let decoded = crate::read_audio(&tmp)?;
        assert_eq!(decoded.sample_rate, sample_rate);
        assert!(decoded.samples.len() > 0);

        let _ = std::fs::remove_file(&tmp);
        Ok(())
    }

    #[test]
    fn test_write_ogg_stereo() -> Result<()> {
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

        let tmp = std::env::temp_dir().join("phonon_test_stereo.ogg");
        write_ogg(&tmp, &buffer, &OggConfig::default())?;

        let metadata = std::fs::metadata(&tmp).map_err(PhononError::Io)?;
        assert!(metadata.len() > 0);

        let decoded = crate::read_audio(&tmp)?;
        assert_eq!(decoded.channels, 2);
        assert_eq!(decoded.sample_rate, sample_rate);

        let _ = std::fs::remove_file(&tmp);
        Ok(())
    }

    #[test]
    fn test_write_ogg_custom_quality() -> Result<()> {
        let buffer = AudioBuffer {
            samples: vec![0.0; 44100],
            sample_rate: 44100,
            channels: 1,
        };

        let config_low = OggConfig { quality: 0.0 };
        let config_high = OggConfig { quality: 1.0 };

        let tmp_low = std::env::temp_dir().join("phonon_test_low_q.ogg");
        let tmp_high = std::env::temp_dir().join("phonon_test_high_q.ogg");

        write_ogg(&tmp_low, &buffer, &config_low)?;
        write_ogg(&tmp_high, &buffer, &config_high)?;

        let size_low = std::fs::metadata(&tmp_low).map_err(PhononError::Io)?.len();
        let size_high = std::fs::metadata(&tmp_high).map_err(PhononError::Io)?.len();

        // Higher quality should produce a larger (or at least equal) file
        assert!(
            size_high >= size_low,
            "High quality ({size_high}) should be >= low quality ({size_low})"
        );

        let _ = std::fs::remove_file(&tmp_low);
        let _ = std::fs::remove_file(&tmp_high);
        Ok(())
    }

    #[test]
    fn test_write_ogg_invalid_channels() {
        let buffer = AudioBuffer {
            samples: vec![0.0; 300],
            sample_rate: 44100,
            channels: 3,
        };
        assert!(write_ogg("/tmp/bad.ogg", &buffer, &OggConfig::default()).is_err());
    }
}
