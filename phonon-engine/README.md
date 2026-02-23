# phonon-engine

Real-time audio playback engine for the Phonon audio engine.

## Features

- Play `AudioBuffer` through the default output device (via cpal)
- Pause / resume / stop / seek controls
- Volume adjustment (0.0–2.0)
- Streaming mode: append samples while playing
- Automatic sample rate and channel matching helpers
- **Real-time effects chain**: `AudioEffect` trait with built-in effects — `VolumeEffect`, `LowPassEffect`, `HighPassEffect`, `CompressorEffect`

## Usage

```rust
use phonon_engine::AudioPlayer;
use phonon_core::AudioBuffer;

let player = AudioPlayer::new()?;
let buffer = AudioBuffer { samples: vec![0.0; 44100], sample_rate: 44100, channels: 1 };

player.play(&buffer)?;
player.pause();
player.resume();
player.seek(1.5)?;
player.set_volume(0.8);
player.stop();

// Add real-time effects
use phonon_engine::effects::{LowPassEffect, CompressorEffect};
player.add_effect(Box::new(LowPassEffect::new(4000.0, 44100)));
player.add_effect(Box::new(CompressorEffect::voice()));
```

## Requirements

Requires a working audio output device. On Linux, `libasound2-dev` must be installed.
