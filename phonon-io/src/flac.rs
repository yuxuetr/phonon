use phonon_core::{AudioBuffer, PhononError, Result};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use tracing::{info, instrument};

use flacenc::bitsink::ByteSink;
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use flacenc::source::MemSource;

/// FLAC encoding configuration.
#[derive(Debug, Clone)]
pub struct FlacConfig {
  /// Bits per sample for FLAC output (16 or 24). Default: 16.
  pub bits_per_sample: u32,
}

impl Default for FlacConfig {
  fn default() -> Self {
    Self {
      bits_per_sample: 16,
    }
  }
}

/// Writes an `AudioBuffer` to a FLAC file.
///
/// Supports mono (1 channel) and stereo (2 channels).
/// Samples are converted from f32 [-1.0, 1.0] to interleaved i32
/// at the specified bit depth before encoding.
#[instrument(skip(buffer, path), fields(path = %path.as_ref().display(), duration = buffer.duration_secs()))]
pub fn write_flac<P: AsRef<Path>>(
  path: P,
  buffer: &AudioBuffer,
  config: &FlacConfig,
) -> Result<()> {
  info!("Encoding FLAC");

  if buffer.channels == 0 || buffer.channels > 8 {
    return Err(PhononError::Wav(format!(
      "FLAC encoding supports 1–8 channels, got {}",
      buffer.channels
    )));
  }

  if config.bits_per_sample != 16 && config.bits_per_sample != 24 {
    return Err(PhononError::Wav(format!(
      "FLAC bits_per_sample must be 16 or 24, got {}",
      config.bits_per_sample
    )));
  }

  // Convert f32 samples to interleaved i32 at the target bit depth
  let max_val = (1_i64 << (config.bits_per_sample - 1)) - 1;
  let samples_i32: Vec<i32> = buffer
    .samples
    .iter()
    .map(|&s| {
      let clamped = s.clamp(-1.0, 1.0) as f64;
      (clamped * max_val as f64) as i32
    })
    .collect();

  // Configure the FLAC encoder
  let enc_config = flacenc::config::Encoder::default()
    .into_verified()
    .map_err(|e| PhononError::Wav(format!("FLAC config error: {:?}", e.1)))?;

  let source = MemSource::from_samples(
    &samples_i32,
    buffer.channels as usize,
    config.bits_per_sample as usize,
    buffer.sample_rate as usize,
  );

  let mut flac_stream =
    flacenc::encode_with_fixed_block_size(&enc_config, source, enc_config.block_size)
      .map_err(|e| PhononError::Wav(format!("FLAC encode error: {e}")))?;

  // flacenc counts the short final block as the stream's minimum block
  // size; the spec excludes the last block, and symphonia refuses to open
  // such a fixed-block stream ("end of stream"). Multi-frame streams use
  // exactly `block_size` everywhere else.
  let block_size = enc_config.block_size;
  if flac_stream.stream_info().max_block_size() == block_size {
    flac_stream
      .stream_info_mut()
      .set_block_sizes(block_size, block_size)
      .map_err(|e| PhononError::Wav(format!("FLAC stream info error: {e:?}")))?;
  }

  // Write encoded stream to bytes
  let mut sink = ByteSink::new();
  flac_stream
    .write(&mut sink)
    .map_err(|e| PhononError::Wav(format!("FLAC stream write error: {e:?}")))?;

  // Write to file
  let mut file = File::create(path.as_ref()).map_err(PhononError::Io)?;
  file.write_all(sink.as_slice()).map_err(PhononError::Io)?;

  info!(bytes = sink.as_slice().len(), "FLAC encoding complete");
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Verify the first 4 bytes of a file are the FLAC magic "fLaC".
  fn assert_valid_flac_header(path: &std::path::Path) -> Result<()> {
    let data = std::fs::read(path).map_err(PhononError::Io)?;
    assert!(data.len() > 4, "FLAC file too small");
    assert_eq!(&data[..4], b"fLaC", "Missing FLAC magic bytes");
    Ok(())
  }

  #[test]
  fn test_write_flac_reads_back() -> Result<()> {
    // 2 s is not a multiple of the 4096-sample block, so the last block is
    // short, which is the case symphonia rejected.
    for channels in [1u16, 2] {
      let frames = 88_200usize;
      let buffer = AudioBuffer {
        samples: (0..frames * channels as usize)
          .map(|i| ((i / channels as usize) as f32 * 0.0627).sin() * 0.5)
          .collect(),
        sample_rate: 44_100,
        channels,
      };
      let tmp = std::env::temp_dir().join(format!("phonon_test_readback_{channels}.flac"));
      write_flac(&tmp, &buffer, &FlacConfig::default())?;
      let decoded = crate::read_audio(&tmp);
      let _ = std::fs::remove_file(&tmp);
      let decoded = decoded?;
      assert_eq!(decoded.channels, channels);
      assert_eq!(decoded.samples.len(), buffer.samples.len());
      for (a, b) in decoded.samples.iter().zip(&buffer.samples) {
        assert!((a - b).abs() < 1e-3, "{a} vs {b}");
      }
    }
    Ok(())
  }

  #[test]
  fn test_write_flac_mono() -> Result<()> {
    let sample_rate = 44100_u32;
    let total_samples = 44100_usize; // 1 second

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

    let tmp = std::env::temp_dir().join("phonon_test_mono.flac");
    write_flac(&tmp, &buffer, &FlacConfig::default())?;

    // Verify file is a valid FLAC (magic bytes + non-empty)
    let metadata = std::fs::metadata(&tmp).map_err(PhononError::Io)?;
    assert!(metadata.len() > 0, "FLAC file should not be empty");
    assert_valid_flac_header(&tmp)?;

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_write_flac_stereo() -> Result<()> {
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

    let tmp = std::env::temp_dir().join("phonon_test_stereo.flac");
    write_flac(&tmp, &buffer, &FlacConfig::default())?;

    let metadata = std::fs::metadata(&tmp).map_err(PhononError::Io)?;
    assert!(metadata.len() > 0);
    assert_valid_flac_header(&tmp)?;

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_write_flac_24bit() -> Result<()> {
    let buffer = AudioBuffer {
      samples: vec![0.0, 0.5, -0.5, 1.0, -1.0],
      sample_rate: 44100,
      channels: 1,
    };

    let config = FlacConfig {
      bits_per_sample: 24,
    };

    let tmp = std::env::temp_dir().join("phonon_test_24bit.flac");
    write_flac(&tmp, &buffer, &config)?;

    let metadata = std::fs::metadata(&tmp).map_err(PhononError::Io)?;
    assert!(metadata.len() > 0);

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_write_flac_invalid_channels() {
    let buffer = AudioBuffer {
      samples: vec![0.0; 100],
      sample_rate: 44100,
      channels: 0,
    };
    assert!(write_flac("/tmp/bad.flac", &buffer, &FlacConfig::default()).is_err());
  }

  #[test]
  fn test_write_flac_invalid_bits() {
    let buffer = AudioBuffer {
      samples: vec![0.0; 100],
      sample_rate: 44100,
      channels: 1,
    };
    let config = FlacConfig {
      bits_per_sample: 32,
    };
    assert!(write_flac("/tmp/bad.flac", &buffer, &config).is_err());
  }
}
