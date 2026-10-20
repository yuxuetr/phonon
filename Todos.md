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

## v0.7 — AI 播客生产就绪 + MCP 暴露 ✅ 已完成

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

## v0.7.1 — 评估修复 ✅ 已完成

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
- [x] **F11** `HttpClient` 等 Debug 输出隐藏 API key ✅
- [x] **F12** 删除未使用依赖(mcp → ai/podcast/schemars,studio → ai/podcast) ✅
- [x] **F13** 效果抽象收敛:删 phonon-plugin、deprecated 包装、`EffectAsPlugin` ✅ (同时删 AudioEffect/add_effect,EffectChain 直接持有 DspPlugin)
- [x] **F14** phonon-io 编码器 feature 开关(aac 默认关)+ docs.rs 元数据 ✅ (只给 aac 加 feature;mp3/flac/ogg 暂无关闭需求,README 注明 LAME LGPL)
- [x] **F15** 文档同步(AGENTS / README / RoadMap / CHANGELOG 0.7) ✅ (CHANGELOG 加手写 v0.7.1 段;v0.6/v0.7 tag 未打,需维护者决定发布点)

## v0.7.2 — 评估遗留 ✅

> 来源同 [docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md),v0.7.1 未覆盖的部分。

- [x] **G1** async 中的阻塞工作改 `spawn_blocking`(server 编码/解码/LUFS、pipeline 重采样与合并、各 provider 的 symphonia 解码、MCP DSP 工具)
  - DONE: pipeline 的 TTS 后处理整体走 `render()` + `spawn_blocking`;server 的 WAV 编码、LUFS、MP3 导出走阻塞池。
    未改:provider 的单段解码(每段几秒音频,毫秒级);MCP 请求串行处理,阻塞不影响其他请求——**重估条件**:MCP 改为并发处理请求
- [x] **G2** Edge TTS 连接/合成超时;MiniMax 限流与 Replicate 轮询的重试 + 退避;`HttpClient` 加 `connect_timeout`
  - DONE: `retry_transient`(3 次,500ms 起指数退避)用于 OpenAI/ElevenLabs/MiniMax/Edge 合成与 Replicate 轮询;MiniMax 1002/1039 → 429;Edge 改 async client + 60s 超时;所有 reqwest client 加 10s connect timeout
- [x] **G3** 测试中 `unsafe { env::remove_var }` 改为不依赖进程环境的构造方式(并行测试竞争)
  - DONE: 4 处构造函数统一走 `env_key()`;删除 3 个 `remove_var` 测试,改为对一个从不设置的变量名测试 `env_key`
- [x] **G4** 引擎:`append_samples` 不在锁内扩容;`ParametricEqPlugin` 不在 `process` 内重建 / 分配
  - DONE: samples 改 `RwLock`,扩容在读锁下拷贝、写锁只做交换;`ParametricEq::set_band` 原地换系数并保留滤波器状态(格式变化时仍重建)
- [x] **G5** Studio:EQ / 效果 / 播放前重采样 / 整轨 FFT 移出 UI 线程
  - DONE: `transform_audio` / `load_audio` 把处理与 waveform/FFT 放到 `spawn_blocking`;播放前的音量/重采样/声道转换同样移出。仅编译 + clippy 验证,GUI 未手动点测
- [x] **G6** 管线:`assemble_segments` 的 O(n²) 合并;`generate_episode` 改用并发 TTS(时间轴已按 span 计算,不再需要顺序合成)
  - DONE: 单个预分配 Vec 内追加 + 原地交叉淡化(与旧实现逐样本对拍);`synthesize_segments()` 供两个入口共用并发合成
- [x] **G7** LUFS 真峰值相位偏移(0.375/0.625/0.875 → 0.25/0.5/0.75)
  - DONE: sinc/窗中心改为 24;fs/4 中点 ISP 0.99 由 0.9766 (−0.12 dB 低读) → 0.9959
- [x] **G8** AAC 结束时未 flush 编码器尾部;`read_audio` 遇单个坏包即整体失败(应跳过 DecodeError)
  - DONE: AAC 按 `nDelay` 补静音帧冲出尾部(解码/输入能量比 0.872 → 0.997);解码错误跳过该包。顺带修复:FLAC STREAMINFO 最小块长导致 phonon 写出的 FLAC 无法被 `read_audio` 读回(d69a41d)。
    未覆盖:OGG 单页 CRC 损坏会在 symphonia 打开阶段失败——**重估条件**:symphonia 的 OggReader 支持容错打开
- [x] **G9** MCP:`AssetRegistry` 无容量上限;每个工具调用整块 clone PCM 只为构造元数据
  - DONE: 默认 1 GiB PCM 上限 + `RegistryFull`;9 处 `insert(buf.clone())` 改为 `store()` 先取元数据再 move
- [x] **G10** Replicate 轮询 / 下载未检查 HTTP 状态码;ElevenLabs `voice_id` 未转义直接拼进 URL
  - DONE: `ensure_success` 用于轮询与下载(下载也走重试);`tts_url` 以单个编码路径段放入 voice_id

## v0.7.3 — 架构项复评 ✅

> 对 [docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md) 架构 3–6 逐条重新核实(2026-10-20)。
> 只做有当前证据的项;不做的写明重估条件。

- [x] **H1** 压缩器插件的 `ratio` 在线性幅度域计算,不是压缩比(阈值 0.5、ratio 20、输入 1.0 → 输出高出阈值 0.42 dB,实际约 14:1;ratio 4 实际约 3:1),与 `compress()` 的 dB 域结果不一致
  - 做法:`compress()` 与 `CompressorPlugin` 共用 compressor.rs 中同一个逐帧增益计算;插件参数不变(threshold 仍为 0–1 线性,内部换算 dB)
  - DONE: 共用 `compress_frames`;0.25/4:1 稳态输出 −7.18 dB → −9.03 dB,插件与 `compress()` 逐样本一致(e43457d)。同样参数下插件压得比以前重
- [x] **H2** `LowPassPlugin` / `HighPassPlugin` 状态固定 2 声道,第 3 声道起原样通过
  - 做法:状态按声道数分配,格式变化时重置
  - DONE: 状态改为按声道数的 Vec(声道数变化时重建);3 声道测试中第 3 声道的 Nyquist 残留 0.5 → <0.1
- [x] **H3** 字节解码五份复制(4 个 TTS provider + Replicate + server),每份都先写临时文件再读
  - 做法:phonon-io 增加 `decode_audio(bytes, ext_hint)`,内存解码;`read_audio` 共用同一解码循环
  - DONE: `decode_audio` 与 `read_audio` 共用 `decode()`;wav/mp3/flac/ogg 四种格式字节解码与文件读取逐样本一致;6 处副本删除,Edge 联网测试通过
- [x] **H4** `filters::filter()` 自带一份与 `ParametricEq` 相同的 DF-I biquad 循环
  - 做法:`filter()` 改为单频段 `ParametricEq`
  - DONE: `filter()` 构造单频段 `ParametricEq`,删 40 行;3 种滤波器、3 声道输入下与旧循环逐位一致
- [x] **H5** `OpenAiScriptGenerator` 不走 G2 的重试(429 / 5xx 直接失败),且 LLM 错误被归为 `PodcastError::Tts`
  - 做法:公开 `phonon_ai::retry_transient` 并在脚本生成中使用;错误归为 `Script`
  - DONE: `retry_transient` 公开(基础延迟仍为内部常量);本地服务器先 429 后 200 的测试中请求数 1 → 2;新增 `HttpClient::with_client` / `OpenAiScriptGenerator::with_client`
- [x] **H6** phonon-cli 声明了未使用的 phonon-wav 依赖
  - DONE: 已删除

不做(当前版本):
- **lufs.rs 的 biquad**:系数按 BS.1770 规范直接给出(DF-II 转置),与 RBJ cookbook 不同;可共享的只有 3 行差分方程。
  重估条件:出现第三个 DF-II 用户
- **phonon-wav 并入 phonon-io**:`write_wav` 本身就是 phonon-io 的 WAV 编码器,不是重复;`read_wav` 唯一调用方是 TTS 缓存,读取的是自己写出的 16-bit 文件,能力缺口不会触发。
  重估条件:`grep -rn "phonon_wav::read_wav" --include='*.rs' . | grep -v cache.rs` 有输出
- **AI trait RPITIT → dyn**:server 仍只用 Edge TTS(无需 API key)。重估条件:server 需要按请求或配置切换 provider
- **server 限流**:默认只监听 127.0.0.1。重估条件:默认监听地址改为非回环,或文档推荐公网部署
