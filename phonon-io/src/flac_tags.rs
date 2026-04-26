//! FLAC tagging — Vorbis Comment block + cover art.
//!
//! Encodes the FLAC stream first via [`crate::flac::write_flac`], then opens
//! the file with [`metaflac`] to attach a Vorbis Comment metadata block and
//! optional cover art picture block.

use crate::flac::{FlacConfig, write_flac};
use crate::tags::PodcastTags;
use metaflac::Tag;
use metaflac::block::PictureType;
use phonon_core::{AudioBuffer, PhononError, Result};
use std::path::Path;
use tracing::{info, instrument};

/// Encode a FLAC file with embedded Vorbis Comment tags + cover art.
#[instrument(
  skip(buffer, path, flac_cfg, tags),
  fields(path = %path.as_ref().display(), duration = buffer.duration_secs())
)]
pub fn write_flac_tagged(
  path: impl AsRef<Path>,
  buffer: &AudioBuffer,
  flac_cfg: &FlacConfig,
  tags: &PodcastTags,
) -> Result<()> {
  let path = path.as_ref();
  write_flac(path, buffer, flac_cfg)?;
  attach_flac_tag(path, tags)?;
  info!("FLAC tagged with metadata");
  Ok(())
}

/// Attach (or replace) Vorbis Comment block + cover on an existing FLAC file.
pub fn attach_flac_tag(path: impl AsRef<Path>, tags: &PodcastTags) -> Result<()> {
  let path = path.as_ref();
  let mut tag =
    Tag::read_from_path(path).map_err(|e| PhononError::Wav(format!("FLAC read tag: {}", e)))?;

  // Vorbis Comment fields use the standard names from xiph.org spec.
  let comments = tag.vorbis_comments_mut();
  // Clear prior values for the keys we set, so this is replace-semantics.
  if let Some(t) = &tags.title {
    comments.set_title(vec![t.clone()]);
  }
  if let Some(a) = &tags.artist {
    comments.set_artist(vec![a.clone()]);
  }
  if let Some(a) = &tags.album {
    comments.set_album(vec![a.clone()]);
  }
  if let Some(y) = tags.year {
    comments.set("DATE", vec![y.to_string()]);
  }
  if let Some(g) = &tags.genre {
    comments.set_genre(vec![g.clone()]);
  }
  if let Some(c) = &tags.comment {
    comments.set("COMMENT", vec![c.clone()]);
  }
  if let Some(lang) = &tags.language {
    comments.set("LANGUAGE", vec![lang.clone()]);
  }
  if let Some(t) = tags.track {
    comments.set_track(t);
  }

  if let Some(cover) = &tags.cover {
    tag.add_picture(
      cover.mime.as_str().to_string(),
      PictureType::CoverFront,
      cover.data.clone(),
    );
  }

  tag
    .save()
    .map_err(|e| PhononError::Wav(format!("FLAC save tag: {}", e)))?;
  Ok(())
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
  fn test_write_flac_tagged_basic() -> Result<()> {
    let buf = make_buffer();
    let tags = PodcastTags {
      title: Some("Episode A".into()),
      artist: Some("Show".into()),
      album: Some("Season 1".into()),
      year: Some(2026),
      genre: Some("Podcast".into()),
      comment: Some("Notes here".into()),
      language: Some("eng".into()),
      track: Some(3),
      cover: None,
    };
    let tmp = std::env::temp_dir().join("phonon_flac_basic.flac");
    write_flac_tagged(&tmp, &buf, &FlacConfig::default(), &tags)?;

    let read =
      Tag::read_from_path(&tmp).map_err(|e| PhononError::Wav(format!("Read tag failed: {}", e)))?;
    let comments = read.vorbis_comments().expect("vorbis comments present");
    assert_eq!(comments.title().map(|v| v[0].as_str()), Some("Episode A"));
    assert_eq!(comments.artist().map(|v| v[0].as_str()), Some("Show"));
    assert_eq!(comments.album().map(|v| v[0].as_str()), Some("Season 1"));
    assert_eq!(comments.genre().map(|v| v[0].as_str()), Some("Podcast"));
    assert!(
      comments
        .get("DATE")
        .and_then(|v| v.first())
        .map(|s| s.as_str())
        == Some("2026")
    );
    assert!(
      comments
        .get("LANGUAGE")
        .and_then(|v| v.first())
        .map(|s| s.as_str())
        == Some("eng")
    );

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_write_flac_tagged_with_cover() -> Result<()> {
    let buf = make_buffer();
    let tags = PodcastTags {
      title: Some("With Cover".into()),
      cover: Some(CoverArt {
        mime: CoverMime::Png,
        data: vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A],
        description: None,
      }),
      ..Default::default()
    };
    let tmp = std::env::temp_dir().join("phonon_flac_cover.flac");
    write_flac_tagged(&tmp, &buf, &FlacConfig::default(), &tags)?;

    let read =
      Tag::read_from_path(&tmp).map_err(|e| PhononError::Wav(format!("Read tag failed: {}", e)))?;
    let pictures: Vec<_> = read.pictures().collect();
    assert_eq!(pictures.len(), 1);
    assert_eq!(pictures[0].mime_type, "image/png");
    assert_eq!(pictures[0].picture_type, PictureType::CoverFront);

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_attach_flac_tag_replaces() -> Result<()> {
    let buf = make_buffer();
    let tmp = std::env::temp_dir().join("phonon_flac_replace.flac");
    write_flac(&tmp, &buf, &FlacConfig::default())?;

    let first = PodcastTags {
      title: Some("First".into()),
      ..Default::default()
    };
    attach_flac_tag(&tmp, &first)?;
    let second = PodcastTags {
      title: Some("Second".into()),
      ..Default::default()
    };
    attach_flac_tag(&tmp, &second)?;

    let read =
      Tag::read_from_path(&tmp).map_err(|e| PhononError::Wav(format!("Read tag failed: {}", e)))?;
    let comments = read.vorbis_comments().expect("vorbis comments present");
    assert_eq!(comments.title().map(|v| v[0].as_str()), Some("Second"));

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }
}
