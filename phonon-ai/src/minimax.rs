//! MiniMax T2A v2 TTS provider.
//!
//! Uses the MiniMax `t2a_v2` HTTP API to synthesize Chinese / English /
//! Japanese / Cantonese speech with emotion control.
//!
//! # Setup
//!
//! Set the `MINIMAX_API_KEY` environment variable. The value is a JWT
//! bearer token issued by the MiniMax console.
//!
//! # Endpoint
//!
//! Default: `https://api.minimaxi.com/v1`. A Beijing backup endpoint
//! (`https://api-bj.minimaxi.com/v1`) is also offered by MiniMax; swap
//! it in via [`MiniMaxTts::with_base_url`].
//!
//! # Example
//!
//! ```rust,no_run
//! use phonon_ai::{MiniMaxTts, TtsProvider, VoiceConfig};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let tts = MiniMaxTts::new()?;
//! let voice = VoiceConfig::new("Chinese (Mandarin)_Lyrical_Voice")
//!   .with_language("zh-CN")
//!   .with_emotion("calm");
//! let audio = tts.synthesize("你好，欢迎收听播客", &voice).await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Differences from other phonon TTS providers
//!
//! * **Response is hex-encoded**, not base64. The `data.audio` field
//!   carries a hex string that decodes to the raw `audio_setting.format`
//!   payload (mp3 / wav / flac / pcm).
//! * **Business-level errors arrive as HTTP 200**. `base_resp.status_code`
//!   must be checked separately — values like `1039` (TPM throttling)
//!   look fine at the HTTP layer.
//! * **No multi-speaker dialogue mode**. Each call renders one voice;
//!   alternating speakers requires N sequential calls (which is exactly
//!   how `phonon_podcast::PodcastPipeline` already drives TTS).

use crate::AiError;
use crate::client::{HttpClient, env_key, retry_transient};
use crate::voice::VoiceConfig;
use phonon_core::AudioBuffer;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

const MINIMAX_BASE_URL: &str = "https://api.minimaxi.com/v1";

/// MiniMax T2A v2 TTS provider.
#[derive(Debug, Clone)]
pub struct MiniMaxTts {
  client: HttpClient,
  model: String,
  sample_rate: u32,
  format: String,
}

impl MiniMaxTts {
  /// Creates a new MiniMax TTS provider reading the API key from
  /// `MINIMAX_API_KEY`. Defaults to the `speech-2.8-hd` model, 32 kHz
  /// sample rate, and `mp3` output.
  pub fn new() -> Result<Self, AiError> {
    Self::with_api_key(env_key("MINIMAX_API_KEY")?)
  }

  /// Creates a new MiniMax TTS provider with an explicit API key.
  pub fn with_api_key(api_key: impl Into<String>) -> Result<Self, AiError> {
    let client = HttpClient::new(api_key, MINIMAX_BASE_URL, Some(120))?;
    Ok(Self {
      client,
      model: "speech-2.8-hd".to_string(),
      sample_rate: 32_000,
      format: "mp3".to_string(),
    })
  }

  /// Override the model name. Valid values per MiniMax docs:
  /// `speech-2.8-hd`, `speech-2.8-turbo`, `speech-2.6-hd`,
  /// `speech-2.6-turbo`, `speech-02-hd`, `speech-02-turbo`,
  /// `speech-01-hd`, `speech-01-turbo`.
  pub fn with_model(mut self, model: impl Into<String>) -> Self {
    self.model = model.into();
    self
  }

  /// Override the output sample rate. Supported values: 8000, 16000,
  /// 22050, 24000, 32000, 44100.
  pub fn with_sample_rate(mut self, sample_rate: u32) -> Self {
    self.sample_rate = sample_rate;
    self
  }

  /// Override the output container format. Supported by phonon-io decode:
  /// `mp3`, `wav`, `flac`. Other MiniMax formats (`pcm`, `opus`, `pcmu_*`)
  /// are rejected at decode time.
  pub fn with_format(mut self, format: impl Into<String>) -> Self {
    self.format = format.into();
    self
  }

  /// Override the base URL (e.g. swap to the Beijing backup endpoint).
  pub fn with_base_url(self, base_url: impl Into<String>) -> Result<Self, AiError> {
    let api_key = self.client.api_key().to_string();
    let client = HttpClient::new(api_key, base_url, Some(120))?;
    Ok(Self {
      client,
      model: self.model,
      sample_rate: self.sample_rate,
      format: self.format,
    })
  }

  /// Build the `voice_setting` block from a [`VoiceConfig`].
  ///
  /// Speed is clamped to MiniMax's documented `[0.5, 2.0]` range
  /// (phonon's `VoiceConfig` allows 0.25-4.0). Emotion is passed through
  /// only when it matches one of MiniMax's documented values.
  fn build_voice_setting<'a>(&self, voice: &'a VoiceConfig) -> VoiceSetting<'a> {
    let speed = clamp_speed(voice.speed);
    let emotion = voice.emotion.as_deref().and_then(normalise_emotion);
    VoiceSetting {
      voice_id: &voice.voice_id,
      speed,
      vol: None,
      pitch: None,
      emotion,
    }
  }

  /// Pick the matching format hint for [`phonon_io::decode_audio`] from the
  /// configured output format. PCM / opus / pcmu variants are returned
  /// as `None` (caller will reject — Symphonia cannot guess the rate
  /// without a container).
  fn decode_suffix(&self) -> Option<&'static str> {
    match self.format.as_str() {
      "mp3" => Some("mp3"),
      "wav" => Some("wav"),
      "flac" => Some("flac"),
      _ => None,
    }
  }
}

/// Maps phonon `VoiceConfig.language` (BCP-47-ish) to MiniMax's
/// `language_boost` parameter.
///
/// MiniMax's docs only enumerate a handful of values; returning `None`
/// leaves the field off the request (server uses its automatic
/// detection).
fn language_boost(language: Option<&str>) -> Option<&'static str> {
  let raw = language?;
  let lower = raw.to_ascii_lowercase();
  if lower.starts_with("zh-hk") || lower.starts_with("zh-yue") {
    Some("Chinese,Yue")
  } else if lower.starts_with("zh") {
    Some("Chinese")
  } else if lower.starts_with("en") {
    Some("English")
  } else if lower.starts_with("ja") {
    Some("Japanese")
  } else if lower.starts_with("ko") {
    Some("Korean")
  } else {
    None
  }
}

/// HTTP-style status for a MiniMax `base_resp.status_code`. Rate limiting
/// (1002 RPM, 1039 TPM) maps to 429 so it is retried as transient; other
/// codes pass through unchanged.
fn business_error_status(code: i32) -> u16 {
  match code {
    1002 | 1039 => 429,
    other => u16::try_from(other).unwrap_or(500),
  }
}

/// Clamp speed into MiniMax's documented `[0.5, 2.0]` range.
fn clamp_speed(speed: f64) -> Option<f64> {
  if (speed - 1.0).abs() < f64::EPSILON {
    None
  } else {
    Some(speed.clamp(0.5, 2.0))
  }
}

/// Whitelist of MiniMax-supported emotion values. Anything else is
/// silently dropped (rather than forwarded and rejected by the server).
fn normalise_emotion(emotion: &str) -> Option<&'static str> {
  match emotion.to_ascii_lowercase().as_str() {
    "happy" => Some("happy"),
    "sad" => Some("sad"),
    "angry" => Some("angry"),
    "fearful" => Some("fearful"),
    "disgusted" => Some("disgusted"),
    "surprised" => Some("surprised"),
    "calm" | "neutral" => Some("calm"),
    "fluent" => Some("fluent"),
    "whisper" => Some("whisper"),
    _ => None,
  }
}

#[derive(Serialize)]
struct T2aRequest<'a> {
  model: &'a str,
  text: &'a str,
  voice_setting: VoiceSetting<'a>,
  audio_setting: AudioSetting<'a>,
  stream: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  language_boost: Option<&'static str>,
}

#[derive(Serialize)]
struct VoiceSetting<'a> {
  voice_id: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  speed: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  vol: Option<f64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pitch: Option<i32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  emotion: Option<&'static str>,
}

#[derive(Serialize)]
struct AudioSetting<'a> {
  sample_rate: u32,
  format: &'a str,
}

#[derive(Deserialize)]
struct T2aResponse {
  data: Option<T2aData>,
  #[serde(default)]
  base_resp: BaseResp,
}

#[derive(Deserialize)]
struct T2aData {
  /// Hex-encoded audio payload in the requested `audio_setting.format`.
  audio: Option<String>,
}

#[derive(Deserialize, Default)]
struct BaseResp {
  #[serde(default)]
  status_code: i32,
  #[serde(default)]
  status_msg: String,
}

impl MiniMaxTts {
  /// One T2A request; business errors in `base_resp` become `AiError::Api`.
  async fn send_t2a(&self, url: &str, request: &T2aRequest<'_>) -> Result<T2aResponse, AiError> {
    let response = self
      .client
      .inner()
      .post(url)
      .header("Authorization", format!("Bearer {}", self.client.api_key()))
      .header("Content-Type", "application/json")
      .json(request)
      .send()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    let http_status = response.status();
    let raw = response
      .text()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    if !http_status.is_success() {
      return Err(AiError::Api {
        status: http_status.as_u16(),
        message: raw,
      });
    }

    let parsed: T2aResponse = serde_json::from_str(&raw).map_err(|e| {
      AiError::Decode(format!(
        "MiniMax T2A response is not valid JSON ({e}); body={raw}"
      ))
    })?;

    let code = parsed.base_resp.status_code;
    if code != 0 {
      // MiniMax surfaces business errors as HTTP 200 with a non-zero
      // `base_resp.status_code`.
      let status = business_error_status(code);
      return Err(AiError::Api {
        status,
        message: format!(
          "MiniMax T2A error ({code}): {}",
          parsed.base_resp.status_msg
        ),
      });
    }
    Ok(parsed)
  }
}

impl crate::tts::TtsProvider for MiniMaxTts {
  #[instrument(skip(self, text), fields(text_len = text.len(), voice = %voice.voice_id, model = %self.model))]
  async fn synthesize(&self, text: &str, voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
    info!("Synthesizing text via MiniMax T2A");

    let suffix = self.decode_suffix().ok_or_else(|| {
      AiError::Config(format!(
        "MiniMax output format '{}' cannot be decoded by phonon-io (use mp3 / wav / flac)",
        self.format
      ))
    })?;

    let request = T2aRequest {
      model: &self.model,
      text,
      voice_setting: self.build_voice_setting(voice),
      audio_setting: AudioSetting {
        sample_rate: self.sample_rate,
        format: &self.format,
      },
      stream: false,
      language_boost: language_boost(voice.language.as_deref()),
    };

    let url = format!("{}/t2a_v2", self.client.base_url());
    let parsed = retry_transient(|| self.send_t2a(&url, &request)).await?;

    let hex_audio = parsed.data.and_then(|d| d.audio).ok_or_else(|| {
      AiError::Decode("MiniMax T2A response missing data.audio field".to_string())
    })?;

    let audio_bytes = hex::decode(&hex_audio).map_err(|e| {
      AiError::Decode(format!(
        "MiniMax T2A data.audio is not valid hex ({e}); first_chars={}",
        hex_audio.chars().take(40).collect::<String>()
      ))
    })?;

    info!(
      bytes = audio_bytes.len(),
      "Received MiniMax T2A audio response"
    );

    let buffer = crate::tts::decode(&audio_bytes, suffix)?;

    info!(
      duration = buffer.duration_secs(),
      samples = buffer.samples.len(),
      "MiniMax T2A synthesis complete"
    );

    Ok(buffer)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_throttling_codes_are_transient() {
    for code in [1002, 1039] {
      let err = AiError::Api {
        status: business_error_status(code),
        message: String::new(),
      };
      assert!(err.is_transient(), "code {code}");
    }
    // Invalid API key (1004) is permanent.
    let err = AiError::Api {
      status: business_error_status(1004),
      message: String::new(),
    };
    assert!(!err.is_transient());
  }

  #[test]
  fn with_api_key_succeeds_with_defaults() {
    let tts = MiniMaxTts::with_api_key("test-key").expect("construct");
    assert_eq!(tts.model, "speech-2.8-hd");
    assert_eq!(tts.sample_rate, 32_000);
    assert_eq!(tts.format, "mp3");
  }

  #[test]
  fn with_model_sample_rate_format_overrides_chain() {
    let tts = MiniMaxTts::with_api_key("test-key")
      .expect("construct")
      .with_model("speech-02-turbo")
      .with_sample_rate(44_100)
      .with_format("wav");
    assert_eq!(tts.model, "speech-02-turbo");
    assert_eq!(tts.sample_rate, 44_100);
    assert_eq!(tts.format, "wav");
  }

  #[test]
  fn decode_suffix_recognises_known_formats() {
    let mp3 = MiniMaxTts::with_api_key("k").unwrap();
    assert_eq!(mp3.decode_suffix(), Some("mp3"));
    let wav = MiniMaxTts::with_api_key("k").unwrap().with_format("wav");
    assert_eq!(wav.decode_suffix(), Some("wav"));
    let flac = MiniMaxTts::with_api_key("k").unwrap().with_format("flac");
    assert_eq!(flac.decode_suffix(), Some("flac"));
    let pcm = MiniMaxTts::with_api_key("k").unwrap().with_format("pcm");
    assert_eq!(pcm.decode_suffix(), None);
    let opus = MiniMaxTts::with_api_key("k").unwrap().with_format("opus");
    assert_eq!(opus.decode_suffix(), None);
  }

  #[test]
  fn clamp_speed_returns_none_at_one() {
    assert_eq!(clamp_speed(1.0), None);
  }

  #[test]
  fn clamp_speed_clamps_to_valid_range() {
    assert_eq!(clamp_speed(0.1), Some(0.5));
    assert_eq!(clamp_speed(5.0), Some(2.0));
    assert_eq!(clamp_speed(1.5), Some(1.5));
  }

  #[test]
  fn normalise_emotion_whitelists_documented_values() {
    assert_eq!(normalise_emotion("happy"), Some("happy"));
    assert_eq!(normalise_emotion("Calm"), Some("calm"));
    assert_eq!(normalise_emotion("neutral"), Some("calm"));
    assert_eq!(normalise_emotion("whisper"), Some("whisper"));
  }

  #[test]
  fn normalise_emotion_drops_unknown_values() {
    assert_eq!(normalise_emotion("conversational"), None);
    assert_eq!(normalise_emotion("excited"), None);
    assert_eq!(normalise_emotion(""), None);
  }

  #[test]
  fn language_boost_maps_chinese_variants() {
    assert_eq!(language_boost(Some("zh-CN")), Some("Chinese"));
    assert_eq!(language_boost(Some("zh-TW")), Some("Chinese"));
    assert_eq!(language_boost(Some("zh-HK")), Some("Chinese,Yue"));
    assert_eq!(language_boost(Some("zh-yue")), Some("Chinese,Yue"));
  }

  #[test]
  fn language_boost_maps_other_languages() {
    assert_eq!(language_boost(Some("en-US")), Some("English"));
    assert_eq!(language_boost(Some("ja-JP")), Some("Japanese"));
    assert_eq!(language_boost(Some("ko-KR")), Some("Korean"));
  }

  #[test]
  fn language_boost_returns_none_for_unknown_and_missing() {
    assert_eq!(language_boost(None), None);
    assert_eq!(language_boost(Some("fr-FR")), None);
    assert_eq!(language_boost(Some("")), None);
  }

  #[test]
  fn build_voice_setting_passes_voice_id_and_drops_default_speed() {
    let tts = MiniMaxTts::with_api_key("k").unwrap();
    let voice = VoiceConfig::new("Chinese (Mandarin)_Lyrical_Voice");
    let setting = tts.build_voice_setting(&voice);
    assert_eq!(setting.voice_id, "Chinese (Mandarin)_Lyrical_Voice");
    assert!(setting.speed.is_none(), "speed=1.0 should not be sent");
    assert!(setting.emotion.is_none(), "no emotion configured");
  }

  #[test]
  fn build_voice_setting_forwards_emotion_and_speed_when_set() {
    let tts = MiniMaxTts::with_api_key("k").unwrap();
    let voice = VoiceConfig::new("English_Insightful_Speaker")
      .with_speed(1.25)
      .with_emotion("happy");
    let setting = tts.build_voice_setting(&voice);
    assert_eq!(setting.speed, Some(1.25));
    assert_eq!(setting.emotion, Some("happy"));
  }

  #[test]
  fn request_serialisation_uses_documented_field_names() {
    // Lock down the wire shape; a renamed field would break the
    // MiniMax API contract immediately.
    let tts = MiniMaxTts::with_api_key("k").unwrap();
    let voice = VoiceConfig::new("vid")
      .with_language("zh-CN")
      .with_emotion("calm");
    let request = T2aRequest {
      model: &tts.model,
      text: "hello",
      voice_setting: tts.build_voice_setting(&voice),
      audio_setting: AudioSetting {
        sample_rate: tts.sample_rate,
        format: &tts.format,
      },
      stream: false,
      language_boost: language_boost(voice.language.as_deref()),
    };
    let json = serde_json::to_value(&request).expect("serialise");
    assert_eq!(json["model"], "speech-2.8-hd");
    assert_eq!(json["text"], "hello");
    assert_eq!(json["voice_setting"]["voice_id"], "vid");
    assert_eq!(json["voice_setting"]["emotion"], "calm");
    assert_eq!(json["audio_setting"]["sample_rate"], 32_000);
    assert_eq!(json["audio_setting"]["format"], "mp3");
    assert_eq!(json["language_boost"], "Chinese");
    assert_eq!(json["stream"], false);
    // Optional fields default-omitted:
    assert!(json["voice_setting"].get("vol").is_none());
    assert!(json["voice_setting"].get("pitch").is_none());
    assert!(json["voice_setting"].get("speed").is_none());
  }

  #[test]
  fn response_deserialisation_extracts_audio_hex_and_status() {
    let raw = r#"{
      "data": {"audio": "deadbeef", "status": 2},
      "extra_info": {"audio_length": 1000, "audio_format": "mp3", "usage_characters": 5},
      "trace_id": "abc",
      "base_resp": {"status_code": 0, "status_msg": "success"}
    }"#;
    let parsed: T2aResponse = serde_json::from_str(raw).expect("parse");
    assert_eq!(parsed.base_resp.status_code, 0);
    assert_eq!(parsed.data.unwrap().audio.unwrap(), "deadbeef");
  }

  #[test]
  fn response_with_business_error_carries_status_code() {
    let raw = r#"{
      "base_resp": {"status_code": 1039, "status_msg": "TPM throttled"}
    }"#;
    let parsed: T2aResponse = serde_json::from_str(raw).expect("parse");
    assert_eq!(parsed.base_resp.status_code, 1039);
    assert!(parsed.data.is_none());
  }
}
