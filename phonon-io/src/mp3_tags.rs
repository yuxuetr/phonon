//! MP3 tagging — ID3v2.4 metadata + PodcastIndex chapters.
//!
//! Builds on the [`crate::mp3`] encoder by writing ID3v2 frames after
//! encoding. Supports basic podcast tags (title, author, album, year,
//! genre, comment, language, track, cover art) and CHAP/CTOC chapter
//! frames so podcast players can jump between sections.

use crate::mp3::{Mp3Config, write_mp3};
use crate::tags::{AudioChapter, PodcastTags};
use id3::frame::{Chapter as Id3Chapter, Content, Picture, PictureType, TableOfContents};
use id3::{Frame, Tag, TagLike, Version};
use phonon_core::{AudioBuffer, PhononError, Result};
use std::path::Path;
use tracing::{info, instrument};

/// Encode an MP3 with embedded ID3v2 metadata and chapter markers.
///
/// Calls [`write_mp3`] first, then attaches an ID3v2.4 tag to the resulting
/// file. Chapters are written as `CHAP` frames plus a top-level `CTOC`
/// table-of-contents frame, matching the convention used by Apple Podcasts,
/// Pocket Casts, and other PodcastIndex consumers.
#[instrument(
  skip(buffer, path, mp3_cfg, tags, chapters),
  fields(
    path = %path.as_ref().display(),
    duration = buffer.duration_secs(),
    chapters = chapters.len()
  )
)]
pub fn write_mp3_tagged(
  path: impl AsRef<Path>,
  buffer: &AudioBuffer,
  mp3_cfg: &Mp3Config,
  tags: &PodcastTags,
  chapters: &[AudioChapter],
) -> Result<()> {
  let path = path.as_ref();
  write_mp3(path, buffer, mp3_cfg)?;
  attach_tag(path, tags, chapters)?;
  info!("MP3 tagged with metadata + chapters");
  Ok(())
}

/// Attach (or replace) an ID3v2 tag on an existing MP3 file.
///
/// Useful when the MP3 has already been produced (e.g. by a separate pipeline)
/// and only needs metadata. Replaces any prior ID3v2 tag.
pub fn attach_tag(
  path: impl AsRef<Path>,
  tags: &PodcastTags,
  chapters: &[AudioChapter],
) -> Result<()> {
  let mut tag = Tag::new();

  if let Some(t) = &tags.title {
    tag.set_title(t);
  }
  if let Some(a) = &tags.artist {
    tag.set_artist(a);
  }
  if let Some(a) = &tags.album {
    tag.set_album(a);
  }
  if let Some(y) = tags.year {
    tag.set_year(y);
  }
  if let Some(g) = &tags.genre {
    tag.set_genre(g);
  }
  if let Some(c) = &tags.comment {
    tag.add_frame(Frame::with_content(
      "COMM",
      Content::Comment(id3::frame::Comment {
        lang: tags.language.clone().unwrap_or_else(|| "eng".to_string()),
        description: String::new(),
        text: c.clone(),
      }),
    ));
  }
  if let Some(lang) = &tags.language {
    tag.add_frame(Frame::with_content("TLAN", Content::Text(lang.clone())));
  }
  if let Some(t) = tags.track {
    tag.set_track(t);
  }
  if let Some(cover) = &tags.cover {
    tag.add_frame(Frame::with_content(
      "APIC",
      Content::Picture(Picture {
        mime_type: cover.mime.as_str().to_string(),
        picture_type: PictureType::CoverFront,
        description: cover.description.clone().unwrap_or_else(|| "Cover".into()),
        data: cover.data.clone(),
      }),
    ));
  }

  // Chapters: one CHAP per entry, plus a top-level CTOC referencing them.
  if !chapters.is_empty() {
    let mut element_ids = Vec::with_capacity(chapters.len());
    for (i, ch) in chapters.iter().enumerate() {
      let element_id = format!("ch{}", i);
      element_ids.push(element_id.clone());

      let mut sub_frames = Vec::new();
      sub_frames.push(Frame::with_content(
        "TIT2",
        Content::Text(ch.title.clone()),
      ));

      let chap = Id3Chapter {
        element_id,
        start_time: secs_to_ms(ch.start_secs),
        end_time: secs_to_ms(ch.end_secs.max(ch.start_secs)),
        start_offset: 0xFFFFFFFF,
        end_offset: 0xFFFFFFFF,
        frames: sub_frames,
      };
      tag.add_frame(Frame::with_content("CHAP", Content::Chapter(chap)));
    }

    let toc = TableOfContents {
      element_id: "toc".to_string(),
      top_level: true,
      ordered: true,
      elements: element_ids,
      frames: Vec::new(),
    };
    tag.add_frame(Frame::with_content("CTOC", Content::TableOfContents(toc)));
  }

  tag
    .write_to_path(path.as_ref(), Version::Id3v24)
    .map_err(|e| PhononError::Wav(format!("ID3 write error: {}", e)))?;
  Ok(())
}

fn secs_to_ms(secs: f64) -> u32 {
  let ms = (secs * 1000.0).max(0.0);
  if ms > u32::MAX as f64 {
    u32::MAX
  } else {
    ms as u32
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::tags::{CoverArt, CoverMime};

  fn make_buffer() -> AudioBuffer {
    let sr = 44100_u32;
    let n = sr as usize; // 1 s
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
  fn test_write_mp3_tagged_basic_fields() -> Result<()> {
    let buf = make_buffer();
    let tags = PodcastTags {
      title: Some("Episode 1: Hello".into()),
      artist: Some("My Podcast".into()),
      album: Some("Season 1".into()),
      year: Some(2026),
      genre: Some("Podcast".into()),
      comment: Some("First episode notes.".into()),
      language: Some("eng".into()),
      track: Some(1),
      cover: None,
    };

    let tmp = std::env::temp_dir().join("phonon_tagged_basic.mp3");
    write_mp3_tagged(&tmp, &buf, &Mp3Config::default(), &tags, &[])?;

    // Read back the tag.
    let read_tag = Tag::read_from_path(&tmp)
      .map_err(|e| PhononError::Wav(format!("Read tag failed: {}", e)))?;
    assert_eq!(read_tag.title(), Some("Episode 1: Hello"));
    assert_eq!(read_tag.artist(), Some("My Podcast"));
    assert_eq!(read_tag.album(), Some("Season 1"));
    assert_eq!(read_tag.year(), Some(2026));
    assert_eq!(read_tag.genre(), Some("Podcast"));
    assert_eq!(read_tag.track(), Some(1));

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_write_mp3_tagged_with_cover() -> Result<()> {
    let buf = make_buffer();
    // Tiny placeholder JPEG header bytes. The ID3 spec doesn't validate
    // image data, only that the MIME type matches what's stored.
    let tags = PodcastTags {
      title: Some("With Cover".into()),
      cover: Some(CoverArt {
        mime: CoverMime::Jpeg,
        data: vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0xFF, 0xD9],
        description: Some("Album art".into()),
      }),
      ..Default::default()
    };

    let tmp = std::env::temp_dir().join("phonon_tagged_cover.mp3");
    write_mp3_tagged(&tmp, &buf, &Mp3Config::default(), &tags, &[])?;

    let read_tag = Tag::read_from_path(&tmp)
      .map_err(|e| PhononError::Wav(format!("Read tag failed: {}", e)))?;
    let pictures: Vec<_> = read_tag.pictures().collect();
    assert_eq!(pictures.len(), 1);
    assert_eq!(pictures[0].mime_type, "image/jpeg");
    assert_eq!(pictures[0].picture_type, PictureType::CoverFront);

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_write_mp3_tagged_with_chapters() -> Result<()> {
    let buf = make_buffer();
    let tags = PodcastTags {
      title: Some("Chaptered".into()),
      ..Default::default()
    };
    let chapters = vec![
      AudioChapter::new("Intro", 0.0, 0.3),
      AudioChapter::new("Main", 0.3, 0.7),
      AudioChapter::new("Outro", 0.7, 1.0),
    ];

    let tmp = std::env::temp_dir().join("phonon_tagged_chapters.mp3");
    write_mp3_tagged(&tmp, &buf, &Mp3Config::default(), &tags, &chapters)?;

    let read_tag = Tag::read_from_path(&tmp)
      .map_err(|e| PhononError::Wav(format!("Read tag failed: {}", e)))?;

    // Count CHAP frames.
    let chap_frames: Vec<_> = read_tag
      .frames()
      .filter(|f| f.id() == "CHAP")
      .collect();
    assert_eq!(chap_frames.len(), 3, "expected 3 CHAP frames");

    // Verify CTOC exists and references all 3 chapters.
    let toc_frame = read_tag.frames().find(|f| f.id() == "CTOC");
    assert!(toc_frame.is_some(), "expected one CTOC frame");
    if let Some(Content::TableOfContents(toc)) = toc_frame.map(|f| f.content()) {
      assert_eq!(toc.elements.len(), 3);
      assert!(toc.top_level);
      assert!(toc.ordered);
    } else {
      panic!("CTOC frame had wrong content type");
    }

    // Verify chapter timings (in ms).
    if let Some(Content::Chapter(ch)) = chap_frames[0].content().into() {
      assert_eq!(ch.start_time, 0);
      assert_eq!(ch.end_time, 300);
    }

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_attach_tag_replaces_existing() -> Result<()> {
    let buf = make_buffer();
    let tmp = std::env::temp_dir().join("phonon_attach_replace.mp3");
    write_mp3(&tmp, &buf, &Mp3Config::default())?;

    let first = PodcastTags {
      title: Some("First".into()),
      ..Default::default()
    };
    attach_tag(&tmp, &first, &[])?;

    let second = PodcastTags {
      title: Some("Second".into()),
      ..Default::default()
    };
    attach_tag(&tmp, &second, &[])?;

    let read_tag = Tag::read_from_path(&tmp)
      .map_err(|e| PhononError::Wav(format!("Read tag failed: {}", e)))?;
    assert_eq!(read_tag.title(), Some("Second"));

    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_secs_to_ms_clamps_negative() {
    assert_eq!(secs_to_ms(-1.0), 0);
    assert_eq!(secs_to_ms(0.0), 0);
    assert_eq!(secs_to_ms(1.5), 1500);
    assert_eq!(secs_to_ms(0.123), 123);
  }
}
