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

## 下一步 — 待规划

- [ ] 流式 TTS (边合成边播放)
- [ ] AI 音效生成 (BGM/SFX)
- [ ] AudioEffect / DspPlugin 架构统一 (phonon-engine 适配 phonon-core plugin)
- [ ] wasm-pack 构建验证 + 在线 demo 部署
- [ ] Studio 插件浏览器面板
- [ ] SIMD 优化 (关键 DSP 路径)
- [ ] 发布到 crates.io
