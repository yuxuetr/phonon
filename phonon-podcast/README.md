# phonon-podcast

Podcast generation pipeline for the Phonon audio engine.

## Features

- JSON script → multi-speaker podcast audio generation
- Concurrent TTS synthesis with configurable concurrency
- SRT subtitle generation from STT output
- Automatic chapter markers (silence detection or speaker switches)
- Per-segment language override for multi-language podcasts
- LLM-powered script generation (`ScriptGenerator` trait)

## Usage

```rust
use phonon_ai::EdgeTts;
use phonon_podcast::{PodcastPipeline, PodcastScript};

let script = PodcastScript::from_file("episode.json")?;
let tts = EdgeTts::new();
let pipeline = PodcastPipeline::new(tts);
pipeline.generate_to_file(&script, "output.wav").await?;
```

## Script Format

```json
{
  "title": "My Podcast",
  "speakers": {
    "Alice": { "voice_id": "en-US-AriaNeural" },
    "Bob": { "voice_id": "en-US-GuyNeural" }
  },
  "segments": [
    { "speaker": "Alice", "text": "Welcome!" },
    { "speaker": "Bob", "text": "Thanks for having me." }
  ]
}
```
