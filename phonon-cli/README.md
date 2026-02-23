# phonon-cli

Command-line interface for the Phonon audio engine.

## Commands

| Command | Description |
|---|---|
| `info` | Inspect audio file (duration, sample rate, channels, peak, RMS, dBFS) |
| `play` | Play audio file through default device |
| `trim` | Trim audio to time range |
| `mix` | Mix two audio files |
| `normalize` | Normalize volume |
| `gen-tone` | Generate sine wave test tone |
| `filter` | Apply biquad filter (low-pass, high-pass, etc.) |
| `speed` | Change playback speed |
| `convert` | Convert between WAV and MP3 |
| `tts` | Text-to-speech (OpenAI) |
| `stt` | Speech-to-text (Whisper) |
| `subtitle` | Generate SRT subtitles from audio |
| `chapters` | Detect chapter markers via silence |
| `video extract` | Extract audio from video |
| `video replace` | Replace video audio track |
| `video probe` | Probe media streams |
| `podcast generate` | Generate podcast from script |
| `podcast preview` | Preview single segment |

## Usage

```bash
# Inspect a file
phonon info -f song.wav

# Convert WAV to MP3
phonon convert -i input.wav -o output.mp3 -b 320

# Detect chapters
phonon chapters -i podcast.wav --threshold-db -35 --min-silence 2.0
```
