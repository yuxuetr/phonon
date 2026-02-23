# Phonon Development Todos

## v0.2 — 核心短板补齐 ✅ 已完成

- [x] MP3 输出 (phonon-io: mp3lame-encoder, write_audio .mp3 路由, 128k-320k bitrate)
- [x] pydub 等效功能补齐 (phonon-core: overlay_at_position, reverse, repeat, loudness/dbfs/rms/adjust_volume_db)
- [x] VoiceConfig 情感扩展 (phonon-ai: emotion, style, language 字段)
- [x] Edge TTS Provider (phonon-ai: msedge-tts WebSocket, 免费无 API Key)
- [x] 并发 TTS 合成 (phonon-podcast: Semaphore + join_all, tts_concurrency 可配置, 默认 4)

## v0.3 — 播客 Agent 完整链路

- [ ] ElevenLabs TTS Provider (phonon-ai: 多角色、情感、voice cloning)
- [ ] LLM 脚本生成集成 (phonon-podcast: ScriptGenerator trait, 话题→PodcastScript)
- [ ] 动态范围压缩 (phonon-core: compressor — threshold/ratio/attack/release)
- [ ] 信号发生器 (phonon-core: Sine/Square/Sawtooth/WhiteNoise)
- [ ] phonon-engine 播放控制 (pause/resume/seek/volume, 进度回调)
- [ ] phonon-studio 播客面板 (脚本编辑, 生成进度条, 波形编辑)

## v0.4+ — 进阶与生态

- [ ] 流式 TTS (边合成边播放)
- [ ] STT 集成 (Whisper)
- [ ] SRT 字幕自动生成
- [ ] 多语言混合 TTS
- [ ] phonon-video: 视频音轨提取与替换
- [ ] AI 音效生成 (BGM/SFX)
- [ ] 播客自动章节标记
- [ ] Web API 服务 (REST/gRPC)
- [ ] Workspace dependencies 统一管理
- [ ] criterion 性能基准测试
- [ ] 发布到 crates.io
