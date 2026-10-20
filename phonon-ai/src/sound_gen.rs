//! AI sound/music generation.
//!
//! Provides the [`SoundGenProvider`] trait and a generic HTTP implementation
//! that can call music/SFX generation APIs.
//!
//! ## Supported APIs
//!
//! The [`HttpSoundGen`] provider works with any API that accepts a JSON prompt
//! and returns audio bytes (e.g. Replicate, Suno, or a local model endpoint).
//!
//! ## Example
//!
//! ```rust,no_run
//! use phonon_ai::sound_gen::{HttpSoundGen, SoundGenProvider, SoundPrompt};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let provider = HttpSoundGen::new(
//!     "https://api.example.com/v1/music",
//!     "my-api-key",
//! )?;
//! let prompt = SoundPrompt::new("calm ambient background music, 30 seconds")
//!     .with_duration(30.0);
//! let audio = provider.generate(&prompt).await?;
//! # Ok(())
//! # }
//! ```

use crate::AiError;
use crate::client::ApiKey;
use phonon_core::AudioBuffer;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::io::Cursor;
use std::time::Duration;
use tracing::{info, instrument};

/// Prompt / request for sound generation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundPrompt {
  /// Text description of the desired sound/music.
  pub prompt: String,
  /// Desired duration in seconds (provider may ignore).
  #[serde(default)]
  pub duration_secs: Option<f64>,
  /// Style hint (e.g. "ambient", "electronic", "orchestral").
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub style: Option<String>,
  /// Negative prompt — what to avoid.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub negative_prompt: Option<String>,
}

/// Preset categories for common sound generation use cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SoundPreset {
  /// Background music (30-60 s, ambient/electronic).
  Bgm,
  /// Short sound effect (1-5 s, punchy, no music).
  Sfx,
  /// Brief jingle or stinger (5-15 s, catchy melody).
  Jingle,
  /// Ambient soundscape (30-120 s, atmospheric, no melody).
  Ambient,
  /// Fully custom — no preset defaults applied.
  Custom,
}

impl SoundPrompt {
  /// Create a new sound generation prompt.
  pub fn new(prompt: impl Into<String>) -> Self {
    Self {
      prompt: prompt.into(),
      duration_secs: None,
      style: None,
      negative_prompt: None,
    }
  }

  /// Create a prompt from a preset with a description.
  ///
  /// The preset provides sensible defaults for duration, style, and
  /// negative prompt. The `description` is used as the main prompt text.
  pub fn from_preset(preset: SoundPreset, description: impl Into<String>) -> Self {
    let desc = description.into();
    match preset {
      SoundPreset::Bgm => Self {
        prompt: desc,
        duration_secs: Some(30.0),
        style: Some("background music".to_string()),
        negative_prompt: Some("vocals, speech, voice".to_string()),
      },
      SoundPreset::Sfx => Self {
        prompt: desc,
        duration_secs: Some(3.0),
        style: Some("sound effect".to_string()),
        negative_prompt: Some("music, melody, vocals".to_string()),
      },
      SoundPreset::Jingle => Self {
        prompt: desc,
        duration_secs: Some(10.0),
        style: Some("jingle".to_string()),
        negative_prompt: Some("vocals, speech".to_string()),
      },
      SoundPreset::Ambient => Self {
        prompt: desc,
        duration_secs: Some(60.0),
        style: Some("ambient soundscape".to_string()),
        negative_prompt: Some("melody, beat, vocals".to_string()),
      },
      SoundPreset::Custom => Self::new(desc),
    }
  }

  /// Set the desired duration.
  pub fn with_duration(mut self, secs: f64) -> Self {
    self.duration_secs = Some(secs);
    self
  }

  /// Set the style hint.
  pub fn with_style(mut self, style: impl Into<String>) -> Self {
    self.style = Some(style.into());
    self
  }

  /// Set a negative prompt.
  pub fn with_negative_prompt(mut self, neg: impl Into<String>) -> Self {
    self.negative_prompt = Some(neg.into());
    self
  }
}

/// Trait for AI sound/music generation providers.
pub trait SoundGenProvider: Send + Sync {
  /// Generate audio from a text prompt.
  fn generate(
    &self,
    prompt: &SoundPrompt,
  ) -> impl Future<Output = Result<AudioBuffer, AiError>> + Send;
}

/// Generic HTTP-based sound generation provider.
///
/// Sends a JSON prompt to a configurable endpoint and expects audio bytes
/// in the response. The response audio is decoded via phonon-io.
#[derive(Debug, Clone)]
pub struct HttpSoundGen {
  client: reqwest::Client,
  endpoint: String,
  api_key: ApiKey,
  response_format: String,
}

impl HttpSoundGen {
  /// Create a new `HttpSoundGen` provider.
  ///
  /// # Arguments
  ///
  /// * `endpoint` - The full URL of the generation API.
  /// * `api_key` - Bearer token for authentication.
  pub fn new(endpoint: impl Into<String>, api_key: impl Into<String>) -> Result<Self, AiError> {
    let client = reqwest::Client::builder()
      .timeout(std::time::Duration::from_secs(300))
      .build()
      .map_err(|e| AiError::Http(e.to_string()))?;

    Ok(Self {
      client,
      endpoint: endpoint.into(),
      api_key: ApiKey::new(api_key),
      response_format: "wav".to_string(),
    })
  }

  /// Set the expected response audio format (e.g. "wav", "mp3").
  pub fn with_response_format(mut self, fmt: impl Into<String>) -> Self {
    self.response_format = fmt.into();
    self
  }

  /// Decode audio bytes using phonon-io via a temp file.
  fn decode_audio_bytes(data: &[u8], suffix: &str) -> Result<AudioBuffer, AiError> {
    let ext = format!(".{suffix}");
    let tmp = tempfile::Builder::new()
      .suffix(&ext)
      .tempfile()
      .map_err(|e| AiError::Io(e.to_string()))?;

    std::io::copy(
      &mut Cursor::new(data),
      &mut std::fs::File::create(tmp.path()).map_err(|e| AiError::Io(e.to_string()))?,
    )
    .map_err(|e| AiError::Io(e.to_string()))?;

    phonon_io::read_audio(tmp.path()).map_err(|e| AiError::Decode(e.to_string()))
  }
}

impl SoundGenProvider for HttpSoundGen {
  #[instrument(skip(self, prompt), fields(prompt_len = prompt.prompt.len()))]
  async fn generate(&self, prompt: &SoundPrompt) -> Result<AudioBuffer, AiError> {
    info!(prompt = %prompt.prompt, "Generating sound via API");

    let response = self
      .client
      .post(&self.endpoint)
      .header("Authorization", format!("Bearer {}", self.api_key.expose()))
      .header("Content-Type", "application/json")
      .json(prompt)
      .send()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    if !response.status().is_success() {
      let status = response.status();
      let body = response
        .text()
        .await
        .unwrap_or_else(|_| "Failed to read error body".to_string());
      return Err(AiError::Api {
        status: status.as_u16(),
        message: body,
      });
    }

    let audio_bytes = response
      .bytes()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    info!(
      bytes = audio_bytes.len(),
      "Received audio from generation API"
    );

    let buffer = Self::decode_audio_bytes(&audio_bytes, &self.response_format)?;

    info!(
      duration = buffer.duration_secs(),
      samples = buffer.samples.len(),
      "Sound generation complete"
    );

    Ok(buffer)
  }
}

// ─── Replicate-based sound generation ───────────────────────────────

const REPLICATE_API_URL: &str = "https://api.replicate.com/v1/predictions";

/// Default MusicGen model version on Replicate.
const DEFAULT_MUSICGEN_VERSION: &str =
  "b05b1dff1d8c6dc63d14b0cdb42135571006571f18c3aeab620b81f740aab4dd";

/// Sound generation provider backed by the [Replicate](https://replicate.com) API.
///
/// Supports MusicGen and similar models. The provider creates a prediction,
/// polls until it completes, then downloads and decodes the output audio.
#[derive(Debug, Clone)]
pub struct ReplicateSoundGen {
  client: reqwest::Client,
  api_token: ApiKey,
  /// Replicate model version hash.
  pub model_version: String,
  /// How often to poll for completion.
  pub poll_interval: Duration,
  /// Maximum total wait time before giving up.
  pub max_wait: Duration,
}

/// Replicate prediction request payload.
#[derive(Serialize)]
struct ReplicateCreateRequest<'a> {
  version: &'a str,
  input: ReplicateInput<'a>,
}

#[derive(Serialize)]
struct ReplicateInput<'a> {
  prompt: &'a str,
  #[serde(skip_serializing_if = "Option::is_none")]
  duration: Option<f64>,
}

/// Replicate prediction response (subset of fields we need).
#[derive(Deserialize)]
struct ReplicatePrediction {
  id: String,
  status: String,
  #[serde(default)]
  output: Option<serde_json::Value>,
  #[serde(default)]
  error: Option<String>,
  urls: Option<ReplicateUrls>,
}

#[derive(Deserialize)]
struct ReplicateUrls {
  get: Option<String>,
}

impl ReplicateSoundGen {
  /// Creates a new `ReplicateSoundGen` reading the API token from
  /// the `REPLICATE_API_TOKEN` environment variable.
  pub fn new() -> Result<Self, AiError> {
    let api_token = std::env::var("REPLICATE_API_TOKEN").map_err(|_| {
      AiError::Config("REPLICATE_API_TOKEN environment variable not set".to_string())
    })?;
    Self::with_token(api_token)
  }

  /// Creates a new `ReplicateSoundGen` with an explicit API token.
  pub fn with_token(api_token: impl Into<String>) -> Result<Self, AiError> {
    let client = reqwest::Client::builder()
      .timeout(Duration::from_secs(30))
      .build()
      .map_err(|e| AiError::Http(e.to_string()))?;

    Ok(Self {
      client,
      api_token: ApiKey::new(api_token),
      model_version: DEFAULT_MUSICGEN_VERSION.to_string(),
      poll_interval: Duration::from_secs(2),
      max_wait: Duration::from_secs(300),
    })
  }

  /// Set a custom model version hash.
  pub fn with_model_version(mut self, version: impl Into<String>) -> Self {
    self.model_version = version.into();
    self
  }

  /// Set the poll interval.
  pub fn with_poll_interval(mut self, interval: Duration) -> Self {
    self.poll_interval = interval;
    self
  }

  /// Set the maximum wait time.
  pub fn with_max_wait(mut self, max_wait: Duration) -> Self {
    self.max_wait = max_wait;
    self
  }

  /// Helper: create prediction and poll until complete.
  async fn run_prediction(&self, prompt: &SoundPrompt) -> Result<String, AiError> {
    let body = ReplicateCreateRequest {
      version: &self.model_version,
      input: ReplicateInput {
        prompt: &prompt.prompt,
        duration: prompt.duration_secs,
      },
    };

    let resp = self
      .client
      .post(REPLICATE_API_URL)
      .header(
        "Authorization",
        format!("Bearer {}", self.api_token.expose()),
      )
      .header("Content-Type", "application/json")
      .json(&body)
      .send()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let text = resp.text().await.unwrap_or_else(|_| "unknown".to_string());
      return Err(AiError::Api {
        status: status.as_u16(),
        message: text,
      });
    }

    let pred: ReplicatePrediction = resp
      .json()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    let poll_url = pred
      .urls
      .and_then(|u| u.get)
      .unwrap_or_else(|| format!("{}/{}", REPLICATE_API_URL, pred.id));

    // Poll loop
    let start = std::time::Instant::now();
    loop {
      if start.elapsed() > self.max_wait {
        return Err(AiError::Http("Replicate prediction timed out".to_string()));
      }
      tokio::time::sleep(self.poll_interval).await;

      let poll_resp = self
        .client
        .get(&poll_url)
        .header(
          "Authorization",
          format!("Bearer {}", self.api_token.expose()),
        )
        .send()
        .await
        .map_err(|e| AiError::Http(e.to_string()))?;

      let status_pred: ReplicatePrediction = poll_resp
        .json()
        .await
        .map_err(|e| AiError::Http(e.to_string()))?;

      match status_pred.status.as_str() {
        "succeeded" => {
          // output is typically a URL string or array of URLs.
          let output_url = match status_pred.output {
            Some(serde_json::Value::String(url)) => url,
            Some(serde_json::Value::Array(arr)) => arr
              .first()
              .and_then(|v| v.as_str())
              .map(String::from)
              .ok_or_else(|| AiError::Decode("Empty output array from Replicate".to_string()))?,
            other => {
              return Err(AiError::Decode(format!(
                "Unexpected Replicate output format: {other:?}"
              )));
            }
          };
          return Ok(output_url);
        }
        "failed" | "canceled" => {
          let msg = status_pred
            .error
            .unwrap_or_else(|| "Prediction failed".to_string());
          return Err(AiError::Api {
            status: 500,
            message: msg,
          });
        }
        _ => {
          // "starting" | "processing" — keep polling
          continue;
        }
      }
    }
  }
}

impl SoundGenProvider for ReplicateSoundGen {
  #[instrument(skip(self, prompt), fields(prompt_len = prompt.prompt.len()))]
  async fn generate(&self, prompt: &SoundPrompt) -> Result<AudioBuffer, AiError> {
    info!(prompt = %prompt.prompt, "Generating sound via Replicate");

    let output_url = self.run_prediction(prompt).await?;
    info!(url = %output_url, "Prediction complete, downloading audio");

    // Download the output audio file.
    let audio_resp = self
      .client
      .get(&output_url)
      .send()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    let audio_bytes = audio_resp
      .bytes()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    info!(bytes = audio_bytes.len(), "Downloaded audio from Replicate");

    // Detect format from URL or default to wav.
    let suffix = output_url
      .rsplit('.')
      .next()
      .and_then(|ext| {
        let e = ext.split('?').next().unwrap_or(ext).to_lowercase();
        if ["wav", "mp3", "flac", "ogg"].contains(&e.as_str()) {
          Some(e)
        } else {
          None
        }
      })
      .unwrap_or_else(|| "wav".to_string());

    let buffer = HttpSoundGen::decode_audio_bytes(&audio_bytes, &suffix)?;

    info!(
      duration = buffer.duration_secs(),
      samples = buffer.samples.len(),
      "Replicate sound generation complete"
    );

    Ok(buffer)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_debug_redacts_tokens() {
    let http = HttpSoundGen::new("https://example.invalid", "sk-very-secret").unwrap();
    let replicate = ReplicateSoundGen::with_token("r8-very-secret").unwrap();
    for debug in [format!("{http:?}"), format!("{replicate:?}")] {
      assert!(!debug.contains("very-secret"), "{debug}");
    }
  }

  #[test]
  fn test_sound_prompt_new() {
    let prompt = SoundPrompt::new("ambient music");
    assert_eq!(prompt.prompt, "ambient music");
    assert!(prompt.duration_secs.is_none());
    assert!(prompt.style.is_none());
    assert!(prompt.negative_prompt.is_none());
  }

  #[test]
  fn test_sound_prompt_builder() {
    let prompt = SoundPrompt::new("epic orchestral")
      .with_duration(60.0)
      .with_style("cinematic")
      .with_negative_prompt("vocals");
    assert_eq!(prompt.prompt, "epic orchestral");
    assert!((prompt.duration_secs.unwrap_or(0.0) - 60.0).abs() < f64::EPSILON);
    assert_eq!(prompt.style.as_deref(), Some("cinematic"));
    assert_eq!(prompt.negative_prompt.as_deref(), Some("vocals"));
  }

  #[test]
  fn test_sound_prompt_serialization() {
    let prompt = SoundPrompt::new("test")
      .with_duration(10.0)
      .with_style("ambient");
    let json = serde_json::to_value(&prompt).expect("serialize");
    assert_eq!(json["prompt"], "test");
    assert_eq!(json["duration_secs"], 10.0);
    assert_eq!(json["style"], "ambient");
    // negative_prompt should be absent (None → skip)
    assert!(json.get("negative_prompt").is_none());
  }

  #[test]
  fn test_sound_prompt_deserialization() {
    let json = r#"{"prompt": "piano melody", "duration_secs": 30.0}"#;
    let prompt: SoundPrompt = serde_json::from_str(json).expect("deserialize");
    assert_eq!(prompt.prompt, "piano melody");
    assert!((prompt.duration_secs.unwrap_or(0.0) - 30.0).abs() < f64::EPSILON);
    assert!(prompt.style.is_none());
  }

  #[test]
  fn test_sound_preset_bgm() {
    let prompt = SoundPrompt::from_preset(SoundPreset::Bgm, "calm lo-fi");
    assert_eq!(prompt.prompt, "calm lo-fi");
    assert!((prompt.duration_secs.unwrap_or(0.0) - 30.0).abs() < f64::EPSILON);
    assert_eq!(prompt.style.as_deref(), Some("background music"));
    assert!(prompt.negative_prompt.is_some());
  }

  #[test]
  fn test_sound_preset_sfx() {
    let prompt = SoundPrompt::from_preset(SoundPreset::Sfx, "explosion");
    assert!((prompt.duration_secs.unwrap_or(0.0) - 3.0).abs() < f64::EPSILON);
    assert_eq!(prompt.style.as_deref(), Some("sound effect"));
  }

  #[test]
  fn test_sound_preset_jingle() {
    let prompt = SoundPrompt::from_preset(SoundPreset::Jingle, "happy ukulele");
    assert!((prompt.duration_secs.unwrap_or(0.0) - 10.0).abs() < f64::EPSILON);
  }

  #[test]
  fn test_sound_preset_ambient() {
    let prompt = SoundPrompt::from_preset(SoundPreset::Ambient, "rain forest");
    assert!((prompt.duration_secs.unwrap_or(0.0) - 60.0).abs() < f64::EPSILON);
    assert_eq!(prompt.style.as_deref(), Some("ambient soundscape"));
  }

  #[test]
  fn test_sound_preset_custom() {
    let prompt = SoundPrompt::from_preset(SoundPreset::Custom, "anything");
    assert_eq!(prompt.prompt, "anything");
    assert!(prompt.duration_secs.is_none());
    assert!(prompt.style.is_none());
    assert!(prompt.negative_prompt.is_none());
  }

  #[test]
  fn test_sound_preset_serialization() {
    let json = serde_json::to_value(SoundPreset::Bgm).expect("serialize");
    assert_eq!(json, "Bgm");
  }

  #[test]
  fn test_http_sound_gen_creation() {
    let provider = HttpSoundGen::new("https://api.example.com/generate", "test-key");
    assert!(provider.is_ok());
    let provider = provider.unwrap();
    assert_eq!(provider.endpoint, "https://api.example.com/generate");
    assert_eq!(provider.response_format, "wav");
  }

  #[test]
  fn test_http_sound_gen_response_format() {
    let provider = HttpSoundGen::new("https://api.example.com/generate", "key")
      .unwrap()
      .with_response_format("mp3");
    assert_eq!(provider.response_format, "mp3");
  }

  #[test]
  fn test_replicate_sound_gen_creation() {
    let provider = ReplicateSoundGen::with_token("test-token");
    assert!(provider.is_ok());
    let provider = provider.unwrap();
    assert_eq!(provider.model_version, DEFAULT_MUSICGEN_VERSION);
    assert_eq!(provider.poll_interval, Duration::from_secs(2));
    assert_eq!(provider.max_wait, Duration::from_secs(300));
  }

  #[test]
  fn test_replicate_sound_gen_builder() {
    let provider = ReplicateSoundGen::with_token("tok")
      .unwrap()
      .with_model_version("abc123")
      .with_poll_interval(Duration::from_secs(5))
      .with_max_wait(Duration::from_secs(60));
    assert_eq!(provider.model_version, "abc123");
    assert_eq!(provider.poll_interval, Duration::from_secs(5));
    assert_eq!(provider.max_wait, Duration::from_secs(60));
  }

  #[test]
  fn test_replicate_request_serialization() {
    let body = ReplicateCreateRequest {
      version: "v1",
      input: ReplicateInput {
        prompt: "calm piano",
        duration: Some(30.0),
      },
    };
    let json = serde_json::to_value(&body).expect("serialize");
    assert_eq!(json["version"], "v1");
    assert_eq!(json["input"]["prompt"], "calm piano");
    assert_eq!(json["input"]["duration"], 30.0);
  }

  #[test]
  fn test_replicate_request_no_duration() {
    let body = ReplicateCreateRequest {
      version: "v1",
      input: ReplicateInput {
        prompt: "test",
        duration: None,
      },
    };
    let json = serde_json::to_value(&body).expect("serialize");
    assert!(json["input"].get("duration").is_none());
  }

  #[test]
  fn test_replicate_env_missing() {
    // SAFETY: test runs single-threaded.
    unsafe { std::env::remove_var("REPLICATE_API_TOKEN") };
    let result = ReplicateSoundGen::new();
    assert!(result.is_err());
  }
}
