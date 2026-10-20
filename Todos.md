# Phonon Development Todos

## v0.2 — 核心短板补齐 ✅ 已完成

- [x] MP3 输出 (phonon-io: mp3lame-encoder, write_audio .mp3 路由, 128k-320k bitrate)
- [x] pydub 等效功能补齐 (phonon-core: overlay_at_position, reverse, repeat, loudness/dbfs/rms/adjust_volume_db)
- [x] VoiceConfig 情感扩展 (phonon-ai: emotion, style, language 字段)
- [x] Edge TTS Provider (phonon-ai: msedge-tts WebSocket, 免费无 API Key)
- [x] 并发 TTS 合成 (phonon-podcast: Semaphore + join_all, tts_concurrency 可配置, 默认 4)

## v0.3 — 播客 Agent 完整链路 ✅ 已完成

- [x] 动态范围压缩 (phonon-core: compressor — threshold/ratio/attack/release, 预设 voice/gentle/limiter)
- [x] 信号发生器 (phonon-core: Sine/Square/Sawtooth/WhiteNoise, generate_tone/generate_beep)
- [x] ElevenLabs TTS Provider (phonon-ai: voice_settings情感映射, 多模型支持)
- [x] LLM 脚本生成 (phonon-podcast: ScriptGenerator trait + OpenAiScriptGenerator, 话题→JSON脚本)
- [x] phonon-engine 播放控制 (pause/resume/stop/seek/volume, position/progress/state 查询)
- [x] phonon-studio 播客面板 (暂停/继续按钮, 依赖 phonon-podcast + phonon-ai)

## v0.4 — 进阶与生态 ✅ 已完成

- [x] STT 集成 (Whisper — phonon-ai: OpenAI Whisper with timed segments)
- [x] SRT 字幕自动生成 (phonon-podcast: subtitle module)
- [x] 多语言混合 TTS (phonon-podcast: multi-language segments)
- [x] phonon-video: 视频音轨提取与替换 (ffmpeg integration)
- [x] 播客自动章节标记 (phonon-podcast: chapters module)
- [x] Web API 服务 (phonon-server: axum REST API — /health, /api/tts, /api/podcast/*)
- [x] Workspace dependencies 统一管理 (workspace Cargo.toml)
- [x] criterion 性能基准测试 (phonon-core/benches/audio_algorithms.rs)

## v0.5 — 性能与质量 ✅ 已完成

- [x] Lanczos sinc 重采样 (phonon-core: resample_sinc windowed interpolation)
- [x] FLAC / OGG Vorbis / AAC 编码输出 (phonon-io: streaming encoders)
- [x] 实时音频效果链 (phonon-engine: AudioEffect trait + EffectChain)
- [x] phonon-cli 扩展, CI/CD, 集成测试, doctests

## v0.6 — Route B: 产品打磨 ✅ 已完成

- [x] 多频段参数化 EQ (phonon-core: eq.rs — EqBand, ParametricEq, presets)
- [x] rayon 并行处理 (phonon-core: 7 个算法文件，PAR_THRESHOLD 自动切换)
- [x] Studio UI 重构 (phonon-studio: 10 模块拆分, 频谱可视化, EQ 面板, 5 格式导出, 拖放)
- [x] Wasm 支持 (phonon-wasm: wasm-bindgen bindings + demo HTML, phonon-core parallel feature gate)
- [x] 插件架构 (phonon-core: DspPlugin trait, PluginParam, PluginRegistry, 5 内建插件)

## v0.7 — AI 播客生产就绪 + MCP 暴露 (进行中)

> 详细规划见 [RoadMap.md](./RoadMap.md)

### Step 0 — WIP 清理基线 ✅ 已完成
- [x] 提交 phonon-ai 新功能 (SoundPreset / ReplicateSoundGen / ElevenLabs PCM 流式)
- [x] 提交 phonon-core SIMD ops 模块
- [x] 提交 phonon-studio plugin browser 面板
- [x] 提交 workspace rustfmt 2-space 整理
- [x] cargo fmt --check + cargo test --workspace 双绿

### Phase A — 音频质量补齐
- [x] **A1** LUFS 响度 (phonon-core/algorithms/lufs.rs) — ITU-R BS.1770-4, K-weighting, gating, LUFS-I/S/M, LRA, 真峰值 dBTP, normalize_lufs ✅
- [x] **A2** MP3 ID3v2 标签 + 章节 (phonon-io/mp3_tags.rs) — id3 crate, PodcastTags, CHAP/CTOC frames, write_mp3_tagged ✅
- [x] **A3** FLAC / OGG Vorbis Comment (phonon-io/tags.rs) — 通用元数据写入 ✅
- [x] **A4** 立体声 Pan (phonon-core/algorithms/pan.rs) — 等功率法则,pan_mono_to_stereo + pan_in_place ✅

### Phase B — 管线深度集成
- [x] **B2** TTS 结果缓存 (phonon-ai/cache.rs) — CachingTts 装饰器,blake3 hash 键,文件缓存 ✅
- [x] **B1** AI 音效生成接入管线 (phonon-podcast/pipeline.rs) — AssetSource::{File, Generated},自动 loop/trim ✅
- [x] **B3** Episode 出口 (phonon-podcast/export.rs) — Episode { audio, chapters, srt, tags },export_mp3/export_wav ✅

### Phase C — MCP 暴露层
- [x] **C1** phonon-mcp crate ✅
  - [x] 基础工具 (audio_load/save/info/list/drop)
  - [x] DSP 工具 (mix/trim/fade/normalize/normalize_lufs/volume_db/pan/resample)
  - [x] 测量工具 (audio_loudness)
  - [ ] AI 工具 (tts_synthesize/tts_list_voices/sound_generate) — 留作下游项目接入(provider 配置在调用方)
  - [ ] 管线工具 (podcast_generate/podcast_export) — 留作下游项目接入

### 收尾 ✅ 已完成
- [x] 集成测试 (phonon-tests: 端到端"脚本→tagged-mp3"流程 + MCP load/normalize_lufs/save 流程) ✅
- [x] README / AGENTS.md 更新 ✅
- [x] Todos.md 全部 DONE 标记 ✅

> v0.7 总计:`cargo test --workspace` 377 个单元/集成测试全部通过,`cargo fmt --check` 干净。新增 9 个独立功能 commit + 4 个 WIP 清理 commit + 2 个文档 commit。

## 远期(v0.8+)

- [x] 流式 TTS (边合成边播放) — phonon-engine `stream::play_stream` + `convert_for_device`,串接 `TtsProvider::synthesize_streaming` ✅
- [x] AudioEffect / DspPlugin 架构统一 (phonon-engine 适配 phonon-core plugin) — 内置 effects 改为 DspPlugin 薄封装并 `#[deprecated]`,新增 `EffectAsPlugin` 反向适配器,README 改为推荐 `add_plugin` 路径 ✅
- [x] wasm-pack 构建验证 + 在线 demo 部署 — `phonon-wasm/build.sh` + README,CI `build-wasm` job 自动跑 wasm-pack,`.github/workflows/wasm-pages.yml` 手动触发部署到 GitHub Pages ✅
- [x] SIMD 优化扩展 (其他热点 DSP 路径) — 新增 `simd_sum_sq` (loudness::rms / LUFS K-weighted) 与 `simd_mul_interleaved_pair` (pan_mono_to_stereo / pan_stereo_in_place),NEON + SSE2 + 标量 fallback,9 个新 SIMD vs 标量等价性测试 ✅
- [x] 发布到 crates.io — workspace version 0.1.0 → 0.7.0,补全各 crate description/keywords/categories/authors,新增 `scripts/publish-crates.sh` (按依赖顺序 dry-run/--execute) 与 `docs/publishing.md`,`cargo publish -p phonon-core --dry-run` 通过 ✅
- [x] VST3 / CLAP 插件兼容层 — 新增 `phonon-plugin` crate (`publish=false`),`PluginManifest` / `ParamInfo` / `PluginExport` / `ExportedPlugin` 把任何 `DspPlugin` 转成 host-friendly 描述符,`clap`/`vst3` 模块按 feature gate 提供工厂入口骨架,README 记录 SDK 选型与后续接入步骤。10 个测试全过 ✅

## v0.7.1 — 评估修复 (进行中)

> 来源:[docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md)。每项一个 commit,先写能红的测试再修。

### P0 — 安全与 CI
- [x] **F1** CI 转绿:修 clippy `-D warnings` 错误;`.cargo/config.toml` 移出版本库 ✅ (另加 CI doctest 步骤)
- [x] **F2** Server 加固:默认绑定 127.0.0.1、CORS 收紧、拒绝请求体内的文件路径资源、参数上界 + `validate()`、显式 body limit、去掉 `expect` ✅ (CORS 直接移除;并发/限流未做,留给反向代理)
- [x] **F3** MCP 路径沙箱:`PHONON_MCP_ROOT` 限定 load/save 根目录;registry 去掉 `expect` ✅ (另加 audio_save overwrite 保护)

### P1 — 结果错误
- [x] **F4** 管线:BGM 为空死循环;章节/SRT 计入 intro 与 crossfade ✅
- [x] **F5** Whisper `TranscribeOptions::default()` 默认开启 timestamps ✅
- [x] **F6** 重采样降采样时核支撑按 ratio 扩展 ✅ (10 kHz@48→16k 泄漏 -26 → -64 dB)
- [x] **F7** SIMD 在非 x86_64/aarch64 架构的标量 fallback ✅ (wasm32-wasip1 + node 实跑验证;CI 加 wasm32 lint)
- [x] **F8** 压缩器(compressor + CompressorPlugin)按帧计算时间常数 ✅ (改为声道联动,同帧同增益)
- [x] **F9** `channels == 0` 防护:相关算法返回错误;`read_audio` 无解码数据时报错 ✅ (AudioBuffer 字段私有化未做,需跨全 workspace 重构)
- [x] **F10** 引擎:回调不阻塞(try_lock)、控制线程不在锁内分配/释放、`seek` 帧对齐 ✅ (遗留:append_samples 锁内扩容、ParametricEqPlugin 回调内重建)

### P2 — 质量与架构
- [ ] **F11** `HttpClient` 等 Debug 输出隐藏 API key
- [ ] **F12** 删除未使用依赖(mcp → ai/podcast/schemars,studio → ai/podcast)
- [ ] **F13** 效果抽象收敛:删 phonon-plugin、deprecated 包装、`EffectAsPlugin`
- [ ] **F14** phonon-io 编码器 feature 开关(aac 默认关)+ docs.rs 元数据
- [ ] **F15** 文档同步(AGENTS / README / RoadMap / CHANGELOG 0.7)
