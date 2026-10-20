# Phonon v0.7 系统评估

**评估日期**: 2026-10-20
**范围**: workspace 全部 14 个 crate、文档、CI、发布准备
**任务清单**: 见 [Todos.md](../Todos.md) 的 v0.7.1 段
**状态**: P0 / P1 / P2、架构 1–2 及架构 5 中的未用依赖已在 v0.7.1(F1–F15)与 v0.7.2(G1–G10)处理;
架构 3–6 于 v0.7.3(H1–H6)复评:压缩器插件、滤波插件声道、字节解码副本、`filter()` 副本、
脚本生成重试已处理;lufs biquad、phonon-wav 合并、RPITIT 保留,重估条件见 Todos.md。

## 结论

分层方向正确（`phonon-core` 无内部依赖、无 I/O；依赖单向向下），单元测试覆盖面广。
但评估时 **main 的 CI 是红的**，**HTTP Server 存在本地文件读取漏洞**，**实时引擎回调不是实时安全的**，
且有若干 DSP **结果错误**的 bug。「v0.7 可发布 crates.io」的状态描述偏乐观。

## 基线数据（评估时实测）

| 项 | 结果 |
|---|---|
| `cargo build --workspace` | 通过 |
| `cargo test --workspace` | 448 passed / 0 failed / 4 ignored（均为联网测试） |
| `cargo clippy --workspace --all-targets -D warnings` | **失败**：phonon-core 9 个错误（rust 1.99 新 lint） |
| GitHub CI 最近一次 | **失败**（run 25058265677） |
| `cargo fmt --check` | 通过 |
| 生产代码 unwrap/expect | `phonon-mcp/src/registry.rs` 6 处、`phonon-server/src/main.rs` 2 处 |
| 代码规模 | ≈22k 行 Rust（core 7.2k / ai 3.2k / podcast 2.9k） |

## P0 — 安全与 CI

1. **Server 任意文件读取**：`PodcastScript` 中的 `bgm_audio` / `intro_audio` / `outro_audio` /
   `AssetSource::File{path}` 直接走 `phonon_io::read_audio`（`phonon-podcast/src/pipeline.rs`），
   解码结果混入返回音频；错误信息回显路径，可探测文件存在性。叠加默认 `0.0.0.0` 绑定、无鉴权、
   `CorsLayer::permissive()`，任意网页可借开发者浏览器读取本机音频文件。
2. **Server 无输入上界**：`output_sample_rate`、停顿/淡入淡出时长、segment 数、文本长度均不受限，
   单个请求可触发巨量分配。`Json<PodcastScript>` 绕过了 `PodcastScript::validate()`。
3. **MCP 无路径沙箱**：`audio_load` / `audio_save` 读写任意路径，参数来自 LLM。
4. **CI 红**：clippy 新 lint；仓库内 `.cargo/config.toml` 写死 `/Users/hal/.target`。

## P1 — 结果错误

| # | 位置 | 问题 |
|---|---|---|
| 5 | `phonon-podcast/src/pipeline.rs` BGM loop | BGM 为空时 `take == 0`，死循环 |
| 6 | `phonon-core/src/algorithms/resample.rs` | 降采样时 Lanczos 核支撑被截断到 ±a 个源样本（应为 ±a·ratio），抗混叠远弱于标称 |
| 7 | `phonon-core/src/algorithms/simd_ops.rs` | 非 x86_64/aarch64 架构只处理尾部 0–3 个样本：mix 静音、normalize/fade/pan 无效 |
| 8 | `phonon-engine/src/lib.rs` 回调 | 回调内阻塞等待 `Mutex`，控制线程持锁做整轨 clone/扩容/释放 → 优先级反转、爆音 |
| 9 | `phonon-ai/src/stt.rs` | `#[derive(Default)]` 使 `timestamps=false`，与文档和 `new()` 相反，`None` 时无分段 |
| 10 | `phonon-podcast/src/pipeline.rs` 章节计时 | 未计入 intro 时长与 crossfade 重叠，章节/SRT 整体偏移 |
| 11 | `compressor.rs` / `CompressorPlugin` | 交错流上按 `sample_rate` 算时间常数，立体声 attack/release 减半 |
| 12 | `phonon-engine` `seek()` | 未对齐帧边界，立体声可能左右互换 |
| 13 | trim / spectrum / resample | `channels == 0` 除零 panic；`read_audio` 无解码包时返回 `channels=0` |

## P2 — 工程质量

- async 上下文直接做 DSP/编码/文件 I/O，仅一处 `spawn_blocking`；Edge TTS 无超时，provider 无重试。
- `HttpClient` 派生 `Debug` 含 `api_key`，可被日志打印。
- 测试中 `unsafe { env::remove_var }` 与并行测试竞争。
- Studio 的 EQ/效果/重采样/整轨 FFT 在 UI 线程同步执行。

## 架构

1. **phonon-plugin 是空壳**：无 FFI，`clap`/`vst3` feature 为空，无人依赖。153 行中实际逻辑约 35 行。
   当前版本应删除；**重估条件**：出现需要加载 Phonon 插件的真实宿主。
2. **四套重叠的效果抽象**：`AudioEffect`、`DspPlugin`、双向 adapter（`EffectAsPlugin` 无使用者）、
   phonon-plugin；另有 5 个零使用者的 `#[deprecated]` 包装。收敛为 `EffectChain` 直接持有 `Box<dyn DspPlugin>`。
3. **DSP 重复**：biquad 三份（filters / eq / lufs）、压缩器两份且语义不同、`decode_audio_bytes` 五份。
4. **phonon-wav 与 phonon-io 重复**，`read_wav` 能力更弱。
5. **边界与 RoadMap 冲突**：phonon-podcast 含完整 OpenAI chat 客户端；phonon-mcp 依赖 ai/podcast
   仅为两个未用的 `From` impl；`schemars`、studio 的 ai/podcast 依赖未使用。
6. AI trait 采用 RPITIT，无法 `dyn`，Server 只能写死 `EdgeTts`。当前版本不改；
   **重估条件**：Server 需要运行时可配置 provider。

## 发布准备

- native 编码器全部强制依赖：LAME 为 **LGPL-3.0** 静态链接；fdk-aac 内嵌库为非 OSI 许可证。
  应在 phonon-io 引入 feature 开关，`aac` 默认关闭。
- docs.rs 需要 `[package.metadata.docs.rs]`（cpal 需 alsa、LAME 需 autotools）。
- CHANGELOG 缺 0.6 / 0.7 段，tag 停在 v0.5.0。

## 文档漂移

AGENTS.md（crate 数、测试数、已删除的 DESIGN.md、过期的「下一步」）、README（缺 phonon-plugin/MiniMax、
REST 路由 4/6、wasm「全部算法」不实、`wasm-pack -p` 命令错误）、RoadMap（`rmcp`/`schemars`/未实现的
`audio_eq`/`audio_compress`）、Todos（不存在的 studio 播客面板）。
