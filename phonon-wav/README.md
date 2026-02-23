# phonon-wav

WAV file reading and writing for the Phonon audio engine.

## Features

- Read 16-bit PCM WAV files into `AudioBuffer`
- Write `AudioBuffer` to 16-bit PCM WAV files
- Uses the `hound` crate internally

## Usage

```rust
use phonon_core::AudioBuffer;

// Write
let buffer = AudioBuffer {
    samples: vec![0.0; 44100],
    sample_rate: 44100,
    channels: 1,
};
phonon_wav::write_wav("output.wav", &buffer)?;

// Read
let loaded = phonon_wav::read_wav("output.wav")?;
```

## Note

For multi-format support (MP3, FLAC, OGG, etc.), use `phonon-io` instead.
