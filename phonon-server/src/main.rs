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
  extract::Json,
  http::{HeaderMap, StatusCode, header},
  response::IntoResponse,
  routing::{get, post},
};
use base64::Engine as _;
use phonon_ai::{EdgeTts, TtsProvider, VoiceConfig};
use phonon_io::{Mp3Config, PodcastTags};
use phonon_podcast::{PodcastPipeline, PodcastScript};
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
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
  /// Voice ID (e.g. "en-US-AriaNeural").
  #[serde(default = "default_voice")]
  voice: String,
}

fn default_voice() -> String {
  "en-US-AriaNeural".to_string()
}

/// Health check response.
#[derive(Serialize)]
struct HealthResponse {
  status: String,
  version: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health() -> Json<HealthResponse> {
  Json(HealthResponse {
    status: "ok".to_string(),
    version: env!("CARGO_PKG_VERSION").to_string(),
  })
}

async fn tts_handler(Json(req): Json<TtsRequest>) -> impl IntoResponse {
  info!(text_len = req.text.len(), voice = %req.voice, "TTS request");

  let tts = EdgeTts::new();
  let voice = VoiceConfig::new(&req.voice);

  match tts.synthesize(&req.text, &voice).await {
    Ok(buffer) => {
      // Encode as WAV bytes
      let tmp = match tempfile::Builder::new().suffix(".wav").tempfile() {
        Ok(t) => t,
        Err(e) => {
          return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
          )
            .into_response();
        }
      };

      if let Err(e) = phonon_wav::write_wav(tmp.path(), &buffer) {
        return (
          StatusCode::INTERNAL_SERVER_ERROR,
          Json(serde_json::json!({"error": e.to_string()})),
        )
          .into_response();
      }

      match std::fs::read(tmp.path()) {
        Ok(bytes) => (StatusCode::OK, [("content-type", "audio/wav")], bytes).into_response(),
        Err(e) => (
          StatusCode::INTERNAL_SERVER_ERROR,
          Json(serde_json::json!({"error": e.to_string()})),
        )
          .into_response(),
      }
    }
    Err(e) => (
      StatusCode::INTERNAL_SERVER_ERROR,
      Json(serde_json::json!({"error": e.to_string()})),
    )
      .into_response(),
  }
}

async fn podcast_generate_handler(Json(script): Json<PodcastScript>) -> impl IntoResponse {
  info!(title = %script.title, segments = script.segment_count(), "Podcast generation request");

  let tts = EdgeTts::new();
  let pipeline = PodcastPipeline::new(tts);

  match pipeline.generate(&script).await {
    Ok(buffer) => {
      let tmp = match tempfile::Builder::new().suffix(".wav").tempfile() {
        Ok(t) => t,
        Err(e) => {
          return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
          )
            .into_response();
        }
      };

      if let Err(e) = phonon_wav::write_wav(tmp.path(), &buffer) {
        return (
          StatusCode::INTERNAL_SERVER_ERROR,
          Json(serde_json::json!({"error": e.to_string()})),
        )
          .into_response();
      }

      match std::fs::read(tmp.path()) {
        Ok(bytes) => {
          info!(
            duration = buffer.duration_secs(),
            size = bytes.len(),
            "Podcast generated successfully"
          );
          (StatusCode::OK, [("content-type", "audio/wav")], bytes).into_response()
        }
        Err(e) => (
          StatusCode::INTERNAL_SERVER_ERROR,
          Json(serde_json::json!({"error": e.to_string()})),
        )
          .into_response(),
      }
    }
    Err(e) => (
      StatusCode::INTERNAL_SERVER_ERROR,
      Json(serde_json::json!({"error": e.to_string()})),
    )
      .into_response(),
  }
}

async fn podcast_preview_handler(Json(req): Json<serde_json::Value>) -> impl IntoResponse {
  let script_value = match req.get("script") {
    Some(v) => v.clone(),
    None => {
      return (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "Missing 'script' field"})),
      )
        .into_response();
    }
  };

  let script: PodcastScript = match serde_json::from_value(script_value) {
    Ok(s) => s,
    Err(e) => {
      return (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": format!("Invalid script: {e}")})),
      )
        .into_response();
    }
  };

  let segment_index = req
    .get("segment_index")
    .and_then(|v| v.as_u64())
    .unwrap_or(0) as usize;

  info!(
      title = %script.title,
      segment = segment_index,
      "Podcast preview request"
  );

  let tts = EdgeTts::new();
  let pipeline = PodcastPipeline::new(tts);

  match pipeline.preview_segment(&script, segment_index).await {
    Ok(buffer) => {
      let tmp = match tempfile::Builder::new().suffix(".wav").tempfile() {
        Ok(t) => t,
        Err(e) => {
          return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
          )
            .into_response();
        }
      };

      if let Err(e) = phonon_wav::write_wav(tmp.path(), &buffer) {
        return (
          StatusCode::INTERNAL_SERVER_ERROR,
          Json(serde_json::json!({"error": e.to_string()})),
        )
          .into_response();
      }

      match std::fs::read(tmp.path()) {
        Ok(bytes) => (StatusCode::OK, [("content-type", "audio/wav")], bytes).into_response(),
        Err(e) => (
          StatusCode::INTERNAL_SERVER_ERROR,
          Json(serde_json::json!({"error": e.to_string()})),
        )
          .into_response(),
      }
    }
    Err(e) => (
      StatusCode::BAD_REQUEST,
      Json(serde_json::json!({"error": e.to_string()})),
    )
      .into_response(),
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
fn decode_audio_bytes(
  bytes: &[u8],
  ext: &str,
) -> std::result::Result<phonon_core::AudioBuffer, String> {
  let tmp = tempfile::Builder::new()
    .suffix(&format!(".{}", ext))
    .tempfile()
    .map_err(|e| e.to_string())?;
  std::fs::write(tmp.path(), bytes).map_err(|e| e.to_string())?;
  phonon_io::read_audio(tmp.path()).map_err(|e| e.to_string())
}

async fn lufs_handler(headers: HeaderMap, body: Bytes) -> impl IntoResponse {
  let ct = headers
    .get(header::CONTENT_TYPE)
    .and_then(|v| v.to_str().ok());
  info!(content_type = ?ct, body_len = body.len(), "LUFS request");

  // Branch 1: JSON body with base64 + format hint.
  let parsed_json = ct
    .map(|s| s.starts_with("application/json"))
    .unwrap_or(false);
  let buf_result = if parsed_json {
    match serde_json::from_slice::<LufsJsonRequest>(&body) {
      Ok(req) => {
        let audio = match base64::engine::general_purpose::STANDARD.decode(req.audio_base64) {
          Ok(b) => b,
          Err(e) => {
            return (
              StatusCode::BAD_REQUEST,
              Json(serde_json::json!({"error": format!("base64 decode failed: {e}")})),
            )
              .into_response();
          }
        };
        let ext = req.format.as_deref().unwrap_or("wav");
        decode_audio_bytes(&audio, ext)
      }
      Err(e) => {
        return (
          StatusCode::BAD_REQUEST,
          Json(serde_json::json!({"error": format!("invalid JSON body: {e}")})),
        )
          .into_response();
      }
    }
  } else {
    if body.is_empty() {
      return (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({"error": "empty body — POST raw audio bytes or a JSON envelope"})),
      )
        .into_response();
    }
    decode_audio_bytes(&body, content_type_to_ext(ct))
  };

  let buf = match buf_result {
    Ok(b) => b,
    Err(e) => {
      return (
        StatusCode::UNPROCESSABLE_ENTITY,
        Json(serde_json::json!({"error": format!("decode failed: {e}")})),
      )
        .into_response();
    }
  };

  let report = phonon_core::measure_loudness(&buf);
  let resp = LufsResponse {
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

async fn podcast_episode_handler(Json(req): Json<EpisodeRequest>) -> impl IntoResponse {
  info!(title = %req.script.title, segments = req.script.segment_count(), "Episode request");

  let pipeline = PodcastPipeline::new(EdgeTts::new());
  let mut episode = match pipeline.generate_episode(&req.script).await {
    Ok(e) => e,
    Err(e) => {
      return (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
      )
        .into_response();
    }
  };

  let lufs_after = match req.lufs {
    Some(target) if target.is_finite() => {
      let tp = req.true_peak.unwrap_or(-1.0);
      phonon_core::normalize_lufs(&mut episode.audio, target, tp);
      finite(phonon_core::loudness_integrated(&episode.audio))
    }
    _ => None,
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
  episode = episode.with_tags(tags);

  let bitrate = req.bitrate_kbps.unwrap_or(192);
  let mp3_cfg = Mp3Config {
    bitrate_kbps: bitrate,
  };

  // Export to a temp MP3 and slurp it back so we can base64 it.
  let tmp = match tempfile::Builder::new().suffix(".mp3").tempfile() {
    Ok(t) => t,
    Err(e) => {
      return (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
      )
        .into_response();
    }
  };
  if let Err(e) = episode.export_mp3(tmp.path(), &mp3_cfg) {
    return (
      StatusCode::INTERNAL_SERVER_ERROR,
      Json(serde_json::json!({"error": e.to_string()})),
    )
      .into_response();
  }
  let mp3_bytes = match std::fs::read(tmp.path()) {
    Ok(b) => b,
    Err(e) => {
      return (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": e.to_string()})),
      )
        .into_response();
    }
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

fn app() -> Router {
  Router::new()
    .route("/health", get(health))
    .route("/api/tts", post(tts_handler))
    .route("/api/podcast/generate", post(podcast_generate_handler))
    .route("/api/podcast/preview", post(podcast_preview_handler))
    .route("/api/lufs", post(lufs_handler))
    .route("/api/podcast/episode", post(podcast_episode_handler))
    .layer(CorsLayer::permissive())
    .layer(TraceLayer::new_for_http())
}

#[tokio::main]
async fn main() {
  tracing_subscriber::fmt()
    .with_env_filter(
      tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "phonon_server=info,tower_http=info".into()),
    )
    .init();

  let bind = std::env::var("PHONON_BIND").unwrap_or_else(|_| "0.0.0.0:3000".to_string());

  info!(bind = %bind, "Starting Phonon server");

  let listener = tokio::net::TcpListener::bind(&bind)
    .await
    .expect("Failed to bind");

  info!("Listening on {bind}");

  axum::serve(listener, app()).await.expect("Server error");
}

#[cfg(test)]
mod tests {
  use super::*;
  use axum::body::Body;
  use axum::http::Request;
  use http_body_util::BodyExt;
  use tower::ServiceExt;

  #[tokio::test]
  async fn test_health_endpoint() {
    let app = app();
    let req = Request::builder()
      .uri("/health")
      .body(Body::empty())
      .unwrap();

    let response = app.oneshot(req).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
  }

  #[tokio::test]
  async fn test_tts_missing_body() {
    let app = app();
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
    let response = app()
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
    let response = app()
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
    let response = app()
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
    let response = app()
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

  #[tokio::test]
  async fn test_episode_missing_script() {
    let response = app()
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
