# Phonon Studio API

## Data Structures

### `LoadedAudio`
```rust
#[derive(Clone, Debug, PartialEq)]
struct LoadedAudio {
    path: String,
    sample_rate: u32,
    channels: u16,
    duration: f64,
    sample_count: usize,
}
```

Represents metadata of a loaded audio file.

## Components

### `App`
Main application component.

**State:**
- `audio_state: Signal<Option<LoadedAudio>>` - Currently loaded audio file
- `status_msg: Signal<String>` - Status message for the user

**Interactions:**
- Click "Open WAV File" → Open file dialog → Load audio file → Update state
