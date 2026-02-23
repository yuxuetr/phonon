//! SRT subtitle generation from transcription segments.
//!
//! Converts [`TranscriptSegment`](phonon_ai::TranscriptSegment) data
//! (produced by the STT pipeline) into standard SRT subtitle files.
//!
//! ## SRT Format
//!
//! ```text
//! 1
//! 00:00:00,000 --> 00:00:02,500
//! Hello and welcome to our podcast.
//!
//! 2
//! 00:00:02,500 --> 00:00:05,800
//! Today we'll talk about audio processing.
//! ```
//!
//! ## Example
//!
//! ```rust
//! use phonon_podcast::subtitle::{SubtitleEntry, generate_srt, write_srt_file};
//!
//! let entries = vec![
//!     SubtitleEntry { start: 0.0, end: 2.5, text: "Hello!".into() },
//!     SubtitleEntry { start: 2.5, end: 5.0, text: "Welcome.".into() },
//! ];
//! let srt = generate_srt(&entries);
//! assert!(srt.contains("00:00:00,000 --> 00:00:02,500"));
//! ```

use crate::PodcastError;
use phonon_ai::TranscriptSegment;
use std::fmt::Write as FmtWrite;
use std::path::Path;

/// A single subtitle entry with timing information.
#[derive(Debug, Clone)]
pub struct SubtitleEntry {
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Subtitle text.
    pub text: String,
}

impl SubtitleEntry {
    /// Create a new subtitle entry.
    pub fn new(start: f64, end: f64, text: impl Into<String>) -> Self {
        Self {
            start,
            end,
            text: text.into(),
        }
    }
}

/// Convert [`TranscriptSegment`]s from STT into [`SubtitleEntry`]s.
pub fn from_segments(segments: &[TranscriptSegment]) -> Vec<SubtitleEntry> {
    segments
        .iter()
        .map(|seg| SubtitleEntry {
            start: seg.start,
            end: seg.end,
            text: seg.text.trim().to_string(),
        })
        .filter(|e| !e.text.is_empty())
        .collect()
}

/// Format seconds as SRT timestamp: `HH:MM:SS,mmm`.
fn format_timestamp(seconds: f64) -> String {
    let total_ms = (seconds * 1000.0).round() as u64;
    let ms = total_ms % 1000;
    let total_secs = total_ms / 1000;
    let s = total_secs % 60;
    let total_mins = total_secs / 60;
    let m = total_mins % 60;
    let h = total_mins / 60;
    format!("{h:02}:{m:02}:{s:02},{ms:03}")
}

/// Generate an SRT-formatted string from subtitle entries.
///
/// Returns a complete SRT file content with sequentially numbered entries.
pub fn generate_srt(entries: &[SubtitleEntry]) -> String {
    let mut srt = String::new();

    for (i, entry) in entries.iter().enumerate() {
        if i > 0 {
            srt.push('\n');
        }
        let _ = writeln!(srt, "{}", i + 1);
        let _ = writeln!(
            srt,
            "{} --> {}",
            format_timestamp(entry.start),
            format_timestamp(entry.end)
        );
        let _ = writeln!(srt, "{}", entry.text);
    }

    srt
}

/// Write subtitle entries to an SRT file.
pub fn write_srt_file(entries: &[SubtitleEntry], path: impl AsRef<Path>) -> Result<(), PodcastError> {
    let srt = generate_srt(entries);
    std::fs::write(path, srt).map_err(|e| PodcastError::Io(e.to_string()))
}

/// Convenience: convert transcript segments directly to an SRT file.
pub fn segments_to_srt_file(
    segments: &[TranscriptSegment],
    path: impl AsRef<Path>,
) -> Result<(), PodcastError> {
    let entries = from_segments(segments);
    write_srt_file(&entries, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp_zero() {
        assert_eq!(format_timestamp(0.0), "00:00:00,000");
    }

    #[test]
    fn test_format_timestamp_simple() {
        assert_eq!(format_timestamp(1.5), "00:00:01,500");
    }

    #[test]
    fn test_format_timestamp_minutes() {
        assert_eq!(format_timestamp(90.123), "00:01:30,123");
    }

    #[test]
    fn test_format_timestamp_hours() {
        assert_eq!(format_timestamp(3661.999), "01:01:01,999"); // 1h 1m 1.999s
    }

    #[test]
    fn test_generate_srt_single_entry() {
        let entries = vec![SubtitleEntry::new(0.0, 2.5, "Hello world")];
        let srt = generate_srt(&entries);
        assert_eq!(
            srt,
            "1\n00:00:00,000 --> 00:00:02,500\nHello world\n"
        );
    }

    #[test]
    fn test_generate_srt_multiple_entries() {
        let entries = vec![
            SubtitleEntry::new(0.0, 2.5, "First line"),
            SubtitleEntry::new(2.5, 5.0, "Second line"),
            SubtitleEntry::new(5.0, 8.3, "Third line"),
        ];
        let srt = generate_srt(&entries);

        let lines: Vec<&str> = srt.lines().collect();
        // Entry 1
        assert_eq!(lines[0], "1");
        assert_eq!(lines[1], "00:00:00,000 --> 00:00:02,500");
        assert_eq!(lines[2], "First line");
        // Blank line separator
        assert_eq!(lines[3], "");
        // Entry 2
        assert_eq!(lines[4], "2");
        assert_eq!(lines[5], "00:00:02,500 --> 00:00:05,000");
        assert_eq!(lines[6], "Second line");
        // Blank line separator
        assert_eq!(lines[7], "");
        // Entry 3
        assert_eq!(lines[8], "3");
        assert_eq!(lines[9], "00:00:05,000 --> 00:00:08,300");
        assert_eq!(lines[10], "Third line");
    }

    #[test]
    fn test_generate_srt_empty() {
        let srt = generate_srt(&[]);
        assert!(srt.is_empty());
    }

    #[test]
    fn test_from_segments() {
        let segments = vec![
            TranscriptSegment {
                id: 0,
                start: 0.0,
                end: 1.5,
                text: " Hello ".to_string(),
            },
            TranscriptSegment {
                id: 1,
                start: 1.5,
                end: 3.0,
                text: "World".to_string(),
            },
        ];
        let entries = from_segments(&segments);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "Hello"); // trimmed
        assert_eq!(entries[1].text, "World");
    }

    #[test]
    fn test_from_segments_filters_empty() {
        let segments = vec![
            TranscriptSegment {
                id: 0,
                start: 0.0,
                end: 1.0,
                text: "  ".to_string(), // whitespace only → filtered out
            },
            TranscriptSegment {
                id: 1,
                start: 1.0,
                end: 2.0,
                text: "Keep me".to_string(),
            },
        ];
        let entries = from_segments(&segments);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Keep me");
    }

    #[test]
    fn test_write_srt_file_roundtrip() {
        let entries = vec![
            SubtitleEntry::new(0.0, 2.0, "Line 1"),
            SubtitleEntry::new(2.0, 4.0, "Line 2"),
        ];
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        write_srt_file(&entries, tmp.path()).expect("write SRT");
        let content = std::fs::read_to_string(tmp.path()).expect("read file");
        assert!(content.contains("00:00:00,000 --> 00:00:02,000"));
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));
    }

    #[test]
    fn test_segments_to_srt_file() {
        let segments = vec![
            TranscriptSegment {
                id: 0,
                start: 0.0,
                end: 3.0,
                text: "Segment one".to_string(),
            },
        ];
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        segments_to_srt_file(&segments, tmp.path()).expect("write SRT");
        let content = std::fs::read_to_string(tmp.path()).expect("read file");
        assert!(content.starts_with("1\n"));
        assert!(content.contains("Segment one"));
    }

    #[test]
    fn test_long_duration_timestamp() {
        // 2 hours, 30 minutes, 45.678 seconds
        let secs = 2.0 * 3600.0 + 30.0 * 60.0 + 45.678;
        assert_eq!(format_timestamp(secs), "02:30:45,678");
    }
}
