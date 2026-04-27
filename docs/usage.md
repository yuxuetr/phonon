# Phonon Usage Guide

> A practical, copy-pasteable reference for the Phonon audio engine.
> Companion document: [`usage.zh-CN.md`](./usage.zh-CN.md).

This guide is organized by *how* you want to use Phonon — CLI, library,
service, MCP, or WebAssembly. Each section is independent; jump to whichever
matches your situation.

| Section | Use Phonon as a… |
|---|---|
| [A. CLI cheatsheet](#a-cli-cheatsheet) | command-line tool |
| [B. DSP pipeline (Rust)](#b-dsp-pipeline-rust) | library, low-level DSP |
| [C. Podcast pipeline (Rust)](#c-podcast-pipeline-rust) | library, podcast generation |
| [D. Real-time playback + EQ](#d-real-time-playback--eq) | library, playback engine |
| [E. REST server](#e-rest-server) | HTTP service |
| [F. MCP server](#f-mcp-server) | tool backend for an LLM agent |
| [G. WebAssembly](#g-webassembly) | in-browser DSP |
| [H. One-liner cookbook](#h-one-liner-cookbook) | quick reference |

---

## A. CLI cheatsheet

All subcommands run via `cargo run -p phonon-cli -- <subcommand>`.
For a permanent install: `cargo install --path phonon-cli`, then call `phonon …`.

```bash
# 1. Inspect a file
phonon info --file song.wav

# 2. Generate an 880 Hz, 3-second sine
phonon gen-tone --output tone.wav --duration 3 --freq 880

# 3. Trim 1.5 s – 4.0 s
phonon trim --input song.wav --output cut.wav --start 1.5 --end 4

# 4. Mix two files (matching sample rate / channels)
phonon mix --input1 a.wav --input2 b.wav --output mixed.wav

# 5. Peak-normalize to 0.95
phonon normalize --input cut.wav --output cut_norm.wav --target 0.95

# 6. High-pass at 200 Hz
phonon filter --input voice.wav --output cleaned.wav \
              --type high-pass --cutoff 200

# 7. Speed change (simple resample-based; pitch is not preserved)
phonon speed --input voice.wav --output fast.wav --factor 1.25

# 8. Convert formats (extension-driven)
phonon convert --input voice.wav --output voice.mp3 --bitrate 192

# 9. TTS via OpenAI (set OPENAI_API_KEY)
phonon tts --text "Welcome to phonon" --voice alloy --model tts-1-hd \
           --output hello.wav

# 10. Speech-to-text + JSON timestamps
phonon stt --input meeting.wav --language en --json > meeting.json

# 11. Generate SRT subtitles
phonon subtitle --input meeting.wav --output meeting.srt

# 12. Detect chapters by silence
phonon chapters --input episode.wav --threshold-db -45 --min-silence 1.5

# 13. Video audio operations
phonon video extract --input talk.mp4 --output talk.wav
phonon video replace --video talk.mp4 --audio dub.wav --output dubbed.mp4
phonon video probe   --input talk.mp4

# 14. Generate or preview a podcast from a JSON script
phonon podcast generate --script ep01.json --output ep01.wav
phonon podcast preview  --script ep01.json --segment 0 --output preview.wav
```

---

## B. DSP pipeline (Rust)

A typical voice-cleanup chain: load → high-pass → compress → loudness-normalize
→ fades → write.

```rust
use phonon_core::{
  algorithms, normalize_lufs, with_compression, fade_in, fade_out,
  CompressorConfig,
};
use phonon_io::{read_audio, write_audio};

fn process_voice(input: &str, output: &str) -> anyhow::Result<()> {
  // 1. Read (any supported format — WAV/MP3/FLAC/OGG/AAC).
  let mut buf = read_audio(input)?;

  // 2. High-pass to remove low rumble.
  algorithms::filter(
    &mut buf,
    phonon_core::FilterType::HighPass,
    80.0, 0.707, 0.0,
  )?;

  // 3. Voice compression preset (-18 dBFS threshold, 3:1 ratio, etc.).
  let buf = with_compression(&buf, &CompressorConfig::voice());

  // 4. EBU R128 loudness target — -16 LUFS / -1 dBTP is the podcast standard.
  let mut buf = buf;
  normalize_lufs(&mut buf, -16.0, -1.0);

  // 5. Short fade-in / fade-out on the head and tail.
  fade_in(&mut buf, 0.05);
  fade_out(&mut buf, 0.20);

  // 6. Write — output format is chosen by extension.
  write_audio(output, &buf)?;
  Ok(())
}
```

Phonon-core also ships a real LUFS measurement API:

```rust
let report = phonon_core::measure_loudness(&buf);
println!(
  "I={:.1} LUFS, S={:.1}, M={:.1}, LRA={:.1} LU, dBTP={:.2}",
  report.integrated_lufs, report.short_term_max_lufs,
  report.momentary_max_lufs, report.range_lu, report.true_peak_dbtp,
);
```

---

## C. Podcast pipeline (Rust)

Full path: cached TTS + AI-generated BGM + multi-speaker assembly +
chapters + SRT + ID3-tagged MP3 export.

```rust
use std::path::PathBuf;
use phonon_ai::{CachingTts, OpenAiTts, ReplicateSoundGen};
use phonon_core::normalize_lufs;
use phonon_io::{Mp3Config, PodcastTags};
use phonon_podcast::{PodcastPipeline, PodcastScript};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // 1. TTS with a content-addressable on-disk cache.
  //    Same (text, voice) → instant hit, zero API cost.
  let tts = CachingTts::new(OpenAiTts::new()?, PathBuf::from("./.tts-cache"));

  // 2. Sound generator — script `bgm.kind == "generated"` will hit this.
  let sg = ReplicateSoundGen::from_env()?;  // REPLICATE_API_TOKEN

  // 3. Compose the pipeline.
  let pipeline = PodcastPipeline::new(tts).with_sound_gen(sg);

  // 4. Load a JSON script (sample shape below).
  let script = PodcastScript::from_file("ep01.json")?;

  // 5. generate_episode() returns audio + chapters + SRT in one call.
  let mut ep = pipeline.generate_episode(&script).await?;

  // 6. Loudness-normalize before publishing.
  normalize_lufs(&mut ep.audio, -16.0, -1.0);

  // 7. Attach metadata and export tagged MP3 (PodcastIndex CHAP/CTOC included).
  ep = ep.with_tags(PodcastTags {
    title:    Some("EP01: Rust × Podcast".into()),
    artist:   Some("Phonon Studio".into()),
    album:    Some("Season 1".into()),
    year:     Some(2026),
    genre:    Some("Podcast".into()),
    language: Some("eng".into()),
    track:    Some(1),
    ..Default::default()
  });

  ep.export_mp3("ep01.mp3", &Mp3Config { bitrate_kbps: 192 })?;

  // 8. Drop the SRT next to the MP3 for upload.
  if let Some(srt) = ep.srt.as_deref() {
    std::fs::write("ep01.srt", srt)?;
  }
  Ok(())
}
```

Sample script (`ep01.json`):

```json
{
  "title": "EP01: Rust × Podcast",
  "speakers": {
    "alice": { "voice_id": "alloy", "model": "tts-1-hd" },
    "bob":   { "voice_id": "echo",  "model": "tts-1-hd" }
  },
  "segments": [
    { "speaker": "alice", "text": "Welcome to the show." },
    { "speaker": "bob",   "text": "Today we discuss Rust audio." },
    { "speaker": "alice", "text": "Let's dive in!" }
  ],
  "config": {
    "pause_between_segments_secs": 0.5,
    "crossfade_secs": 0.05,
    "output_sample_rate": 44100,
    "output_channels": 1,
    "bgm": {
      "kind": "generated",
      "prompt": { "prompt": "soft ambient pad, 90 bpm" }
    },
    "bgm_volume": 0.06,
    "fade_in_secs": 1.0,
    "fade_out_secs": 2.0
  }
}
```

---

## D. Real-time playback + EQ

```rust
use phonon_core::{ParametricEqPlugin, EqBand, FilterType};
use phonon_engine::AudioPlayer;
use phonon_io::read_audio;

fn play_with_eq() -> anyhow::Result<()> {
  let buf = read_audio("song.wav")?;
  let player = AudioPlayer::new()?;     // default output device

  // Two-band parametric EQ on the playback chain.
  player.add_plugin(Box::new(ParametricEqPlugin::with_bands(vec![
    EqBand { filter_type: FilterType::HighShelf, freq_hz: 8000.0, q: 0.7, gain_db: 3.0 },
    EqBand { filter_type: FilterType::Peaking,   freq_hz: 200.0,  q: 1.0, gain_db: -2.0 },
  ])));

  player.play(&buf)?;
  player.set_volume(0.8);
  // … pause / resume / seek / progress as needed
  Ok(())
}
```

Streaming mode (start playing while TTS is still generating chunks):

```rust
let player = AudioPlayer::new()?;
while let Some(chunk) = next_chunk().await? {
  player.append_samples(&chunk)?;       // auto-activates the player
}
player.finish_streaming();              // auto-stop when buffer drains
```

---

## E. REST server

```bash
# Default — listens on 0.0.0.0:3000
cargo run -p phonon-server

# Custom port
PHONON_BIND=127.0.0.1:8080 cargo run -p phonon-server
```

```bash
# Health
curl http://localhost:3000/health

# TTS (defaults to Edge TTS, no API key required)
curl -X POST http://localhost:3000/api/tts \
  -H 'Content-Type: application/json' \
  -d '{"text":"Hello phonon","voice":"en-US-AriaNeural"}' \
  --output hello.wav

# Full podcast (returns audio/wav)
curl -X POST http://localhost:3000/api/podcast/generate \
  -H 'Content-Type: application/json' \
  -d @ep01.json --output ep01.wav

# Single-segment preview
curl -X POST http://localhost:3000/api/podcast/preview \
  -H 'Content-Type: application/json' \
  -d '{"script": <script-json>, "segment_index": 0}' \
  --output preview.wav
```

---

## F. MCP server

Phonon ships a JSON-RPC 2.0 server over stdio so any MCP-aware LLM client
(Claude Desktop, Cursor, custom agents) can drive Phonon directly.

```bash
cargo run -p phonon-mcp --bin phonon-mcp
# or after `cargo install --path phonon-mcp`:
phonon-mcp
```

Wire it into Claude Desktop (`~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "phonon": {
      "command": "/Users/you/.cargo/bin/phonon-mcp",
      "args": []
    }
  }
}
```

You can then say things like:

> Load `/tmp/voice.wav`, normalize to -16 LUFS, save as `/tmp/voice_loud.mp3`.

The agent will chain `audio_load → audio_normalize_lufs → audio_save` for you.

Built-in tools (14):

| Category | Tools |
|---|---|
| basic   | `audio_load`, `audio_save`, `audio_info`, `audio_list`, `audio_drop` |
| DSP     | `audio_mix`, `audio_trim`, `audio_fade`, `audio_normalize`, `audio_normalize_lufs`, `audio_volume_db`, `audio_pan`, `audio_resample` |
| measure | `audio_loudness` (integrated/short-term/momentary LUFS, LRA, dBTP, peak, RMS, dBFS) |

Audio buffers cross the JSON-RPC boundary as `AudioHandle = UUID` references
into an in-memory registry. **PCM data never travels on the wire.**

---

## G. WebAssembly

```bash
cargo install wasm-pack
wasm-pack build phonon-wasm --target web --out-dir www/pkg
python3 -m http.server -d phonon-wasm/www 8000
# Open http://localhost:8000/
```

JavaScript usage:

```js
import init, { process_audio } from "./pkg/phonon_wasm.js";
await init();

const ops = JSON.stringify([
  { op: "normalize" },
  { op: "fade_in",  duration_secs: 0.5 },
  { op: "fade_out", duration_secs: 0.5 },
  { op: "eq", bands: [
    { filter_type: "high_shelf", freq_hz: 8000, q: 0.7, gain_db: 3 }
  ]}
]);
const out = process_audio(samples, sampleRate, channels, ops);
```

`samples` is an interleaved `Float32Array`; the demo at `phonon-wasm/www/index.html`
shows full drag-and-drop + WebAudio playback.

---

## H. One-liner cookbook

| Task | API |
|---|---|
| Read any format | `phonon_io::read_audio(path)` |
| Write any format (extension-routed) | `phonon_io::write_audio(path, &buf)` |
| Direct 16-bit PCM WAV write | `phonon_wav::write_wav(path, &buf)` |
| Loudness-normalize for podcasts | `phonon_core::normalize_lufs(&mut buf, -16.0, -1.0)` |
| Measure LUFS-I/S/M / LRA / dBTP | `phonon_core::measure_loudness(&buf)` |
| Mono → stereo with constant-power pan | `phonon_core::pan_mono_to_stereo(&buf, +0.5)` |
| Lanczos-sinc resample | `phonon_core::resample(&buf, 48_000)` |
| Crossfade two buffers | `phonon_core::crossfade(&a, &b, 0.5)` |
| Tagged MP3 (chapters + cover + ID3) | `phonon_io::write_mp3_tagged(p, &buf, &cfg, &tags, &chapters)` |
| Tagged FLAC | `phonon_io::write_flac_tagged(p, &buf, &cfg, &tags)` |
| Tagged OGG | `phonon_io::write_ogg_tagged(p, &buf, &cfg, &tags)` |
| Full podcast → audio + chapters + SRT | `pipeline.generate_episode(&script).await` |
| Real-time playback | `AudioPlayer::new()?.play(&buf)?` |

---

## Requirements

- **Rust 1.85+** (2024 edition)
- **ffmpeg / ffprobe** on `$PATH` for `phonon-video`
- API keys (only when you use the corresponding provider):
  - `OPENAI_API_KEY` for OpenAI TTS / Whisper STT / OpenAI script generator
  - `ELEVENLABS_API_KEY` for ElevenLabs TTS
  - `REPLICATE_API_TOKEN` for `ReplicateSoundGen`
- Edge TTS is free and needs no key.

## License

Dual-licensed under MIT or Apache-2.0.
