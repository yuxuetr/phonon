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

## TTS provider

TTS (`/api/tts` and the podcast endpoints) uses the provider from the config
file (`$PHONON_CONFIG`, `./phonon.toml`, `~/.config/phonon/config.toml`), else
the first preset whose API key is set, else keyless Edge TTS. It is resolved
once at startup and logged; a config that cannot be read stops startup. See
[docs/providers.md](../docs/providers.md). `voice` is optional and defaults to
the provider's; podcast scripts must use that provider's voice ids.

## TTS Request Example

```bash
curl -X POST http://localhost:3000/api/tts \
  -H "Content-Type: application/json" \
  -d '{"text": "Hello world", "voice": "en-US-AriaNeural"}' \
  --output speech.wav
```
