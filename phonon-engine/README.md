# phonon-engine

Real-time audio playback engine for the Phonon audio engine.

## Features

- Play `AudioBuffer` through the default output device (via cpal)
- Pause / resume / stop / seek / volume
- **Streaming mode**: append samples while playing — `append_samples`,
  `finish_streaming`, plus the `stream::play_stream` helper that pipes
  an `mpsc::Receiver<AudioBuffer>` (e.g. from a TTS provider) live to
  the device
- Sample-rate / channel conversion helpers (`stream::convert_for_device`)
- **Real-time effects chain** unified with `phonon_core::DspPlugin`:
  - `add_plugin(Box::new(VolumePlugin::new(0.8)))` — preferred path
  - Inverse adapter `EffectAsPlugin` exposes any `AudioEffect` as a
    `DspPlugin` for plugin-shaped APIs

## Usage — basic playback

```rust,no_run
use phonon_engine::AudioPlayer;
use phonon_core::AudioBuffer;

# fn doc() -> Result<(), Box<dyn std::error::Error>> {
let player = AudioPlayer::new()?;
let buffer = AudioBuffer { samples: vec![0.0; 44100], sample_rate: 44100, channels: 1 };

player.play(&buffer)?;
player.pause();
player.resume();
player.seek(1.5)?;
player.set_volume(0.8);
player.stop();
# Ok(()) }
```

## Usage — real-time effects (preferred plugin path)

```rust,no_run
use phonon_engine::AudioPlayer;
use phonon_core::{LowPassPlugin, CompressorPlugin};

# fn doc() -> Result<(), Box<dyn std::error::Error>> {
let player = AudioPlayer::new()?;
player.add_plugin(Box::new(LowPassPlugin::new(4000.0)));
player.add_plugin(Box::new(CompressorPlugin::default()));
# Ok(()) }
```

## Usage — streaming TTS playback

```rust,no_run
use phonon_engine::{AudioPlayer, stream::play_stream};
use phonon_core::AudioBuffer;
use tokio::sync::mpsc;

# async fn doc() -> Result<(), Box<dyn std::error::Error>> {
let (tx, rx) = mpsc::channel::<AudioBuffer>(8);
let player = AudioPlayer::new()?;

// Spawn a task that pushes TTS chunks into `tx` (e.g. via
// `phonon_ai::TtsProvider::synthesize_streaming`).
play_stream(&player, rx).await?;
# Ok(()) }
```

## Requirements

Requires a working audio output device. On Linux, `libasound2-dev` must be installed.
