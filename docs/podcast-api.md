# Phonon Podcast API

从 JSON 脚本自动生成多人播客音频。

## 快速开始

### CLI 使用

```bash
# 单次 TTS
phonon tts --text "Hello world" --voice alloy --output hello.wav

# 生成完整播客
phonon podcast generate --script episode.json --output episode.wav

# 预览单段
phonon podcast preview --script episode.json --segment 0 --output preview.wav
```

### Rust API

```rust
use phonon_ai::OpenAiTts;
use phonon_podcast::{PodcastPipeline, PodcastScript};

let tts = OpenAiTts::new()?;
let script = PodcastScript::from_file("episode.json")?;
let pipeline = PodcastPipeline::new(tts);

// 生成到内存
let audio = pipeline.generate(&script).await?;

// 或直接写文件
pipeline.generate_to_file(&script, "output.wav").await?;
```

## 脚本格式

播客脚本是一个 JSON 文件，包含角色定义和对话段落：

```json
{
  "title": "EP01: Rust 与音频处理",
  "speakers": {
    "host": {
      "voice_id": "alloy",
      "speed": 1.0,
      "model": "tts-1-hd"
    },
    "guest": {
      "voice_id": "echo",
      "speed": 1.0,
      "model": "tts-1-hd"
    }
  },
  "segments": [
    { "speaker": "host", "text": "大家好，欢迎收听本期播客。" },
    { "speaker": "guest", "text": "谢谢邀请，很高兴来到这里。" },
    { "speaker": "host", "text": "今天我们来聊聊 Rust 在音频处理领域的应用。" }
  ],
  "config": {
    "pause_between_segments_secs": 0.5,
    "intro_audio": "assets/intro.wav",
    "outro_audio": "assets/outro.wav",
    "bgm_audio": "assets/bgm.wav",
    "bgm_volume": 0.08,
    "crossfade_secs": 0.1,
    "output_sample_rate": 44100,
    "output_channels": 1,
    "normalize_target": 0.95,
    "fade_in_secs": 1.0,
    "fade_out_secs": 2.0
  }
}
```

### 字段说明

**speakers**: 角色名 → 语音配置

| 字段 | 类型 | 默认值 | 说明 |
|---|---|---|---|
| voice_id | string | "alloy" | OpenAI 语音 ID: alloy, echo, nova, shimmer, fable, onyx |
| speed | float | 1.0 | 语速 (0.25 - 4.0) |
| model | string | "tts-1" | TTS 模型: tts-1 (快) 或 tts-1-hd (高质量) |

**config** (均可选，使用默认值):

| 字段 | 默认值 | 说明 |
|---|---|---|
| pause_between_segments_secs | 0.5 | 段落间停顿 |
| intro_audio | null | 片头音频路径 |
| outro_audio | null | 片尾音频路径 |
| bgm_audio | null | 背景音乐路径 |
| bgm_volume | 0.08 | BGM 音量 (0.0-1.0) |
| crossfade_secs | 0.0 | 段落间交叉淡化 |
| output_sample_rate | 44100 | 输出采样率 |
| output_channels | 1 | 输出声道数 |
| normalize_target | 0.95 | 最终归一化峰值 |
| fade_in_secs | 0.0 | 开头淡入 |
| fade_out_secs | 0.0 | 结尾淡出 |

## Pipeline 流程

1. **解析脚本** → 验证角色引用和文本内容
2. **TTS 合成** → 为每段文本调用 OpenAI TTS API
3. **后处理** → 重采样、声道转换、音量归一化
4. **组装** → 插入段落间停顿 → 交叉淡化拼接
5. **Intro/Outro** → 拼接片头片尾
6. **BGM 混音** → 降低 BGM 音量 → 循环适配长度 → 混音
7. **最终处理** → 全局归一化 → 淡入/淡出

## 环境变量

| 变量 | 说明 |
|---|---|
| OPENAI_API_KEY | OpenAI API 密钥 (必需) |

## 自定义 TTS Provider

实现 `TtsProvider` trait 即可使用自定义 TTS 服务：

```rust
use phonon_ai::{TtsProvider, VoiceConfig, AiError};
use phonon_core::AudioBuffer;

struct MyTts;

impl TtsProvider for MyTts {
    async fn synthesize(&self, text: &str, voice: &VoiceConfig) -> Result<AudioBuffer, AiError> {
        // 你的 TTS 实现
        todo!()
    }
}

let pipeline = PodcastPipeline::new(MyTts);
```
