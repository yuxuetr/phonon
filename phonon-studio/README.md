# Phonon Studio

Desktop audio processing application built with Dioxus.

## Features (Current)

- Open WAV files
- Display audio file information (sample rate, channels, duration)

## Dependencies

- [Dioxus](https://dioxuslabs.com/) 0.7 - Desktop GUI framework
- [rfd](https://github.com/PolyMeilex/rfd) - Native file dialogs
- phonon-wav - WAV file reading
- phonon-core - Audio algorithms
- phonon-engine - Audio playback

## Running

```bash
cargo run -p phonon-studio
```

Or with Dioxus CLI for hot reload:

```bash
dx serve --platform desktop
```

## Architecture

The app uses Dioxus signals for state management:
- `audio_state` - Loaded audio file information
- `status_msg` - Status messages for user feedback

## Development

See [Todos.md](./Todos.md) for development roadmap.
