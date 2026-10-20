# AGENTS.md - Phonon AI Agent 开发指南

本文档为 AI Agent (如 Warp AI、Cursor、GitHub Copilot 等) 提供 Phonon 项目的开发上下文和指导。

## ⚠️ 核心法则 (Constitution)

**注意**: 本项目严格遵循 [CONSTITUTION.md](./CONSTITUTION.md) 中的规定。在编写任何代码前，必须查阅该文件。
关键要求包括：
- **严禁使用 `unwrap()` / `expect()`**
- **必须使用 Result 错误处理**
- **保持高内聚低耦合架构**

## 项目概述

**项目名称**: Phonon (声子音频引擎)  
**语言**: Rust  
**架构**: Workspace 多 crate 架构  
**目标**: 构建从底层到应用层的完整音频处理生态系统。

**音频格式支持策略**:
虽然 Phase 1 (v0.1) 仅关注 **WAV** 格式，但这只是为了快速启动和验证核心算法。
**Phonon 的最终目标是格式无关 (Format Agnostic)**。
- 内部表示 (IR)：统一使用 `Vec<f32>` (PCM)
- 输入支持：WAV / MP3 / AAC / FLAC / OGG 等,经 `symphonia` 解码
- 输出支持：支持编码为多种常见格式

## 核心原则

### 1. Rust 编码规范
- **严格遵守 Rust 2024 Edition 最佳实践**
- **遵守 CONSTITUTION.md 中的一切条款**
- **错误类型使用 `thiserror` 定义**
- **构建目标目录**: `/Users/hal/.target`（在 `.cargo/config.toml` 中配置）

### 2. 依赖管理原则
- 优先使用纯 Rust 实现的库
- 音频处理核心不依赖外部二进制工具
- 使用 workspace 统一管理依赖版本
- 新增依赖需评估必要性和维护状态

### 3. 性能优先
- 音频处理使用 `Vec<f32>` 存储，范围 `[-1.0, 1.0]`
- 避免不必要的内存分配和拷贝
- 大文件使用流式处理
- 关键路径考虑 SIMD 优化

### 4. 错误处理模式
```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PhononError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Invalid sample rate: {0}")]
    InvalidSampleRate(u32),
    
    #[error("Audio processing error: {0}")]
    Processing(String),
}

pub type Result<T> = std::result::Result<T, PhononError>;
```

## 项目结构

```
phonon/
├── Cargo.toml              # Workspace 根配置 (13 crates)
├── CONSTITUTION.md         # 项目宪法(最高约束)
├── AGENTS.md               # 本文档
├── README.md               # 项目说明
├── RoadMap.md              # 定位、边界与路线
├── Todos.md                # 开发计划跟踪
├── CHANGELOG.md
├── docs/                   # usage / mcp-tools / publishing / 评估报告
│
├── phonon-core/            # 音频算法核心 (AudioBuffer, DSP, EQ, Plugin)
│   └── src/
│       ├── lib.rs          # 公开 API + AudioBuffer
│       ├── plugin.rs       # DspPlugin trait + PluginRegistry + 5 内建插件
│       └── algorithms/     # 23 个算法模块 (mix, trim, eq, compressor, lufs, simd_ops, ...)
│
├── phonon-wav/             # WAV 文件读写
├── phonon-io/              # 多格式 I/O (WAV/MP3/FLAC/OGG;AAC 需 `aac` feature)
├── phonon-engine/          # 实时播放引擎 (EffectChain = Vec<Box<dyn DspPlugin>>)
├── phonon-ai/              # AI 集成 (TTS/STT/SoundGen)
├── phonon-podcast/         # 播客生成管线 (脚本→TTS→拼装+字幕+章节)
├── phonon-video/           # 视频音轨提取/替换 (ffmpeg)
├── phonon-server/          # REST API 服务 (axum,默认仅监听 127.0.0.1)
├── phonon-mcp/             # MCP server (JSON-RPC stdio,文件访问限定在 PHONON_MCP_ROOT)
├── phonon-wasm/            # WebAssembly 绑定 (wasm-bindgen + demo)
├── phonon-studio/          # 桌面 GUI (Dioxus 0.7)
│   └── src/
│       ├── main.rs
│       ├── state.rs        # 全局状态 (AppState)
│       └── components/     # UI 组件 (waveform, spectrum, eq, export, ...)
├── phonon-cli/             # 命令行工具
└── phonon-tests/           # 集成测试
```

## 当前开发阶段

**已完成**: v0.1 (WAV) → v0.2 (pydub等价) → v0.3 (播客Agent) → v0.4 (STT/字幕/视频/Server) → v0.5 (sinc resampler/多格式/EffectChain/CI) → v0.6 (EQ/rayon/Studio UI/Wasm/Plugin) → v0.7 (LUFS/MP3+FLAC+OGG tags/Pan/TTS cache/AI BGM 接管线/Episode export/MCP server) → **v0.8 (可配置模型 provider)**

**当前版本**: v0.8.0(模型 provider 可配置，见 [docs/providers.md](./docs/providers.md);此前:评估修复 v0.7.1 + 遗留项 v0.7.2 + 架构复评 v0.7.3,见 [docs/evaluation-v0.7.md](./docs/evaluation-v0.7.md))

**phonon-core 特性**:
- `features = ["parallel", "simd"]`(默认开启)— rayon 并行 + NEON/SSE SIMD,其他架构走标量
- 23 个算法模块,含 `lufs`(BS.1770-4)和 `pan`
- 测试数以 `cargo test --workspace` 实际输出为准(不在文档里维护数字)

**phonon-wasm**:
- `phonon-core` 的 wasm-bindgen 封装(无 rayon / 无 simd)
- process_audio() + 独立函数 + demo HTML

**phonon-mcp**(v0.7 新增):
- JSON-RPC 2.0 over stdio,基于 hand-rolled 实现(避免 rmcp 复杂性)
- 14 个内建 tool:基础 / DSP / 测量类
- AssetRegistry 用 UUID 句柄管理音频缓冲,LLM 永不接触 PCM 字节

**下一步待做**(详见 RoadMap.md / Todos.md):
- crates.io 暂不发布:`phonon` 已被无关项目占用,重估条件见 docs/publishing.md;发布只走 git tag + GitHub Release
- OGG 单页损坏时 symphonia 在打开阶段即失败(上游行为)
- AI / 管线工具下沉到下游 data-source 项目(provider 配置在调用方)

## 开发工作流

### 添加新功能
1. 在对应 crate 的 `src/` 下创建模块文件
2. 在 `lib.rs` 中声明并公开模块
3. 编写实现代码，确保错误处理完善
4. 编写单元测试（`#[cfg(test)] mod tests`）
5. 更新 crate 的 README.md 文档
6. 运行测试：`cargo test`
7. 运行 linter：`cargo clippy`
8. 格式化代码：`cargo fmt`

### 测试规范
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_buffer_creation() {
        let buffer = AudioBuffer {
            samples: vec![0.0; 44100],
            sample_rate: 44100,
            channels: 1,
        };
        
        assert_eq!(buffer.sample_rate, 44100);
        assert_eq!(buffer.channels, 1);
        assert!((buffer.duration_secs() - 1.0).abs() < 0.001);
    }
    
    #[test]
    fn test_wav_roundtrip() -> Result<()> {
        // 创建测试数据
        let original = AudioBuffer::from_wav_file("tests/fixtures/test.wav")?;
        
        // 保存再读取
        original.to_wav_file("/tmp/test_output.wav")?;
        let loaded = AudioBuffer::from_wav_file("/tmp/test_output.wav")?;
        
        // 验证数据一致
        assert_eq!(original.sample_rate, loaded.sample_rate);
        assert_eq!(original.channels, loaded.channels);
        assert_eq!(original.samples.len(), loaded.samples.len());
        
        Ok(())
    }
}
```

### 构建配置
本地可创建 `.cargo/config.toml`(已 gitignore,不要提交——CI runner 上不存在这个路径)：
```toml
[build]
target-dir = "/Users/hal/.target"

[target.x86_64-apple-darwin]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]
```

## Agent 协作指南

### 当收到"实现 XXX 功能"的请求时

1. **理解需求**:
   - 确认功能属于哪个 Phase
   - 确认应该在哪个 crate 中实现
   - 检查是否有依赖其他未实现的功能

2. **设计接口**:
   - 设计清晰的公开 API
   - 考虑错误处理路径
   - 评估性能影响

3. **实现代码**:
   - 遵循本文档的编码规范
   - 添加详细的文档注释
   - 实现时考虑边界情况

4. **编写测试**:
   - 单元测试覆盖主要逻辑
   - 边界条件测试
   - 错误处理测试

5. **验证与优化**:
   - 运行 `cargo test`
   - 运行 `cargo clippy`
   - 运行 `cargo fmt`
   - 必要时运行性能测试

### 当收到"调试错误"的请求时

1. **错误分类**:
   - 编译错误：检查类型、生命周期、trait bound
   - 运行时错误：检查边界条件、空指针、资源泄漏
   - 逻辑错误：检查算法正确性

2. **逐步修复**:
   - 按错误类型分组
   - 先修复编译错误
   - 再处理运行时错误
   - 最后验证逻辑正确性

3. **添加防御代码**:
   - 添加断言验证假设
   - 添加边界检查
   - 改进错误消息

### 当收到"添加依赖"的请求时

1. **评估必要性**:
   - 是否有纯 Rust 替代方案？
   - 是否可以自己实现？
   - 依赖的维护状态如何？

2. **添加到 workspace**:
```toml
# 在根 Cargo.toml 的 [workspace.dependencies] 中添加
new-dep = "x.y.z"
```

3. **在 crate 中使用**:
```toml
# 在具体 crate 的 Cargo.toml 中
[dependencies]
new-dep.workspace = true
```

## 音频处理知识库

### 音频基础概念

**采样率 (Sample Rate)**:
- 每秒采样次数，单位 Hz
- 常见值：8000 (电话), 16000 (语音), 44100 (CD), 48000 (专业), 96000 (高保真)
- 根据奈奎斯特定理，采样率至少是最高频率的 2 倍

**声道 (Channels)**:
- 1: 单声道 (Mono)
- 2: 立体声 (Stereo)
- 多声道：5.1, 7.1 环绕声

**位深度 (Bit Depth)**:
- 每个采样点的比特数
- 16-bit: CD 质量，范围 -32768 到 32767
- 24-bit: 专业录音
- 32-bit float: 高精度处理，范围 -1.0 到 1.0

**PCM (Pulse Code Modulation)**:
- 脉冲编码调制，最基础的音频编码方式
- 直接存储采样点的数值
- WAV 文件通常使用 PCM 编码

### 音频算法实现要点

**混音 (Mixing)**:
```rust
pub fn mix(buffer1: &AudioBuffer, buffer2: &AudioBuffer) -> Result<AudioBuffer> {
    if buffer1.sample_rate != buffer2.sample_rate {
        return Err(PhononError::SampleRateMismatch);
    }
    
    let max_len = buffer1.samples.len().max(buffer2.samples.len());
    let mut mixed = Vec::with_capacity(max_len);
    
    for i in 0..max_len {
        let s1 = buffer1.samples.get(i).copied().unwrap_or(0.0);
        let s2 = buffer2.samples.get(i).copied().unwrap_or(0.0);
        
        // 简单相加，注意防止爆音
        let mixed_sample = (s1 + s2).clamp(-1.0, 1.0);
        mixed.push(mixed_sample);
    }
    
    Ok(AudioBuffer {
        samples: mixed,
        sample_rate: buffer1.sample_rate,
        channels: buffer1.channels,
    })
}
```

**归一化 (Normalization)**:
```rust
pub fn normalize(buffer: &mut AudioBuffer, target_peak: f32) {
    // 找到最大幅度
    let peak = buffer.samples.iter()
        .map(|&s| s.abs())
        .fold(0.0f32, f32::max);
    
    if peak > 0.0 {
        let gain = target_peak / peak;
        
        // 应用增益
        for sample in &mut buffer.samples {
            *sample *= gain;
        }
    }
}
```

**淡入淡出 (Fade In/Out)**:
```rust
pub fn fade_in(buffer: &mut AudioBuffer, duration_secs: f64) {
    let fade_samples = (duration_secs * buffer.sample_rate as f64) as usize;
    let fade_samples = fade_samples.min(buffer.samples.len());
    
    for i in 0..fade_samples {
        let gain = i as f32 / fade_samples as f32;
        buffer.samples[i] *= gain;
    }
}

pub fn fade_out(buffer: &mut AudioBuffer, duration_secs: f64) {
    let fade_samples = (duration_secs * buffer.sample_rate as f64) as usize;
    let start = buffer.samples.len().saturating_sub(fade_samples);
    
    for i in start..buffer.samples.len() {
        let position = i - start;
        let gain = 1.0 - (position as f32 / fade_samples as f32);
        buffer.samples[i] *= gain;
    }
}
```

## 常见问题解答

### Q: 为什么使用 `Vec<f32>` 而不是 `Vec<i16>`？
A: `f32` 提供更高的处理精度，避免整数运算的溢出问题，且现代处理器的浮点运算性能优秀。最终输出时再转换为 `i16`。

### Q: 如何处理不同采样率的音频？
A: `phonon_core::resample()` 使用 Lanczos 窗 sinc 插值(降采样时按比例展宽核以抗混叠);`resample_linear()` 是快速低质量版本。

### Q: 音频处理是否需要多线程？
A: 
- **算法层** (phonon-core): 使用 `rayon` 进行数据并行
- **播放层** (phonon-engine): 需要独立音频线程
- **GUI 层** (phonon-studio): 文件加载、EQ / 效果、播放前重采样与波形/FFT 都走 `spawn_blocking`,UI 线程只交换 signal

### Q: 如何避免音频爆音 (Clipping)？
A: 
- 混音时使用 `clamp(-1.0, 1.0)` 限幅
- 或使用软限幅算法 (Soft Clipping)
- 或降低增益后再混音

### Q: 测试音频文件从哪里获取？
A: 
- 使用代码生成简单波形（正弦波、方波等）
- 从 freesound.org 下载开源音频
- 录制自己的测试音频

## 开发环境设置

### 必需工具
```bash
# Rust 工具链
rustup update stable

# 格式化和 linting
rustup component add rustfmt clippy

# 可选：更快的链接器
brew install lld  # macOS
```

### 推荐 VS Code 插件
- rust-analyzer
- CodeLLDB (调试)
- Even Better TOML
- Error Lens

### 环境变量
```bash
# .zshrc 或 .bashrc
export CARGO_TARGET_DIR="/Users/hal/.target"
export RUST_BACKTRACE=1  # 开发时启用堆栈跟踪
```

## 版本发布流程

### 版本号规范 (Semantic Versioning)
- 0.x 期间每个次版本对应一个里程碑(历史见 Todos.md / CHANGELOG.md),
  次版本内允许破坏性改动,需在 commit 中标注 `BREAKING`
- **1.0.0**: 第一个稳定版本

### 发布前检查清单
- [ ] 所有测试通过
- [ ] 文档更新
- [ ] CHANGELOG.md 更新
- [ ] 版本号更新
- [ ] 代码格式化
- [ ] Clippy 无警告

## 参考资源

### Rust 音频库
- [hound](https://github.com/ruuda/hound) - WAV 文件处理
- [cpal](https://github.com/RustAudio/cpal) - 跨平台音频
- [rodio](https://github.com/RustAudio/rodio) - 音频播放
- [symphonia](https://github.com/pdeljanov/Symphonia) - 音频解码

### 音频处理理论
- [Digital Signal Processing (DSP) Guide](https://www.dspguide.com/)
- [The Audio Programming Book (MIT Press)](https://mitpress.mit.edu/books/audio-programming-book)

### WAV 格式规范
- [WAVE PCM soundfile format](http://soundfile.sapp.org/doc/WaveFormat/)
- [Multimedia Programming Interface and Data Specifications (Microsoft)](https://docs.microsoft.com/en-us/previous-versions/windows/hardware/design/dn653308(v=vs.85))

---

**文档版本**: v1.0  
**最后更新**: 2026-10-20  
**维护者**: hal  

**注意**: 本文档会随着项目进展持续更新，Agent 在协作时应优先参考本文档的最新版本。
