//! Speech-to-Text (STT) integration.
//!
//! Provides the [`SttProvider`] trait and an OpenAI Whisper implementation
//! ([`WhisperStt`]) that calls the `/audio/transcriptions` API.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use phonon_ai::{WhisperStt, SttProvider};
//!
//! # async fn example() -> Result<(), phonon_ai::AiError> {
//! let stt = WhisperStt::new()?;
//! let result = stt.transcribe_file("recording.wav", None).await?;
//! println!("Text: {}", result.text);
//! for seg in &result.segments {
//!     println!("[{:.1}s - {:.1}s] {}", seg.start, seg.end, seg.text);
//! }
//! # Ok(())
//! # }
//! ```

use crate::AiError;
use crate::client::HttpClient;
use phonon_core::AudioBuffer;
use serde::Deserialize;
use std::future::Future;
use std::path::Path;
use tracing::{info, instrument};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A single timed segment returned by transcription.
#[derive(Debug, Clone, Deserialize)]
pub struct TranscriptSegment {
  /// Segment index (0-based).
  #[serde(default)]
  pub id: usize,
  /// Start time in seconds.
  pub start: f64,
  /// End time in seconds.
  pub end: f64,
  /// Transcribed text for this segment.
  pub text: String,
}

/// Full transcription result.
#[derive(Debug, Clone)]
pub struct TranscriptResult {
  /// Complete transcribed text.
  pub text: String,
  /// Optional language detected (ISO-639-1, e.g. "en").
  pub language: Option<String>,
  /// Duration of the audio in seconds.
  pub duration: Option<f64>,
  /// Timed segments (only available when timestamps are requested).
  pub segments: Vec<TranscriptSegment>,
}

/// Options for transcription requests.
#[derive(Debug, Clone)]
pub struct TranscribeOptions {
  /// Language hint (ISO-639-1 code, e.g. "en", "zh").
  /// If `None`, the model auto-detects.
  pub language: Option<String>,
  /// Optional prompt to guide the model (e.g. proper nouns, style).
  pub prompt: Option<String>,
  /// Whether to request timed segments. Defaults to `true`.
  pub timestamps: bool,
  /// Temperature for sampling (0.0 = deterministic). Default 0.0.
  pub temperature: f64,
}

// Hand-written so `None` options (which use `unwrap_or_default()`) get
// timed segments, matching the documented default.
impl Default for TranscribeOptions {
  fn default() -> Self {
    Self {
      language: None,
      prompt: None,
      timestamps: true,
      temperature: 0.0,
    }
  }
}

impl TranscribeOptions {
  /// Create default options with timestamps enabled.
  pub fn new() -> Self {
    Self::default()
  }

  /// Set the language hint.
  pub fn with_language(mut self, lang: &str) -> Self {
    self.language = Some(lang.to_string());
    self
  }

  /// Set a guiding prompt.
  pub fn with_prompt(mut self, prompt: &str) -> Self {
    self.prompt = Some(prompt.to_string());
    self
  }
}

// ---------------------------------------------------------------------------
// Trait
// ---------------------------------------------------------------------------

/// Trait for Speech-to-Text providers.
///
/// Implementors convert audio files or raw bytes into text transcriptions.
pub trait SttProvider: Send + Sync {
  /// Transcribe an audio file at the given path.
  fn transcribe_file(
    &self,
    path: &str,
    options: Option<TranscribeOptions>,
  ) -> impl Future<Output = Result<TranscriptResult, AiError>> + Send;

  /// Transcribe raw audio bytes.
  ///
  /// `filename` is the hint for the API (e.g. "audio.wav").
  fn transcribe_bytes(
    &self,
    data: Vec<u8>,
    filename: &str,
    options: Option<TranscribeOptions>,
  ) -> impl Future<Output = Result<TranscriptResult, AiError>> + Send;
}

// ---------------------------------------------------------------------------
// OpenAI Whisper implementation
// ---------------------------------------------------------------------------

/// OpenAI Whisper STT provider.
///
/// Calls the OpenAI `/audio/transcriptions` endpoint (compatible with the
/// Whisper model).
#[derive(Debug, Clone)]
pub struct WhisperStt {
  client: HttpClient,
  model: String,
  segment_mode: SegmentMode,
  path: String,
}

const DEFAULT_PATH: &str = "audio/transcriptions";

/// How an OpenAI-compatible `/audio/transcriptions` endpoint returns timed segments.
///
/// Vendors differ: sending `verbose_json` where it is not accepted is a 400.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentMode {
  /// Segments only with `response_format=verbose_json` (OpenAI, MiniMax).
  VerboseJson,
  /// Segments in the default response; `response_format` is not sent (GLM).
  Always,
  /// Text only; `verbose_json` is rejected (StepFun).
  Unsupported,
}

impl SegmentMode {
  /// The `response_format` form field to send, if any.
  fn response_format(self, timestamps: bool) -> Option<&'static str> {
    match self {
      SegmentMode::Always => None,
      SegmentMode::VerboseJson if timestamps => Some("verbose_json"),
      SegmentMode::VerboseJson | SegmentMode::Unsupported => Some("json"),
    }
  }
}

/// Raw verbose_json response from the Whisper API.
#[derive(Deserialize)]
struct WhisperVerboseResponse {
  text: String,
  #[serde(default)]
  language: Option<String>,
  #[serde(default)]
  duration: Option<f64>,
  #[serde(default)]
  segments: Vec<TranscriptSegment>,
}

impl WhisperStt {
  /// Creates a new `WhisperStt` using the default OpenAI endpoint.
  ///
  /// Reads the API key from `OPENAI_API_KEY`.
  pub fn new() -> Result<Self, AiError> {
    let client = HttpClient::openai()?;
    Ok(Self {
      client,
      model: "whisper-1".to_string(),
      segment_mode: SegmentMode::VerboseJson,
      path: DEFAULT_PATH.to_string(),
    })
  }

  /// Creates a `WhisperStt` with a custom HTTP client and model.
  pub fn with_client(client: HttpClient, model: impl Into<String>) -> Self {
    Self {
      client,
      model: model.into(),
      segment_mode: SegmentMode::VerboseJson,
      path: DEFAULT_PATH.to_string(),
    }
  }

  /// Set the model name (e.g. "whisper-1").
  pub fn set_model(&mut self, model: impl Into<String>) {
    self.model = model.into();
  }

  /// Set how the endpoint returns timed segments. Default: [`SegmentMode::VerboseJson`].
  pub fn with_segment_mode(mut self, mode: SegmentMode) -> Self {
    self.segment_mode = mode;
    self
  }

  /// Set the endpoint path under the base URL (default `"audio/transcriptions"`).
  /// MiniMax takes the same multipart body at `"speech_to_text"`.
  pub fn with_path(mut self, path: impl Into<String>) -> Self {
    self.path = path.into();
    self
  }

  /// Build the multipart form for the Whisper API.
  fn build_form(
    &self,
    file_part: reqwest::multipart::Part,
    options: &TranscribeOptions,
  ) -> reqwest::multipart::Form {
    let response_format = self.segment_mode.response_format(options.timestamps);
    let verbose = response_format == Some("verbose_json");

    let mut form = reqwest::multipart::Form::new()
      .part("file", file_part)
      .text("model", self.model.clone())
      .text("temperature", options.temperature.to_string());
    if let Some(format) = response_format {
      form = form.text("response_format", format);
    }

    if let Some(ref lang) = options.language {
      form = form.text("language", lang.clone());
    }
    if let Some(ref prompt) = options.prompt {
      form = form.text("prompt", prompt.clone());
    }

    // Request segment-level timestamps when segments are requested.
    if verbose {
      form = form.text("timestamp_granularities[]", "segment".to_string());
    }

    form
  }

  /// Send the form and parse the response.
  async fn send_request(
    &self,
    form: reqwest::multipart::Form,
  ) -> Result<TranscriptResult, AiError> {
    let url = format!("{}/{}", self.client.base_url(), self.path);

    let response = self
      .client
      .inner()
      .post(&url)
      .header("Authorization", format!("Bearer {}", self.client.api_key()))
      .multipart(form)
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

    let body = response
      .text()
      .await
      .map_err(|e| AiError::Http(e.to_string()))?;

    // Every mode's body fits the verbose shape: `segments` defaults to empty.
    let resp: WhisperVerboseResponse =
      serde_json::from_str(&body).map_err(|e| AiError::Decode(e.to_string()))?;
    Ok(TranscriptResult {
      text: resp.text,
      language: resp.language,
      duration: resp.duration,
      segments: resp.segments,
    })
  }
}

impl SttProvider for WhisperStt {
  #[instrument(skip(self), fields(model = %self.model))]
  async fn transcribe_file(
    &self,
    path: &str,
    options: Option<TranscribeOptions>,
  ) -> Result<TranscriptResult, AiError> {
    let opts = options.unwrap_or_default();
    info!(path = %path, "Transcribing audio file via Whisper");

    let file_bytes = tokio::fs::read(path)
      .await
      .map_err(|e| AiError::Io(e.to_string()))?;

    let filename = Path::new(path)
      .file_name()
      .and_then(|n| n.to_str())
      .unwrap_or("audio.wav")
      .to_string();

    let part = reqwest::multipart::Part::bytes(file_bytes)
      .file_name(filename)
      .mime_str("application/octet-stream")
      .map_err(|e| AiError::Http(e.to_string()))?;

    let form = self.build_form(part, &opts);
    let result = self.send_request(form).await?;

    info!(
      text_len = result.text.len(),
      segments = result.segments.len(),
      "Transcription complete"
    );

    Ok(result)
  }

  #[instrument(skip(self, data), fields(model = %self.model, size = data.len()))]
  async fn transcribe_bytes(
    &self,
    data: Vec<u8>,
    filename: &str,
    options: Option<TranscribeOptions>,
  ) -> Result<TranscriptResult, AiError> {
    let opts = options.unwrap_or_default();
    info!(filename = %filename, "Transcribing audio bytes via Whisper");

    let part = reqwest::multipart::Part::bytes(data)
      .file_name(filename.to_string())
      .mime_str("application/octet-stream")
      .map_err(|e| AiError::Http(e.to_string()))?;

    let form = self.build_form(part, &opts);
    let result = self.send_request(form).await?;

    info!(
      text_len = result.text.len(),
      segments = result.segments.len(),
      "Transcription complete"
    );

    Ok(result)
  }
}

/// Split mono `samples` into consecutive `(start, end)` ranges no longer than
/// `max_len`, cutting at the quietest 20 ms block in the last quarter of each
/// window so words are not split where avoidable.
pub(crate) fn chunk_ranges(
  samples: &[f32],
  sample_rate: u32,
  max_len: usize,
) -> Vec<(usize, usize)> {
  let block = (sample_rate as usize / 50).max(1);
  let max_len = max_len.max(block);
  let mut ranges = Vec::new();
  let mut start = 0;
  while samples.len() - start > max_len {
    let window_end = start + max_len;
    let search_from = window_end - max_len / 4;
    let energy = |at: usize| samples[at..at + block].iter().map(|s| s * s).sum::<f32>();
    let cut = (search_from..=window_end - block)
      .step_by(block)
      .min_by(|&a, &b| energy(a).total_cmp(&energy(b)))
      .map_or(window_end, |at| at + block / 2);
    ranges.push((start, cut));
    start = cut;
  }
  ranges.push((start, samples.len()));
  ranges
}

/// Sample rate of the chunks sent to providers with a duration limit; speech
/// models work at 16 kHz, and it keeps a chunk's WAV small.
const CHUNK_SAMPLE_RATE: u32 = 16_000;

/// Re-encode `audio` as mono 16 kHz WAV pieces of at most `max_secs`, each
/// paired with its start time in seconds.
pub(crate) fn wav_chunks(
  audio: &AudioBuffer,
  max_secs: u32,
) -> Result<Vec<(f64, Vec<u8>)>, AiError> {
  let to_ai = |e: phonon_core::PhononError| AiError::Decode(e.to_string());
  let mono = phonon_core::ensure_channels(audio, 1).map_err(to_ai)?;
  let mono = phonon_core::resample(&mono, CHUNK_SAMPLE_RATE).map_err(to_ai)?;
  let max_len = max_secs as usize * CHUNK_SAMPLE_RATE as usize;
  chunk_ranges(&mono.samples, CHUNK_SAMPLE_RATE, max_len)
    .into_iter()
    .map(|(start, end)| {
      let piece = AudioBuffer {
        samples: mono.samples[start..end].to_vec(),
        sample_rate: CHUNK_SAMPLE_RATE,
        channels: 1,
      };
      let bytes = phonon_wav::encode_wav(&piece).map_err(to_ai)?;
      Ok((start as f64 / CHUNK_SAMPLE_RATE as f64, bytes))
    })
    .collect()
}

/// Append a chunk's transcript to `total`, shifting its times by `offset`
/// seconds and continuing the segment ids.
pub(crate) fn append_transcript(total: &mut TranscriptResult, part: TranscriptResult, offset: f64) {
  let ascii_boundary = total
    .text
    .ends_with(|c: char| c.is_ascii_alphanumeric() || c.is_ascii_punctuation())
    && part.text.starts_with(|c: char| c.is_ascii_alphanumeric());
  if ascii_boundary {
    total.text.push(' ');
  }
  total.text.push_str(&part.text);
  total.language = total.language.take().or(part.language);
  if let Some(duration) = part.duration {
    total.duration = Some(offset + duration);
  }
  let first_id = total.segments.len();
  total.segments.extend(
    part
      .segments
      .into_iter()
      .enumerate()
      .map(|(i, seg)| TranscriptSegment {
        id: first_id + i,
        start: seg.start + offset,
        end: seg.end + offset,
        text: seg.text,
      }),
  );
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_default_options_request_timestamps() {
    assert!(TranscribeOptions::default().timestamps);
    assert!(TranscribeOptions::new().timestamps);
  }

  #[test]
  fn test_transcribe_options_defaults() {
    let opts = TranscribeOptions::new();
    assert!(opts.timestamps);
    assert!((opts.temperature - 0.0).abs() < f64::EPSILON);
    assert!(opts.language.is_none());
    assert!(opts.prompt.is_none());
  }

  #[test]
  fn test_transcribe_options_builder() {
    let opts = TranscribeOptions::new()
      .with_language("zh")
      .with_prompt("Phonon audio engine");
    assert_eq!(opts.language.as_deref(), Some("zh"));
    assert_eq!(opts.prompt.as_deref(), Some("Phonon audio engine"));
  }

  #[test]
  fn test_verbose_json_deserialization() {
    let json = r#"{
            "text": "Hello world",
            "language": "en",
            "duration": 2.5,
            "segments": [
                {"id": 0, "start": 0.0, "end": 1.2, "text": "Hello"},
                {"id": 1, "start": 1.2, "end": 2.5, "text": " world"}
            ]
        }"#;
    let resp: WhisperVerboseResponse =
      serde_json::from_str(json).expect("Should deserialize verbose response");
    assert_eq!(resp.text, "Hello world");
    assert_eq!(resp.language.as_deref(), Some("en"));
    assert!((resp.duration.unwrap_or(0.0) - 2.5).abs() < f64::EPSILON);
    assert_eq!(resp.segments.len(), 2);
    assert_eq!(resp.segments[0].text, "Hello");
    assert!((resp.segments[1].start - 1.2).abs() < f64::EPSILON);
  }

  #[test]
  fn test_append_transcript_offsets_and_joins() {
    let part = |text: &str, segs: &[(f64, f64, &str)]| TranscriptResult {
      text: text.to_string(),
      language: Some("zh".to_string()),
      duration: segs.last().map(|s| s.1),
      segments: segs
        .iter()
        .enumerate()
        .map(|(id, &(start, end, text))| TranscriptSegment {
          id,
          start,
          end,
          text: text.to_string(),
        })
        .collect(),
    };
    let mut total = TranscriptResult {
      text: String::new(),
      language: None,
      duration: None,
      segments: vec![],
    };
    append_transcript(&mut total, part("你好。", &[(0.5, 1.0, "你好。")]), 0.0);
    append_transcript(
      &mut total,
      part("Hello world.", &[(0.2, 0.9, "Hello"), (1.0, 2.0, "world.")]),
      29.0,
    );
    append_transcript(&mut total, part("Again", &[(0.1, 0.4, "Again")]), 58.0);
    assert_eq!(total.text, "你好。Hello world. Again");
    assert_eq!(total.language.as_deref(), Some("zh"));
    assert_eq!(total.duration, Some(58.4));
    let summary: Vec<_> = total
      .segments
      .iter()
      .map(|s| (s.id, s.start, s.end))
      .collect();
    assert_eq!(
      summary,
      [
        (0, 0.5, 1.0),
        (1, 29.2, 29.9),
        (2, 30.0, 31.0),
        (3, 58.1, 58.4)
      ]
    );
  }

  #[test]
  fn test_wav_chunks_are_mono_16k_and_bounded() {
    let stereo = AudioBuffer {
      samples: vec![0.1; 48_000 * 2 * 5],
      sample_rate: 48_000,
      channels: 2,
    };
    let chunks = wav_chunks(&stereo, 2).expect("chunks");
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].0, 0.0);
    let mut total = 0.0;
    for (offset, bytes) in &chunks {
      let piece = phonon_io::decode_audio(bytes, "wav").expect("decodes");
      assert_eq!((piece.sample_rate, piece.channels), (16_000, 1));
      assert!(piece.duration_secs() <= 2.0);
      assert!((offset - total).abs() < 1e-9);
      total += piece.duration_secs();
    }
    assert!((total - 5.0).abs() < 0.01, "total {total}");
  }

  #[test]
  fn test_chunk_ranges_cut_in_silence() {
    // 1 kHz rate, 20 ms blocks; tone everywhere except a gap at 800-840.
    let mut samples = vec![0.5_f32; 2500];
    samples[800..840].fill(0.0);
    let ranges = chunk_ranges(&samples, 1000, 1000);
    let first_cut = ranges[0].1;
    assert!((800..840).contains(&first_cut), "cut at {first_cut}");
    assert!(ranges.iter().all(|(a, b)| b - a <= 1000));
    assert_eq!(ranges.last().map(|r| r.1), Some(2500));
    assert!(ranges.windows(2).all(|w| w[0].1 == w[1].0));
    assert_eq!(chunk_ranges(&samples[..900], 1000, 1000), [(0, 900)]);
    assert_eq!(chunk_ranges(&[], 1000, 1000), [(0, 0)]);
  }

  #[test]
  fn test_segment_mode_response_format() {
    assert_eq!(
      SegmentMode::VerboseJson.response_format(true),
      Some("verbose_json")
    );
    assert_eq!(
      SegmentMode::VerboseJson.response_format(false),
      Some("json")
    );
    assert_eq!(SegmentMode::Always.response_format(true), None);
    assert_eq!(SegmentMode::Unsupported.response_format(true), Some("json"));
  }

  #[test]
  fn test_plain_json_parses_without_segments() {
    let resp: WhisperVerboseResponse =
      serde_json::from_str(r#"{"text": "你好"}"#).expect("plain json");
    assert_eq!(resp.text, "你好");
    assert!(resp.segments.is_empty());
  }

  #[test]
  fn test_transcript_segment_fields() {
    let seg = TranscriptSegment {
      id: 0,
      start: 1.5,
      end: 3.0,
      text: "Test segment".to_string(),
    };
    assert_eq!(seg.id, 0);
    assert!((seg.start - 1.5).abs() < f64::EPSILON);
    assert!((seg.end - 3.0).abs() < f64::EPSILON);
    assert_eq!(seg.text, "Test segment");
  }

  #[test]
  fn test_verbose_json_missing_optional_fields() {
    // Whisper API may omit language/duration in some edge cases
    let json = r#"{
            "text": "Some text",
            "segments": []
        }"#;
    let resp: WhisperVerboseResponse =
      serde_json::from_str(json).expect("Should handle missing optional fields");
    assert_eq!(resp.text, "Some text");
    assert!(resp.language.is_none());
    assert!(resp.duration.is_none());
    assert!(resp.segments.is_empty());
  }

  #[test]
  fn test_whisper_stt_model_setter() {
    // Can't call new() without OPENAI_API_KEY, but we test the setter on a mock instance
    let client =
      HttpClient::new("test-key", "http://localhost:8080/v1", None).expect("Should create client");
    let mut stt = WhisperStt::with_client(client, "whisper-1");
    assert_eq!(stt.model, "whisper-1");
    stt.set_model("whisper-large-v3");
    assert_eq!(stt.model, "whisper-large-v3");
  }
}
