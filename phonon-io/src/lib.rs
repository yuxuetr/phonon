use phonon_core::{AudioBuffer, PhononError, Result};
use std::ffi::OsStr;
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use tracing::{info, instrument};

pub mod aac;
pub mod flac;
pub mod flac_tags;
pub mod mp3;
pub mod mp3_tags;
pub mod ogg;
pub mod ogg_tags;
pub mod tags;

pub use aac::{AacConfig, write_aac};
pub use flac::{FlacConfig, write_flac};
pub use flac_tags::write_flac_tagged;
pub use mp3::{Mp3Config, write_mp3};
pub use mp3_tags::{attach_tag, write_mp3_tagged};
pub use ogg::{OggConfig, write_ogg};
pub use ogg_tags::write_ogg_tagged;
pub use tags::{AudioChapter, CoverArt, CoverMime, PodcastTags};

/// Reads an audio file and decodes it into an AudioBuffer.
///
/// Supports all formats supported by Symphonia (MP3, WAV, FLAC, OGG, AAC, etc.).
///
/// # Example
///
/// ```rust,no_run
/// let buffer = phonon_io::read_audio("song.wav").unwrap();
/// println!("Duration: {:.2}s", buffer.duration_secs());
/// ```
#[instrument(skip(path), fields(path = %path.as_ref().display()))]
pub fn read_audio<P: AsRef<Path>>(path: P) -> Result<AudioBuffer> {
  let path = path.as_ref();
  let src = File::open(path).map_err(PhononError::Io)?;

  // Create the media source stream.
  let mss = MediaSourceStream::new(Box::new(src), Default::default());

  // Create a probe hint using the file's extension. [Optional]
  let mut hint = Hint::new();
  if let Some(extension) = path.extension() {
    if let Some(extension_str) = extension.to_str() {
      hint.with_extension(extension_str);
    }
  }

  // Use the default options for metadata and format readers.
  let meta_opts: MetadataOptions = Default::default();
  let fmt_opts: FormatOptions = Default::default();

  // Probe the media source.
  let probed = symphonia::default::get_probe()
    .format(&hint, mss, &fmt_opts, &meta_opts)
    .map_err(|e| PhononError::Io(std::io::Error::other(e.to_string())))?;

  // Get the instantiated format reader.
  let mut format = probed.format;

  // Find the first audio track with a known (decodeable) codec.
  let track = format
    .tracks()
    .iter()
    .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
    .ok_or_else(|| PhononError::Wav("No supported audio track found".to_string()))?;

  // Use the default options for the decoder.
  let dec_opts: DecoderOptions = Default::default();

  // Create a decoder for the track.
  let mut decoder = symphonia::default::get_codecs()
    .make(&track.codec_params, &dec_opts)
    .map_err(|e| PhononError::Wav(format!("Unsupported codec: {}", e)))?;

  // Store the track identifier, it will be used to filter packets.
  let track_id = track.id;
  // Declared format, used when no packet decodes (e.g. a header-only WAV).
  let declared_rate = track.codec_params.sample_rate;
  let declared_channels = track.codec_params.channels.map(|c| c.count() as u16);

  let mut samples: Vec<f32> = Vec::new();
  let mut sample_rate = 0;
  let mut channels = 0;

  // The decode loop.
  loop {
    // Get the next packet from the media format.
    let packet = match format.next_packet() {
      Ok(packet) => packet,
      Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
        break; // End of stream
      }
      Err(e) => return Err(PhononError::Wav(format!("Failed to read packet: {}", e))),
    };

    // Consume any new metadata that has been read since the last packet.
    while !format.metadata().is_latest() {
      format.metadata().pop();
    }

    // If the packet does not belong to the selected track, skip it.
    if packet.track_id() != track_id {
      continue;
    }

    // Decode the packet into audio samples.
    match decoder.decode(&packet) {
      Ok(decoded) => {
        // Get the audio buffer specification.
        let spec = *decoded.spec();

        // Initialize metadata on first packet
        if sample_rate == 0 {
          sample_rate = spec.rate;
          channels = spec.channels.count() as u16;
        }

        // Copy the decoded audio samples into a SampleBuffer for easy conversion.
        let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);

        // The samples are now floating point numbers.
        samples.extend_from_slice(sample_buf.samples());
      }
      Err(Error::IoError(e)) => return Err(PhononError::Io(e)),
      Err(Error::DecodeError(e)) => return Err(PhononError::Wav(format!("Decode error: {}", e))),
      Err(e) => return Err(PhononError::Wav(format!("Unhandled decode error: {}", e))),
    }
  }

  if sample_rate == 0 || channels == 0 {
    match (declared_rate, declared_channels) {
      (Some(rate), Some(ch)) if rate > 0 && ch > 0 => {
        sample_rate = rate;
        channels = ch;
      }
      _ => {
        return Err(PhononError::Wav(
          "No audio decoded and the track declares no sample rate / channel layout".to_string(),
        ));
      }
    }
  }

  info!(
    samples = samples.len(),
    sample_rate, channels, "Audio loaded"
  );

  Ok(AudioBuffer {
    samples,
    sample_rate,
    channels,
  })
}

/// Writes an AudioBuffer to an audio file.
///
/// The output format is determined by the file extension:
/// - `.wav` — WAV format (16-bit PCM) via phonon-wav
/// - `.mp3` — MP3 format (192 kbps default) via LAME encoder
/// - `.flac` — FLAC format (16-bit lossless) via flacenc
/// - `.ogg` — OGG Vorbis format (quality 0.5 VBR) via vorbis_rs
/// - `.aac` — AAC format (128 kbps ADTS) via fdk-aac
///
/// # Example
///
/// ```rust,no_run
/// use phonon_core::AudioBuffer;
///
/// let buffer = AudioBuffer { samples: vec![0.0; 44100], sample_rate: 44100, channels: 1 };
/// phonon_io::write_audio("output.wav", &buffer).unwrap();
/// phonon_io::write_audio("output.mp3", &buffer).unwrap();
/// phonon_io::write_audio("output.flac", &buffer).unwrap();
/// phonon_io::write_audio("output.ogg", &buffer).unwrap();
/// phonon_io::write_audio("output.aac", &buffer).unwrap();
/// ```
#[instrument(skip(buffer, path), fields(path = %path.as_ref().display(), duration = buffer.duration_secs()))]
pub fn write_audio<P: AsRef<Path>>(path: P, buffer: &AudioBuffer) -> Result<()> {
  let path = path.as_ref();
  let extension = path.extension().and_then(OsStr::to_str).unwrap_or("wav");

  match extension.to_lowercase().as_str() {
    "wav" => {
      phonon_wav::write_wav(path, buffer)?;
    }
    "mp3" => {
      mp3::write_mp3(path, buffer, &Mp3Config::default())?;
    }
    "flac" => {
      flac::write_flac(path, buffer, &FlacConfig::default())?;
    }
    "ogg" | "oga" => {
      ogg::write_ogg(path, buffer, &OggConfig::default())?;
    }
    "aac" => {
      aac::write_aac(path, buffer, &AacConfig::default())?;
    }
    other => {
      return Err(PhononError::Wav(format!(
        "Unsupported output format: .{}. Supported: wav, mp3, flac, ogg, aac.",
        other
      )));
    }
  }

  info!("Audio written successfully");
  Ok(())
}

/// Writes an AudioBuffer to an MP3 file with custom configuration.
///
/// Use this when you need control over bitrate or other encoding options.
#[instrument(skip(buffer, path, config), fields(path = %path.as_ref().display(), bitrate = config.bitrate_kbps))]
pub fn write_mp3_with_config<P: AsRef<Path>>(
  path: P,
  buffer: &AudioBuffer,
  config: &Mp3Config,
) -> Result<()> {
  mp3::write_mp3(path, buffer, config)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_read_empty_wav_keeps_declared_format() -> Result<()> {
    let path = std::env::temp_dir().join("phonon_io_empty_stereo.wav");
    write_audio(&path, &AudioBuffer::new(48_000, 2, 0))?;
    let loaded = read_audio(&path)?;
    assert!(loaded.samples.is_empty());
    assert_eq!(loaded.sample_rate, 48_000);
    assert_eq!(loaded.channels, 2);
    Ok(())
  }
}
