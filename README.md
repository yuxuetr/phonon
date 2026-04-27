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
| `phonon-mcp` | MCP (Model Context Protocol) server — exposes audio tools to LLM agents over JSON-RPC stdio |
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

For copy-pasteable recipes (CLI / Rust / REST / MCP / Wasm), see the
**usage guide**: [`docs/usage.md`](./docs/usage.md) ([中文版](./docs/usage.zh-CN.md)).

## Features

### Audio Processing (phonon-core)
- Mix, merge, trim, crossfade, overlay
- Resample (Lanczos sinc interpolation), channel conversion
- Normalize, volume adjustment, fade in/out
- Stereo panning (constant-power law)
- Dynamic range compressor (voice/gentle/limiter presets)
- Multi-band parametric EQ (8 filter types, presets for voice/bass/bright)
- Biquad filters (low-pass, high-pass, band-pass, notch, peaking, shelving)
- FFT spectrum analysis
- Signal generators (sine, square, sawtooth, white noise)
- Loudness measurement: peak, RMS, dBFS
- **LUFS / EBU R128** — integrated, short-term, momentary, LRA, true peak (dBTP),
  with `normalize_lufs` for podcast-spec output (-16 LUFS / -1 dBTP)
- Plugin architecture (DspPlugin trait, PluginRegistry, 5 built-in plugins)
- Parallel processing via rayon (auto-threshold, optional via `parallel` feature)
- SIMD optimization (NEON / SSE) for hot kernels (optional via `simd` feature)

### AI Integration (phonon-ai)
- **TTS**: OpenAI, Microsoft Edge TTS, ElevenLabs
- **STT**: OpenAI Whisper (with timed segments)
- **Sound Generation**: Generic HTTP provider + Replicate-flavored async polling
- Streaming TTS (PCM chunked HTTP for OpenAI / ElevenLabs)
- **Filesystem cache** (`CachingTts`) — content-addressable, blake3-keyed,
  decorator over any `TtsProvider`

### Podcast Pipeline (phonon-podcast)
- JSON script → multi-speaker podcast generation
- Concurrent TTS synthesis with configurable concurrency
- Automatic SRT subtitle generation from STT
- Chapter markers from silence detection or speaker switches
- Multi-language segment support
- LLM-powered script generation
- **AI-generated assets** in the pipeline (`AssetSource::Generated { prompt }`
  for BGM / intro / outro, auto-loop / auto-trim)
- **Episode bundle** — `generate_episode()` produces audio + chapters + SRT
  + tags in one call, with `export_mp3 / export_flac / export_ogg / export_wav`
  helpers for publish-ready files

### Audio I/O Tagging (phonon-io)
- **MP3 ID3v2.4** — title / artist / album / year / genre / comment / language /
  track / cover art (APIC), plus PodcastIndex `CHAP` + `CTOC` chapter frames
- **FLAC Vorbis Comments** + cover art picture block
- **OGG Vorbis Comments** + base64 `METADATA_BLOCK_PICTURE` cover art
- Shared `PodcastTags` struct used uniformly across formats

### REST API (phonon-server)
- `GET /health` — Health check
- `POST /api/tts` — Text-to-speech synthesis
- `POST /api/podcast/generate` — Full podcast generation
- `POST /api/podcast/preview` — Single segment preview

### MCP Server (phonon-mcp)
JSON-RPC 2.0 over stdio — drives Phonon from Claude Desktop, Cursor, or any
MCP-aware LLM agent. 14 built-in tools covering load/save/info, mix/trim/fade,
normalize (peak + LUFS), volume_db, pan, resample, and full BS.1770 loudness
measurement. Audio buffers cross the JSON boundary as UUID handles into an
in-memory asset registry — no PCM data on the wire.

```bash
cargo run -p phonon-mcp --bin phonon-mcp
# or wire it into your MCP client config as the launch command
```

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
