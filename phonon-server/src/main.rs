//! # phonon-server
//!
//! REST API server for the Phonon audio engine.
//!
//! ## Endpoints
//!
//! - `GET  /health` — Health check
//! - `POST /api/tts` — Synthesize text to audio (WAV)
//! - `POST /api/podcast/generate` — Generate a podcast from a script (WAV)
//! - `POST /api/podcast/preview` — Preview a single segment
//! - `POST /api/lufs` — Measure loudness of an uploaded audio file
//! - `POST /api/podcast/episode` — Script -> tagged MP3 + chapters + SRT (JSON)

use axum::{
  Router,
  body::Bytes,
  extract::{DefaultBodyLimit, Json, State},
  http::{HeaderMap, StatusCode, header},
  response::{IntoResponse, Response},
  routing::{get, post},
};
use base64::Engine as _;
use phonon_ai::{AnyTts, Config, TtsProvider, VoiceConfig};
use phonon_core::AudioBuffer;
use phonon_io::{Mp3Config, PodcastTags};
use phonon_podcast::{AssetSource, PodcastPipeline, PodcastScript};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use tracing::info;

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// TTS synthesis request.
#[derive(Debug, Deserialize)]
struct TtsRequest {
  /// Text to synthesize.
  text: String,
  /// Voice ID of the configured provider; defaults to the provider's.
  voice: Option<String>,
}

/// Health check response.
#[derive(Serialize)]
struct HealthResponse {
  status: String,
  version: String,
}

// ---------------------------------------------------------------------------
// Request limits
// ---------------------------------------------------------------------------

/// Request bodies are capped explicitly so `/api/lufs` can accept real audio
/// files (axum's implicit default is 2 MB) while still bounding memory.
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;
/// Per-text cap for `/api/tts` and for each script segment.
const MAX_TEXT_CHARS: usize = 5_000;
const MAX_SEGMENTS: usize = 200;
/// Upper bound for every seconds-valued pipeline parameter (pauses, fades).
const MAX_DURATION_PARAM_SECS: f64 = 30.0;
const MAX_TTS_CONCURRENCY: usize = 8;
const VALID_MP3_BITRATES: [u32; 10] = [64, 80, 96, 112, 128, 160, 192, 224, 256, 320];

fn bad_request(msg: impl std::fmt::Display) -> Response {
  (
    StatusCode::BAD_REQUEST,
    Json(serde_json::json!({"error": msg.to_string()})),
  )
    .into_response()
}

/// Validates a script received over HTTP before any work is done on it.
///
/// Beyond `PodcastScript::validate()`, this rejects file-path assets (the
/// pipeline would read them from the server's filesystem and return their
/// audio to the caller) and bounds every size/duration parameter that
/// drives allocation.
fn check_untrusted_script(script: &PodcastScript) -> Result<(), String> {
  script.validate().map_err(|e| e.to_string())?;
  if script.segments.len() > MAX_SEGMENTS {
    return Err(format!("at most {MAX_SEGMENTS} segments are allowed"));
  }
  if let Some(i) = script
    .segments
    .iter()
    .position(|s| s.text.chars().count() > MAX_TEXT_CHARS)
  {
    return Err(format!("segment {i} exceeds {MAX_TEXT_CHARS} characters"));
  }

  let cfg = &script.config;
  for (name, asset) in [
    ("bgm", cfg.effective_bgm()),
    ("intro", cfg.effective_intro()),
    ("outro", cfg.effective_outro()),
  ] {
    if let Some(AssetSource::File { .. }) = asset {
      return Err(format!(
        "{name}: file-path assets are not accepted over HTTP"
      ));
    }
  }
  if !(8_000..=192_000).contains(&cfg.output_sample_rate) {
    return Err("output_sample_rate must be within 8000..=192000".to_string());
  }
  if !(1..=2).contains(&cfg.output_channels) {
    return Err("output_channels must be 1 or 2".to_string());
  }
  for (name, secs) in [
    (
      "pause_between_segments_secs",
      cfg.pause_between_segments_secs,
    ),
    ("crossfade_secs", cfg.crossfade_secs),
    ("fade_in_secs", cfg.fade_in_secs),
    ("fade_out_secs", cfg.fade_out_secs),
  ] {
    // `contains` is false for NaN, so non-finite values are rejected too.
    if !(0.0..=MAX_DURATION_PARAM_SECS).contains(&secs) {
      return Err(format!(
        "{name} must be within 0..={MAX_DURATION_PARAM_SECS}"
      ));
    }
  }
  if !(1..=MAX_TTS_CONCURRENCY).contains(&cfg.tts_concurrency) {
    return Err(format!(
      "tts_concurrency must be within 1..={MAX_TTS_CONCURRENCY}"
    ));
  }
  Ok(())
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn internal_error(msg: impl std::fmt::Display) -> Response {
  (
    StatusCode::INTERNAL_SERVER_ERROR,
    Json(serde_json::json!({"error": msg.to_string()})),
  )
    .into_response()
}

/// Run decoding / encoding / DSP on tokio's blocking pool so a long request
/// doesn't stall the worker thread serving everyone else.
async fn blocking<T, F>(f: F) -> Result<T, String>
where
  T: Send + 'static,
  F: FnOnce() -> Result<T, String> + Send + 'static,
{
  tokio::task::spawn_blocking(f)
    .await
    .map_err(|e| format!("worker task failed: {e}"))?
}

/// Encode `buffer` as a WAV response body.
async fn wav_response(buffer: AudioBuffer) -> Response {
  let encoded = blocking(move || {
    let tmp = tempfile::Builder::new()
      .suffix(".wav")
      .tempfile()
      .map_err(|e| e.to_string())?;
    phonon_wav::write_wav(tmp.path(), &buffer).map_err(|e| e.to_string())?;
    std::fs::read(tmp.path()).map_err(|e| e.to_string())
  })
  .await;
  match encoded {
    Ok(bytes) => (StatusCode::OK, [("content-type", "audio/wav")], bytes).into_response(),
    Err(e) => internal_error(e),
  }
}

async fn health() -> Json<HealthResponse> {
  Json(HealthResponse {
    status: "ok".to_string(),
    version: env!("CARGO_PKG_VERSION").to_string(),
  })
}

async fn tts_handler(State(tts): State<AnyTts>, Json(req): Json<TtsRequest>) -> impl IntoResponse {
  let voice = req.voice.as_deref().unwrap_or(tts.default_voice());
  info!(text_len = req.text.len(), voice, "TTS request");
  if req.text.chars().count() > MAX_TEXT_CHARS {
    return bad_request(format!("text exceeds {MAX_TEXT_CHARS} characters"));
  }
  if voice.is_empty() {
    return bad_request("'voice' is required: the configured provider has no default");
  }
  let voice = VoiceConfig::new(voice);

  match tts.synthesize(&req.text, &voice).await {
    Ok(buffer) => wav_response(buffer).await,
    Err(e) => internal_error(e),
  }
}

async fn podcast_generate_handler(
  State(tts): State<AnyTts>,
  Json(script): Json<PodcastScript>,
) -> impl IntoResponse {
  info!(title = %script.title, segments = script.segment_count(), "Podcast generation request");
  if let Err(e) = check_untrusted_script(&script) {
    return bad_request(e);
  }

  let pipeline = PodcastPipeline::new(tts);

  match pipeline.generate(&script).await {
    Ok(buffer) => {
      info!(
        duration = buffer.duration_secs(),
        "Podcast generated successfully"
      );
      wav_response(buffer).await
    }
    Err(e) => internal_error(e),
  }
}

async fn podcast_preview_handler(
  State(tts): State<AnyTts>,
  Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
  let script_value = match req.get("script") {
    Some(v) => v.clone(),
    None => return bad_request("Missing 'script' field"),
  };

  let script: PodcastScript = match serde_json::from_value(script_value) {
    Ok(s) => s,
    Err(e) => return bad_request(format!("Invalid script: {e}")),
  };

  if let Err(e) = check_untrusted_script(&script) {
    return bad_request(e);
  }

  let segment_index = req
    .get("segment_index")
    .and_then(|v| v.as_u64())
    .unwrap_or(0) as usize;

  info!(
      title = %script.title,
      segment = segment_index,
      "Podcast preview request"
  );

  let pipeline = PodcastPipeline::new(tts);

  match pipeline.preview_segment(&script, segment_index).await {
    Ok(buffer) => wav_response(buffer).await,
    Err(e) => bad_request(e),
  }
}

// ---------------------------------------------------------------------------
// /api/lufs — measure loudness of uploaded audio
// ---------------------------------------------------------------------------

/// JSON body accepted in addition to the raw-bytes form. Only one mode is
/// active per request; if the JSON shape parses we go via base64 input,
/// otherwise the raw body bytes are interpreted as audio.
#[derive(Debug, Deserialize)]
struct LufsJsonRequest {
  /// Base64-encoded audio bytes.
  audio_base64: String,
  /// Hint for the file extension (`wav`, `mp3`, `flac`, `ogg`, `aac`).
  /// If absent the symphonia probe falls back to content sniffing.
  #[serde(default)]
  format: Option<String>,
}

/// LUFS measurement response shape (matches `phonon_core::LoudnessReport`
/// plus extra peak/RMS/dBFS for convenience).
#[derive(Debug, Serialize)]
struct LufsResponse {
  duration_secs: f64,
  sample_rate: u32,
  channels: u16,
  integrated_lufs: Option<f32>,
  short_term_max_lufs: Option<f32>,
  momentary_max_lufs: Option<f32>,
  range_lu: f32,
  true_peak_dbtp: Option<f32>,
  peak: f32,
  rms: f32,
  dbfs: Option<f32>,
}

/// Map a HTTP `Content-Type` to a file extension that symphonia can probe.
fn content_type_to_ext(ct: Option<&str>) -> &'static str {
  match ct.unwrap_or("").to_ascii_lowercase().as_str() {
    "audio/mpeg" | "audio/mp3" => "mp3",
    "audio/flac" | "audio/x-flac" => "flac",
    "audio/ogg" | "application/ogg" => "ogg",
    "audio/aac" | "audio/x-aac" => "aac",
    _ => "wav",
  }
}

/// Decode the JSON-LufsResponse-friendly value of an `f32` that may be NaN.
fn finite(v: f32) -> Option<f32> {
  if v.is_finite() { Some(v) } else { None }
}

/// Write `bytes` to a temp file with the given extension and decode via
/// `phonon_io::read_audio`. Returns the decoded buffer.
async fn lufs_handler(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
  let ct = headers
    .get(header::CONTENT_TYPE)
    .and_then(|v| v.to_str().ok());
  info!(content_type = ?ct, body_len = body.len(), "LUFS request");

  // Branch 1: JSON body with base64 + format hint.
  let parsed_json = ct
    .map(|s| s.starts_with("application/json"))
    .unwrap_or(false);
  let (audio, ext) = if parsed_json {
    match serde_json::from_slice::<LufsJsonRequest>(&body) {
      Ok(req) => {
        let audio = match base64::engine::general_purpose::STANDARD.decode(req.audio_base64) {
          Ok(b) => Bytes::from(b),
          Err(e) => return bad_request(format!("base64 decode failed: {e}")),
        };
        let ext = req.format.unwrap_or_else(|| "wav".to_string());
        (audio, ext)
      }
      Err(e) => return bad_request(format!("invalid JSON body: {e}")),
    }
  } else {
    if body.is_empty() {
      return bad_request("empty body — POST raw audio bytes or a JSON envelope");
    }
    let ext = content_type_to_ext(ct).to_string();
    (body, ext)
  };

  // The outer error is a failed worker task, the inner one a decode error.
  let measured = blocking(move || {
    Ok(
      phonon_io::decode_audio(&audio, &ext)
        .map_err(|e| e.to_string())
        .map(|buf| {
          let report = phonon_core::measure_loudness(&buf);
          LufsResponse {
            duration_secs: buf.duration_secs(),
            sample_rate: buf.sample_rate,
            channels: buf.channels,
            integrated_lufs: finite(report.integrated_lufs),
            short_term_max_lufs: finite(report.short_term_max_lufs),
            momentary_max_lufs: finite(report.momentary_max_lufs),
            range_lu: report.range_lu,
            true_peak_dbtp: finite(report.true_peak_dbtp),
            peak: phonon_core::peak(&buf),
            rms: phonon_core::rms(&buf),
            dbfs: finite(phonon_core::dbfs(&buf)),
          }
        }),
    )
  })
  .await;

  let resp = match measured {
    Ok(Ok(resp)) => resp,
    Ok(Err(e)) => {
      return (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({"error": format!("decode failed: {e}")})),
      )
        .into_response();
    }
    Err(e) => return internal_error(e),
  };
  (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// /api/podcast/episode — script -> tagged MP3 + chapters + SRT (JSON)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct EpisodeRequest {
  script: PodcastScript,
  /// Optional metadata for the produced MP3.
  #[serde(default)]
  tags: Option<TagsBody>,
  /// Loudness target in LUFS. `None` skips normalization.
  #[serde(default)]
  lufs: Option<f32>,
  /// True-peak ceiling, defaults to -1.0 dBTP when `lufs` is set.
  #[serde(default)]
  true_peak: Option<f32>,
  /// MP3 bitrate in kbps. Defaults to 192.
  #[serde(default)]
  bitrate_kbps: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
struct TagsBody {
  title: Option<String>,
  artist: Option<String>,
  album: Option<String>,
  year: Option<i32>,
  genre: Option<String>,
  comment: Option<String>,
  language: Option<String>,
  track: Option<u32>,
}

#[derive(Debug, Serialize)]
struct EpisodeChapterOut {
  title: String,
  start_secs: f64,
  end_secs: f64,
}

#[derive(Debug, Serialize)]
struct EpisodeResponse {
  duration_secs: f64,
  bitrate_kbps: u32,
  lufs_after: Option<f32>,
  chapters: Vec<EpisodeChapterOut>,
  srt: Option<String>,
  /// Base64-encoded tagged MP3 (with embedded ID3v2 + CHAP/CTOC frames).
  mp3_base64: String,
}

async fn podcast_episode_handler(
  State(tts): State<AnyTts>,
  Json(req): Json<EpisodeRequest>,
) -> impl IntoResponse {
  info!(title = %req.script.title, segments = req.script.segment_count(), "Episode request");
  if let Err(e) = check_untrusted_script(&req.script) {
    return bad_request(e);
  }
  let bitrate = req.bitrate_kbps.unwrap_or(192);
  if !VALID_MP3_BITRATES.contains(&bitrate) {
    return bad_request(format!(
      "bitrate_kbps must be one of {VALID_MP3_BITRATES:?}"
    ));
  }
  if req.lufs.is_some_and(|l| !(-70.0..=0.0).contains(&l)) {
    return bad_request("lufs must be within -70..=0");
  }
  if req.true_peak.is_some_and(|tp| !(-20.0..=0.0).contains(&tp)) {
    return bad_request("true_peak must be within -20..=0");
  }

  let pipeline = PodcastPipeline::new(tts);
  let episode = match pipeline.generate_episode(&req.script).await {
    Ok(e) => e,
    Err(e) => return internal_error(e),
  };

  // Tags fall back to the script title.
  let tags_body = req.tags.unwrap_or_default();
  let tags = PodcastTags {
    title: tags_body.title.or_else(|| Some(req.script.title.clone())),
    artist: tags_body.artist,
    album: tags_body.album,
    year: tags_body.year,
    genre: tags_body.genre,
    comment: tags_body.comment,
    language: tags_body.language,
    track: tags_body.track,
    cover: None,
  };
  let mut episode = episode.with_tags(tags);
  let (lufs, true_peak) = (req.lufs, req.true_peak);

  // Loudness normalization and MP3 encoding are the expensive part of this
  // request; export to a temp file and slurp it back so we can base64 it.
  let rendered = blocking(move || {
    let lufs_after = match lufs {
      Some(target) => {
        phonon_core::normalize_lufs(&mut episode.audio, target, true_peak.unwrap_or(-1.0));
        finite(phonon_core::loudness_integrated(&episode.audio))
      }
      None => None,
    };
    let tmp = tempfile::Builder::new()
      .suffix(".mp3")
      .tempfile()
      .map_err(|e| e.to_string())?;
    let mp3_cfg = Mp3Config {
      bitrate_kbps: bitrate,
    };
    episode
      .export_mp3(tmp.path(), &mp3_cfg)
      .map_err(|e| e.to_string())?;
    let mp3_bytes = std::fs::read(tmp.path()).map_err(|e| e.to_string())?;
    Ok((episode, lufs_after, mp3_bytes))
  })
  .await;
  let (episode, lufs_after, mp3_bytes) = match rendered {
    Ok(r) => r,
    Err(e) => return internal_error(e),
  };

  let total = episode.audio.duration_secs();
  let resp = EpisodeResponse {
    duration_secs: total,
    bitrate_kbps: bitrate,
    lufs_after,
    chapters: episode
      .chapters
      .iter()
      .map(|c| EpisodeChapterOut {
        title: c.title.clone(),
        start_secs: c.start,
        end_secs: c.end.unwrap_or(total),
      })
      .collect(),
    srt: episode.srt.clone(),
    mp3_base64: base64::engine::general_purpose::STANDARD.encode(&mp3_bytes),
  };
  (StatusCode::OK, Json(resp)).into_response()
}

// ---------------------------------------------------------------------------
// Server setup
// ---------------------------------------------------------------------------

/// Routes, with every TTS request served by `tts`.
fn app(tts: AnyTts) -> Router {
  Router::new()
    .route("/health", get(health))
    .route("/api/tts", post(tts_handler))
    .route("/api/podcast/generate", post(podcast_generate_handler))
    .route("/api/podcast/preview", post(podcast_preview_handler))
    .route("/api/lufs", post(lufs_handler))
    .route("/api/podcast/episode", post(podcast_episode_handler))
    .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
    .layer(TraceLayer::new_for_http())
    .with_state(tts)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
  tracing_subscriber::fmt()
    .with_env_filter(
      tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "phonon_server=info,tower_http=info".into()),
    )
    .init();

  // Loopback by default: the API has no authentication. Set PHONON_BIND
  // (e.g. 0.0.0.0:3000) explicitly to expose it.
  let bind = std::env::var("PHONON_BIND").unwrap_or_else(|_| "127.0.0.1:3000".to_string());

  // Resolved once: a broken config should stop startup, not fail requests.
  // Without config or keys this is keyless Edge TTS.
  let tts = Config::load(None)
    .and_then(|config| config.tts())
    .map_err(std::io::Error::other)?;
  info!(bind = %bind, provider = tts.provider(), model = tts.model(), "Starting Phonon server");

  let listener = tokio::net::TcpListener::bind(&bind).await?;

  info!("Listening on {bind}");

  axum::serve(listener, app(tts)).await
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::body::Body;
  use axum::http::Request;
  use http_body_util::BodyExt;
  use tower::ServiceExt;

  /// Keyless Edge TTS, whatever keys the environment has.
  fn test_app() -> Router {
    let tts = Config::from_toml("[tts]\nprovider = \"edge\"")
      .and_then(|c| c.tts())
      .expect("edge tts");
    app(tts)
  }

  #[tokio::test]
  async fn test_health_endpoint() {
    let app = test_app();
    let req = Request::builder()
      .uri("/health")
      .body(Body::empty())
      .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_tts_missing_body() {
    let app = test_app();
    let req = Request::builder()
      .method("POST")
      .uri("/api/tts")
      .header("content-type", "application/json")
      .body(Body::from("{}"))
      .unwrap();

    let response = app.oneshot(req).await.unwrap();
    // Missing "text" field → 422 Unprocessable Entity
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
  }

  /// Build an in-memory mono WAV with a 440 Hz sine for LUFS tests.
  fn sine_wav_bytes(secs: f32, sample_rate: u32, amp: f32) -> Vec<u8> {
    let spec = hound::WavSpec {
      channels: 1,
      sample_rate,
      bits_per_sample: 32,
      sample_format: hound::SampleFormat::Float,
    };
    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    {
      let mut w = hound::WavWriter::new(&mut cursor, spec).unwrap();
      let n = (secs * sample_rate as f32) as u32;
      for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let s = amp * (2.0 * std::f32::consts::PI * 440.0 * t).sin();
        w.write_sample(s).unwrap();
      }
      w.finalize().unwrap();
    }
    cursor.into_inner()
  }

  #[tokio::test]
  async fn test_lufs_empty_body() {
    let response = test_app()
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/api/lufs")
          .body(Body::empty())
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_lufs_raw_wav() {
    let wav = sine_wav_bytes(2.0, 48_000, 0.5);
    let response = test_app()
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/api/lufs")
          .header("content-type", "audio/wav")
          .body(Body::from(wav))
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sample_rate"], 48_000);
    assert_eq!(json["channels"], 1);
    assert!((json["duration_secs"].as_f64().unwrap() - 2.0).abs() < 0.05);
    assert!(json["integrated_lufs"].as_f64().is_some());
    assert!(json["peak"].as_f64().unwrap() > 0.0);
  }

  #[tokio::test]
  async fn test_lufs_json_base64() {
    let wav = sine_wav_bytes(1.0, 44_100, 0.25);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&wav);
    let envelope = serde_json::json!({"audio_base64": b64, "format": "wav"});
    let response = test_app()
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/api/lufs")
          .header("content-type", "application/json")
          .body(Body::from(envelope.to_string()))
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["sample_rate"], 44_100);
  }

  #[tokio::test]
  async fn test_lufs_json_bad_base64() {
    let envelope = serde_json::json!({"audio_base64": "%%not-base64%%"});
    let response = test_app()
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/api/lufs")
          .header("content-type", "application/json")
          .body(Body::from(envelope.to_string()))
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
  }

  fn script_with_config(config: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
      "title": "t",
      "speakers": {"host": {"voice_id": "en-US-AriaNeural"}},
      "segments": [{"speaker": "host", "text": "hello"}],
      "config": config,
    })
  }

  async fn post_json(uri: &str, body: serde_json::Value) -> StatusCode {
    test_app()
      .oneshot(
        Request::builder()
          .method("POST")
          .uri(uri)
          .header("content-type", "application/json")
          .body(Body::from(body.to_string()))
          .unwrap(),
      )
      .await
      .unwrap()
      .status()
  }

  #[tokio::test]
  async fn test_generate_rejects_file_path_assets() {
    for config in [
      serde_json::json!({"bgm_audio": "/etc/passwd"}),
      serde_json::json!({"intro_audio": "/etc/passwd"}),
      serde_json::json!({"outro": {"kind": "file", "path": "/etc/passwd"}}),
    ] {
      let status = post_json("/api/podcast/generate", script_with_config(config.clone())).await;
      assert_eq!(status, StatusCode::BAD_REQUEST, "config {config}");
    }
  }

  #[tokio::test]
  async fn test_generate_rejects_out_of_range_config() {
    for config in [
      serde_json::json!({"output_sample_rate": 4_000_000_000u64}),
      serde_json::json!({"output_channels": 0}),
      serde_json::json!({"pause_between_segments_secs": 1.0e9}),
      serde_json::json!({"fade_out_secs": -1.0}),
      serde_json::json!({"tts_concurrency": 10_000}),
    ] {
      let status = post_json("/api/podcast/generate", script_with_config(config.clone())).await;
      assert_eq!(status, StatusCode::BAD_REQUEST, "config {config}");
    }
  }

  #[tokio::test]
  async fn test_generate_rejects_invalid_script() {
    let mut script = script_with_config(serde_json::json!({}));
    script["segments"] = serde_json::json!([{"speaker": "nobody", "text": "hi"}]);
    let status = post_json("/api/podcast/generate", script).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_preview_rejects_file_path_assets() {
    let body = serde_json::json!({
      "script": script_with_config(serde_json::json!({"bgm_audio": "/etc/passwd"})),
    });
    assert_eq!(
      post_json("/api/podcast/preview", body).await,
      StatusCode::BAD_REQUEST
    );
  }

  #[tokio::test]
  async fn test_episode_rejects_bad_parameters() {
    let script = script_with_config(serde_json::json!({}));
    for extra in [
      serde_json::json!({"bitrate_kbps": 999}),
      serde_json::json!({"lufs": 50.0}),
      serde_json::json!({"lufs": -16.0, "true_peak": 10.0}),
    ] {
      let mut body = serde_json::json!({"script": script.clone()});
      for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
      }
      let status = post_json("/api/podcast/episode", body).await;
      assert_eq!(status, StatusCode::BAD_REQUEST, "extra {extra}");
    }
  }

  #[tokio::test]
  async fn test_tts_rejects_oversized_text() {
    let body = serde_json::json!({"text": "a".repeat(MAX_TEXT_CHARS + 1)});
    assert_eq!(post_json("/api/tts", body).await, StatusCode::BAD_REQUEST);
  }

  #[tokio::test]
  async fn test_episode_missing_script() {
    let response = test_app()
      .oneshot(
        Request::builder()
          .method("POST")
          .uri("/api/podcast/episode")
          .header("content-type", "application/json")
          .body(Body::from("{}"))
          .unwrap(),
      )
      .await
      .unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
  }
}
