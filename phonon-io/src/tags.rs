//! Shared tag types used by every output format.
//!
//! [`PodcastTags`] is the canonical metadata struct; format-specific writers
//! (`mp3_tags`, `flac_tags`, `ogg_tags`) consume it and translate to the
//! native frame/block representation.

/// MIME type of an embedded cover image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverMime {
  Jpeg,
  Png,
}

impl CoverMime {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Jpeg => "image/jpeg",
      Self::Png => "image/png",
    }
  }
}

/// Cover image to embed in the file.
#[derive(Debug, Clone)]
pub struct CoverArt {
  pub mime: CoverMime,
  pub data: Vec<u8>,
  /// Optional human-readable description.
  pub description: Option<String>,
}

/// Generic podcast / track metadata.
///
/// All fields are optional; only the `Some` ones are written. This struct is
/// shared by every format-specific writer; fields that a particular format
/// cannot represent are simply skipped.
#[derive(Debug, Clone, Default)]
pub struct PodcastTags {
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub year: Option<i32>,
  pub genre: Option<String>,
  pub comment: Option<String>,
  /// Language code, e.g. "eng", "zho".
  pub language: Option<String>,
  pub track: Option<u32>,
  pub cover: Option<CoverArt>,
}

/// A chapter marker (currently used by MP3 only — FLAC and OGG have no
/// standard chapter representation).
#[derive(Debug, Clone)]
pub struct AudioChapter {
  pub title: String,
  pub start_secs: f64,
  pub end_secs: f64,
}

impl AudioChapter {
  pub fn new(title: impl Into<String>, start_secs: f64, end_secs: f64) -> Self {
    Self {
      title: title.into(),
      start_secs,
      end_secs,
    }
  }
}
