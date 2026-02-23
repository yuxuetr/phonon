# phonon-ai

AI-powered audio integrations for the Phonon audio engine.

## Providers

### Text-to-Speech (TTS)

| Provider | Struct | API Key Required |
|---|---|---|
| OpenAI | `OpenAiTts` | `OPENAI_API_KEY` |
| Microsoft Edge | `EdgeTts` | No (free) |
| ElevenLabs | `ElevenLabsTts` | `ELEVENLABS_API_KEY` |

All providers implement the `TtsProvider` trait with `synthesize()` and optional `synthesize_streaming()`.

### Speech-to-Text (STT)

- `WhisperStt` — OpenAI Whisper API with timed segment output

### Sound Generation

- `HttpSoundGen` — Generic HTTP provider for music/SFX generation APIs

## Usage

```rust
use phonon_ai::{EdgeTts, TtsProvider, VoiceConfig};

let tts = EdgeTts::new();
let voice = VoiceConfig::new("en-US-AriaNeural");
let audio = tts.synthesize("Hello!", &voice).await?;
```
