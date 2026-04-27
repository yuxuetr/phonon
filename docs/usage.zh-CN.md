# Phonon 使用指南

> Phonon 音频引擎的实战手册，按"使用形态"组织。
> 英文版：[`usage.md`](./usage.md)。

| 章节 | 把 phonon 当成什么用 |
|---|---|
| [A. CLI 速查](#a-cli-速查) | 命令行工具 |
| [B. DSP 流水线（Rust）](#b-dsp-流水线rust) | 库 / 底层 DSP |
| [C. 播客管线（Rust）](#c-播客管线rust) | 库 / 一键出播客 |
| [D. 实时播放 + EQ](#d-实时播放--eq) | 库 / 播放引擎 |
| [E. REST 服务](#e-rest-服务) | HTTP 服务 |
| [F. MCP 服务](#f-mcp-服务) | LLM Agent 的工具后端 |
| [G. WebAssembly](#g-webassembly) | 浏览器内 DSP |
| [H. 一句话速查表](#h-一句话速查表) | 备忘录 |

---

## A. CLI 速查

所有子命令都通过 `cargo run -p phonon-cli -- <subcommand>` 调用；
release 安装后 `cargo install --path phonon-cli`，再用 `phonon …` 即可。

```bash
# 1. 看一个文件
phonon info --file song.wav

# 2. 生成 880 Hz、3 秒正弦
phonon gen-tone --output tone.wav --duration 3 --freq 880

# 3. 截取 1.5–4.0 秒
phonon trim --input song.wav --output cut.wav --start 1.5 --end 4

# 4. 两个文件叠加（采样率/通道一致）
phonon mix --input1 a.wav --input2 b.wav --output mixed.wav

# 5. 峰值归一到 0.95
phonon normalize --input cut.wav --output cut_norm.wav --target 0.95

# 6. 200 Hz 高通滤波
phonon filter --input voice.wav --output cleaned.wav \
              --type high-pass --cutoff 200

# 7. 变速（注意：基于重采样，会改变音高）
phonon speed --input voice.wav --output fast.wav --factor 1.25

# 8. WAV → MP3 192 kbps（按扩展名分发）
phonon convert --input voice.wav --output voice.mp3 --bitrate 192

# 9. TTS（OpenAI，需要 OPENAI_API_KEY）
phonon tts --text "你好，欢迎使用 phonon" --voice alloy --model tts-1-hd \
           --output hello.wav

# 10. STT 转录 + JSON 时间戳
phonon stt --input meeting.wav --language zh --json > meeting.json

# 11. 生成 SRT 字幕
phonon subtitle --input meeting.wav --output meeting.srt

# 12. 静音检测做章节
phonon chapters --input episode.wav --threshold-db -45 --min-silence 1.5

# 13. 视频音轨抽取/替换/查询
phonon video extract --input talk.mp4 --output talk.wav
phonon video replace --video talk.mp4 --audio dub.wav --output dubbed.mp4
phonon video probe   --input talk.mp4

# 14. 一键播客
phonon podcast generate --script ep01.json --output ep01.wav
phonon podcast preview  --script ep01.json --segment 0 --output preview.wav
```

---

## B. DSP 流水线（Rust）

典型语音清洗链：读取 → 高通去隆隆 → 压缩 → LUFS 归一 → 头尾淡变 → 写出。

```rust
use phonon_core::{
  algorithms, normalize_lufs, with_compression, fade_in, fade_out,
  CompressorConfig,
};
use phonon_io::{read_audio, write_audio};

fn process_voice(input: &str, output: &str) -> anyhow::Result<()> {
  // 1. 读（支持 WAV/MP3/FLAC/OGG/AAC）
  let mut buf = read_audio(input)?;

  // 2. 80 Hz 高通去低频噪
  algorithms::filter(
    &mut buf,
    phonon_core::FilterType::HighPass,
    80.0, 0.707, 0.0,
  )?;

  // 3. 语音压缩预设（threshold ≈ -18 dB，3:1 等）
  let buf = with_compression(&buf, &CompressorConfig::voice());

  // 4. EBU R128 归一到 -16 LUFS / -1 dBTP（podcast 行业基准）
  let mut buf = buf;
  normalize_lufs(&mut buf, -16.0, -1.0);

  // 5. 头尾淡入淡出
  fade_in(&mut buf, 0.05);
  fade_out(&mut buf, 0.20);

  // 6. 按扩展名写出
  write_audio(output, &buf)?;
  Ok(())
}
```

如果只想拿 LUFS 报告：

```rust
let report = phonon_core::measure_loudness(&buf);
println!(
  "I={:.1} LUFS, S={:.1}, M={:.1}, LRA={:.1} LU, dBTP={:.2}",
  report.integrated_lufs, report.short_term_max_lufs,
  report.momentary_max_lufs, report.range_lu, report.true_peak_dbtp,
);
```

---

## C. 播客管线（Rust）

完整链路：带缓存的 TTS + AI 生成 BGM + 多人拼装 + 章节 + SRT + tagged MP3。

```rust
use std::path::PathBuf;
use phonon_ai::{CachingTts, OpenAiTts, ReplicateSoundGen};
use phonon_core::normalize_lufs;
use phonon_io::{Mp3Config, PodcastTags};
use phonon_podcast::{PodcastPipeline, PodcastScript};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  // 1. TTS：OpenAI + 文件级 blake3 缓存。同样 (text, voice) 直接命中
  let tts = CachingTts::new(OpenAiTts::new()?, PathBuf::from("./.tts-cache"));

  // 2. 音效生成（Replicate 异步轮询；脚本里 bgm.kind == "generated" 时被调用）
  let sg = ReplicateSoundGen::from_env()?;          // REPLICATE_API_TOKEN

  // 3. 装配管线
  let pipeline = PodcastPipeline::new(tts).with_sound_gen(sg);

  // 4. 加载脚本
  let script = PodcastScript::from_file("ep01.json")?;

  // 5. 一次产出 audio + chapters + SRT
  let mut ep = pipeline.generate_episode(&script).await?;

  // 6. 行业级响度归一（导出前必做）
  normalize_lufs(&mut ep.audio, -16.0, -1.0);

  // 7. 元数据 + 导出 MP3（PodcastIndex CHAP/CTOC 章节自动写入）
  ep = ep.with_tags(PodcastTags {
    title:    Some("EP01：Rust × 播客".into()),
    artist:   Some("Phonon Studio".into()),
    album:    Some("Season 1".into()),
    year:     Some(2026),
    genre:    Some("Podcast".into()),
    language: Some("zho".into()),
    track:    Some(1),
    ..Default::default()
  });

  ep.export_mp3("ep01.mp3", &Mp3Config { bitrate_kbps: 192 })?;

  // 8. 字幕落盘
  if let Some(srt) = ep.srt.as_deref() {
    std::fs::write("ep01.srt", srt)?;
  }
  Ok(())
}
```

脚本（`ep01.json`）骨架：

```json
{
  "title": "EP01：Rust × 播客",
  "speakers": {
    "alice": { "voice_id": "alloy", "model": "tts-1-hd" },
    "bob":   { "voice_id": "echo",  "model": "tts-1-hd" }
  },
  "segments": [
    { "speaker": "alice", "text": "欢迎来到本期节目。" },
    { "speaker": "bob",   "text": "我们今天讨论 Rust 在音频处理中的应用。" },
    { "speaker": "alice", "text": "让我们开始吧！" }
  ],
  "config": {
    "pause_between_segments_secs": 0.5,
    "crossfade_secs": 0.05,
    "output_sample_rate": 44100,
    "output_channels": 1,
    "bgm": {
      "kind": "generated",
      "prompt": { "prompt": "soft ambient pad, 90 bpm" }
    },
    "bgm_volume": 0.06,
    "fade_in_secs": 1.0,
    "fade_out_secs": 2.0
  }
}
```

> 提示：`generate_episode()` 为了精确的章节时间戳走的是顺序 TTS。
> 如果只关心成片不关心章节/SRT，可以用 `pipeline.generate(&script).await`，
> 内部会按 `config.tts_concurrency`（默认 4）并发合成。

---

## D. 实时播放 + EQ

```rust
use phonon_core::{ParametricEqPlugin, EqBand, FilterType};
use phonon_engine::AudioPlayer;
use phonon_io::read_audio;

fn play_with_eq() -> anyhow::Result<()> {
  let buf = read_audio("song.wav")?;
  let player = AudioPlayer::new()?;     // 默认输出设备

  // 给播放链挂两段 EQ
  player.add_plugin(Box::new(ParametricEqPlugin::with_bands(vec![
    EqBand { filter_type: FilterType::HighShelf, freq_hz: 8000.0, q: 0.7, gain_db: 3.0 },
    EqBand { filter_type: FilterType::Peaking,   freq_hz: 200.0,  q: 1.0, gain_db: -2.0 },
  ])));

  player.play(&buf)?;
  player.set_volume(0.8);
  // pause / resume / seek / progress 任你用
  Ok(())
}
```

流式播放（边合成边播）：

```rust
let player = AudioPlayer::new()?;
while let Some(chunk) = next_chunk().await? {
  player.append_samples(&chunk)?;       // 第一次调用会自动 active
}
player.finish_streaming();              // 播完缓冲后自动停
```

---

## E. REST 服务

```bash
# 默认监听 0.0.0.0:3000
cargo run -p phonon-server

# 自定义端口
PHONON_BIND=127.0.0.1:8080 cargo run -p phonon-server
```

```bash
# 健康检查
curl http://localhost:3000/health

# TTS（默认 EdgeTTS，无需 API key）
curl -X POST http://localhost:3000/api/tts \
  -H 'Content-Type: application/json' \
  -d '{"text":"Hello phonon","voice":"en-US-AriaNeural"}' \
  --output hello.wav

# 一键播客（返回 audio/wav）
curl -X POST http://localhost:3000/api/podcast/generate \
  -H 'Content-Type: application/json' \
  -d @ep01.json --output ep01.wav

# 单段预览
curl -X POST http://localhost:3000/api/podcast/preview \
  -H 'Content-Type: application/json' \
  -d '{"script": <script-json>, "segment_index": 0}' \
  --output preview.wav
```

---

## F. MCP 服务

stdio 上跑的 JSON-RPC 2.0 服务，可被 Claude Desktop / Cursor 等任意
MCP 客户端直接调用。

```bash
cargo run -p phonon-mcp --bin phonon-mcp
# 或 cargo install --path phonon-mcp 后：
phonon-mcp
```

接入 Claude Desktop（`~/Library/Application Support/Claude/claude_desktop_config.json`）：

```json
{
  "mcpServers": {
    "phonon": {
      "command": "/Users/you/.cargo/bin/phonon-mcp",
      "args": []
    }
  }
}
```

随后即可对 Claude 说：

> 加载 `/tmp/voice.wav`，归一到 -16 LUFS，存到 `/tmp/voice_loud.mp3`。

它会串起 `audio_load → audio_normalize_lufs → audio_save`。

内置工具（14 个）：

| 类别 | 工具 |
|---|---|
| 基础 | `audio_load`, `audio_save`, `audio_info`, `audio_list`, `audio_drop` |
| DSP  | `audio_mix`, `audio_trim`, `audio_fade`, `audio_normalize`, `audio_normalize_lufs`, `audio_volume_db`, `audio_pan`, `audio_resample` |
| 测量 | `audio_loudness`（integrated / short-term / momentary LUFS、LRA、dBTP、peak、RMS、dBFS） |

设计要点：音频缓冲只以 `AudioHandle = UUID` 形式跨 JSON-RPC，
**PCM 数据从不出现在 wire 上**。

---

## G. WebAssembly

```bash
cargo install wasm-pack
wasm-pack build phonon-wasm --target web --out-dir www/pkg
python3 -m http.server -d phonon-wasm/www 8000
# 打开 http://localhost:8000/
```

JS 用法：

```js
import init, { process_audio } from "./pkg/phonon_wasm.js";
await init();

const ops = JSON.stringify([
  { op: "normalize" },
  { op: "fade_in",  duration_secs: 0.5 },
  { op: "fade_out", duration_secs: 0.5 },
  { op: "eq", bands: [
    { filter_type: "high_shelf", freq_hz: 8000, q: 0.7, gain_db: 3 }
  ]}
]);
const out = process_audio(samples, sampleRate, channels, ops);
```

`samples` 是 interleaved `Float32Array`；`phonon-wasm/www/index.html`
就是一个完整的 drag & drop + WebAudio 演示页。

---

## H. 一句话速查表

| 任务 | API |
|---|---|
| 读任意格式 | `phonon_io::read_audio(p)` |
| 写任意格式（按扩展名分发） | `phonon_io::write_audio(p, &buf)` |
| 16-bit PCM WAV 直写 | `phonon_wav::write_wav(p, &buf)` |
| 行业级响度归一 | `phonon_core::normalize_lufs(&mut buf, -16.0, -1.0)` |
| 测量 LUFS-I/S/M/LRA/dBTP | `phonon_core::measure_loudness(&buf)` |
| Mono → Stereo 等功率定位 | `phonon_core::pan_mono_to_stereo(&buf, +0.5)` |
| Lanczos sinc 重采样 | `phonon_core::resample(&buf, 48_000)` |
| 拼接 + 交叉淡变 | `phonon_core::crossfade(&a, &b, 0.5)` |
| MP3 + 章节 + 封面 + ID3 | `phonon_io::write_mp3_tagged(p, &buf, &cfg, &tags, &chapters)` |
| Tagged FLAC | `phonon_io::write_flac_tagged(p, &buf, &cfg, &tags)` |
| Tagged OGG | `phonon_io::write_ogg_tagged(p, &buf, &cfg, &tags)` |
| 播客脚本 → audio + chapters + SRT | `pipeline.generate_episode(&script).await` |
| 实时播放 | `AudioPlayer::new()?.play(&buf)?` |

---

## 环境与依赖

- **Rust 1.85+**（2024 edition）
- **ffmpeg / ffprobe** 在 `$PATH` 上（`phonon-video` 依赖）
- API key（只在使用对应 provider 时需要）：
  - `OPENAI_API_KEY` —— OpenAI TTS / Whisper STT / OpenAI 脚本生成
  - `ELEVENLABS_API_KEY` —— ElevenLabs TTS
  - `REPLICATE_API_TOKEN` —— `ReplicateSoundGen`
- Edge TTS 免费，无需任何 key。

## 许可证

双许可：MIT 或 Apache-2.0。
