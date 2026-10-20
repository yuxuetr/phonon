//! Episode bundle — audio plus chapters / subtitles / metadata, with
//! one-step export to WAV / MP3 / FLAC / OGG.
//!
//! [`PodcastPipeline::generate_episode`] returns an [`Episode`]; the user
//! attaches tags (or accepts the defaults) and calls one of the
//! `export_*` methods to produce a publish-ready file.

use crate::PodcastError;
use crate::chapters::{Chapter, assign_chapter_times};
use crate::script::PodcastScript;
use crate::subtitle::{SubtitleEntry, generate_srt};
use phonon_core::AudioBuffer;
#[cfg(feature = "aac")]
use phonon_io::{AacConfig, write_aac};
use phonon_io::{
  AudioChapter, FlacConfig, Mp3Config, OggConfig, PodcastTags, write_flac, write_flac_tagged,
  write_mp3, write_mp3_tagged, write_ogg, write_ogg_tagged,
};
use std::path::Path;
use tracing::{info, instrument};

/// A complete podcast episode: rendered audio plus everything needed for
/// publishing — chapter markers, SRT subtitles, and metadata tags.
///
/// Build via [`PodcastPipeline::generate_episode`].
#[derive(Debug, Clone)]
pub struct Episode {
  /// The fully assembled audio.
  pub audio: AudioBuffer,
  /// Chapter markers with timestamps in seconds.
  pub chapters: Vec<Chapter>,
  /// SRT subtitle text. `None` if no subtitle source was available.
  pub srt: Option<String>,
  /// Metadata tags (title, artist, …). Defaults to empty; populate via
  /// [`Episode::with_tags`] before exporting.
  pub tags: PodcastTags,
}

impl Episode {
  /// Construct an Episode from raw parts (used by the pipeline).
  pub fn new(audio: AudioBuffer) -> Self {
    Self {
      audio,
      chapters: Vec::new(),
      srt: None,
      tags: PodcastTags::default(),
    }
  }

  /// Builder: replace this episode's chapters.
  pub fn with_chapters(mut self, chapters: Vec<Chapter>) -> Self {
    self.chapters = chapters;
    self
  }

  /// Builder: replace this episode's SRT text.
  pub fn with_srt(mut self, srt: String) -> Self {
    self.srt = Some(srt);
    self
  }

  /// Builder: replace this episode's tags.
  pub fn with_tags(mut self, tags: PodcastTags) -> Self {
    self.tags = tags;
    self
  }

  /// Convert this episode's chapters into the [`AudioChapter`] form
  /// expected by `phonon-io` writers.
  fn chapter_frames(&self) -> Vec<AudioChapter> {
    let total = self.audio.duration_secs();
    self
      .chapters
      .iter()
      .map(|ch| {
        let end = ch.end.unwrap_or(total).max(ch.start);
        AudioChapter::new(ch.title.clone(), ch.start, end)
      })
      .collect()
  }

  /// Export this episode as a plain WAV file. Tags and chapters are
  /// not written — the WAV format has no native metadata story we use.
  #[instrument(skip(self, path), fields(path = %path.as_ref().display()))]
  pub fn export_wav(&self, path: impl AsRef<Path>) -> Result<(), PodcastError> {
    phonon_wav::write_wav(path, &self.audio).map_err(|e| PodcastError::Io(e.to_string()))?;
    info!("Episode exported (WAV)");
    Ok(())
  }

  /// Export as MP3 with embedded ID3v2.4 tags + PodcastIndex chapters.
  #[instrument(skip(self, path, mp3_cfg), fields(path = %path.as_ref().display()))]
  pub fn export_mp3(
    &self,
    path: impl AsRef<Path>,
    mp3_cfg: &Mp3Config,
  ) -> Result<(), PodcastError> {
    let chapters = self.chapter_frames();
    write_mp3_tagged(path, &self.audio, mp3_cfg, &self.tags, &chapters)
      .map_err(|e| PodcastError::Io(e.to_string()))?;
    info!("Episode exported (MP3 + tags + chapters)");
    Ok(())
  }

  /// Export as FLAC with Vorbis Comment block + cover art.
  #[instrument(skip(self, path, flac_cfg), fields(path = %path.as_ref().display()))]
  pub fn export_flac(
    &self,
    path: impl AsRef<Path>,
    flac_cfg: &FlacConfig,
  ) -> Result<(), PodcastError> {
    write_flac_tagged(path, &self.audio, flac_cfg, &self.tags)
      .map_err(|e| PodcastError::Io(e.to_string()))?;
    info!("Episode exported (FLAC + tags)");
    Ok(())
  }

  /// Export as OGG Vorbis with Vorbis Comments + cover art.
  #[instrument(skip(self, path, ogg_cfg), fields(path = %path.as_ref().display()))]
  pub fn export_ogg(
    &self,
    path: impl AsRef<Path>,
    ogg_cfg: &OggConfig,
  ) -> Result<(), PodcastError> {
    write_ogg_tagged(path, &self.audio, ogg_cfg, &self.tags)
      .map_err(|e| PodcastError::Io(e.to_string()))?;
    info!("Episode exported (OGG + tags)");
    Ok(())
  }

  /// Export as AAC. AAC tagging is not supported here — only the audio is written.
  /// Requires the `aac` feature (see phonon-io's licensing notes).
  #[cfg(feature = "aac")]
  pub fn export_aac(
    &self,
    path: impl AsRef<Path>,
    aac_cfg: &AacConfig,
  ) -> Result<(), PodcastError> {
    write_aac(path, &self.audio, aac_cfg).map_err(|e| PodcastError::Io(e.to_string()))?;
    Ok(())
  }

  /// Plain WAV export with no tagging — alternate name to mirror
  /// the format-specific export functions.
  pub fn export_wav_plain(&self, path: impl AsRef<Path>) -> Result<(), PodcastError> {
    self.export_wav(path)
  }

  /// Bypass the tagging path and write a plain MP3 (for tests / sanity).
  pub fn export_mp3_plain(
    &self,
    path: impl AsRef<Path>,
    mp3_cfg: &Mp3Config,
  ) -> Result<(), PodcastError> {
    write_mp3(path, &self.audio, mp3_cfg).map_err(|e| PodcastError::Io(e.to_string()))?;
    Ok(())
  }

  /// Bypass the tagging path for FLAC.
  pub fn export_flac_plain(
    &self,
    path: impl AsRef<Path>,
    flac_cfg: &FlacConfig,
  ) -> Result<(), PodcastError> {
    write_flac(path, &self.audio, flac_cfg).map_err(|e| PodcastError::Io(e.to_string()))?;
    Ok(())
  }

  /// Bypass the tagging path for OGG.
  pub fn export_ogg_plain(
    &self,
    path: impl AsRef<Path>,
    ogg_cfg: &OggConfig,
  ) -> Result<(), PodcastError> {
    write_ogg(path, &self.audio, ogg_cfg).map_err(|e| PodcastError::Io(e.to_string()))?;
    Ok(())
  }

  /// Write the SRT subtitle file as a sidecar. Returns an error if the
  /// episode has no SRT data.
  #[instrument(skip(self, path), fields(path = %path.as_ref().display()))]
  pub fn write_srt(&self, path: impl AsRef<Path>) -> Result<(), PodcastError> {
    let Some(srt) = &self.srt else {
      return Err(PodcastError::Processing(
        "Episode has no SRT data to write".to_string(),
      ));
    };
    std::fs::write(path, srt).map_err(|e| PodcastError::Io(e.to_string()))?;
    Ok(())
  }
}

/// Build chapters + SRT from a script and per-segment `(start, end)` times
/// in seconds within the final audio.
///
/// Used internally by [`crate::PodcastPipeline::generate_episode`] but
/// exposed so callers can re-create episode metadata from a known timing
/// table (e.g. when re-rendering only a subset of segments).
pub fn build_episode_metadata(
  script: &PodcastScript,
  segment_spans: &[(f64, f64)],
) -> (Vec<Chapter>, String) {
  let chapters = assign_chapter_times(script, segment_spans);
  let entries = entries_from_script(script, segment_spans);
  let srt = generate_srt(&entries);
  (chapters, srt)
}

/// Convert a script + segment spans into per-segment SRT entries.
/// Segments without a span are skipped.
fn entries_from_script(script: &PodcastScript, segment_spans: &[(f64, f64)]) -> Vec<SubtitleEntry> {
  script
    .segments
    .iter()
    .zip(segment_spans)
    .map(|(seg, &(start, end))| SubtitleEntry {
      start,
      end,
      text: seg.text.clone(),
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::Segment;
  use phonon_ai::VoiceConfig;
  use phonon_io::{CoverArt, CoverMime};
  use std::collections::HashMap;

  fn dummy_audio(sr: u32, secs: f64) -> AudioBuffer {
    let n = (sr as f64 * secs) as usize;
    AudioBuffer {
      samples: vec![0.1; n],
      sample_rate: sr,
      channels: 1,
    }
  }

  fn make_script() -> PodcastScript {
    let mut speakers = HashMap::new();
    speakers.insert("alice".to_string(), VoiceConfig::new("alloy"));
    speakers.insert("bob".to_string(), VoiceConfig::new("echo"));
    PodcastScript {
      title: "Test".into(),
      speakers,
      segments: vec![
        Segment {
          speaker: "alice".into(),
          text: "Welcome".into(),
          language: None,
        },
        Segment {
          speaker: "bob".into(),
          text: "Thanks".into(),
          language: None,
        },
      ],
      config: Default::default(),
    }
  }

  #[test]
  fn test_build_episode_metadata_chapters_match_speakers() {
    let script = make_script();
    let (chapters, _srt) = build_episode_metadata(&script, &[(0.0, 2.0), (2.5, 5.5)]);
    assert_eq!(chapters.len(), 2);
    assert_eq!(chapters[0].title, "alice");
    assert_eq!(chapters[1].title, "bob");
    // alice chapter: 0..2.5 (incl. pause); bob: 2.5..5.5.
    assert!((chapters[0].start - 0.0).abs() < 1e-6);
    assert!((chapters[0].end.unwrap_or(0.0) - 2.5).abs() < 1e-6);
    assert!((chapters[1].start - 2.5).abs() < 1e-6);
  }

  #[test]
  fn test_build_episode_metadata_srt_contains_segments() {
    let script = make_script();
    let (_, srt) = build_episode_metadata(&script, &[(0.0, 2.0), (2.5, 5.5)]);
    assert!(srt.contains("Welcome"));
    assert!(srt.contains("Thanks"));
    assert!(srt.contains("00:00:00,000"));
  }

  #[test]
  fn test_episode_with_tags_overrides_defaults() {
    let ep = Episode::new(dummy_audio(44100, 0.1)).with_tags(PodcastTags {
      title: Some("My Show".into()),
      artist: Some("Author".into()),
      ..Default::default()
    });
    assert_eq!(ep.tags.title.as_deref(), Some("My Show"));
    assert_eq!(ep.tags.artist.as_deref(), Some("Author"));
  }

  #[test]
  fn test_episode_chapter_frames_clamp_to_total_duration() {
    let audio = dummy_audio(44100, 1.0);
    let chapters = vec![
      Chapter::new("Intro", 0.0).with_end(0.5),
      Chapter::new("Body", 0.5), // no end → should fall back to total
    ];
    let ep = Episode::new(audio).with_chapters(chapters);
    let frames = ep.chapter_frames();
    assert_eq!(frames.len(), 2);
    assert!((frames[1].end_secs - 1.0).abs() < 1e-3);
  }

  #[test]
  fn test_export_wav_writes_file() -> Result<(), PodcastError> {
    let ep = Episode::new(dummy_audio(44100, 0.1));
    let tmp = std::env::temp_dir().join("phonon_episode_export.wav");
    ep.export_wav(&tmp)?;
    let meta = std::fs::metadata(&tmp).map_err(|e| PodcastError::Io(e.to_string()))?;
    assert!(meta.len() > 0);
    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_export_mp3_with_chapters_and_tags() -> Result<(), PodcastError> {
    let ep = Episode::new(dummy_audio(44100, 0.5))
      .with_chapters(vec![
        Chapter::new("First", 0.0).with_end(0.25),
        Chapter::new("Second", 0.25).with_end(0.5),
      ])
      .with_tags(PodcastTags {
        title: Some("Bundled Export".into()),
        artist: Some("Phonon".into()),
        cover: Some(CoverArt {
          mime: CoverMime::Jpeg,
          data: vec![0xFF, 0xD8, 0xFF, 0xE0],
          description: None,
        }),
        ..Default::default()
      });
    let tmp = std::env::temp_dir().join("phonon_episode_bundle.mp3");
    ep.export_mp3(&tmp, &Mp3Config::default())?;
    let meta = std::fs::metadata(&tmp).map_err(|e| PodcastError::Io(e.to_string()))?;
    assert!(meta.len() > 0);
    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }

  #[test]
  fn test_write_srt_without_data_errors() {
    let ep = Episode::new(dummy_audio(44100, 0.1));
    assert!(ep.write_srt("/tmp/should_not_be_written.srt").is_err());
  }

  #[test]
  fn test_write_srt_writes_file() -> Result<(), PodcastError> {
    let ep = Episode::new(dummy_audio(44100, 0.1))
      .with_srt("1\n00:00:00,000 --> 00:00:01,000\nHello\n".to_string());
    let tmp = std::env::temp_dir().join("phonon_episode.srt");
    ep.write_srt(&tmp)?;
    let contents = std::fs::read_to_string(&tmp).map_err(|e| PodcastError::Io(e.to_string()))?;
    assert!(contents.contains("Hello"));
    let _ = std::fs::remove_file(&tmp);
    Ok(())
  }
}
