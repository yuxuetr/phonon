//! LLM-powered podcast script generation.
//!
//! Generates a `PodcastScript` from a topic using a Large Language Model.
//!
//! # Example
//!
//! ```rust,no_run
//! use phonon_podcast::script_gen::{OpenAiScriptGenerator, ScriptGenerator, ScriptRequest};
//!
//! # async fn example() -> Result<(), phonon_podcast::PodcastError> {
//! let generator = OpenAiScriptGenerator::from_env()?;
//! let request = ScriptRequest::new("Rust 语言的未来发展趋势")
//!     .with_host("小明", "zh-CN-YunyangNeural")
//!     .with_guest("小红", "zh-CN-XiaoxiaoNeural");
//! let script = generator.generate(&request).await?;
//! println!("Generated {} segments", script.segment_count());
//! # Ok(())
//! # }
//! ```

use crate::PodcastError;
use crate::script::{PodcastScript, Segment};
use phonon_ai::{AiError, VoiceConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use tracing::{info, instrument};

/// Trait for podcast script generators.
pub trait ScriptGenerator: Send + Sync {
  /// Generates a `PodcastScript` from a topic request.
  fn generate(
    &self,
    request: &ScriptRequest,
  ) -> impl Future<Output = Result<PodcastScript, PodcastError>> + Send;
}

/// A request to generate a podcast script.
#[derive(Debug, Clone)]
pub struct ScriptRequest {
  /// The topic or prompt for the episode.
  pub topic: String,

  /// Speaker definitions: (name, voice_id) pairs.
  pub speakers: Vec<SpeakerDef>,

  /// Optional system prompt to customize LLM behavior.
  pub system_prompt: Option<String>,

  /// Approximate number of segments to generate.
  pub target_segments: usize,

  /// Language for the generated script.
  pub language: String,
}

/// A speaker definition for the script request.
#[derive(Debug, Clone)]
pub struct SpeakerDef {
  /// Display name (e.g. "host", "Alice").
  pub name: String,
  /// TTS voice ID.
  pub voice_id: String,
  /// Optional role description for the LLM.
  pub role: Option<String>,
}

impl ScriptRequest {
  /// Create a new script request with the given topic.
  /// Defaults to 2 speakers (host + guest) and ~10 segments.
  pub fn new(topic: impl Into<String>) -> Self {
    Self {
      topic: topic.into(),
      speakers: vec![
        SpeakerDef {
          name: "host".to_string(),
          voice_id: "alloy".to_string(),
          role: Some("Podcast host, asks questions and guides conversation".to_string()),
        },
        SpeakerDef {
          name: "guest".to_string(),
          voice_id: "echo".to_string(),
          role: Some("Guest expert, provides insights and answers".to_string()),
        },
      ],
      system_prompt: None,
      target_segments: 10,
      language: "zh-CN".to_string(),
    }
  }

  /// Set the host speaker.
  pub fn with_host(mut self, name: impl Into<String>, voice_id: impl Into<String>) -> Self {
    let name = name.into();
    // Replace existing host or add new one
    if let Some(host) = self.speakers.iter_mut().find(|s| s.name == "host") {
      host.name = name;
      host.voice_id = voice_id.into();
    } else {
      self.speakers.insert(
        0,
        SpeakerDef {
          name,
          voice_id: voice_id.into(),
          role: Some("Podcast host".to_string()),
        },
      );
    }
    self
  }

  /// Set the guest speaker.
  pub fn with_guest(mut self, name: impl Into<String>, voice_id: impl Into<String>) -> Self {
    let name = name.into();
    if let Some(guest) = self.speakers.iter_mut().find(|s| s.name == "guest") {
      guest.name = name;
      guest.voice_id = voice_id.into();
    } else {
      self.speakers.push(SpeakerDef {
        name,
        voice_id: voice_id.into(),
        role: Some("Guest expert".to_string()),
      });
    }
    self
  }

  /// Add an arbitrary speaker.
  pub fn with_speaker(
    mut self,
    name: impl Into<String>,
    voice_id: impl Into<String>,
    role: Option<String>,
  ) -> Self {
    self.speakers.push(SpeakerDef {
      name: name.into(),
      voice_id: voice_id.into(),
      role,
    });
    self
  }

  /// Set the target number of segments.
  pub fn with_target_segments(mut self, n: usize) -> Self {
    self.target_segments = n.max(2);
    self
  }

  /// Set a custom system prompt.
  pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
    self.system_prompt = Some(prompt.into());
    self
  }

  /// Set the language.
  pub fn with_language(mut self, language: impl Into<String>) -> Self {
    self.language = language.into();
    self
  }
}

/// LLM response format — the structure we ask the LLM to output.
#[derive(Debug, Deserialize, Serialize)]
struct LlmScriptResponse {
  title: String,
  segments: Vec<LlmSegment>,
}

#[derive(Debug, Deserialize, Serialize)]
struct LlmSegment {
  speaker: String,
  text: String,
}

/// OpenAI-compatible LLM script generator.
///
/// Sends a structured prompt to an OpenAI Chat API and parses the JSON response
/// into a `PodcastScript`.
#[derive(Debug, Clone)]
pub struct OpenAiScriptGenerator {
  client: phonon_ai::HttpClient,
  /// Chat model to use (e.g. "gpt-4o", "gpt-4o-mini").
  model: String,
}

impl OpenAiScriptGenerator {
  /// Create from `OPENAI_API_KEY` environment variable.
  pub fn from_env() -> Result<Self, PodcastError> {
    let client =
      phonon_ai::HttpClient::openai().map_err(|e| PodcastError::Script(e.to_string()))?;
    Ok(Self {
      client,
      model: "gpt-4o-mini".to_string(),
    })
  }

  /// Create with a specific API key and model.
  pub fn new(
    api_key: impl Into<String>,
    base_url: impl Into<String>,
    model: impl Into<String>,
  ) -> Result<Self, PodcastError> {
    let client = phonon_ai::HttpClient::new(api_key, base_url, Some(120))
      .map_err(|e| PodcastError::Script(e.to_string()))?;
    Ok(Self {
      client,
      model: model.into(),
    })
  }

  /// Use a caller-configured client (proxy, TLS, timeouts).
  pub fn with_client(client: phonon_ai::HttpClient, model: impl Into<String>) -> Self {
    Self {
      client,
      model: model.into(),
    }
  }

  /// Set the chat model.
  pub fn with_model(mut self, model: impl Into<String>) -> Self {
    self.model = model.into();
    self
  }

  /// Build the system prompt for script generation.
  fn build_system_prompt(request: &ScriptRequest) -> String {
    let speaker_list: Vec<String> = request
      .speakers
      .iter()
      .map(|s| {
        let role_desc = s.role.as_deref().unwrap_or("Speaker");
        format!("- \"{}\": {}", s.name, role_desc)
      })
      .collect();

    if let Some(ref custom) = request.system_prompt {
      return custom.clone();
    }

    format!(
      r#"You are a professional podcast script writer. Generate a natural, engaging podcast conversation script.

Rules:
1. Output ONLY valid JSON (no markdown, no code fences, no explanation).
2. The JSON must have this exact structure:
   {{"title": "...", "segments": [{{"speaker": "...", "text": "..."}}]}}
3. Use ONLY the speaker names listed below.
4. Generate approximately {} dialogue segments.
5. The conversation should feel natural, with back-and-forth exchanges.
6. Use the language: {}.
7. Each segment should be 1-3 sentences (natural speech length for TTS).

Speakers:
{}

Topic: {}"#,
      request.target_segments,
      request.language,
      speaker_list.join("\n"),
      request.topic
    )
  }
}

impl ScriptGenerator for OpenAiScriptGenerator {
  #[instrument(skip(self, request), fields(topic = %request.topic, speakers = request.speakers.len()))]
  async fn generate(&self, request: &ScriptRequest) -> Result<PodcastScript, PodcastError> {
    info!("Generating podcast script via LLM");

    let system_prompt = Self::build_system_prompt(request);
    let user_message = format!(
      "请围绕「{}」这个话题，生成一期播客对话脚本。",
      request.topic
    );

    let messages = serde_json::json!([
        {"role": "system", "content": system_prompt},
        {"role": "user", "content": user_message}
    ]);

    let body = serde_json::json!({
        "model": self.model,
        "messages": messages,
        "temperature": 0.8,
        "max_tokens": 4096,
        "response_format": {"type": "json_object"}
    });

    let url = format!("{}/chat/completions", self.client.base_url());

    // Same request every attempt, so rate limits and 5xx are safe to retry.
    let resp_json: serde_json::Value = phonon_ai::retry_transient(|| async {
      let response = self
        .client
        .inner()
        .post(&url)
        .header("Authorization", format!("Bearer {}", self.client.api_key()))
        .json(&body)
        .send()
        .await
        .map_err(|e| AiError::Http(e.to_string()))?;

      let status = response.status();
      if !status.is_success() {
        let message = response
          .text()
          .await
          .unwrap_or_else(|_| "Failed to read body".to_string());
        return Err(AiError::Api {
          status: status.as_u16(),
          message,
        });
      }
      response
        .json()
        .await
        .map_err(|e| AiError::Decode(format!("Failed to parse LLM response: {e}")))
    })
    .await
    .map_err(|e| PodcastError::Script(format!("LLM request failed: {e}")))?;

    // Extract the assistant message content
    let content = resp_json["choices"][0]["message"]["content"]
      .as_str()
      .ok_or_else(|| PodcastError::Script("LLM response missing content field".to_string()))?;

    info!(content_len = content.len(), "Received LLM script response");

    // Parse the LLM JSON output
    let llm_response: LlmScriptResponse = serde_json::from_str(content).map_err(|e| {
      PodcastError::Script(format!(
        "Failed to parse LLM script JSON: {e}\nRaw: {content}"
      ))
    })?;

    // Convert to PodcastScript
    let mut speakers = HashMap::new();
    for speaker_def in &request.speakers {
      speakers.insert(
        speaker_def.name.clone(),
        VoiceConfig::new(&speaker_def.voice_id),
      );
    }

    let segments: Vec<Segment> = llm_response
      .segments
      .into_iter()
      .filter(|s| speakers.contains_key(&s.speaker) && !s.text.trim().is_empty())
      .map(|s| Segment {
        speaker: s.speaker,
        text: s.text,
        language: None,
      })
      .collect();

    if segments.is_empty() {
      return Err(PodcastError::Script(
        "LLM generated no valid segments".to_string(),
      ));
    }

    let script = PodcastScript {
      title: llm_response.title,
      speakers,
      segments,
      config: Default::default(),
    };

    info!(
        title = %script.title,
        segments = script.segment_count(),
        "Script generation complete"
    );

    Ok(script)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use tokio::io::{AsyncReadExt, AsyncWriteExt};

  /// Serves `responses` in order, one per connection, on a local port.
  async fn serve(responses: Vec<(u16, String)>) -> (String, Arc<AtomicUsize>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
      for (status, body) in responses {
        let (mut stream, _) = listener.accept().await.unwrap();
        // Read the whole request so the client never sees a reset.
        let mut request = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
          let n = stream.read(&mut chunk).await.unwrap();
          request.extend_from_slice(&chunk[..n]);
          let text = String::from_utf8_lossy(&request);
          if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
              .lines()
              .find_map(|l| {
                l.to_ascii_lowercase()
                  .strip_prefix("content-length:")
                  .map(|v| v.trim().parse::<usize>().unwrap())
              })
              .unwrap_or(0);
            if request.len() >= end + 4 + length {
              break;
            }
          }
        }
        counter.fetch_add(1, Ordering::SeqCst);
        let response = format!(
          "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
          body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
      }
    });
    (url, hits)
  }

  #[tokio::test]
  async fn test_openai_generator_retries_rate_limit() {
    let script = r#"{"title":"T","segments":[{"speaker":"host","text":"Hi"},{"speaker":"guest","text":"Hello"}]}"#;
    let ok = serde_json::json!({"choices": [{"message": {"content": script}}]}).to_string();
    let (url, hits) = serve(vec![(429, "{}".to_string()), (200, ok)]).await;

    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let generator = OpenAiScriptGenerator::with_client(
      phonon_ai::HttpClient::with_client(http, "k", url),
      "test-model",
    );
    let result = generator.generate(&ScriptRequest::new("topic")).await;

    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(result.unwrap().segment_count(), 2);
  }

  #[test]
  fn test_script_request_defaults() {
    let req = ScriptRequest::new("AI and Music");
    assert_eq!(req.topic, "AI and Music");
    assert_eq!(req.speakers.len(), 2);
    assert_eq!(req.speakers[0].name, "host");
    assert_eq!(req.speakers[1].name, "guest");
    assert_eq!(req.target_segments, 10);
  }

  #[test]
  fn test_script_request_custom_speakers() {
    let req = ScriptRequest::new("Test")
      .with_host("Alice", "voice-1")
      .with_guest("Bob", "voice-2");

    assert_eq!(req.speakers[0].name, "Alice");
    assert_eq!(req.speakers[0].voice_id, "voice-1");
    assert_eq!(req.speakers[1].name, "Bob");
    assert_eq!(req.speakers[1].voice_id, "voice-2");
  }

  #[test]
  fn test_script_request_extra_speaker() {
    let req = ScriptRequest::new("Test").with_speaker(
      "narrator",
      "voice-3",
      Some("Narrator role".to_string()),
    );
    assert_eq!(req.speakers.len(), 3);
    assert_eq!(req.speakers[2].name, "narrator");
  }

  #[test]
  fn test_script_request_target_segments_min() {
    let req = ScriptRequest::new("Test").with_target_segments(0);
    assert_eq!(req.target_segments, 2); // clamped to min 2
  }

  #[test]
  fn test_build_system_prompt() {
    let req = ScriptRequest::new("Rust Programming");
    let prompt = OpenAiScriptGenerator::build_system_prompt(&req);
    assert!(prompt.contains("Rust Programming"));
    assert!(prompt.contains("host"));
    assert!(prompt.contains("guest"));
    assert!(prompt.contains("JSON"));
  }

  #[test]
  fn test_build_system_prompt_custom() {
    let req = ScriptRequest::new("Test").with_system_prompt("Custom system prompt for testing");
    let prompt = OpenAiScriptGenerator::build_system_prompt(&req);
    assert_eq!(prompt, "Custom system prompt for testing");
  }

  #[test]
  fn test_llm_response_parsing() {
    let json = r#"{
            "title": "AI 与音乐的未来",
            "segments": [
                {"speaker": "host", "text": "欢迎收听今天的节目！"},
                {"speaker": "guest", "text": "谢谢邀请！"},
                {"speaker": "host", "text": "今天我们来聊聊AI和音乐。"}
            ]
        }"#;

    let response: LlmScriptResponse =
      serde_json::from_str(json).expect("Should parse LLM response");
    assert_eq!(response.title, "AI 与音乐的未来");
    assert_eq!(response.segments.len(), 3);
    assert_eq!(response.segments[0].speaker, "host");
  }

  #[test]
  fn test_llm_response_to_podcast_script() {
    // Simulate what the generate method does after parsing the LLM response
    let llm_response = LlmScriptResponse {
      title: "Test Episode".to_string(),
      segments: vec![
        LlmSegment {
          speaker: "host".to_string(),
          text: "Welcome!".to_string(),
        },
        LlmSegment {
          speaker: "guest".to_string(),
          text: "Thanks!".to_string(),
        },
        LlmSegment {
          speaker: "unknown".to_string(),
          text: "This should be filtered".to_string(),
        },
      ],
    };

    let request = ScriptRequest::new("Test");
    let mut speakers = HashMap::new();
    for sd in &request.speakers {
      speakers.insert(sd.name.clone(), VoiceConfig::new(&sd.voice_id));
    }

    let segments: Vec<Segment> = llm_response
      .segments
      .into_iter()
      .filter(|s| speakers.contains_key(&s.speaker) && !s.text.trim().is_empty())
      .map(|s| Segment {
        speaker: s.speaker,
        text: s.text,
        language: None,
      })
      .collect();

    // "unknown" speaker should be filtered out
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].speaker, "host");
    assert_eq!(segments[1].speaker, "guest");
  }

  #[test]
  fn test_openai_generator_requires_env() {
    // Without OPENAI_API_KEY this should fail
    // (may pass if the env var is set — that's OK)
    let result = OpenAiScriptGenerator::from_env();
    // Just verify it doesn't panic
    let _ = result;
  }

  // Integration test — requires OPENAI_API_KEY.
  // Run with: cargo test -p phonon-podcast -- --ignored
  #[tokio::test]
  #[ignore]
  async fn test_generate_script_integration() {
    let generator = OpenAiScriptGenerator::from_env().expect("Need OPENAI_API_KEY");
    let request = ScriptRequest::new("Rust 语言的异步编程").with_target_segments(6);

    let script = generator
      .generate(&request)
      .await
      .expect("Script generation should succeed");

    assert!(!script.title.is_empty());
    assert!(!script.segments.is_empty());
    assert!(script.segments.len() >= 3);
  }
}
