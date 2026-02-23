# phonon-server

REST API server for the Phonon audio engine, built with axum.

## Endpoints

| Method | Path | Description |
|---|---|---|
| GET | `/health` | Health check |
| POST | `/api/tts` | Text-to-speech synthesis (returns WAV) |
| POST | `/api/podcast/generate` | Generate podcast from JSON script |
| POST | `/api/podcast/preview` | Preview a single segment |

## Usage

```bash
# Start with default bind address (0.0.0.0:3000)
cargo run -p phonon-server

# Custom bind address
PHONON_BIND=127.0.0.1:8080 cargo run -p phonon-server
```

## TTS Request Example

```bash
curl -X POST http://localhost:3000/api/tts \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello world", "voice": "en-US-AriaNeural"}' \
  --output speech.wav
```
