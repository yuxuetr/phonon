# Changelog

All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

---
## v0.7.2 (unreleased) — evaluation backlog

Items G1–G10 in [Todos.md](./Todos.md); see `git log` for individual commits.

### Breaking changes

- **phonon-mcp**: `AssetRegistry::insert` returns `Result` (`McpError::RegistryFull` past a
  1 GiB PCM cap, configurable with `AssetRegistry::with_max_bytes`); `insert_with` is removed.

### Bug fixes

- **Edge TTS works again**: msedge-tts 0.3 got `403 Forbidden` after the service started
  requiring the v2 `Sec-MS-GEC` token (October 2026); upgraded to 0.4.
- **FLAC files written by phonon now read back** with `read_audio` (STREAMINFO minimum block
  size no longer counts the short final block, which symphonia rejected).
- AAC: the encoder's look-ahead is flushed, so the last ~2 frames are no longer dropped.
- `read_audio` skips packets the decoder rejects instead of failing the whole file.
- True peak: oversampling phases sit at 0 / ¼ / ½ / ¾ sample (were offset by ⅛, reading
  mid-sample peaks up to 0.12 dB low).
- Engine: growing a streamed track no longer copies it under the callback's lock;
  `ParametricEqPlugin` parameter changes keep filter state (no click, no allocation).
- Replicate: HTTP errors from status polls and downloads are reported as such.
- ElevenLabs: `voice_id` is percent-encoded as a single URL path segment.

### Reliability and performance

- Transient provider errors (network, 429, 5xx) are retried with backoff for OpenAI /
  ElevenLabs / MiniMax / Edge synthesis and Replicate polls; MiniMax throttling codes map to
  429; Edge TTS has a 60 s timeout; HTTP clients have a 10 s connect timeout.
- Podcast pipeline: post-TTS processing runs on the blocking pool; assembly is linear instead
  of quadratic; `generate_episode` synthesizes concurrently.
- Server: WAV / MP3 encoding and LUFS measurement run on the blocking pool.
- Studio: EQ, effects, playback resampling and waveform/FFT run off the UI thread.

## v0.7.1 (unreleased) — evaluation fixes

Hand-written summary of the fixes from [docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md);
see `git log` for individual commits.

### Breaking changes

- **phonon-mcp**: `ToolRegistry::new(fs_root)` / `Server::new(fs_root)` take a sandbox root;
  `McpError` gains `PathDenied` and loses `From<AiError>` / `From<PodcastError>`.
- **phonon-podcast**: `build_episode_metadata(script, spans)` and
  `assign_chapter_times(script, spans)` take per-segment `(start, end)` times.
- **phonon-engine**: `AudioEffect`, `add_effect`, `PluginAdapter`, `EffectAsPlugin` and the
  `*Effect` wrappers are removed; `EffectChain` holds `Box<dyn DspPlugin>` (`add_plugin`).
- **phonon-io**: AAC encoding (`write_aac`, `AacConfig`, `.aac` in `write_audio`) requires
  the `aac` feature.
- **phonon-plugin** crate removed (scaffold without FFI).

### Security

- **phonon-server**: rejects file-path assets in posted scripts, bounds all size/duration
  parameters, binds `127.0.0.1` by default, drops permissive CORS, explicit 32 MB body limit.
- **phonon-mcp**: `audio_load` / `audio_save` confined to `PHONON_MCP_ROOT`; no silent overwrite.
- **phonon-ai**: API keys are redacted from `Debug` output.

### Bug fixes

- Resampler: full anti-aliasing kernel support when downsampling (48k→16k leakage at 10 kHz
  -26 dB → -64 dB).
- SIMD helpers processed only the last 0–3 samples on non-x86_64/aarch64 targets.
- Compressor: channel-linked, attack/release correct for multichannel audio.
- Podcast: empty BGM no longer hangs; chapters/SRT account for intro and crossfade.
- Whisper: `TranscribeOptions::default()` requests timestamps.
- Engine: audio callback never blocks on control locks; `seek` is frame-aligned.
- Zero-channel buffers return errors instead of panicking; `read_audio` keeps the declared
  format for header-only files.
- CI: clippy `-D warnings` passes on stable 1.99; doctests and AAC feature are tested.

---
## [unreleased]

### Bug Fixes

- clippy + READMEs - ([fed888c](https://github.com/yuxuetr/phonon/commit/fed888c2260e28d5507b44dba73cd852be5d07e4)) - yuxuetr

### Documentation

- **(roadmap)** add v0.7 RoadMap + restructure Todos.md - ([713bca4](https://github.com/yuxuetr/phonon/commit/713bca4127c20b97af21b1ffbb10e8e66be660f4)) - yuxuetr
- **(todos)** mark Step 0 (WIP cleanup) as completed - ([f64120a](https://github.com/yuxuetr/phonon/commit/f64120a327cf10ccca76f635d4e07079e725e85a)) - yuxuetr
- **(usage)** add bilingual usage guide - ([238902e](https://github.com/yuxuetr/phonon/commit/238902e441889704d1485bb6c6f13f2adda46a2c)) - yuxuetr
- update Todos.md, README.md, AGENTS.md for v0.6 completeness - ([c58b199](https://github.com/yuxuetr/phonon/commit/c58b1995b345309a3f4176f9d541f9d26621df22)) - yuxuetr
- v0.7 integration tests, README/AGENTS update, rustfmt sweep - ([eccd0dc](https://github.com/yuxuetr/phonon/commit/eccd0dca0b7b16217b15435c15360e556348f269)) - yuxuetr

### Features

- **(phonon-ai)** SoundPreset, ReplicateSoundGen, streaming TTS, PCM helper - ([187310c](https://github.com/yuxuetr/phonon/commit/187310cfbbdc851d2d35957a95f26c1f84b060af)) - yuxuetr
- **(phonon-ai)** TTS result cache (CachingTts decorator) - ([305e18b](https://github.com/yuxuetr/phonon/commit/305e18baa8dd2d1750e78404541266035d049fec)) - yuxuetr
- **(phonon-cli)** add lufs / tag-mp3 / episode-export subcommands - ([3eeccfd](https://github.com/yuxuetr/phonon/commit/3eeccfd72de95151b173dff76555cae4ffc2bbfc)) - yuxuetr
- **(phonon-core)** upgrade resampler to windowed sinc (Lanczos) interpolation - ([12b3a83](https://github.com/yuxuetr/phonon/commit/12b3a830897f4ac17c8e9f9e8d5e1518fcdcd6c5)) - yuxuetr
- **(phonon-core)** SIMD ops module + algorithm refactor - ([3c79d0e](https://github.com/yuxuetr/phonon/commit/3c79d0e45ef6efaf375b051c8fa481ed4223fa09)) - yuxuetr
- **(phonon-core)** LUFS loudness measurement (ITU-R BS.1770-4 / EBU R128) - ([591c122](https://github.com/yuxuetr/phonon/commit/591c122b2ced7f21bd3854cb84280488182fefa9)) - yuxuetr
- **(phonon-core)** stereo panning with constant-power law - ([e92037d](https://github.com/yuxuetr/phonon/commit/e92037de356cd8976356fe65c12f8ae83326e29e)) - yuxuetr
- **(phonon-engine)** add real-time audio effects chain - ([d85a99c](https://github.com/yuxuetr/phonon/commit/d85a99ca4ec073b7d3c6e2aa4fe4f3a86d0eed54)) - yuxuetr
- **(phonon-io)** add FLAC and OGG Vorbis encoding output - ([2a5a287](https://github.com/yuxuetr/phonon/commit/2a5a287e9d7c20f00abb8489b9c43f9060d60e65)) - yuxuetr
- **(phonon-io)** add AAC encoding output via fdk-aac - ([750928c](https://github.com/yuxuetr/phonon/commit/750928c9eac437b56800d98f4877e6384a32f9c4)) - yuxuetr
- **(phonon-io)** MP3 ID3v2 tags + PodcastIndex chapters - ([81bcd10](https://github.com/yuxuetr/phonon/commit/81bcd1098ba0f015d68bedb5ddd8bfe05b7339bf)) - yuxuetr
- **(phonon-io)** FLAC/OGG Vorbis comments + shared tag types - ([96098bd](https://github.com/yuxuetr/phonon/commit/96098bd6afaafd925027f8974ddfe9e3166dfe1e)) - yuxuetr
- **(phonon-mcp)** MCP server exposing audio tools to LLM agents - ([7e091ff](https://github.com/yuxuetr/phonon/commit/7e091fff824acf1240e1b691ff481ecb71ee43dc)) - yuxuetr
- **(phonon-podcast)** integrate AI sound gen into the pipeline - ([70fdba4](https://github.com/yuxuetr/phonon/commit/70fdba422a27cd64a761ca8d18420ad531f5a363)) - yuxuetr
- **(phonon-podcast)** Episode bundle + format-specific export - ([6afad4e](https://github.com/yuxuetr/phonon/commit/6afad4ed9c4dcb06174f96314ded6427f6fd69f0)) - yuxuetr
- **(phonon-server)** add /api/lufs and /api/podcast/episode - ([5c38509](https://github.com/yuxuetr/phonon/commit/5c38509d3775792cc745a83655ee59ad9a5923fa)) - yuxuetr
- **(phonon-studio)** plugin browser side-panel - ([f20f6e9](https://github.com/yuxuetr/phonon/commit/f20f6e90a231e3d9dac2cb68b912a2a41037a6b3)) - yuxuetr
- add multi-band parametric EQ (Phase 1) - ([7e5b426](https://github.com/yuxuetr/phonon/commit/7e5b426766d5dc1c5d29e6bcd92d65f69da1ac90)) - yuxuetr
- Studio UI overhaul (Phase 2) - ([7092a41](https://github.com/yuxuetr/phonon/commit/7092a4173e772692ca17d542e8cbc43868d875d9)) - yuxuetr
- Wasm support — feature-gate rayon in phonon-core, add phonon-wasm crate - ([30cc588](https://github.com/yuxuetr/phonon/commit/30cc588c37b4e8889a2c0e825747f2dbfa01b919)) - yuxuetr
- plugin architecture — DspPlugin trait, PluginRegistry, built-in plugins - ([e449d00](https://github.com/yuxuetr/phonon/commit/e449d001b3feb5c6286a1b2f87c97faba49157c2)) - yuxuetr
- bridge DspPlugin into AudioEffect via PluginAdapter - ([dd5131f](https://github.com/yuxuetr/phonon/commit/dd5131f5df74d047a8e8f2e4b731435f952fe5a6)) - yuxuetr

### Miscellaneous Chores

- add phonon-wasm/www/pkg/ to .gitignore - ([f734a9c](https://github.com/yuxuetr/phonon/commit/f734a9cade159381681c20ff169a92861ef1b842)) - yuxuetr

### Performance

- **(phonon-core)** add rayon parallel processing to DSP algorithms - ([0e32f2e](https://github.com/yuxuetr/phonon/commit/0e32f2e353326bb094d945ce8c9d516743e995ce)) - yuxuetr
- **(phonon-io)** stream encoding output directly to file - ([868f13f](https://github.com/yuxuetr/phonon/commit/868f13f41f12abf222d00724dd8fb6078b4dfce7)) - yuxuetr

### Style

- apply rustfmt (2-space indent) to phonon-core algorithms and benches - ([3a138ca](https://github.com/yuxuetr/phonon/commit/3a138ca5d04aeb1ffd55a334f8e7d0ef24217da6)) - yuxuetr
- rustfmt 2-space across workspace - ([1345d20](https://github.com/yuxuetr/phonon/commit/1345d20e1798fd6802cab7bfd6bb20ef408a8a98)) - yuxuetr

---
## [0.5.0](https://github.com/yuxuetr/phonon/compare/v0.4.0..v0.5.0) - 2026-02-23

### Features

- v0.5.0 — CLI expansion, CI/CD, integration tests, READMEs, Studio enhancement, doctests - ([9432293](https://github.com/yuxuetr/phonon/commit/94322932d387f1da4a9bd3b31294cee98ce2c672)) - yuxuetr

---
## [0.4.0](https://github.com/yuxuetr/phonon/compare/v0.3.0..v0.4.0) - 2026-02-23

### Features

- v0.4.0 — benchmarks, STT, subtitles, chapters, multi-lang TTS, streaming, video, sound gen, server, publish prep - ([5438119](https://github.com/yuxuetr/phonon/commit/54381193b56902cdfadff93b8f35b5f14ca023ae)) - yuxuetr

### Miscellaneous Chores

- unify workspace dependencies - ([e8e96d6](https://github.com/yuxuetr/phonon/commit/e8e96d6eb6015b3651eaabf30d1f31bf2c671dd7)) - yuxuetr

---
## [0.3.0](https://github.com/yuxuetr/phonon/compare/v0.2.0..v0.3.0) - 2026-02-23

### Features

- complete v0.3 — podcast Agent full chain - ([69a8768](https://github.com/yuxuetr/phonon/commit/69a8768161f62a4a75435f046fd8b4ad79ed319f)) - yuxuetr

---
## [0.2.0](https://github.com/yuxuetr/phonon/compare/v0.1.0..v0.2.0) - 2026-02-23

### Features

- complete v0.2 — MP3 output, pydub parity, Edge TTS, concurrent synthesis - ([e96fb44](https://github.com/yuxuetr/phonon/commit/e96fb447e0bd197535ca9df36ddb64d3fb234171)) - yuxuetr

---
## [0.1.0] - 2026-02-23

### Bug Fixes

- **(studio)** handle sample rate mismatch in playback - ([ccf7a98](https://github.com/yuxuetr/phonon/commit/ccf7a98b6d72c77b32a29eb5dcef6652317fc8d0)) - yuxuetr

### Documentation

- add project constitution, design docs and guidelines - ([34f3251](https://github.com/yuxuetr/phonon/commit/34f3251b3da524e7b2077a763044e3bbcb133731)) - yuxuetr

### Features

- **(io)** support multi-format audio reading via symphonia - ([62beed7](https://github.com/yuxuetr/phonon/commit/62beed757caa2e8efe722e135bdfb4cbede6f353)) - yuxuetr
- **(phonon-studio)** initial release of Dioxus GUI with WAV loader - ([0670e2c](https://github.com/yuxuetr/phonon/commit/0670e2cd2c6b892000273a1b9e4af68f5d8acace)) - yuxuetr
- **(studio)** implement audio waveform visualization - ([2fdf2b8](https://github.com/yuxuetr/phonon/commit/2fdf2b8ead7f61fa32e39834f87f7f9ffe534b76)) - yuxuetr
- **(studio)** integrate audio playback with engine - ([e35b30d](https://github.com/yuxuetr/phonon/commit/e35b30dcaeb2db18db1301fbcbd7f0d75f243176)) - yuxuetr
- **(studio)** integrate Tailwind CSS V4 and optimize file loading - ([755901d](https://github.com/yuxuetr/phonon/commit/755901dee8cb803d974351a3cee9f8b937d85bfb)) - yuxuetr
- implement core audio engine, wav io and cli tools - ([357d871](https://github.com/yuxuetr/phonon/commit/357d871e515ac53bde900d5ecd3bb7166fb528b1)) - yuxuetr
- add tracing instrumentation across crates - ([8be385f](https://github.com/yuxuetr/phonon/commit/8be385fc7c7b8cc32335b4cd9ebfed45e56296d0)) - yuxuetr
- add podcast production pipeline for AI agent audio backend - ([dad6e63](https://github.com/yuxuetr/phonon/commit/dad6e63bf1f80459b704cd8268a8625b663e4854)) - yuxuetr

### Miscellaneous Chores

- add CI workflow, pre-commit config, and update .gitignore - ([79b4281](https://github.com/yuxuetr/phonon/commit/79b4281b5414a6fa3e8bb1d9ec0de6bcc03e6042)) - yuxuetr

### Other

- init workspace configuration and toolchain setup - ([b03cc1c](https://github.com/yuxuetr/phonon/commit/b03cc1c4532d6f82c91e6bd2254f4a936b71bb44)) - yuxuetr

### Style

- **(studio)** modernize UI with dark theme and responsive layout - ([dfa4690](https://github.com/yuxuetr/phonon/commit/dfa46902ed0aa8a3440ad31beef88d3f07a075da)) - yuxuetr
- **(studio)** migrate to Tailwind CSS - ([1426c07](https://github.com/yuxuetr/phonon/commit/1426c07ab2677d3a7166873690be66745672d092)) - yuxuetr

### Tests

- add audio samples for manual testing - ([057ebc5](https://github.com/yuxuetr/phonon/commit/057ebc5de855134e2df606717504c059503b7761)) - yuxuetr

<!-- generated by git-cliff -->
