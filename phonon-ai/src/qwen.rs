//! Qwen (Alibaba DashScope) native TTS provider.
//!
//! DashScope's OpenAI-compatible mode has no `/audio/speech`, so this talks
//! to the native `multimodal-generation` API. Set `DASHSCOPE_API_KEY`.
//!
//! The response carries a short-lived URL to the audio, not the audio
//! itself, so every synthesis is two requests: generate, then download.
//! The returned WAV declares placeholder chunk sizes; `decode_audio`
//! (symphonia) reads it correctly.

use crate::AiError;
use crate::client::{HttpClient, ensure_success, env_key, retry_transient};
use crate::voice::VoiceConfig;
use phonon_core::AudioBuffer;
use serde::{Deserialize, Serialize};
use tracing::{info, instrument};

/// Mainland endpoint; the international one is `https://dashscope-intl.aliyuncs.com/api/v1`.
pub const DASHSCOPE_BASE_URL: &str = "https://dashscope.aliyuncs.com/api/v1";
const GENERATION_PATH: &str = "services/aigc/multimodal-generation/generation";

/// Qwen TTS provider. Like [`crate::MiniMaxTts`], the model is set on the
/// provider (default `qwen3-tts-flash`); [`VoiceConfig::model`] is ignored.
#[derive(Debug, Clone)]
pub struct QwenTts {
  client: HttpClient,
  model: String,
}

impl QwenTts {
  /// Reads the key from `DASHSCOPE_API_KEY`.
  pub fn new() -> Result<Self, AiError> {
    Self::with_api_key(env_key("DASHSCOPE_API_KEY")?)
  }

  /// Uses an explicit key and the mainland endpoint.
  pub fn with_api_key(api_key: impl Into<String>) -> Result<Self, AiError> {
    Ok(Self::with_client(HttpClient::new(
      api_key,
      DASHSCOPE_BASE_URL,
      None,
    )?))
  }

  /// Uses a caller-configured client (base URL is the `.../api/v1` root).
  pub fn with_client(client: HttpClient) -> Self {
    Self {
      client,
      model: "qwen3-tts-flash".to_string(),
    }
  }

  /// Override the default model.
  pub fn with_model(mut self, model: impl Into<String>) -> Self {
    self.model = model.into();
    self
  }
}

#[derive(Serialize)]
struct TtsRequest<'a> {
  model: &'a str,
  input: TtsInput<'a>,
}

#[derive(Serialize)]
struct TtsInput<'a> {
  text: &'a str,
  voice: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  language_type: Option<&'static str>,
}

#[derive(Deserialize)]
struct TtsResponse {
  output: TtsOutput,
}

#[derive(Deserialize)]
struct TtsOutput {
  audio: TtsAudio,
}

#[derive(Deserialize)]
struct TtsAudio {
  url: String,
}

/// DashScope's `language_type` for a locale such as `zh-CN`; `None` lets
/// the model detect the language.
fn language_type(language: Option<&str>) -> Option<&'static str> {
  let primary = language?.split(['-', '_']).next()?.to_ascii_lowercase();
  match primary.as_str() {
    "zh" => Some("Chinese"),
    "en" => Some("English"),
    "ja" => Some("Japanese"),
    "ko" => Some("Korean"),
    _ => None,
  }
}

impl crate::tts::TtsProvider for QwenTts {
  #[instrument(skip(self, text), fields(text_len = text.len(), voice = %voice.voice_id))]
  async fn synthesize(&self, text: &str, voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
    let request = TtsRequest {
      model: &self.model,
      input: TtsInput {
        text,
        voice: &voice.voice_id,
        language_type: language_type(voice.language.as_deref()),
      },
    };
    let url = format!("{}/{GENERATION_PATH}", self.client.base_url());

    let parsed: TtsResponse = retry_transient(|| async {
      let resp = self
        .client
        .inner()
        .post(&url)
        .header("Authorization", format!("Bearer {}", self.client.api_key()))
        .json(&request)
        .send()
        .await
        .map_err(|e| AiError::Http(e.to_string()))?;
      ensure_success(resp)
        .await?
        .json()
        .await
        .map_err(|e| AiError::Decode(format!("Qwen TTS response: {e}")))
    })
    .await?;

    // The URL is pre-signed; it must not carry our key.
    let audio_bytes = retry_transient(|| async {
      let resp = self
        .client
        .inner()
        .get(&parsed.output.audio.url)
        .send()
        .await
        .map_err(|e| AiError::Http(e.to_string()))?;
      ensure_success(resp)
        .await?
        .bytes()
        .await
        .map_err(|e| AiError::Http(e.to_string()))
    })
    .await?;

    info!(bytes = audio_bytes.len(), "Downloaded Qwen TTS audio");
    crate::tts::decode(&audio_bytes, "wav")
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_language_type_maps_locales() {
    assert_eq!(language_type(Some("zh-CN")), Some("Chinese"));
    assert_eq!(language_type(Some("EN_us")), Some("English"));
    assert_eq!(language_type(Some("ja")), Some("Japanese"));
    assert_eq!(language_type(Some("fr-FR")), None);
    assert_eq!(language_type(None), None);
  }

  #[test]
  fn test_request_shape() {
    let request = TtsRequest {
      model: "qwen3-tts-flash",
      input: TtsInput {
        text: "你好",
        voice: "Cherry",
        language_type: None,
      },
    };
    let json = serde_json::to_value(&request).unwrap_or_default();
    assert_eq!(json["input"]["voice"], "Cherry");
    assert!(json["input"].get("language_type").is_none());
  }
}
