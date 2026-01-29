# AGENTS.md - Phonon AI Agent 开发指南

本文档为 AI Agent (如 Warp AI、Cursor、GitHub Copilot 等) 提供 Phonon 项目的开发上下文和指导。

## ⚠️ 核心法则 (Constitution)

**注意**: 本项目严格遵循 [CONSTITUTION.md](./CONSTITUTION.md) 中的规定。在编写任何代码前，必须查阅该文件。
关键要求包括：
- **严禁使用 `unwrap()` / `except()`**
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
- 输入支持：未来将支持 MP3, AAC, FLAC, OGG 等 (计划通过 `symphonia` 集成)
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
├── Cargo.toml              # Workspace 根配置
├── DESIGN.md               # 架构设计文档
├── AGENTS.md               # 本文档
├── README.md               # 项目说明
│
├── phonon-wav/             # Phase 1: WAV 文件处理
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs          # 公开 API
│       ├── reader.rs       # WAV 读取
│       ├── writer.rs       # WAV 写入
│       └── types.rs        # 核心数据结构
│
├── phonon-core/            # Phase 2: 音频算法核心
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── buffer.rs       # AudioBuffer 定义
│       ├── algorithms/     # 算法模块
│       │   ├── mod.rs
│       │   ├── trim.rs     # 剪辑
│       │   ├── merge.rs    # 拼接
│       │   ├── mix.rs      # 混音
│       │   └── normalize.rs # 归一化
│       └── utils.rs        # 工具函数
│
├── phonon-engine/          # Phase 3: 播放引擎
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── player.rs       # 播放器核心
│       ├── device.rs       # 设备管理
│       └── stream.rs       # 音频流
│
├── phonon-ai/              # Phase 5: AI 集成
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── tts.rs          # 文本转语音
│       ├── stt.rs          # 语音识别
│       └── models.rs       # 模型加载
│
├── phonon-video/           # Phase 6: 视频音频处理
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── extract.rs      # 音轨提取
│       └── mux.rs          # 音频封装
│
└── phonon-studio/          # Phase 4: GUI 应用
    ├── Cargo.toml
    ├── tauri.conf.json
    ├── src/
    │   └── main.rs
    └── ui/                 # 前端代码
        ├── index.html
        ├── main.js
        └── style.css
```

## 当前开发阶段

### Phase 1: Phonon v0.1 - WAV 读写 (进行中)

**当前 crate**: `phonon-wav`

**核心任务**:
1. ✅ 创建 Workspace 结构
2. ⏳ 实现 `AudioBuffer` 数据结构
3. ⏳ WAV 文件读取功能
4. ⏳ WAV 文件写入功能
5. ⏳ CLI 工具验证
6. ⏳ 单元测试覆盖

**关键数据结构**:
```rust
/// 核心音频缓冲区
pub struct AudioBuffer {
    /// 音频采样数据，范围 [-1.0, 1.0]
    pub samples: Vec<f32>,
    
    /// 采样率 (Hz)
    pub sample_rate: u32,
    
    /// 声道数 (1=单声道, 2=立体声)
    pub channels: u16,
}

impl AudioBuffer {
    /// 从 WAV 文件读取
    pub fn from_wav_file<P: AsRef<Path>>(path: P) -> Result<Self>;
    
    /// 从字节流读取 WAV
    pub fn from_wav_bytes(data: &[u8]) -> Result<Self>;
    
    /// 保存为 WAV 文件
    pub fn to_wav_file<P: AsRef<Path>>(&self, path: P) -> Result<()>;
    
    /// 转换为 WAV 字节流
    pub fn to_wav_bytes(&self) -> Result<Vec<u8>>;
    
    /// 获取时长（秒）
    pub fn duration_secs(&self) -> f64;
}
```

**WAV 格式关键点**:
- RIFF 文件格式：`RIFF` + ChunkSize + `WAVE`
- fmt 子块：包含采样率、声道数、位深度等信息
- data 子块：实际音频数据（PCM 编码）
- 支持 16-bit PCM (主要) 和 32-bit float PCM
- 注意大小端序处理（WAV 使用小端序）

**依赖**:
```toml
[dependencies]
hound = "3.5"        # WAV 格式处理
thiserror = "1.0"    # 错误定义
```

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
在项目根目录创建 `.cargo/config.toml`：
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
A: 在 Phase 2 会实现 `resample()` 函数，使用插值算法转换采样率。简单场景可用线性插值，专业场景需要使用 sinc 插值。

### Q: 音频处理是否需要多线程？
A: 
- **算法层** (phonon-core): 使用 `rayon` 进行数据并行
- **播放层** (phonon-engine): 需要独立音频线程
- **GUI 层** (phonon-studio): UI 线程与处理线程分离

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
- **0.1.x**: Phase 1 (WAV 读写)
- **0.2.x**: Phase 2 (算法核心)
- **0.3.x**: Phase 3 (播放引擎)
- **0.4.x**: Phase 4 (GUI 应用)
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
**最后更新**: 2026-01-12  
**维护者**: hal  

**注意**: 本文档会随着项目进展持续更新，Agent 在协作时应优先参考本文档的最新版本。
