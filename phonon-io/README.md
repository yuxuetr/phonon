# phonon-io

Multi-format audio I/O for the Phonon audio engine.

## Supported Formats

**Read** (via Symphonia): WAV, MP3, FLAC, OGG, AAC, and more.

**Write**: WAV (16-bit PCM), MP3 (via LAME encoder with configurable bitrate).

## Usage

```rust
// Read any supported format
let buffer = phonon_io::read_audio("song.mp3")?;

// Write — format determined by extension
phonon_io::write_audio("output.wav", &buffer)?;
phonon_io::write_audio("output.mp3", &buffer)?;

// MP3 with custom bitrate
let config = phonon_io::Mp3Config { bitrate_kbps: 320 };
phonon_io::write_mp3("output.mp3", &buffer, &config)?;
```
