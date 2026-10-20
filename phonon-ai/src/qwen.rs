//! Qwen (Alibaba DashScope) native TTS and STT providers.
//!
//! DashScope's OpenAI-compatible mode has neither `/audio/speech` nor
//! `/audio/transcriptions`, so both talk to the native
//! `multimodal-generation` API. Set `DASHSCOPE_API_KEY`.
//!
//! TTS responses carry a short-lived URL to the audio, not the audio
//! itself, so every synthesis is two requests: generate, then download.
//! The returned WAV declares placeholder chunk sizes; `decode_audio`
//! (symphonia) reads it correctly.
//!
//! STT returns one "sentence" with word-level times however long the clip
//! is; [`QwenStt`] splits it into segments at sentence-ending punctuation.

use crate::AiError;
use crate::client::{HttpClient, ensure_success, env_key, retry_transient};
use crate::stt::{SttProvider, TranscribeOptions, TranscriptResult, TranscriptSegment};
use crate::voice::VoiceConfig;
use base64::Engine;
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

/// Qwen STT provider (`qwen-audio-3.0-asr-flash` by default). DashScope caps
/// the request at 10 MB of audio. `language` and `prompt` options are ignored.
#[derive(Debug, Clone)]
pub struct QwenStt {
  client: HttpClient,
  model: String,
}

impl QwenStt {
  /// Reads the key from `DASHSCOPE_API_KEY`.
  pub fn new() -> Result<Self, AiError> {
    Ok(Self::with_client(HttpClient::new(
      env_key("DASHSCOPE_API_KEY")?,
      DASHSCOPE_BASE_URL,
      None,
    )?))
  }

  /// Uses a caller-configured client (base URL is the `.../api/v1` root).
  pub fn with_client(client: HttpClient) -> Self {
    Self {
      client,
      model: "qwen-audio-3.0-asr-flash".to_string(),
    }
  }

  /// Override the default model.
  pub fn with_model(mut self, model: impl Into<String>) -> Self {
    self.model = model.into();
    self
  }
}

#[derive(Deserialize)]
struct SttResponse {
  output: SttOutput,
}

#[derive(Deserialize)]
struct SttOutput {
  #[serde(default)]
  text: String,
  sentence: Option<SttSentence>,
}

#[derive(Deserialize)]
struct SttSentence {
  #[serde(default)]
  words: Vec<SttWord>,
}

#[derive(Deserialize)]
struct SttWord {
  begin_time: u64,
  end_time: u64,
  text: String,
  #[serde(default)]
  punctuation: String,
}

/// Group words (times in ms) into segments ending at sentence punctuation.
fn segments_from_words(words: &[SttWord]) -> Vec<TranscriptSegment> {
  let mut segments = Vec::new();
  let mut text = String::new();
  let mut start = None;
  for word in words {
    let begin = *start.get_or_insert(word.begin_time);
    text.push_str(&word.text);
    text.push_str(&word.punctuation);
    let ends_sentence = word
      .punctuation
      .chars()
      .any(|c| matches!(c, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';'));
    if ends_sentence {
      segments.push(TranscriptSegment {
        id: segments.len(),
        start: begin as f64 / 1000.0,
        end: word.end_time as f64 / 1000.0,
        text: std::mem::take(&mut text),
      });
      start = None;
    }
  }
  if let (Some(begin), Some(last)) = (start, words.last()) {
    segments.push(TranscriptSegment {
      id: segments.len(),
      start: begin as f64 / 1000.0,
      end: last.end_time as f64 / 1000.0,
      text,
    });
  }
  segments
}

impl SttProvider for QwenStt {
  async fn transcribe_file(
    &self,
    path: &str,
    options: Option<TranscribeOptions>,
  ) -> Result<TranscriptResult, AiError> {
    let data = tokio::fs::read(path)
      .await
      .map_err(|e| AiError::Io(e.to_string()))?;
    self.transcribe_bytes(data, path, options).await
  }

  #[instrument(skip(self, data, _options), fields(model = %self.model, size = data.len()))]
  async fn transcribe_bytes(
    &self,
    data: Vec<u8>,
    filename: &str,
    _options: Option<TranscribeOptions>,
  ) -> Result<TranscriptResult, AiError> {
    let format = std::path::Path::new(filename)
      .extension()
      .and_then(|e| e.to_str())
      .unwrap_or("wav")
      .to_ascii_lowercase();
    let audio = format!(
      "data:audio/{format};base64,{}",
      base64::engine::general_purpose::STANDARD.encode(&data)
    );
    let body = serde_json::json!({
      "model": self.model,
      "input": {"messages": [{"role": "user", "content": [
        {"type": "input_audio", "input_audio": {"data": audio}}
      ]}]},
      "parameters": {"format": format},
    });
    let url = format!("{}/{GENERATION_PATH}", self.client.base_url());

    let parsed: SttResponse = retry_transient(|| async {
      let resp = self
        .client
        .inner()
        .post(&url)
        .header("Authorization", format!("Bearer {}", self.client.api_key()))
        .json(&body)
        .send()
        .await
        .map_err(|e| AiError::Http(e.to_string()))?;
      ensure_success(resp)
        .await?
        .json()
        .await
        .map_err(|e| AiError::Decode(format!("Qwen STT response: {e}")))
    })
    .await?;

    let words = parsed.output.sentence.map(|s| s.words).unwrap_or_default();
    let segments = segments_from_words(&words);
    info!(segments = segments.len(), "Qwen transcription complete");
    Ok(TranscriptResult {
      text: parsed.output.text,
      language: None,
      duration: words.last().map(|w| w.end_time as f64 / 1000.0),
      segments,
    })
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

  fn word(begin_time: u64, end_time: u64, text: &str, punctuation: &str) -> SttWord {
    SttWord {
      begin_time,
      end_time,
      text: text.to_string(),
      punctuation: punctuation.to_string(),
    }
  }

  #[test]
  fn test_segments_split_at_sentence_punctuation() {
    let words = [
      word(160, 640, "今天", ""),
      word(640, 1160, "很好", "，"),
      word(1200, 1800, "散步", "。"),
      word(2500, 3000, "回家", "！"),
      word(3500, 4000, "再见", ""),
    ];
    let segments = segments_from_words(&words);
    let summary: Vec<_> = segments
      .iter()
      .map(|s| (s.id, s.start, s.end, s.text.as_str()))
      .collect();
    assert_eq!(
      summary,
      [
        (0, 0.16, 1.8, "今天很好，散步。"),
        (1, 2.5, 3.0, "回家！"),
        (2, 3.5, 4.0, "再见"),
      ]
    );
    assert!(segments_from_words(&[]).is_empty());
  }

  #[test]
  fn test_stt_response_parses_observed_shape() {
    let json = r#"{"output":{"sentence":{"begin_time":1960,"end_time":4148,
      "words":[{"begin_time":1960,"end_time":2440,"fixed":true,"punctuation":"，","text":"你好"}]},
      "text":"你好，"},"usage":{"duration":4}}"#;
    let parsed: SttResponse = serde_json::from_str(json).expect("observed shape");
    assert_eq!(parsed.output.text, "你好，");
    assert_eq!(parsed.output.sentence.map(|s| s.words.len()), Some(1));
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
