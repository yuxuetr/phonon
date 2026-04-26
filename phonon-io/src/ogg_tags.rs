//! OGG Vorbis tagging — writes Vorbis Comments inline during encoding.
//!
//! Unlike MP3 (where ID3 is appended after encoding) and FLAC (where the
//! comment block can be patched in), OGG Vorbis comments must be set
//! *before* the encoder is built. This module wraps `vorbis_rs` so the same
//! [`PodcastTags`] surface used by the other formats can drive it.
//!
//! Cover art is embedded using the `METADATA_BLOCK_PICTURE` Vorbis comment
//! convention (base64-encoded FLAC picture block) — the de-facto standard
//! used by every modern OGG/Vorbis player.

use crate::ogg::OggConfig;
use crate::tags::{CoverArt, PodcastTags};
use phonon_core::{AudioBuffer, PhononError, Result};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::num::NonZero;
use std::path::Path;
use tracing::{info, instrument};
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoderBuilder};

/// Encode an OGG Vorbis file with embedded Vorbis comments + cover art.
#[instrument(
  skip(buffer, path, ogg_cfg, tags),
  fields(path = %path.as_ref().display(), duration = buffer.duration_secs())
)]
pub fn write_ogg_tagged(
  path: impl AsRef<Path>,
  buffer: &AudioBuffer,
  ogg_cfg: &OggConfig,
  tags: &PodcastTags,
) -> Result<()> {
  let ch = buffer.channels as usize;
  if ch == 0 || ch > 2 {
    return Err(PhononError::Wav(format!(
      "OGG Vorbis encoding supports 1 or 2 channels, got {}",
      buffer.channels
    )));
  }

  let sample_rate_nz = NonZero::new(buffer.sample_rate)
    .ok_or_else(|| PhononError::Wav("Sample rate must be non-zero".to_string()))?;
  let channels_nz = NonZero::new(buffer.channels as u8)
    .ok_or_else(|| PhononError::Wav("Channels must be non-zero".to_string()))?;

  let file = File::create(path.as_ref()).map_err(PhononError::Io)?;
  let mut writer = BufWriter::new(file);

  let mut builder = VorbisEncoderBuilder::new(sample_rate_nz, channels_nz, &mut writer)
    .map_err(|e| PhononError::Wav(format!("OGG encoder init error: {e}")))?;

  apply_tag_builder(&mut builder, tags)?;

  builder.bitrate_management_strategy(VorbisBitrateManagementStrategy::QualityVbr {
    target_quality: ogg_cfg.quality,
  });

  let mut encoder = builder
    .build()
    .map_err(|e| PhononError::Wav(format!("OGG encoder build error: {e}")))?;

  let frame_count = buffer.samples.len() / ch;
  let mut planar: Vec<Vec<f32>> = vec![Vec::with_capacity(frame_count); ch];
  for frame in 0..frame_count {
    for (c, channel) in planar.iter_mut().enumerate() {
      channel.push(buffer.samples[frame * ch + c]);
    }
  }

  let block_size = 1024;
  let mut offset = 0;
  while offset < frame_count {
    let end = (offset + block_size).min(frame_count);
    let block_slices: Vec<&[f32]> = planar.iter().map(|channel| &channel[offset..end]).collect();
    encoder
      .encode_audio_block(block_slices.as_slice())
      .map_err(|e| PhononError::Wav(format!("OGG encode block error: {e}")))?;
    offset = end;
  }

  encoder
    .finish()
    .map_err(|e| PhononError::Wav(format!("OGG finish error: {e}")))?;
  writer.flush().map_err(PhononError::Io)?;

  info!("OGG Vorbis tagged encoding complete");
  Ok(())
}

fn apply_tag_builder<W: Write>(
  builder: &mut VorbisEncoderBuilder<W>,
  tags: &PodcastTags,
) -> Result<()> {
  let map = |b: &mut VorbisEncoderBuilder<W>, key: &str, value: String| -> Result<()> {
    b.comment_tag(key.to_string(), value)
      .map_err(|e| PhononError::Wav(format!("OGG comment_tag '{}': {}", key, e)))?;
    Ok(())
  };

  if let Some(t) = &tags.title {
    map(builder, "TITLE", t.clone())?;
  }
  if let Some(a) = &tags.artist {
    map(builder, "ARTIST", a.clone())?;
  }
  if let Some(a) = &tags.album {
    map(builder, "ALBUM", a.clone())?;
  }
  if let Some(y) = tags.year {
    map(builder, "DATE", y.to_string())?;
  }
  if let Some(g) = &tags.genre {
    map(builder, "GENRE", g.clone())?;
  }
  if let Some(c) = &tags.comment {
    map(builder, "COMMENT", c.clone())?;
  }
  if let Some(lang) = &tags.language {
    map(builder, "LANGUAGE", lang.clone())?;
  }
  if let Some(t) = tags.track {
    map(builder, "TRACKNUMBER", t.to_string())?;
  }
  if let Some(cover) = &tags.cover {
    map(
      builder,
      "METADATA_BLOCK_PICTURE",
      encode_metadata_block_picture(cover),
    )?;
  }
  Ok(())
}

/// Encode a cover image as base64-encoded FLAC PICTURE block per the
/// Xiph "METADATA_BLOCK_PICTURE" convention.
fn encode_metadata_block_picture(cover: &CoverArt) -> String {
  let mime = cover.mime.as_str();
  let description = cover.description.as_deref().unwrap_or("");

  let mut block = Vec::with_capacity(32 + mime.len() + description.len() + cover.data.len());
  // Picture type (3 = CoverFront).
  block.extend_from_slice(&3_u32.to_be_bytes());
  // MIME type length + bytes.
  block.extend_from_slice(&(mime.len() as u32).to_be_bytes());
  block.extend_from_slice(mime.as_bytes());
  // Description length + bytes (UTF-8).
  block.extend_from_slice(&(description.len() as u32).to_be_bytes());
  block.extend_from_slice(description.as_bytes());
  // Width, height, color depth, indexed colors — unknown, leave 0.
  block.extend_from_slice(&0_u32.to_be_bytes());
  block.extend_from_slice(&0_u32.to_be_bytes());
  block.extend_from_slice(&0_u32.to_be_bytes());
  block.extend_from_slice(&0_u32.to_be_bytes());
  // Picture data length + bytes.
  block.extend_from_slice(&(cover.data.len() as u32).to_be_bytes());
  block.extend_from_slice(&cover.data);

  base64_encode(&block)
}

/// Minimal RFC 4648 base64 encoder so we don't pull in another dependency.
fn base64_encode(input: &[u8]) -> String {
  const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
  for chunk in input.chunks(3) {
    let b0 = chunk[0] as u32;
    let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
    let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
    let triple = (b0 << 16) | (b1 << 8) | b2;
    out.push(ALPHABET[((triple >> 18) & 0x3F) as usize] as char);
    out.push(ALPHABET[((triple >> 12) & 0x3F) as usize] as char);
    if chunk.len() >= 2 {
      out.push(ALPHABET[((triple >> 6) & 0x3F) as usize] as char);
    } else {
      out.push('=');
    }
    if chunk.len() == 3 {
      out.push(ALPHABET[(triple & 0x3F) as usize] as char);
    } else {
      out.push('=');
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::tags::{CoverArt, CoverMime};

  fn make_buffer() -> AudioBuffer {
    let sr = 44100_u32;
    let n = sr as usize;
    let samples: Vec<f32> = (0..n)
      .map(|i| {
        let t = i as f64 / sr as f64;
        (t * 440.0 * 2.0 * std::f64::consts::PI).sin() as f32 * 0.5
      })
      .collect();
    AudioBuffer {
      samples,
      sample_rate: sr,
      channels: 1,
    }
  }

  #[test]
  fn test_base64_roundtrip_known_vectors() {
    assert_eq!(base64_encode(b""), "");
    assert_eq!(base64_encode(b"f"), "Zg==");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_encode(b"foo"), "Zm9v");
    assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
    assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
  }

  #[test]
  fn test_metadata_block_picture_starts_with_picture_type_3() {
    let cover = CoverArt {
      mime: CoverMime::Jpeg,
      data: vec![0xFF, 0xD8],
      description: None,
    };
    let encoded = encode_metadata_block_picture(&cover);
    // Reverse base64 → decode the first 4 bytes; should be 0x00000003.
    let raw = decode_base64(&encoded);
    assert_eq!(raw[0..4], [0, 0, 0, 3]);
  }

  fn decode_base64(s: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let map = |c: u8| -> Option<u8> {
      match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
      }
    };
    let bytes: Vec<u8> = s.bytes().filter(|c| *c != b'=').collect();
    for chunk in bytes.chunks(4) {
      let mut buf = [0u8; 4];
      for (i, &c) in chunk.iter().enumerate() {
        buf[i] = map(c).unwrap_or(0);
      }
      let triple = ((buf[0] as u32) << 18)
        | ((buf[1] as u32) << 12)
        | ((buf[2] as u32) << 6)
        | (buf[3] as u32);
      out.push((triple >> 16) as u8);
      if chunk.len() >= 3 {
        out.push((triple >> 8) as u8);
      }
      if chunk.len() == 4 {
        out.push(triple as u8);
      }
    }
    out
  }

  #[test]
  fn test_write_ogg_tagged_produces_valid_file() -> Result<()> {
    let buf = make_buffer();
    let tags = PodcastTags {
      title: Some("OGG Test".into()),
      artist: Some("Phonon".into()),
      album: Some("Demo".into()),
      year: Some(2026),
      cover: Some(CoverArt {
        mime: CoverMime::Jpeg,
        data: vec![0xFF, 0xD8, 0xFF, 0xE0],
        description: Some("Cover".into()),
      }),
      ..Default::default()
    };
    let tmp = std::env::temp_dir().join("phonon_ogg_tagged.ogg");
    write_ogg_tagged(&tmp, &buf, &OggConfig::default(), &tags)?;

    // Verify the file exists, has content, and starts with the OggS magic bytes.
    let bytes = std::fs::read(&tmp).map_err(PhononError::Io)?;
    assert!(bytes.len() > 100);
    assert_eq!(&bytes[..4], b"OggS");

    // Verify our tag string appears in the file (Vorbis comments are stored
    // in plain text inside the OGG container, so a substring check is robust).
    let haystack = String::from_utf8_lossy(&bytes);
    assert!(
      haystack.contains("OGG Test"),
      "TITLE not found in encoded file"
    );
    assert!(haystack.contains("Phonon"), "ARTIST not found");

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }
}
