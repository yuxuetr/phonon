use serde::{Deserialize, Serialize};

/// Configuration for the podcast generation pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Silence duration between segments in seconds.
    #[serde(default = "default_pause")]
    pub pause_between_segments_secs: f64,

    /// Path to intro audio file (optional).
    #[serde(default)]
    pub intro_audio: Option<String>,

    /// Path to outro audio file (optional).
    #[serde(default)]
    pub outro_audio: Option<String>,

    /// Path to background music audio file (optional).
    #[serde(default)]
    pub bgm_audio: Option<String>,

    /// BGM volume level (0.0 - 1.0). Default: 0.08.
    #[serde(default = "default_bgm_volume")]
    pub bgm_volume: f32,

    /// Crossfade duration between segments in seconds.
    #[serde(default = "default_crossfade")]
    pub crossfade_secs: f64,

    /// Output sample rate in Hz.
    #[serde(default = "default_sample_rate")]
    pub output_sample_rate: u32,

    /// Output channel count.
    #[serde(default = "default_channels")]
    pub output_channels: u16,

    /// Target peak amplitude for final normalization (0.0 - 1.0).
    #[serde(default = "default_normalize_target")]
    pub normalize_target: f32,

    /// Fade-in duration at the start of the episode in seconds.
    #[serde(default = "default_fade_in")]
    pub fade_in_secs: f64,

    /// Fade-out duration at the end of the episode in seconds.
    #[serde(default = "default_fade_out")]
    pub fade_out_secs: f64,
}

fn default_pause() -> f64 {
    0.5
}
fn default_bgm_volume() -> f32 {
    0.08
}
fn default_crossfade() -> f64 {
    0.0
}
fn default_sample_rate() -> u32 {
    44100
}
fn default_channels() -> u16 {
    1
}
fn default_normalize_target() -> f32 {
    0.95
}
fn default_fade_in() -> f64 {
    0.0
}
fn default_fade_out() -> f64 {
    0.0
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            pause_between_segments_secs: default_pause(),
            intro_audio: None,
            outro_audio: None,
            bgm_audio: None,
            bgm_volume: default_bgm_volume(),
            crossfade_secs: default_crossfade(),
            output_sample_rate: default_sample_rate(),
            output_channels: default_channels(),
            normalize_target: default_normalize_target(),
            fade_in_secs: default_fade_in(),
            fade_out_secs: default_fade_out(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PipelineConfig::default();
        assert!((config.pause_between_segments_secs - 0.5).abs() < f64::EPSILON);
        assert!((config.bgm_volume - 0.08).abs() < f32::EPSILON);
        assert_eq!(config.output_sample_rate, 44100);
        assert_eq!(config.output_channels, 1);
        assert!((config.normalize_target - 0.95).abs() < f32::EPSILON);
        assert!(config.intro_audio.is_none());
        assert!(config.bgm_audio.is_none());
    }

    #[test]
    fn test_config_serde_partial() {
        let json = r#"{"pause_between_segments_secs": 1.0, "bgm_volume": 0.1}"#;
        let config: PipelineConfig = serde_json::from_str(json).unwrap();
        assert!((config.pause_between_segments_secs - 1.0).abs() < f64::EPSILON);
        assert!((config.bgm_volume - 0.1).abs() < f32::EPSILON);
        // Defaults for unspecified fields
        assert_eq!(config.output_sample_rate, 44100);
        assert!((config.normalize_target - 0.95).abs() < f32::EPSILON);
    }
}
