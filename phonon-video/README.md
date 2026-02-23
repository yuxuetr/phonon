# phonon-video

Video audio track extraction and replacement for the Phonon audio engine.

Wraps the `ffmpeg` / `ffprobe` CLI tools.

## Features

- Extract audio from video files into `AudioBuffer`
- Replace a video's audio track with a new one
- Probe stream metadata (codecs, sample rate, channels, duration)

## Requirements

`ffmpeg` and `ffprobe` must be available on `$PATH`.

## Usage

```rust
use phonon_video::{extract_audio, replace_audio, probe_streams};

// Extract audio
let buffer = extract_audio("input.mp4", None).await?;

// Replace audio
replace_audio("input.mp4", "new_audio.wav", "output.mp4").await?;

// Probe
let streams = probe_streams("input.mp4").await?;
```
