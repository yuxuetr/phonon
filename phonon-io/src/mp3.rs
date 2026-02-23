use phonon_core::{AudioBuffer, PhononError, Result};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::mem::MaybeUninit;
use std::path::Path;
use tracing::{info, instrument};

/// MP3 encoding configuration.
#[derive(Debug, Clone)]
pub struct Mp3Config {
    /// Bitrate in kbps (e.g. 128, 192, 256, 320). Default: 192.
    pub bitrate_kbps: u32,
}

impl Default for Mp3Config {
    fn default() -> Self {
        Self { bitrate_kbps: 192 }
    }
}

/// Maps a kbps value to the `mp3lame_encoder::Bitrate` enum.
fn to_lame_bitrate(kbps: u32) -> mp3lame_encoder::Bitrate {
    match kbps {
        8 => mp3lame_encoder::Bitrate::Kbps8,
        16 => mp3lame_encoder::Bitrate::Kbps16,
        24 => mp3lame_encoder::Bitrate::Kbps24,
        32 => mp3lame_encoder::Bitrate::Kbps32,
        40 => mp3lame_encoder::Bitrate::Kbps40,
        48 => mp3lame_encoder::Bitrate::Kbps48,
        64 => mp3lame_encoder::Bitrate::Kbps64,
        80 => mp3lame_encoder::Bitrate::Kbps80,
        96 => mp3lame_encoder::Bitrate::Kbps96,
        112 => mp3lame_encoder::Bitrate::Kbps112,
        128 => mp3lame_encoder::Bitrate::Kbps128,
        160 => mp3lame_encoder::Bitrate::Kbps160,
        192 => mp3lame_encoder::Bitrate::Kbps192,
        224 => mp3lame_encoder::Bitrate::Kbps224,
        256 => mp3lame_encoder::Bitrate::Kbps256,
        320 => mp3lame_encoder::Bitrate::Kbps320,
        _ => mp3lame_encoder::Bitrate::Kbps192, // fallback
    }
}

/// Writes an `AudioBuffer` to an MP3 file.
///
/// Supports mono (1 channel) and stereo (2 channels).
/// Samples are converted from f32 [-1.0, 1.0] to i16 before encoding.
#[instrument(skip(buffer, path), fields(path = %path.as_ref().display(), duration = buffer.duration_secs()))]
pub fn write_mp3<P: AsRef<Path>>(
    path: P,
    buffer: &AudioBuffer,
    config: &Mp3Config,
) -> Result<()> {
    info!("Encoding MP3");

    if buffer.channels == 0 || buffer.channels > 2 {
        return Err(PhononError::Wav(format!(
            "MP3 encoding supports 1 or 2 channels, got {}",
            buffer.channels
        )));
    }

    // Build the LAME encoder
    let mut builder = mp3lame_encoder::Builder::new()
        .ok_or_else(|| PhononError::Wav("Failed to create MP3 encoder".to_string()))?;

    builder
        .set_num_channels(buffer.channels as u8)
        .map_err(|e| PhononError::Wav(format!("Failed to set channels: {e:?}")))?;

    builder
        .set_sample_rate(buffer.sample_rate)
        .map_err(|e| PhononError::Wav(format!("Failed to set sample rate: {e:?}")))?;

    builder
        .set_brate(to_lame_bitrate(config.bitrate_kbps))
        .map_err(|e| PhononError::Wav(format!("Failed to set bitrate: {e:?}")))?;

    builder
        .set_quality(mp3lame_encoder::Quality::Best)
        .map_err(|e| PhononError::Wav(format!("Failed to set quality: {e:?}")))?;

    let mut encoder = builder
        .build()
        .map_err(|e| PhononError::Wav(format!("Failed to build MP3 encoder: {e:?}")))?;

    // Convert f32 samples to i16
    let samples_i16: Vec<i16> = buffer
        .samples
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect();

    // Stream encoded data directly to file
    let file = File::create(path.as_ref()).map_err(PhononError::Io)?;
    let mut writer = BufWriter::new(file);

    let max_out_size = mp3lame_encoder::max_required_buffer_size(
        samples_i16.len() / buffer.channels as usize,
    );
    let mut encode_buf: Vec<MaybeUninit<u8>> =
        vec![MaybeUninit::uninit(); max_out_size.max(7200)];

    // Encode based on channel count
    let encoded_size = if buffer.channels == 1 {
        let input = mp3lame_encoder::MonoPcm(&samples_i16);
        encoder
            .encode(input, &mut encode_buf)
            .map_err(|e| PhononError::Wav(format!("MP3 encode error: {e:?}")))?
    } else {
        // Deinterleave stereo samples
        let frame_count = samples_i16.len() / 2;
        let mut left = Vec::with_capacity(frame_count);
        let mut right = Vec::with_capacity(frame_count);
        for i in 0..frame_count {
            left.push(samples_i16[i * 2]);
            right.push(samples_i16[i * 2 + 1]);
        }

        let input = mp3lame_encoder::DualPcm {
            left: &left,
            right: &right,
        };
        encoder
            .encode(input, &mut encode_buf)
            .map_err(|e| PhononError::Wav(format!("MP3 encode error: {e:?}")))?
    };

    // Write encoded bytes directly to file
    // SAFETY: encode returns the number of valid initialized bytes
    let encoded_slice: &[u8] = unsafe {
        std::slice::from_raw_parts(encode_buf.as_ptr().cast::<u8>(), encoded_size)
    };
    writer.write_all(encoded_slice).map_err(PhononError::Io)?;

    // Flush remaining data from encoder
    let mut flush_buf: Vec<MaybeUninit<u8>> = vec![MaybeUninit::uninit(); 7200];
    let flush_size = encoder
        .flush::<mp3lame_encoder::FlushNoGap>(&mut flush_buf)
        .map_err(|e| PhononError::Wav(format!("MP3 flush error: {e:?}")))?;

    let flush_slice: &[u8] = unsafe {
        std::slice::from_raw_parts(flush_buf.as_ptr().cast::<u8>(), flush_size)
    };
    writer.write_all(flush_slice).map_err(PhononError::Io)?;
    writer.flush().map_err(PhononError::Io)?;

    info!("MP3 encoding complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_mp3_mono() -> Result<()> {
        // Generate a 1-second 440Hz sine wave
        let sample_rate = 44100_u32;
        let duration_secs = 1.0;
        let total_samples = (duration_secs * sample_rate as f64) as usize;

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

        let tmp = std::env::temp_dir().join("phonon_test_mono.mp3");
        write_mp3(&tmp, &buffer, &Mp3Config::default())?;

        // Verify file exists and has content
        let metadata = std::fs::metadata(&tmp)
            .map_err(PhononError::Io)?;
        assert!(metadata.len() > 0, "MP3 file should not be empty");

        // Verify we can decode it back via phonon-io
        let decoded = crate::read_audio(&tmp)?;
        assert_eq!(decoded.sample_rate, sample_rate);
        assert!(!decoded.samples.is_empty());

        let _ = std::fs::remove_file(&tmp);
        Ok(())
    }

    #[test]
    fn test_write_mp3_stereo() -> Result<()> {
        let sample_rate = 44100_u32;
        let frames = 44100_usize; // 1 second
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

        let tmp = std::env::temp_dir().join("phonon_test_stereo.mp3");
        write_mp3(&tmp, &buffer, &Mp3Config::default())?;

        let metadata = std::fs::metadata(&tmp)
            .map_err(PhononError::Io)?;
        assert!(metadata.len() > 0);

        let decoded = crate::read_audio(&tmp)?;
        assert_eq!(decoded.channels, 2);

        let _ = std::fs::remove_file(&tmp);
        Ok(())
    }

    #[test]
    fn test_write_mp3_custom_bitrate() -> Result<()> {
        let buffer = AudioBuffer {
            samples: vec![0.0; 44100],
            sample_rate: 44100,
            channels: 1,
        };

        let config_128 = Mp3Config { bitrate_kbps: 128 };
        let config_320 = Mp3Config { bitrate_kbps: 320 };

        let tmp_128 = std::env::temp_dir().join("phonon_test_128k.mp3");
        let tmp_320 = std::env::temp_dir().join("phonon_test_320k.mp3");

        write_mp3(&tmp_128, &buffer, &config_128)?;
        write_mp3(&tmp_320, &buffer, &config_320)?;

        let size_128 = std::fs::metadata(&tmp_128)
            .map_err(PhononError::Io)?.len();
        let size_320 = std::fs::metadata(&tmp_320)
            .map_err(PhononError::Io)?.len();

        // Higher bitrate should produce a larger file
        assert!(size_320 > size_128, "320k ({size_320}) should be larger than 128k ({size_128})");

        let _ = std::fs::remove_file(&tmp_128);
        let _ = std::fs::remove_file(&tmp_320);
        Ok(())
    }

    #[test]
    fn test_write_mp3_invalid_channels() {
        let buffer = AudioBuffer {
            samples: vec![0.0; 100],
            sample_rate: 44100,
            channels: 3,
        };
        assert!(write_mp3("/tmp/bad.mp3", &buffer, &Mp3Config::default()).is_err());
    }
}
