# phonon-core

Core audio buffer and DSP algorithms for the Phonon audio engine.

## Features

- `AudioBuffer` — unified audio representation (`Vec<f32>`, interleaved, `[-1.0, 1.0]`)
- **19 algorithm modules**: mix, merge, trim, crossfade, overlay, resample, channel conversion, normalize, volume, fade in/out, reverse, repeat, speed change, biquad filters, FFT spectrum, dynamic range compressor, signal generators, loudness measurement (RMS / peak / dBFS), silence detection

## Usage

```rust
use phonon_core::{algorithms, AudioBuffer};

let mut buffer = AudioBuffer {
    samples: vec![0.5; 44100],
    sample_rate: 44100,
    channels: 1,
};

// Normalize to 0.9 peak
algorithms::normalize(&mut buffer, 0.9);

// Generate a 440 Hz tone
let tone = algorithms::generate_tone(440.0, 1.0, 44100)?;
```

## Error Handling

All fallible operations return `phonon_core::Result<T>` with `PhononError` variants for IO, format, parameter, and channel mismatch errors.
