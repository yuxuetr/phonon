use serde::{Deserialize, Serialize};

/// Configuration for a TTS voice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// The voice identifier (e.g. "alloy", "echo", "nova", "shimmer", "fable", "onyx").
    pub voice_id: String,

    /// Speech speed multiplier. Range: 0.25 - 4.0. Default: 1.0.
    #[serde(default = "default_speed")]
    pub speed: f64,

    /// The TTS model to use (e.g. "tts-1", "tts-1-hd").
    #[serde(default = "default_model")]
    pub model: String,

    /// Emotion for the voice (e.g. "happy", "sad", "excited", "calm").
    /// Provider-specific; ignored if the provider does not support it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emotion: Option<String>,

    /// Speaking style (e.g. "conversational", "narration", "newscast").
    /// Provider-specific; ignored if the provider does not support it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,

    /// Language / locale (e.g. "en-US", "zh-CN", "ja-JP").
    /// Provider-specific; used by multi-language providers like Edge TTS.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

fn default_speed() -> f64 {
    1.0
}

fn default_model() -> String {
    "tts-1".to_string()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            voice_id: "alloy".to_string(),
            speed: default_speed(),
            model: default_model(),
            emotion: None,
            style: None,
            language: None,
        }
    }
}

impl VoiceConfig {
    /// Create a new VoiceConfig with the given voice ID, using defaults for other fields.
    pub fn new(voice_id: impl Into<String>) -> Self {
        Self {
            voice_id: voice_id.into(),
            ..Default::default()
        }
    }

    /// Set the speech speed.
    pub fn with_speed(mut self, speed: f64) -> Self {
        self.speed = speed.clamp(0.25, 4.0);
        self
    }

    /// Set the TTS model.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    /// Set the emotion.
    pub fn with_emotion(mut self, emotion: impl Into<String>) -> Self {
        self.emotion = Some(emotion.into());
        self
    }

    /// Set the speaking style.
    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// Set the language / locale.
    pub fn with_language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_config_default() {
        let config = VoiceConfig::default();
        assert_eq!(config.voice_id, "alloy");
        assert!((config.speed - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.model, "tts-1");
    }

    #[test]
    fn test_voice_config_builder() {
        let config = VoiceConfig::new("echo")
            .with_speed(1.5)
            .with_model("tts-1-hd");
        assert_eq!(config.voice_id, "echo");
        assert!((config.speed - 1.5).abs() < f64::EPSILON);
        assert_eq!(config.model, "tts-1-hd");
    }

    #[test]
    fn test_voice_config_speed_clamped() {
        let config = VoiceConfig::new("alloy").with_speed(10.0);
        assert!((config.speed - 4.0).abs() < f64::EPSILON);

        let config = VoiceConfig::new("alloy").with_speed(-1.0);
        assert!((config.speed - 0.25).abs() < f64::EPSILON);
    }

    #[test]
    fn test_voice_config_serde_roundtrip() {
        let config = VoiceConfig::new("nova").with_speed(1.2).with_model("tts-1-hd");
        let json = serde_json::to_string(&config).unwrap();
        let parsed: VoiceConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.voice_id, "nova");
        assert!((parsed.speed - 1.2).abs() < f64::EPSILON);
        assert_eq!(parsed.model, "tts-1-hd");
    }

    #[test]
    fn test_voice_config_serde_defaults() {
        let json = r#"{"voice_id": "shimmer"}"#;
        let config: VoiceConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.voice_id, "shimmer");
        assert!((config.speed - 1.0).abs() < f64::EPSILON);
        assert_eq!(config.model, "tts-1");
    }
}
