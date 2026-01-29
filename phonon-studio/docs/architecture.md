# Phonon Studio Architecture

Phonon Studio is a Dioxus-based desktop application for audio processing.

## Tech Stack
- **Framework**: Dioxus (Desktop)
- **State Management**: Signals (Dioxus 0.6+)
- **Audio IO**: phonon-wav (local crate)
- **Audio Core**: phonon-core (local crate)
- **File Dialogs**: rfd

## Data Flow
1. User clicks "Open WAV File".
2. `rfd` opens a native file dialog (async).
3. On file selection, `phonon_wav::read_wav` loads the file into an `AudioBuffer`.
4. Metadata is extracted and stored in `audio_state` Signal.
5. UI updates to show the file info.

## Future Plans
- Integrate `phonon-engine` for real-time playback.
- Use `wgpu` or HTML5 Canvas for waveform visualization.
