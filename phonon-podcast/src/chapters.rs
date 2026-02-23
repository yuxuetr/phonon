//! Automatic chapter marker generation for podcasts.
//!
//! Generates chapter markers from two complementary sources:
//!
//! 1. **Silence detection** — scans an `AudioBuffer` for periods of silence
//!    longer than a threshold, treating them as chapter boundaries.
//! 2. **Speaker switches** — detects speaker changes in a `PodcastScript`,
//!    optionally merging consecutive segments by the same speaker.
//!
//! ## Example
//!
//! ```rust
//! use phonon_podcast::chapters::{Chapter, chapters_from_speakers};
//! use phonon_podcast::{PodcastScript, Segment};
//! use std::collections::HashMap;
//! use phonon_ai::VoiceConfig;
//!
//! let script = PodcastScript {
//!     title: "Demo".into(),
//!     speakers: HashMap::from([
//!         ("Alice".into(), VoiceConfig::new("alloy")),
//!         ("Bob".into(), VoiceConfig::new("echo")),
//!     ]),
//!     segments: vec![
//!         Segment { speaker: "Alice".into(), text: "Welcome!".into(), language: None },
//!         Segment { speaker: "Bob".into(), text: "Thanks!".into(), language: None },
//!     ],
//!     config: Default::default(),
//! };
//!
//! let chapters = chapters_from_speakers(&script);
//! assert_eq!(chapters.len(), 2);
//! assert_eq!(chapters[0].title, "Alice");
//! ```

use crate::PodcastScript;
use phonon_core::AudioBuffer;
use serde::{Deserialize, Serialize};

/// A single chapter marker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    /// Chapter title (e.g. speaker name or topic).
    pub title: String,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds (if known).
    pub end: Option<f64>,
}

impl Chapter {
    /// Create a new chapter.
    pub fn new(title: impl Into<String>, start: f64) -> Self {
        Self {
            title: title.into(),
            start,
            end: None,
        }
    }

    /// Set the end time.
    pub fn with_end(mut self, end: f64) -> Self {
        self.end = Some(end);
        self
    }

    /// Duration in seconds (returns 0 if end is not set).
    pub fn duration(&self) -> f64 {
        self.end.map_or(0.0, |e| (e - self.start).max(0.0))
    }
}

// ---------------------------------------------------------------------------
// Silence-based chapter detection
// ---------------------------------------------------------------------------

/// Configuration for silence-based chapter detection.
#[derive(Debug, Clone)]
pub struct SilenceDetectConfig {
    /// Amplitude threshold below which audio is considered silent.
    /// Samples with `abs(sample) < threshold` are silent. Default: 0.01
    pub threshold: f32,
    /// Minimum duration of silence (in seconds) to be treated as a chapter break.
    /// Default: 1.0 second.
    pub min_silence_secs: f64,
}

impl Default for SilenceDetectConfig {
    fn default() -> Self {
        Self {
            threshold: 0.01,
            min_silence_secs: 1.0,
        }
    }
}

/// Detect silence regions in an audio buffer.
///
/// Returns a list of `(start_secs, end_secs)` for each detected silence region.
pub fn detect_silence(buffer: &AudioBuffer, config: &SilenceDetectConfig) -> Vec<(f64, f64)> {
    if buffer.samples.is_empty() || buffer.sample_rate == 0 || buffer.channels == 0 {
        return vec![];
    }

    let channels = buffer.channels as usize;
    let sample_rate = buffer.sample_rate as f64;
    let min_silence_samples = (config.min_silence_secs * sample_rate) as usize * channels;

    let mut silence_regions = Vec::new();
    let mut silence_start: Option<usize> = None;

    for (i, &sample) in buffer.samples.iter().enumerate() {
        let is_silent = sample.abs() < config.threshold;

        match (is_silent, silence_start) {
            (true, None) => {
                silence_start = Some(i);
            }
            (false, Some(start)) => {
                let len = i - start;
                if len >= min_silence_samples {
                    let start_sec = start as f64 / (sample_rate * channels as f64);
                    let end_sec = i as f64 / (sample_rate * channels as f64);
                    silence_regions.push((start_sec, end_sec));
                }
                silence_start = None;
            }
            _ => {}
        }
    }

    // Handle trailing silence
    if let Some(start) = silence_start {
        let len = buffer.samples.len() - start;
        if len >= min_silence_samples {
            let start_sec = start as f64 / (sample_rate * channels as f64);
            let end_sec = buffer.samples.len() as f64 / (sample_rate * channels as f64);
            silence_regions.push((start_sec, end_sec));
        }
    }

    silence_regions
}

/// Generate chapter markers from silence regions in an audio buffer.
///
/// Each chapter spans from the midpoint of one silence region to the midpoint
/// of the next. Chapters are titled "Chapter 1", "Chapter 2", etc.
pub fn chapters_from_silence(
    buffer: &AudioBuffer,
    config: &SilenceDetectConfig,
) -> Vec<Chapter> {
    let silences = detect_silence(buffer, config);
    let total_duration = buffer.duration_secs();

    if silences.is_empty() {
        // No silence detected → single chapter
        if total_duration > 0.0 {
            return vec![Chapter::new("Chapter 1", 0.0).with_end(total_duration)];
        }
        return vec![];
    }

    let mut chapters = Vec::new();
    let mut prev_end = 0.0;

    for (i, &(sil_start, sil_end)) in silences.iter().enumerate() {
        let mid = (sil_start + sil_end) / 2.0;
        let title = format!("Chapter {}", i + 1);
        chapters.push(Chapter::new(title, prev_end).with_end(mid));
        prev_end = mid;
    }

    // Last chapter: from last silence midpoint to end
    let last_title = format!("Chapter {}", silences.len() + 1);
    chapters.push(Chapter::new(last_title, prev_end).with_end(total_duration));

    chapters
}

// ---------------------------------------------------------------------------
// Speaker-based chapter detection
// ---------------------------------------------------------------------------

/// Generate chapter markers from speaker transitions in a podcast script.
///
/// Each time the speaker changes, a new chapter begins. Consecutive segments
/// by the same speaker are merged into one chapter.
///
/// Note: Since the script does not contain timing, chapters have `start = 0.0`
/// and sequential indices. Use [`assign_chapter_times`] to attach real timestamps
/// after audio is generated.
pub fn chapters_from_speakers(script: &PodcastScript) -> Vec<Chapter> {
    if script.segments.is_empty() {
        return vec![];
    }

    let mut chapters: Vec<Chapter> = Vec::new();
    let mut current_speaker: Option<&str> = None;

    for seg in &script.segments {
        match current_speaker {
            Some(prev) if prev == seg.speaker => {
                // Same speaker — continue current chapter
            }
            _ => {
                // New speaker or first segment
                chapters.push(Chapter::new(&seg.speaker, 0.0));
                current_speaker = Some(&seg.speaker);
            }
        }
    }

    chapters
}

/// Assign actual timestamps to speaker-based chapters given per-segment durations.
///
/// `segment_durations` should have one entry per segment in the script (in seconds).
/// Chapters are computed by accumulating durations and detecting speaker switches.
pub fn assign_chapter_times(
    script: &PodcastScript,
    segment_durations: &[f64],
    pause_secs: f64,
) -> Vec<Chapter> {
    if script.segments.is_empty() {
        return vec![];
    }

    let mut chapters: Vec<Chapter> = Vec::new();
    let mut current_time = 0.0;
    let mut current_speaker: Option<&str> = None;

    for (i, seg) in script.segments.iter().enumerate() {
        let duration = segment_durations.get(i).copied().unwrap_or(0.0);

        match current_speaker {
            Some(prev) if prev == seg.speaker => {
                // Same speaker — extend current chapter
            }
            _ => {
                // Close previous chapter
                if let Some(last) = chapters.last_mut() {
                    last.end = Some(current_time);
                }
                chapters.push(Chapter::new(&seg.speaker, current_time));
                current_speaker = Some(&seg.speaker);
            }
        }

        current_time += duration;
        if i + 1 < script.segments.len() {
            current_time += pause_secs;
        }
    }

    // Close last chapter
    if let Some(last) = chapters.last_mut() {
        last.end = Some(current_time);
    }

    chapters
}

/// Format chapters as a simple text listing.
///
/// ```text
/// 00:00:00 Chapter 1
/// 00:01:30 Chapter 2
/// ```
pub fn format_chapters_text(chapters: &[Chapter]) -> String {
    let mut out = String::new();
    for ch in chapters {
        let total_secs = ch.start as u64;
        let h = total_secs / 3600;
        let m = (total_secs % 3600) / 60;
        let s = total_secs % 60;
        out.push_str(&format!("{h:02}:{m:02}:{s:02} {}\n", ch.title));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Segment;
    use phonon_ai::VoiceConfig;
    use std::collections::HashMap;

    fn make_script(segments: Vec<(&str, &str)>) -> PodcastScript {
        let mut speakers = HashMap::new();
        for (speaker, _) in &segments {
            speakers
                .entry(speaker.to_string())
                .or_insert_with(|| VoiceConfig::new("alloy"));
        }
        PodcastScript {
            title: "Test".into(),
            speakers,
            segments: segments
                .into_iter()
                .map(|(speaker, text)| Segment {
                    speaker: speaker.to_string(),
                    text: text.to_string(),
                    language: None,
                })
                .collect(),
            config: Default::default(),
        }
    }

    // --- Silence detection tests ---

    #[test]
    fn test_detect_silence_empty_buffer() {
        let buf = AudioBuffer {
            samples: vec![],
            sample_rate: 44100,
            channels: 1,
        };
        let regions = detect_silence(&buf, &SilenceDetectConfig::default());
        assert!(regions.is_empty());
    }

    #[test]
    fn test_detect_silence_all_silent() {
        let buf = AudioBuffer {
            samples: vec![0.0; 44100], // 1 second of silence
            sample_rate: 44100,
            channels: 1,
        };
        let regions = detect_silence(&buf, &SilenceDetectConfig::default());
        assert_eq!(regions.len(), 1);
        assert!((regions[0].0 - 0.0).abs() < 0.01);
        assert!((regions[0].1 - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_detect_silence_no_silence() {
        let buf = AudioBuffer {
            samples: vec![0.5; 44100],
            sample_rate: 44100,
            channels: 1,
        };
        let regions = detect_silence(&buf, &SilenceDetectConfig::default());
        assert!(regions.is_empty());
    }

    #[test]
    fn test_detect_silence_middle_gap() {
        // 0.5s loud + 1.5s silent + 0.5s loud
        let sr = 1000_u32;
        let mut samples = vec![0.5; 500]; // 0.5s loud
        samples.extend(vec![0.0; 1500]); // 1.5s silent
        samples.extend(vec![0.5; 500]); // 0.5s loud

        let buf = AudioBuffer {
            samples,
            sample_rate: sr,
            channels: 1,
        };
        let regions = detect_silence(&buf, &SilenceDetectConfig {
            threshold: 0.01,
            min_silence_secs: 1.0,
        });
        assert_eq!(regions.len(), 1);
        assert!((regions[0].0 - 0.5).abs() < 0.01);
        assert!((regions[0].1 - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_detect_silence_short_gap_ignored() {
        // 0.5s silence (below 1.0s threshold) → should not be detected
        let sr = 1000_u32;
        let mut samples = vec![0.5; 500];
        samples.extend(vec![0.0; 500]); // 0.5s silence — too short
        samples.extend(vec![0.5; 500]);

        let buf = AudioBuffer {
            samples,
            sample_rate: sr,
            channels: 1,
        };
        let regions = detect_silence(&buf, &SilenceDetectConfig::default());
        assert!(regions.is_empty());
    }

    // --- Chapters from silence tests ---

    #[test]
    fn test_chapters_from_silence_no_silence() {
        let buf = AudioBuffer {
            samples: vec![0.5; 44100],
            sample_rate: 44100,
            channels: 1,
        };
        let chapters = chapters_from_silence(&buf, &SilenceDetectConfig::default());
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "Chapter 1");
    }

    #[test]
    fn test_chapters_from_silence_one_gap() {
        let sr = 1000_u32;
        let mut samples = vec![0.5; 1000]; // 1s loud
        samples.extend(vec![0.0; 2000]); // 2s silent
        samples.extend(vec![0.5; 1000]); // 1s loud

        let buf = AudioBuffer {
            samples,
            sample_rate: sr,
            channels: 1,
        };
        let chapters = chapters_from_silence(&buf, &SilenceDetectConfig::default());
        assert_eq!(chapters.len(), 2);
        assert_eq!(chapters[0].title, "Chapter 1");
        assert_eq!(chapters[1].title, "Chapter 2");
        assert!((chapters[0].start - 0.0).abs() < 0.01);
        // Midpoint of silence region [1.0, 3.0] = 2.0
        assert!((chapters[1].start - 2.0).abs() < 0.01);
    }

    // --- Chapters from speakers tests ---

    #[test]
    fn test_chapters_from_speakers_basic() {
        let script = make_script(vec![
            ("Alice", "Hello"),
            ("Bob", "Hi"),
            ("Alice", "How are you?"),
        ]);
        let chapters = chapters_from_speakers(&script);
        assert_eq!(chapters.len(), 3);
        assert_eq!(chapters[0].title, "Alice");
        assert_eq!(chapters[1].title, "Bob");
        assert_eq!(chapters[2].title, "Alice");
    }

    #[test]
    fn test_chapters_from_speakers_merged() {
        let script = make_script(vec![
            ("Alice", "Part 1"),
            ("Alice", "Part 2"),
            ("Bob", "Response"),
        ]);
        let chapters = chapters_from_speakers(&script);
        assert_eq!(chapters.len(), 2); // Alice segments merged
        assert_eq!(chapters[0].title, "Alice");
        assert_eq!(chapters[1].title, "Bob");
    }

    #[test]
    fn test_chapters_from_speakers_empty() {
        let script = PodcastScript {
            title: "Empty".into(),
            speakers: HashMap::new(),
            segments: vec![],
            config: Default::default(),
        };
        let chapters = chapters_from_speakers(&script);
        assert!(chapters.is_empty());
    }

    // --- Assign chapter times tests ---

    #[test]
    fn test_assign_chapter_times() {
        let script = make_script(vec![
            ("Alice", "Hello"),      // 2.0s
            ("Bob", "Hi there"),     // 3.0s
            ("Alice", "Continuing"), // 1.5s
        ]);
        let durations = [2.0, 3.0, 1.5];
        let chapters = assign_chapter_times(&script, &durations, 0.5);

        assert_eq!(chapters.len(), 3);

        // Alice starts at 0.0, ends at 2.0 (duration) + 0.5 (pause) = 2.5
        assert!((chapters[0].start - 0.0).abs() < 0.01);
        assert!((chapters[0].end.unwrap_or(0.0) - 2.5).abs() < 0.01);

        // Bob starts at 2.5, lasts 3.0 + 0.5 pause = ends at 6.0
        assert!((chapters[1].start - 2.5).abs() < 0.01);
        assert!((chapters[1].end.unwrap_or(0.0) - 6.0).abs() < 0.01);

        // Alice starts at 6.0, lasts 1.5, no trailing pause
        assert!((chapters[2].start - 6.0).abs() < 0.01);
        assert!((chapters[2].end.unwrap_or(0.0) - 7.5).abs() < 0.01);
    }

    #[test]
    fn test_assign_chapter_times_merged_speakers() {
        let script = make_script(vec![
            ("Alice", "Part 1"), // 2.0s
            ("Alice", "Part 2"), // 1.0s
            ("Bob", "Response"), // 3.0s
        ]);
        let durations = [2.0, 1.0, 3.0];
        let chapters = assign_chapter_times(&script, &durations, 0.5);

        assert_eq!(chapters.len(), 2); // Alice merged
        assert_eq!(chapters[0].title, "Alice");
        assert!((chapters[0].start - 0.0).abs() < 0.01);
        // Alice: 2.0 + 0.5 + 1.0 + 0.5 = 4.0
        assert!((chapters[0].end.unwrap_or(0.0) - 4.0).abs() < 0.01);

        assert_eq!(chapters[1].title, "Bob");
        assert!((chapters[1].start - 4.0).abs() < 0.01);
        assert!((chapters[1].end.unwrap_or(0.0) - 7.0).abs() < 0.01);
    }

    // --- Format tests ---

    #[test]
    fn test_format_chapters_text() {
        let chapters = vec![
            Chapter::new("Intro", 0.0).with_end(90.0),
            Chapter::new("Topic 1", 90.0).with_end(300.0),
        ];
        let text = format_chapters_text(&chapters);
        assert!(text.contains("00:00:00 Intro"));
        assert!(text.contains("00:01:30 Topic 1"));
    }

    #[test]
    fn test_chapter_duration() {
        let ch = Chapter::new("Test", 10.0).with_end(25.0);
        assert!((ch.duration() - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_chapter_no_end_duration() {
        let ch = Chapter::new("Test", 10.0);
        assert!((ch.duration() - 0.0).abs() < 0.01);
    }
}
