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

use axum::{
    extract::Json,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Router,
};
use phonon_ai::{EdgeTts, TtsProvider, VoiceConfig};
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
                Ok(bytes) => (
                    StatusCode::OK,
                    [("content-type", "audio/wav")],
                    bytes,
                )
                    .into_response(),
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
                    (
                        StatusCode::OK,
                        [("content-type", "audio/wav")],
                        bytes,
                    )
                        .into_response()
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

async fn podcast_preview_handler(
    Json(req): Json<serde_json::Value>,
) -> impl IntoResponse {
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
                Ok(bytes) => (
                    StatusCode::OK,
                    [("content-type", "audio/wav")],
                    bytes,
                )
                    .into_response(),
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
// Server setup
// ---------------------------------------------------------------------------

fn app() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/api/tts", post(tts_handler))
        .route("/api/podcast/generate", post(podcast_generate_handler))
        .route("/api/podcast/preview", post(podcast_preview_handler))
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

    axum::serve(listener, app())
        .await
        .expect("Server error");
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
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
}
