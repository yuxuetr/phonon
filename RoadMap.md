# Phonon RoadMap

## 项目定位

**Phonon 是一个纯 Rust 音频库**,为"AI 播客 / AI 音频应用"提供从底层 DSP 到 LLM Tool Calling 的完整能力。

### 职责边界

| 职责 | 归属 |
|---|---|
| **音频处理 / TTS / STT / 混音 / 编解码** | ✅ 本项目 (phonon) |
| **音频功能的 MCP 暴露 / Tool 调用** | ✅ 本项目 (phonon-mcp) |
| **Markdown / HTML / RSS / URL 解析** | ❌ 另立项目 (data-source 项目) |
| **网页抓取 / 文档摘要 / 长文切分** | ❌ 另立项目 |
| **LLM Agent 编排 / 业务工作流** | ❌ 另立项目 |

数据源项目通过 MCP 协议消费 phonon 的能力,本项目专注于"声音"。

## 已交付能力(v0.1 → v0.7)

- **v0.1**: WAV 读写 + AudioBuffer
- **v0.2**: pydub 等价(overlay/reverse/repeat/dBFS)+ MP3 输出 + Edge TTS + 并发合成
- **v0.3**: 压缩器 + 信号发生器 + ElevenLabs + LLM 脚本 + 播放控制
- **v0.4**: STT (Whisper) + SRT + 章节 + 视频音轨 + Server + benchmarks
- **v0.5**: Lanczos sinc + FLAC/OGG/AAC + EffectChain + CI
- **v0.6**: 多频段 EQ + rayon 并行 + Studio UI + Wasm + Plugin 架构
- **v0.7**: LUFS + ID3/Vorbis 标签与章节 + Pan + TTS 缓存 + AI 资产接管线 + Episode 导出 + MCP server
- **v0.7.1**: 评估修复(安全 / 结果正确性 / 实时安全 / 抽象收敛),见 [docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md)
- **v0.7.2**: 评估遗留(阻塞工作移出 async / UI 线程、provider 重试与超时、引擎实时安全、
  真峰值相位、AAC 尾部、FLAC 回读、Edge TTS 恢复可用)
- **v0.7.3**: 架构复评(压缩器插件 ratio 改为 dB 域、滤波插件全声道、内存解码 `decode_audio`、脚本生成重试)
- **v0.8**: 模型 provider 可配置(MiniMax / Qwen / GLM / StepFun 预置 + OpenAI 兼容自部署;TTS 按字数、STT 按时长自动切分;播客分段按响度对齐),见 [docs/providers.md](./docs/providers.md)

## v0.7 计划 — AI 播客生产就绪 + MCP 暴露

### 设计目标

1. **音频输出达到播客平台合规标准**(LUFS / ID3 / Vorbis Comment / 章节)
2. **管线深度集成 AI 音效生成 + TTS 缓存**
3. **以 MCP 协议向上层 Agent 暴露所有能力**

### Phase A — 音频质量补齐

| ID | 模块 | 内容 | 输出 |
|---|---|---|---|
| **A1** | `phonon-core/algorithms/lufs.rs` | ITU-R BS.1770-4 / EBU R128 实现 | LUFS-I / S / M, LRA, dBTP, `normalize_lufs` |
| **A2** | `phonon-io/mp3_tags.rs` | id3 crate 集成 | `PodcastTags`, CHAP/CTOC 章节, `write_mp3_tagged` |
| **A3** | `phonon-io/tags.rs` | Vorbis Comment block | FLAC/OGG 通用元数据 |
| **A4** | `phonon-core/algorithms/pan.rs` | 等功率声场定位 | `pan_mono_to_stereo`, `pan_in_place` |

### Phase B — 管线深度集成

| ID | 模块 | 内容 |
|---|---|---|
| **B1** | `phonon-podcast/pipeline.rs` + `config.rs` | `BgmSource::{File, Generated, None}`,intro/outro 同样支持 AI 生成,自动 loop/trim |
| **B2** | `phonon-ai/cache.rs` | `CachingTts<T>` 装饰器,blake3 缓存键,可选 dir |
| **B3** | `phonon-podcast/export.rs` | `Episode { audio, chapters, srt, tags }` + `export_mp3 / export_wav` |

### Phase C — MCP 暴露层

**新增 `phonon-mcp` crate**,手写 JSON-RPC 2.0(未用 `rmcp`,避免其复杂度),stdio 传输。

#### Asset Registry
- `AudioHandle = Uuid`
- 内存 `HashMap<Uuid, Arc<AudioBuffer>>`
- 工具调用只在 LLM ↔ MCP 之间传句柄,不传大块音频数据

#### 工具集

| 类别 | Tools |
|---|---|
| **基础** | `audio_load`, `audio_save`, `audio_info`, `audio_list`, `audio_drop` |
| **DSP** | `audio_mix`, `audio_trim`, `audio_fade`, `audio_normalize`, `audio_normalize_lufs`, `audio_volume_db`, `audio_pan`, `audio_resample`(`audio_eq` / `audio_compress` 未实现) |
| **测量** | `audio_loudness`(返回 LUFS / peak / RMS / dBTP) |
| **AI** | `tts_synthesize`, `tts_list_voices`, `sound_generate` |
| **管线** | `podcast_generate`(脚本→episode 句柄)/ `podcast_export`(句柄→tagged mp3) |

每个 tool 的 JSON Schema 手写在注册处(未使用 `schemars`),LLM 可零配置发现并调用。
`audio_load` / `audio_save` 限定在 `PHONON_MCP_ROOT` 目录内。

## 不在本项目范围(数据源项目处理)

- 文档解析:Markdown / HTML / PDF / DOCX
- 内容获取:URL / RSS / 文件系统监听
- 长文工程:切分 / 摘要 / 主题抽取
- 脚本工程:从博客内容生成 `PodcastScript` 的业务逻辑(本项目仅提供 `ScriptGenerator` trait + 一个 OpenAI 实现作为参考,数据源项目可替换)

## 后续(v0.8+ 远期)

已完成:流式 TTS、EffectChain 统一到 `DspPlugin`(v0.7.1 删除了 `AudioEffect`)、wasm-pack demo。

- crates.io 暂不发布(`phonon` 已被无关项目占用,见 docs/publishing.md);v0.7.0 / v0.7.3 / v0.8.0 已打 tag
- VST3 / CLAP 兼容层 —— v0.7.1 删除了无 FFI 的 `phonon-plugin` 骨架;
  **重估条件**:出现需要加载 Phonon 插件的真实宿主

## 执行节奏

- 每个子任务完成后单独提交,提交信息符合 Conventional Commits
- 每完成一个子任务更新一次 `Todos.md`(TODO → DONE)
- 通过 `cargo test --workspace` 才标记完成
- 严禁 `unwrap()` / `expect()`(测试除外)

---

**最后更新**: 2026-10-20
