# Phonon — 声子音频引擎

A comprehensive Rust audio processing ecosystem, from low-level WAV I/O to AI-powered podcast generation.

## Architecture

Phonon is organized as a Rust workspace with the following crates:

| Crate | Description |
|---|---|
| `phonon-core` | Core audio buffer, DSP algorithms (mix, resample, normalize, FFT, compressor, etc.) |
| `phonon-wav` | WAV file reading and writing (16-bit PCM) |
| `phonon-io` | Multi-format audio I/O — read any format via Symphonia, write WAV/MP3/FLAC/OGG/AAC with streaming encoders |
| `phonon-engine` | Real-time audio playback with pause/resume/seek/volume, streaming support, and effects chain |
| `phonon-ai` | AI integrations — TTS (OpenAI, Edge, ElevenLabs), STT (Whisper), sound generation |
| `phonon-podcast` | Podcast generation pipeline — script → TTS → assembly with chapters & subtitles |
| `phonon-video` | Video audio track extraction/replacement via ffmpeg |
| `phonon-server` | REST API server (axum) exposing TTS and podcast generation |
| `phonon-wasm` | WebAssembly bindings — run phonon-core in the browser via wasm-bindgen |
| `phonon-studio` | Desktop GUI application (Dioxus 0.7) with spectrum visualization, EQ panel, drag & drop |
| `phonon-cli` | Command-line interface |

## Quick Start

```bash
# Build all crates
cargo build

# Run tests
cargo test --workspace

# Run benchmarks
cargo bench -p phonon-core

# Start the REST API server
cargo run -p phonon-server
```

## Features

### Audio Processing (phonon-core)
- Mix, merge, trim, crossfade, overlay
- Resample (Lanczos sinc interpolation), channel conversion
- Normalize, volume adjustment, fade in/out
- Dynamic range compressor (voice/gentle/limiter presets)
- Multi-band parametric EQ (8 filter types, presets for voice/bass/bright)
- Biquad filters (low-pass, high-pass, band-pass, notch, peaking, shelving)
- FFT spectrum analysis
- Signal generators (sine, square, sawtooth, white noise)
- Loudness measurement (RMS, peak, dBFS)
- Plugin architecture (DspPlugin trait, PluginRegistry, 5 built-in plugins)
- Parallel processing via rayon (auto-threshold, optional via `parallel` feature)

### AI Integration (phonon-ai)
- **TTS**: OpenAI, Microsoft Edge TTS, ElevenLabs
- **STT**: OpenAI Whisper (with timed segments)
- **Sound Generation**: Generic HTTP provider for music/SFX APIs
- Streaming TTS support (synthesize while playing)

### Podcast Pipeline (phonon-podcast)
- JSON script → multi-speaker podcast generation
- Concurrent TTS synthesis with configurable concurrency
- Automatic SRT subtitle generation from STT
- Chapter markers from silence detection or speaker switches
- Multi-language segment support
- LLM-powered script generation

### REST API (phonon-server)
- `GET /health` — Health check
- `POST /api/tts` — Text-to-speech synthesis
- `POST /api/podcast/generate` — Full podcast generation
- `POST /api/podcast/preview` — Single segment preview

### WebAssembly (phonon-wasm)
- All phonon-core algorithms available in the browser
- `process_audio()` — chain operations via JSON config
- Individual functions: normalize, reverse, fade, volume, speed, spectrum, EQ
- Demo page with drag & drop, waveform/spectrum visualization, Web Audio playback
- Build: `wasm-pack build -p phonon-wasm --target web`

## Requirements

- Rust 1.85+ (2024 edition)
- ffmpeg (optional, for phonon-video)
- API keys (optional): `OPENAI_API_KEY`, `ELEVENLABS_API_KEY`

## License

Dual-licensed under MIT or Apache-2.0.
