//! Provider configuration: pick TTS / STT / LLM providers from a TOML file,
//! built-in vendor presets, and the API keys present in the environment.
//!
//! See `docs/providers.md` for the file format and the preset table.
//!
//! ```rust,no_run
//! use phonon_ai::{Config, TtsProvider, VoiceConfig};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let config = Config::load(None)?;
//! let tts = config.tts()?;
//! let audio = tts.synthesize("你好", &VoiceConfig::new(tts.default_voice())).await?;
//! # Ok(())
//! # }
//! ```

use crate::stt::{SegmentMode, SttProvider, TranscribeOptions, TranscriptResult};
use crate::tts::{TtsProvider, split_text};
use crate::voice::VoiceConfig;
use crate::{
  AiError, EdgeTts, ElevenLabsTts, HttpClient, MiniMaxTts, OpenAiTts, QwenStt, QwenTts, WhisperStt,
};
use phonon_core::AudioBuffer;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Wire protocol of a provider. Vendors are data over these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
  /// OpenAI-compatible `/audio/speech`, `/audio/transcriptions`, `/chat/completions`.
  OpenAi,
  /// MiniMax T2A v2 (TTS only).
  MiniMax,
  /// Alibaba DashScope native (TTS and STT).
  Qwen,
  /// Microsoft Edge online TTS (no key).
  Edge,
  /// ElevenLabs TTS.
  ElevenLabs,
}

/// Parsed configuration file. Every section is optional.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
  #[serde(default)]
  tts: TtsSection,
  #[serde(default)]
  stt: SttSection,
  #[serde(default)]
  llm: LlmSection,
  /// User-defined providers, e.g. self-hosted OpenAI-compatible servers.
  #[serde(default)]
  providers: HashMap<String, CustomProvider>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct TtsSection {
  provider: Option<String>,
  model: Option<String>,
  voice: Option<String>,
  format: Option<String>,
  max_chars: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct SttSection {
  provider: Option<String>,
  model: Option<String>,
  segments: Option<SegmentsSetting>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SegmentsSetting {
  VerboseJson,
  Always,
  None,
}

impl From<SegmentsSetting> for SegmentMode {
  fn from(setting: SegmentsSetting) -> Self {
    match setting {
      SegmentsSetting::VerboseJson => SegmentMode::VerboseJson,
      SegmentsSetting::Always => SegmentMode::Always,
      SegmentsSetting::None => SegmentMode::Unsupported,
    }
  }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LlmSection {
  provider: Option<String>,
  model: Option<String>,
  extra: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CustomProvider {
  protocol: Protocol,
  base_url: String,
  /// Environment variable holding the key; without it no key is sent.
  api_key_env: Option<String>,
}

// ─── Presets ────────────────────────────────────────────────────────

struct Preset {
  name: &'static str,
  key_env: Option<&'static str>,
  tts: Option<TtsPreset>,
  stt: Option<SttPreset>,
  llm: Option<LlmPreset>,
}

struct TtsPreset {
  protocol: Protocol,
  base_url: &'static str,
  model: &'static str,
  voice: &'static str,
  format: &'static str,
  /// Per-request text limit in characters.
  max_chars: Option<usize>,
}

struct SttPreset {
  protocol: Protocol,
  base_url: &'static str,
  model: &'static str,
  segments: SegmentMode,
  path: &'static str,
}

struct LlmPreset {
  base_url: &'static str,
  model: &'static str,
  /// JSON object merged into chat requests (switches reasoning off).
  extra: &'static str,
}

const MINIMAX: &str = "https://api.minimaxi.com/v1";
const DASHSCOPE: &str = crate::qwen::DASHSCOPE_BASE_URL;
const DASHSCOPE_COMPAT: &str = "https://dashscope.aliyuncs.com/compatible-mode/v1";
const ZHIPU: &str = "https://open.bigmodel.cn/api/paas/v4";
const STEPFUN: &str = "https://api.stepfun.com/v1";
const OPENAI: &str = "https://api.openai.com/v1";
const TRANSCRIPTIONS: &str = "audio/transcriptions";

/// Built-in vendors; models verified against the live APIs on 2026-10-20.
const PRESETS: &[Preset] = &[
  Preset {
    name: "minimax",
    key_env: Some("MINIMAX_API_KEY"),
    tts: Some(TtsPreset {
      protocol: Protocol::MiniMax,
      base_url: MINIMAX,
      model: "speech-2.8-hd",
      voice: "male-qn-qingse",
      format: "mp3",
      max_chars: Some(5000),
    }),
    stt: Some(SttPreset {
      protocol: Protocol::OpenAi,
      base_url: MINIMAX,
      model: "asr-1.0",
      segments: SegmentMode::VerboseJson,
      path: "speech_to_text",
    }),
    llm: Some(LlmPreset {
      base_url: MINIMAX,
      model: "MiniMax-M3.1-Flash",
      extra: "{}",
    }),
  },
  Preset {
    name: "qwen",
    key_env: Some("DASHSCOPE_API_KEY"),
    tts: Some(TtsPreset {
      protocol: Protocol::Qwen,
      base_url: DASHSCOPE,
      model: "qwen3-tts-flash",
      voice: "Cherry",
      format: "wav",
      // The API accepts 600, but near that length the same text comes back
      // 1.2x to 5x too long; 200 measured stable (docs/providers.md).
      max_chars: Some(200),
    }),
    stt: Some(SttPreset {
      protocol: Protocol::Qwen,
      base_url: DASHSCOPE,
      model: "qwen-audio-3.0-asr-flash",
      segments: SegmentMode::Always,
      path: "",
    }),
    llm: Some(LlmPreset {
      base_url: DASHSCOPE_COMPAT,
      model: "qwen3.8-flash",
      extra: r#"{"enable_thinking": false}"#,
    }),
  },
  Preset {
    name: "glm",
    key_env: Some("ZHIPU_API_KEY"),
    tts: Some(TtsPreset {
      protocol: Protocol::OpenAi,
      base_url: ZHIPU,
      model: "glm-tts",
      voice: "tongtong",
      format: "wav",
      max_chars: Some(1024),
    }),
    stt: Some(SttPreset {
      protocol: Protocol::OpenAi,
      base_url: ZHIPU,
      // `glm-asr-2512` returns text only; `glm-asr` adds segments.
      model: "glm-asr",
      segments: SegmentMode::Always,
      path: TRANSCRIPTIONS,
    }),
    llm: Some(LlmPreset {
      base_url: ZHIPU,
      model: "glm-4.7",
      extra: r#"{"thinking": {"type": "disabled"}}"#,
    }),
  },
  Preset {
    name: "stepfun",
    key_env: Some("STEP_API_KEY"),
    tts: Some(TtsPreset {
      protocol: Protocol::OpenAi,
      base_url: STEPFUN,
      model: "step-tts-2",
      voice: "cixingnansheng",
      format: "wav",
      max_chars: Some(1000),
    }),
    stt: Some(SttPreset {
      protocol: Protocol::OpenAi,
      base_url: STEPFUN,
      model: "stepaudio-2.5-asr",
      segments: SegmentMode::Unsupported,
      path: TRANSCRIPTIONS,
    }),
    llm: Some(LlmPreset {
      base_url: STEPFUN,
      model: "step-3.7-flash",
      extra: "{}",
    }),
  },
  Preset {
    name: "deepseek",
    key_env: Some("DEEPSEEK_API_KEY"),
    tts: None,
    stt: None,
    llm: Some(LlmPreset {
      base_url: "https://api.deepseek.com",
      model: "deepseek-flash",
      extra: r#"{"thinking": {"type": "disabled"}}"#,
    }),
  },
  Preset {
    name: "openai",
    key_env: Some("OPENAI_API_KEY"),
    tts: Some(TtsPreset {
      protocol: Protocol::OpenAi,
      base_url: OPENAI,
      model: "tts-1",
      voice: "alloy",
      format: "mp3",
      max_chars: Some(4096),
    }),
    stt: Some(SttPreset {
      protocol: Protocol::OpenAi,
      base_url: OPENAI,
      model: "whisper-1",
      segments: SegmentMode::VerboseJson,
      path: TRANSCRIPTIONS,
    }),
    llm: Some(LlmPreset {
      base_url: OPENAI,
      model: "gpt-4o-mini",
      extra: "{}",
    }),
  },
  Preset {
    name: "elevenlabs",
    key_env: Some("ELEVENLABS_API_KEY"),
    tts: Some(TtsPreset {
      protocol: Protocol::ElevenLabs,
      base_url: "https://api.elevenlabs.io/v1",
      model: "eleven_multilingual_v2",
      voice: "21m00Tcm4TlvDq8ikWAM",
      format: "mp3",
      max_chars: Some(5000),
    }),
    stt: None,
    llm: None,
  },
  Preset {
    name: "edge",
    key_env: None,
    tts: Some(TtsPreset {
      protocol: Protocol::Edge,
      base_url: "",
      model: "",
      voice: "zh-CN-XiaoxiaoNeural",
      format: "mp3",
      max_chars: None,
    }),
    stt: None,
    llm: None,
  },
];

/// Auto-selection order when a section names no provider: the first one
/// whose key is set wins. Edge (keyless) is the TTS fallback.
const TTS_ORDER: &[&str] = &[
  "minimax",
  "qwen",
  "glm",
  "stepfun",
  "openai",
  "elevenlabs",
  "edge",
];
const STT_ORDER: &[&str] = &["minimax", "qwen", "glm", "stepfun", "openai"];
const LLM_ORDER: &[&str] = &["minimax", "qwen", "glm", "stepfun", "deepseek", "openai"];

fn preset(name: &str) -> Option<&'static Preset> {
  PRESETS.iter().find(|p| p.name == name)
}

/// Names of the built-in presets, in table order.
pub fn preset_names() -> impl Iterator<Item = &'static str> {
  PRESETS.iter().map(|p| p.name)
}

// ─── Resolution ─────────────────────────────────────────────────────

/// Where a capability's provider comes from, before building clients.
#[derive(Debug, Clone, PartialEq)]
struct Endpoint {
  provider: String,
  protocol: Protocol,
  base_url: String,
  api_key: String,
}

impl Config {
  /// Parse a TOML document.
  pub fn from_toml(text: &str) -> Result<Self, AiError> {
    toml::from_str(text).map_err(|e| AiError::Config(format!("invalid provider config: {e}")))
  }

  /// Load the configuration: `explicit` path, else `$PHONON_CONFIG`, else
  /// `./phonon.toml`, else `~/.config/phonon/config.toml`, else defaults.
  /// A path given explicitly or via `PHONON_CONFIG` must exist.
  pub fn load(explicit: Option<&Path>) -> Result<Self, AiError> {
    let required = explicit
      .map(Path::to_path_buf)
      .or_else(|| std::env::var_os("PHONON_CONFIG").map(PathBuf::from));
    if let Some(path) = required {
      return Self::read(&path);
    }
    let candidates = [
      Some(PathBuf::from("phonon.toml")),
      dirs::home_dir().map(|h| h.join(".config/phonon/config.toml")),
    ];
    match candidates.into_iter().flatten().find(|p| p.is_file()) {
      Some(path) => Self::read(&path),
      None => Ok(Self::default()),
    }
  }

  fn read(path: &Path) -> Result<Self, AiError> {
    let text = std::fs::read_to_string(path)
      .map_err(|e| AiError::Config(format!("cannot read {}: {e}", path.display())))?;
    Self::from_toml(&text)
  }

  /// Build the configured TTS provider.
  pub fn tts(&self) -> Result<AnyTts, AiError> {
    self.tts_with(&env_var)
  }

  /// Build the configured STT provider.
  pub fn stt(&self) -> Result<AnyStt, AiError> {
    self.stt_with(&env_var)
  }

  /// Resolve the configured chat endpoint (for podcast script generation).
  pub fn llm(&self) -> Result<ChatEndpoint, AiError> {
    self.llm_with(&env_var)
  }

  /// Pick the provider name for a capability: the section's choice, else
  /// the first auto-selection entry whose key is present.
  fn choose(
    &self,
    section: Option<&str>,
    order: &[&str],
    capability: &str,
    env: &dyn Fn(&str) -> Option<String>,
  ) -> Result<String, AiError> {
    if let Some(name) = section {
      return Ok(name.to_string());
    }
    order
      .iter()
      .find(|name| match preset(name).and_then(|p| p.key_env) {
        Some(key) => env(key).is_some(),
        None => true,
      })
      .map(|name| name.to_string())
      .ok_or_else(|| {
        let keys: Vec<_> = order
          .iter()
          .filter_map(|n| preset(n).and_then(|p| p.key_env))
          .collect();
        AiError::Config(format!(
          "no {capability} provider configured and none of {} is set",
          keys.join(", ")
        ))
      })
  }

  /// Resolve `name` to an endpoint. Custom providers shadow presets.
  fn endpoint(
    &self,
    name: &str,
    preset_part: impl Fn(&Preset) -> Option<(Protocol, &'static str)>,
    capability: &str,
    env: &dyn Fn(&str) -> Option<String>,
  ) -> Result<Endpoint, AiError> {
    if let Some(custom) = self.providers.get(name) {
      let api_key = match &custom.api_key_env {
        Some(var) => env(var).ok_or_else(|| {
          AiError::Config(format!(
            "provider '{name}' needs the {var} environment variable"
          ))
        })?,
        None => String::new(),
      };
      return Ok(Endpoint {
        provider: name.to_string(),
        protocol: custom.protocol,
        base_url: custom.base_url.trim_end_matches('/').to_string(),
        api_key,
      });
    }
    let preset = preset(name).ok_or_else(|| {
      AiError::Config(format!(
        "unknown provider '{name}' (presets: {}; or define [providers.{name}])",
        preset_names().collect::<Vec<_>>().join(", ")
      ))
    })?;
    let (protocol, base_url) = preset_part(preset)
      .ok_or_else(|| AiError::Config(format!("provider '{name}' has no {capability} support")))?;
    let api_key = match preset.key_env {
      Some(var) => env(var).ok_or_else(|| {
        AiError::Config(format!(
          "provider '{name}' needs the {var} environment variable"
        ))
      })?,
      None => String::new(),
    };
    Ok(Endpoint {
      provider: name.to_string(),
      protocol,
      base_url: base_url.to_string(),
      api_key,
    })
  }

  fn tts_with(&self, env: &dyn Fn(&str) -> Option<String>) -> Result<AnyTts, AiError> {
    let section = &self.tts;
    let name = self.choose(section.provider.as_deref(), TTS_ORDER, "TTS", env)?;
    let ep = self.endpoint(
      &name,
      |p| p.tts.as_ref().map(|t| (t.protocol, t.base_url)),
      "TTS",
      env,
    )?;
    let defaults = self.preset_for(&name).and_then(|p| p.tts.as_ref());
    let model = pick(
      &section.model,
      defaults.map(|d| d.model),
      "tts",
      &name,
      ep.protocol,
    )?;
    let voice = section
      .voice
      .clone()
      .or_else(|| defaults.map(|d| d.voice.to_string()))
      .unwrap_or_default();
    let format = section
      .format
      .clone()
      .or_else(|| defaults.map(|d| d.format.to_string()))
      .unwrap_or_else(|| "wav".to_string());

    let client = || HttpClient::new(ep.api_key.clone(), ep.base_url.clone(), None);
    let backend = match ep.protocol {
      Protocol::OpenAi => TtsBackend::OpenAi(OpenAiTts::with_client(client()?).with_format(format)),
      Protocol::MiniMax => TtsBackend::MiniMax(
        MiniMaxTts::with_api_key(ep.api_key.clone())?
          .with_base_url(ep.base_url.clone())?
          .with_model(model.clone()),
      ),
      Protocol::Qwen => TtsBackend::Qwen(QwenTts::with_client(client()?).with_model(model.clone())),
      Protocol::Edge => TtsBackend::Edge(EdgeTts::new()),
      Protocol::ElevenLabs => {
        TtsBackend::ElevenLabs(ElevenLabsTts::with_client(client()?, model.clone()))
      }
    };
    Ok(AnyTts {
      backend,
      provider: ep.provider,
      model,
      voice,
      max_chars: section.max_chars.or(defaults.and_then(|d| d.max_chars)),
    })
  }

  fn stt_with(&self, env: &dyn Fn(&str) -> Option<String>) -> Result<AnyStt, AiError> {
    let section = &self.stt;
    let name = self.choose(section.provider.as_deref(), STT_ORDER, "STT", env)?;
    let ep = self.endpoint(
      &name,
      |p| p.stt.as_ref().map(|s| (s.protocol, s.base_url)),
      "STT",
      env,
    )?;
    let defaults = self.preset_for(&name).and_then(|p| p.stt.as_ref());
    let model = pick(
      &section.model,
      defaults.map(|d| d.model),
      "stt",
      &name,
      ep.protocol,
    )?;
    let client = HttpClient::new(ep.api_key, ep.base_url, None)?;
    let backend = match ep.protocol {
      Protocol::OpenAi => {
        let segments = section
          .segments
          .map(SegmentMode::from)
          .or(defaults.map(|d| d.segments))
          .unwrap_or(SegmentMode::VerboseJson);
        let path = defaults.map(|d| d.path).unwrap_or(TRANSCRIPTIONS);
        SttBackend::OpenAi(
          WhisperStt::with_client(client, model.clone())
            .with_segment_mode(segments)
            .with_path(path),
        )
      }
      Protocol::Qwen => SttBackend::Qwen(QwenStt::with_client(client).with_model(model.clone())),
      other => {
        return Err(AiError::Config(format!(
          "provider '{name}': protocol {other:?} has no STT support"
        )));
      }
    };
    Ok(AnyStt {
      backend,
      provider: ep.provider,
      model,
    })
  }

  fn llm_with(&self, env: &dyn Fn(&str) -> Option<String>) -> Result<ChatEndpoint, AiError> {
    let section = &self.llm;
    let name = self.choose(section.provider.as_deref(), LLM_ORDER, "LLM", env)?;
    let ep = self.endpoint(
      &name,
      |p| p.llm.as_ref().map(|l| (Protocol::OpenAi, l.base_url)),
      "LLM",
      env,
    )?;
    if ep.protocol != Protocol::OpenAi {
      return Err(AiError::Config(format!(
        "provider '{name}': chat needs the openai protocol"
      )));
    }
    let defaults = self.preset_for(&name).and_then(|p| p.llm.as_ref());
    let model = pick(
      &section.model,
      defaults.map(|d| d.model),
      "llm",
      &name,
      ep.protocol,
    )?;
    let extra = match (&section.extra, defaults) {
      (Some(extra), _) => extra.clone(),
      (None, Some(d)) => serde_json::from_str(d.extra)
        .map_err(|e| AiError::Config(format!("preset '{name}' extra: {e}")))?,
      (None, None) => serde_json::Map::new(),
    };
    Ok(ChatEndpoint {
      client: HttpClient::new(ep.api_key, ep.base_url, Some(120))?,
      provider: ep.provider,
      model,
      extra,
    })
  }
}

impl Config {
  /// The preset backing `name`, unless a custom provider shadows it.
  fn preset_for(&self, name: &str) -> Option<&'static Preset> {
    match self.providers.contains_key(name) {
      true => None,
      false => preset(name),
    }
  }
}

fn env_var(name: &str) -> Option<String> {
  std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// The section's model, else the preset's; custom providers must name one
/// (Edge has no model).
fn pick(
  section: &Option<String>,
  preset: Option<&'static str>,
  capability: &str,
  provider: &str,
  protocol: Protocol,
) -> Result<String, AiError> {
  match (section, preset) {
    (Some(model), _) => Ok(model.clone()),
    (None, Some(model)) => Ok(model.to_string()),
    (None, None) if protocol == Protocol::Edge => Ok(String::new()),
    (None, None) => Err(AiError::Config(format!(
      "[{capability}] model is required for custom provider '{provider}'"
    ))),
  }
}

// ─── Runtime dispatch ───────────────────────────────────────────────

#[derive(Debug, Clone)]
enum TtsBackend {
  OpenAi(OpenAiTts),
  MiniMax(MiniMaxTts),
  Qwen(QwenTts),
  Edge(EdgeTts),
  ElevenLabs(ElevenLabsTts),
}

/// A TTS provider chosen at runtime from [`Config`].
#[derive(Debug, Clone)]
pub struct AnyTts {
  backend: TtsBackend,
  provider: String,
  model: String,
  voice: String,
  max_chars: Option<usize>,
}

impl AnyTts {
  /// Provider name (preset or `[providers.*]` key).
  pub fn provider(&self) -> &str {
    &self.provider
  }

  /// Model sent with every request (empty for Edge).
  pub fn model(&self) -> &str {
    &self.model
  }

  /// Voice to use when the caller has none (empty for custom providers
  /// without `voice`).
  pub fn default_voice(&self) -> &str {
    &self.voice
  }

  /// Per-request text limit; longer text is synthesized in chunks.
  pub fn max_chars(&self) -> Option<usize> {
    self.max_chars
  }

  /// `text` as one request, or as sentence chunks within `max_chars`.
  /// Chunks with nothing to speak (punctuation only) are skipped.
  fn chunks(&self, text: &str) -> Vec<String> {
    match self.max_chars {
      Some(max) if text.chars().count() > max => split_text(text, max)
        .into_iter()
        .filter(|c| c.chars().any(char::is_alphanumeric))
        .collect(),
      _ => vec![text.to_string()],
    }
  }

  async fn synthesize_one(&self, text: &str, voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
    match &self.backend {
      TtsBackend::OpenAi(p) => p.synthesize(text, &self.voice_for_request(voice)).await,
      TtsBackend::MiniMax(p) => p.synthesize(text, voice).await,
      TtsBackend::Qwen(p) => p.synthesize(text, voice).await,
      TtsBackend::Edge(p) => p.synthesize(text, voice).await,
      TtsBackend::ElevenLabs(p) => p.synthesize(text, voice).await,
    }
  }

  /// OpenAI-protocol servers read the model from the voice; the configured
  /// model wins over `VoiceConfig`'s OpenAI default.
  fn voice_for_request(&self, voice: &VoiceConfig) -> VoiceConfig {
    VoiceConfig {
      model: self.model.clone(),
      ..voice.clone()
    }
  }
}

impl TtsProvider for AnyTts {
  async fn synthesize(&self, text: &str, voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
    let mut chunks = self.chunks(text).into_iter();
    let first = chunks.next().unwrap_or_default();
    let mut audio = self.synthesize_one(&first, voice).await?;
    for chunk in chunks {
      let next = self.synthesize_one(&chunk, voice).await?;
      if (next.sample_rate, next.channels) != (audio.sample_rate, audio.channels) {
        return Err(AiError::Decode(format!(
          "chunk format changed mid-text: {} Hz / {} ch after {} Hz / {} ch",
          next.sample_rate, next.channels, audio.sample_rate, audio.channels
        )));
      }
      audio.samples.extend_from_slice(&next.samples);
    }
    Ok(audio)
  }

  /// Chunked text streams one buffer per chunk; otherwise the backend's
  /// own streaming is used.
  async fn synthesize_streaming(
    &self,
    text: &str,
    voice: &VoiceConfig,
    tx: tokio::sync::mpsc::Sender<Result<AudioBuffer, AiError>>,
  ) -> Result<(), AiError> {
    let chunks = self.chunks(text);
    if chunks.len() > 1 {
      for chunk in chunks {
        let audio = self.synthesize_one(&chunk, voice).await;
        let failed = audio.is_err();
        if tx.send(audio).await.is_err() || failed {
          break;
        }
      }
      return Ok(());
    }
    match &self.backend {
      TtsBackend::OpenAi(p) => {
        p.synthesize_streaming(text, &self.voice_for_request(voice), tx)
          .await
      }
      TtsBackend::MiniMax(p) => p.synthesize_streaming(text, voice, tx).await,
      TtsBackend::Qwen(p) => p.synthesize_streaming(text, voice, tx).await,
      TtsBackend::Edge(p) => p.synthesize_streaming(text, voice, tx).await,
      TtsBackend::ElevenLabs(p) => p.synthesize_streaming(text, voice, tx).await,
    }
  }
}

#[derive(Debug, Clone)]
enum SttBackend {
  OpenAi(WhisperStt),
  Qwen(QwenStt),
}

/// An STT provider chosen at runtime from [`Config`].
#[derive(Debug, Clone)]
pub struct AnyStt {
  backend: SttBackend,
  provider: String,
  model: String,
}

impl AnyStt {
  /// Provider name (preset or `[providers.*]` key).
  pub fn provider(&self) -> &str {
    &self.provider
  }

  /// Model sent with every request.
  pub fn model(&self) -> &str {
    &self.model
  }
}

impl SttProvider for AnyStt {
  async fn transcribe_file(
    &self,
    path: &str,
    options: Option<TranscribeOptions>,
  ) -> Result<TranscriptResult, AiError> {
    match &self.backend {
      SttBackend::OpenAi(p) => p.transcribe_file(path, options).await,
      SttBackend::Qwen(p) => p.transcribe_file(path, options).await,
    }
  }

  async fn transcribe_bytes(
    &self,
    data: Vec<u8>,
    filename: &str,
    options: Option<TranscribeOptions>,
  ) -> Result<TranscriptResult, AiError> {
    match &self.backend {
      SttBackend::OpenAi(p) => p.transcribe_bytes(data, filename, options).await,
      SttBackend::Qwen(p) => p.transcribe_bytes(data, filename, options).await,
    }
  }
}

/// An OpenAI-compatible chat endpoint chosen from [`Config`].
#[derive(Debug, Clone)]
pub struct ChatEndpoint {
  /// Client carrying the base URL and key.
  pub client: HttpClient,
  /// Provider name (preset or `[providers.*]` key).
  pub provider: String,
  /// Chat model.
  pub model: String,
  /// Fields merged into each request body (e.g. disabling reasoning).
  pub extra: serde_json::Map<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
  use super::*;

  fn env_of(vars: &'static [&'static str]) -> impl Fn(&str) -> Option<String> {
    move |name| vars.contains(&name).then(|| format!("key-{name}"))
  }

  #[test]
  fn test_auto_selection_follows_priority() {
    let config = Config::default();
    let env = env_of(&["STEP_API_KEY", "ZHIPU_API_KEY", "DEEPSEEK_API_KEY"]);
    let tts = config.tts_with(&env).expect("tts");
    assert_eq!((tts.provider(), tts.model()), ("glm", "glm-tts"));
    assert_eq!(tts.max_chars(), Some(1024));
    assert_eq!(config.stt_with(&env).expect("stt").provider(), "glm");
    assert_eq!(config.llm_with(&env).expect("llm").provider, "glm");

    // Only DeepSeek: chat works, TTS falls back to keyless Edge, STT fails.
    let env = env_of(&["DEEPSEEK_API_KEY"]);
    assert_eq!(config.llm_with(&env).expect("llm").model, "deepseek-flash");
    assert_eq!(config.tts_with(&env).expect("tts").provider(), "edge");
    let err = config.stt_with(&env).expect_err("no stt key");
    assert!(err.to_string().contains("MINIMAX_API_KEY"), "{err}");
  }

  #[test]
  fn test_section_overrides_preset_defaults() {
    let config = Config::from_toml(
      r#"
      [tts]
      provider = "qwen"
      voice = "Ethan"
      [llm]
      provider = "qwen"
      model = "qwen3.8-max"
      extra = { enable_thinking = true }
      "#,
    )
    .expect("parse");
    let env = env_of(&["DASHSCOPE_API_KEY", "MINIMAX_API_KEY"]);
    let tts = config.tts_with(&env).expect("tts");
    assert_eq!(
      (tts.provider(), tts.model(), tts.default_voice()),
      ("qwen", "qwen3-tts-flash", "Ethan")
    );
    let llm = config.llm_with(&env).expect("llm");
    assert_eq!(llm.model, "qwen3.8-max");
    assert_eq!(llm.extra["enable_thinking"], serde_json::json!(true));
    assert_eq!(llm.client.base_url(), DASHSCOPE_COMPAT);
    // STT has no section: auto-selected by priority.
    assert_eq!(config.stt_with(&env).expect("stt").provider(), "minimax");
  }

  #[test]
  fn test_preset_extra_disables_reasoning() {
    let config = Config::from_toml("[llm]\nprovider = \"deepseek\"").expect("parse");
    let llm = config
      .llm_with(&env_of(&["DEEPSEEK_API_KEY"]))
      .expect("llm");
    assert_eq!(llm.extra["thinking"]["type"], "disabled");
  }

  #[test]
  fn test_custom_provider() {
    let config = Config::from_toml(
      r#"
      [tts]
      provider = "kokoro"
      model = "kokoro"
      voice = "zf_xiaobei"
      [llm]
      provider = "ollama"
      model = "qwen3:8b"
      [stt]
      provider = "kokoro"
      [providers.kokoro]
      protocol = "openai"
      base_url = "http://127.0.0.1:8880/v1/"
      [providers.ollama]
      protocol = "openai"
      base_url = "http://127.0.0.1:11434/v1"
      api_key_env = "OLLAMA_KEY"
      "#,
    )
    .expect("parse");
    let tts = config.tts_with(&env_of(&[])).expect("keyless custom tts");
    assert_eq!(
      (tts.provider(), tts.model(), tts.default_voice()),
      ("kokoro", "kokoro", "zf_xiaobei")
    );

    let err = config.llm_with(&env_of(&[])).expect_err("missing key var");
    assert!(err.to_string().contains("OLLAMA_KEY"), "{err}");
    let llm = config.llm_with(&env_of(&["OLLAMA_KEY"])).expect("llm");
    assert_eq!(llm.client.base_url(), "http://127.0.0.1:11434/v1");
    assert!(llm.extra.is_empty());

    let err = config
      .stt_with(&env_of(&[]))
      .expect_err("custom stt needs a model");
    assert!(err.to_string().contains("[stt] model is required"), "{err}");
  }

  #[test]
  fn test_custom_provider_shadows_preset_defaults() {
    let config = Config::from_toml(
      "[tts]\nprovider = \"qwen\"\n[providers.qwen]\nprotocol = \"openai\"\nbase_url = \"http://h/v1\"",
    )
    .expect("parse");
    let err = config.tts_with(&env_of(&[])).expect_err("no preset model");
    assert!(err.to_string().contains("[tts] model is required"), "{err}");
  }

  #[test]
  fn test_invalid_configs_are_rejected() {
    let err = Config::from_toml("[tts]\nprovidr = \"qwen\"").expect_err("typo");
    assert!(err.to_string().contains("providr"), "{err}");

    let config = Config::from_toml("[stt]\nprovider = \"edge\"").expect("parse");
    let err = config.stt_with(&env_of(&[])).expect_err("edge has no stt");
    assert!(err.to_string().contains("no STT support"), "{err}");

    let config = Config::from_toml("[tts]\nprovider = \"nope\"").expect("parse");
    let err = config.tts_with(&env_of(&[])).expect_err("unknown");
    assert!(err.to_string().contains("presets: minimax"), "{err}");

    let config = Config::from_toml("[tts]\nprovider = \"qwen\"").expect("parse");
    let err = config.tts_with(&env_of(&[])).expect_err("key missing");
    assert!(err.to_string().contains("DASHSCOPE_API_KEY"), "{err}");
  }

  #[test]
  fn test_every_preset_extra_is_a_json_object() {
    for p in PRESETS {
      if let Some(llm) = &p.llm {
        let parsed: Result<serde_json::Map<String, serde_json::Value>, _> =
          serde_json::from_str(llm.extra);
        assert!(parsed.is_ok(), "{}", p.name);
      }
    }
  }

  #[test]
  fn test_load_explicit_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("phonon.toml");
    std::fs::write(&path, "[tts]\nprovider = \"edge\"").expect("write");
    let config = Config::load(Some(&path)).expect("load");
    assert_eq!(config.tts().expect("tts").provider(), "edge");
    assert!(Config::load(Some(&dir.path().join("missing.toml"))).is_err());
  }
}
