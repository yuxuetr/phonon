use crate::config::PipelineConfig;
use phonon_ai::VoiceConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tracing::instrument;

/// A complete podcast script ready for generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PodcastScript {
    /// Episode title.
    pub title: String,

    /// Speaker definitions: name → voice configuration.
    pub speakers: HashMap<String, VoiceConfig>,

    /// Ordered list of dialogue segments.
    pub segments: Vec<Segment>,

    /// Pipeline configuration (optional, uses defaults if absent).
    #[serde(default)]
    pub config: PipelineConfig,
}

/// A single dialogue segment in the podcast.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    /// The speaker name (must match a key in `speakers`).
    pub speaker: String,

    /// The text content to synthesize.
    pub text: String,

    /// Optional language override for this segment (e.g. "en-US", "zh-CN").
    /// When set, the pipeline will override the speaker's `VoiceConfig.language`
    /// for this segment, enabling multi-language podcasts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

impl PodcastScript {
    /// Load a podcast script from a JSON file.
    #[instrument(skip(path), fields(path = %path.as_ref().display()))]
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, crate::PodcastError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| crate::PodcastError::Io(e.to_string()))?;
        Self::from_json(&content)
    }

    /// Parse a podcast script from a JSON string.
    pub fn from_json(json: &str) -> Result<Self, crate::PodcastError> {
        let script: PodcastScript = serde_json::from_str(json)
            .map_err(|e| crate::PodcastError::Script(format!("Invalid script JSON: {}", e)))?;
        script.validate()?;
        Ok(script)
    }

    /// Validate that all segment speakers exist in the speakers map.
    pub fn validate(&self) -> Result<(), crate::PodcastError> {
        if self.segments.is_empty() {
            return Err(crate::PodcastError::Script(
                "Script has no segments".to_string(),
            ));
        }

        for (i, segment) in self.segments.iter().enumerate() {
            if !self.speakers.contains_key(&segment.speaker) {
                return Err(crate::PodcastError::Script(format!(
                    "Segment {} references unknown speaker '{}'",
                    i, segment.speaker
                )));
            }
            if segment.text.trim().is_empty() {
                return Err(crate::PodcastError::Script(format!(
                    "Segment {} has empty text",
                    i
                )));
            }
        }

        Ok(())
    }

    /// Returns the total number of segments.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Returns the unique speaker names.
    pub fn speaker_names(&self) -> Vec<&str> {
        self.speakers.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_script_json() -> &'static str {
        r#"{
            "title": "Test Episode",
            "speakers": {
                "host": { "voice_id": "alloy", "speed": 1.0, "model": "tts-1" },
                "guest": { "voice_id": "echo" }
            },
            "segments": [
                { "speaker": "host", "text": "Welcome to the show!" },
                { "speaker": "guest", "text": "Thanks for having me." },
                { "speaker": "host", "text": "Let's dive in." }
            ]
        }"#
    }

    #[test]
    fn test_parse_script() {
        let script = PodcastScript::from_json(sample_script_json()).unwrap();
        assert_eq!(script.title, "Test Episode");
        assert_eq!(script.speakers.len(), 2);
        assert_eq!(script.segments.len(), 3);
        assert_eq!(script.segments[0].speaker, "host");
        assert_eq!(script.segments[1].speaker, "guest");
    }

    #[test]
    fn test_parse_script_with_config() {
        let json = r#"{
            "title": "Configured Episode",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "Hello!" }
            ],
            "config": {
                "pause_between_segments_secs": 1.0,
                "normalize_target": 0.9
            }
        }"#;
        let script = PodcastScript::from_json(json).unwrap();
        assert!((script.config.pause_between_segments_secs - 1.0).abs() < f64::EPSILON);
        assert!((script.config.normalize_target - 0.9_f32).abs() < f32::EPSILON);
    }

    #[test]
    fn test_validate_unknown_speaker() {
        let json = r#"{
            "title": "Bad",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "unknown_person", "text": "Hello" }
            ]
        }"#;
        assert!(PodcastScript::from_json(json).is_err());
    }

    #[test]
    fn test_validate_empty_segments() {
        let json = r#"{
            "title": "Empty",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": []
        }"#;
        assert!(PodcastScript::from_json(json).is_err());
    }

    #[test]
    fn test_validate_empty_text() {
        let json = r#"{
            "title": "Empty Text",
            "speakers": {
                "host": { "voice_id": "alloy" }
            },
            "segments": [
                { "speaker": "host", "text": "   " }
            ]
        }"#;
        assert!(PodcastScript::from_json(json).is_err());
    }

    #[test]
    fn test_segment_count() {
        let script = PodcastScript::from_json(sample_script_json()).unwrap();
        assert_eq!(script.segment_count(), 3);
    }
}
