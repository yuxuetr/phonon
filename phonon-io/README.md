# phonon-io

Multi-format audio I/O for the Phonon audio engine.

## Supported Formats

**Read** (via Symphonia): WAV, MP3, FLAC, OGG, AAC, and more.

**Write**: WAV (16-bit PCM), MP3 (LAME encoder), FLAC (lossless, pure Rust), OGG Vorbis (VBR quality), AAC (fdk-aac, ADTS transport).

MP3, OGG, and AAC encoders use streaming I/O (`BufWriter<File>`) to keep memory usage constant regardless of file size.

## Usage

```rust
// Read any supported format
let buffer = phonon_io::read_audio("song.mp3")?;

// Write — format determined by extension
phonon_io::write_audio("output.wav", &buffer)?;
phonon_io::write_audio("output.mp3", &buffer)?;
phonon_io::write_audio("output.flac", &buffer)?;
phonon_io::write_audio("output.ogg", &buffer)?;
phonon_io::write_audio("output.aac", &buffer)?;

// MP3 with custom bitrate
let config = phonon_io::Mp3Config { bitrate_kbps: 320 };
phonon_io::write_mp3("output.mp3", &buffer, &config)?;

// FLAC with 24-bit depth
let config = phonon_io::FlacConfig { bits_per_sample: 24 };
phonon_io::write_flac("output.flac", &buffer, &config)?;

// OGG Vorbis with custom quality
let config = phonon_io::OggConfig { quality: 0.8 };
phonon_io::write_ogg("output.ogg", &buffer, &config)?;

// AAC with custom bitrate
let config = phonon_io::AacConfig { bitrate_kbps: 192 };
phonon_io::write_aac("output.aac", &buffer, &config)?;
```
