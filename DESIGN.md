# Phonon - 声子音频引擎设计文档

> *The elementary particle of sound, built in Rust.*

## 一、项目愿景

Phonon 是一个从底层构建的 Rust 音频处理引擎，旨在提供：

- **高性能 WAV 音频处理**：读写、剪辑、混音等核心功能
- **跨平台音频播放**：支持实时音频播放
- **AI 音频生成集成**：对接 TTS/AIGC 大模型
- **视频音频提取与处理**：从视频中提取、处理、重新封装音频
- **桌面应用支持**：通过 Tauri/Dioxus 构建现代化 GUI

## 二、命名哲学

**Phonon (声子)** - 晶格振动的量子，是声音在微观物理上的最小单位。

- 契合从采样点 (sample) 级别处理音频的底层特性
- 体现 Rust 极客风格和"从底层写起"的野心
- 便于生态扩展：`phonon-core`、`phonon-player`、`phonon-ai`、`phonon-studio`

## 三、分层架构设计

### L1: 核心数据层 (phonon-core)

**职责**：纯音频数据处理，无任何 I/O 依赖

**核心数据结构**：
```rust
pub struct AudioBuffer {
    pub samples: Vec<f32>,      // 音频采样数据 [-1.0, 1.0]
    pub sample_rate: u32,        // 采样率 (e.g., 44100)
    pub channels: u16,           // 声道数 (1=单声道, 2=立体声)
}
```

**核心功能**：
- WAV 文件解析与生成 (RIFF 格式处理)
- 音频基础算法：
  - `trim()` - 时间范围剪辑
  - `merge()` - 音频拼接
  - `mix()` - 多轨混音
  - `normalize()` - 响度归一化
  - `fade_in()` / `fade_out()` - 淡入淡出
  - `resample()` - 采样率转换

**技术要点**：
- 使用 `hound` 库处理 WAV 格式
- 错误处理使用 `thiserror`
- 所有计算基于 `Vec<f32>`，确保精度和性能
- 防止 Clipping (爆音)：混音时自动限幅

### L2: 引擎与播放层 (phonon-engine)

**职责**：对接硬件，提供音频播放能力

**核心功能**：
- 音频设备管理 (枚举、选择)
- 实时音频流播放
- 播放控制：播放/暂停/停止/跳转
- 音量控制和实时调整
- 播放进度回调

**技术选型**：
- **主要库**：`cpal` (跨平台音频库) 或 `rodio` (高层抽象)
- **线程模型**：独立音频线程，使用 `crossbeam-channel` 通信
- **缓冲策略**：环形缓冲区 (Ring Buffer)，防止音频断续

**接口设计**：
```rust
pub struct AudioPlayer {
    // 内部实现
}

impl AudioPlayer {
    pub fn new() -> Result<Self>;
    pub fn play(&mut self, buffer: AudioBuffer) -> Result<()>;
    pub fn pause(&mut self);
    pub fn resume(&mut self);
    pub fn stop(&mut self);
    pub fn seek(&mut self, position_secs: f64);
    pub fn set_volume(&mut self, volume: f32); // 0.0 - 1.0
}
```

### L3: 业务与 AI 层

#### 3.1 phonon-ai (AI 音频生成)

**职责**：集成语音合成和生成模型

**支持场景**：
- **Text-to-Speech (TTS)**：文本转语音
- **Speech-to-Text (STT)**：语音识别 (集成 Whisper)
- **AI 音频生成**：音乐/音效生成

**集成方案**：
1. **HTTP API 方式**：
   - OpenAI TTS API
   - 其他云服务 API
   - 使用 `reqwest` 库调用

2. **本地模型方式**：
   - ONNX Runtime：加载 `.onnx` 模型
   - GGUF 格式：通过 `llama.cpp` 或 Rust binding

3. **Python 混调**：
   - 使用 `PyO3` 调用 Python TTS 库
   - 适用于快速原型验证

**数据流**：
```
文本输入 → AI模型 → PCM音频流 → AudioBuffer → phonon-core处理 → 播放/保存
```

#### 3.2 phonon-video (视频音频处理)

**职责**：从视频中提取、处理、重新封装音频

**核心功能**：
- 视频解封装 (Demux)：提取音轨
- 音频编解码：支持 MP3/AAC/FLAC 等格式
- 音频处理：降噪、人声增强、均衡器
- 重新封装 (Mux)：将处理后音频塞回视频

**技术选型**：
- **FFmpeg binding**：`ffmpeg-next` 或 `ffmpeg-sys-next`
- **格式解码**：`symphonia` (纯 Rust 音频解码器)

**工作流程**：
```
MP4文件 → ffmpeg解封装 → 音频流 → 解码为PCM → AudioBuffer
         ↓
处理 (降噪/剪辑等)
         ↓
编码 → 封装回视频 → 输出MP4
```

### L4: 交互层 (phonon-studio)

**职责**：提供桌面 GUI 应用

**技术栈选择**：
- **Tauri**：推荐方案，Web 技术栈 + Rust 后端
- **Dioxus Desktop**：纯 Rust 方案，类 React 开发体验

**核心界面功能**：
1. **波形可视化**：
   - 使用 HTML5 Canvas 或 WebGL 绘制
   - 参考 `wavesurfer.js` 实现
   - 显示双声道波形

2. **时间轴编辑**：
   - 拖拽选区进行剪辑
   - 多轨道显示和编辑
   - 实时预览

3. **播放控制**：
   - 播放/暂停/停止按钮
   - 进度条拖动
   - 音量滑块

4. **文件管理**：
   - 拖入文件解析
   - 批量处理
   - 导出设置

**前后端交互**：
```rust
// Tauri Command 示例
#[tauri::command]
async fn load_audio(path: String) -> Result<AudioInfo, String> {
    let buffer = phonon_core::read_wav(&path)?;
    Ok(AudioInfo {
        duration: buffer.duration_secs(),
        sample_rate: buffer.sample_rate,
        channels: buffer.channels,
    })
}

#[tauri::command]
async fn trim_audio(start: f64, end: f64) -> Result<(), String> {
    // 调用 phonon-core 进行剪辑
}
```

## 四、技术架构图

```
┌─────────────────────────────────────────────────────────┐
│                  phonon-studio (Tauri/Dioxus)           │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  波形可视化   │  │  时间轴编辑   │  │  播放控制     │  │
│  └──────────────┘  └──────────────┘  └──────────────┘  │
└────────────────────────┬────────────────────────────────┘
                         │ Tauri IPC
┌────────────────────────▼────────────────────────────────┐
│                   Rust Backend Layer                     │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  │
│  │  phonon-ai   │  │ phonon-video │  │phonon-engine │  │
│  │  (TTS/STT)   │  │  (视频音频)   │  │   (播放器)    │  │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘  │
│         │                  │                  │          │
│         └─────────┬────────┴──────────────────┘          │
│                   ▼                                      │
│         ┌──────────────────────┐                        │
│         │    phonon-core       │                        │
│         │  ┌────────────────┐  │                        │
│         │  │ AudioBuffer    │  │                        │
│         │  │ Vec<f32>       │  │                        │
│         │  └────────────────┘  │                        │
│         │  • WAV 读写          │                        │
│         │  • 音频算法          │                        │
│         │  • 格式转换          │                        │
│         └──────────────────────┘                        │
└─────────────────────────────────────────────────────────┘
```

## 五、开发路线图 (RoadMap)

### Phase 1: Phonon v0.1 - Hello World
**目标**：实现基础 WAV 读写

**功能清单**：
- [x] 创建 Workspace 结构
- [ ] 实现 `AudioBuffer` 数据结构
- [ ] 完成 WAV 文件读取 (PCM 16-bit)
- [ ] 完成 WAV 文件写入
- [ ] CLI 工具：读取 wav，音量减半，保存
- [ ] 单元测试覆盖

**产出**：`phonon-wav` crate

**学习收获**：
- RIFF 文件格式解析
- 大小端序处理
- PCM 数据结构理解
- Rust 错误处理实践

### Phase 2: Phonon v0.2 - The Algorithms
**目标**：实现音频处理算法

**功能清单**：
- [ ] `trim(start, end)` - 时间剪辑
- [ ] `merge(buffers)` - 音频拼接
- [ ] `mix(buffer1, buffer2)` - 双轨混音
- [ ] `normalize()` - 响度归一化
- [ ] `fade_in(duration)` / `fade_out(duration)` - 淡入淡出
- [ ] 防爆音处理 (Clipping Prevention)
- [ ] 性能测试和优化

**产出**：`phonon-core` crate

**学习收获**：
- 数组操作优化
- 内存管理技巧
- 音频算法原理
- SIMD 加速探索

### Phase 3: Phonon v0.3 - The Player
**目标**：实现音频播放功能

**功能清单**：
- [ ] 集成 `cpal` 库
- [ ] 音频设备枚举和选择
- [ ] 实时播放 `AudioBuffer`
- [ ] 播放控制：播放/暂停/停止
- [ ] 音量控制
- [ ] 播放进度回调
- [ ] 多线程音频流管理
- [ ] CLI 播放器工具

**产出**：`phonon-engine` crate

**学习收获**：
- 音频线程编程
- 缓冲区管理
- 异步编程实践
- 跨平台音频 API

### Phase 4: Phonon Studio v0.1 - The App
**目标**：构建桌面 GUI 应用

**功能清单**：
- [ ] 选择技术栈 (Tauri vs Dioxus)
- [ ] 搭建项目框架
- [ ] 实现文件拖入和解析
- [ ] 波形可视化 (Canvas)
- [ ] 基础播放控制 UI
- [ ] 简单剪辑功能
- [ ] 导出 WAV 文件
- [ ] 打包跨平台应用

**产出**：`phonon-studio` 应用

**学习收获**：
- Rust 与前端交互
- Tauri IPC 机制
- 状态管理
- 桌面应用打包

### Phase 5: Phonon AI v1.0 - The Future
**目标**：集成 AI 音频生成

**功能清单**：
- [ ] 集成 OpenAI TTS API
- [ ] 集成 Whisper (STT)
- [ ] 本地 ONNX 模型支持
- [ ] 实时语音合成
- [ ] 批量文本转语音
- [ ] 音频后处理 Pipeline
- [ ] Studio 中集成 AI 面板

**产出**：`phonon-ai` crate

**学习收获**：
- AI 模型集成
- HTTP API 调用
- 本地模型推理
- 音频生成工作流

### Phase 6: Phonon Video v1.0 - The Complete
**目标**：视频音频处理

**功能清单**：
- [ ] 集成 FFmpeg
- [ ] 视频音轨提取
- [ ] 支持 MP3/AAC 解码
- [ ] 音频降噪算法
- [ ] 音频重新封装
- [ ] 批量视频处理
- [ ] Studio 中集成视频面板

**产出**：`phonon-video` crate

## 六、技术选型与依赖

### 核心依赖

```toml
[workspace.dependencies]
# 音频处理
hound = "3.5"              # WAV 格式处理
cpal = "0.15"              # 跨平台音频播放
rodio = "0.17"             # 高层音频库 (可选)
symphonia = "0.5"          # 音频解码 (MP3/AAC/FLAC)

# 视频处理
ffmpeg-next = "6.0"        # FFmpeg binding

# 错误处理
thiserror = "1.0"          # 错误定义
anyhow = "1.0"             # 错误传播

# 并发
crossbeam-channel = "0.5"  # 线程间通信
rayon = "1.7"              # 数据并行

# 序列化
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"

# AI 集成 (可选)
reqwest = "0.11"           # HTTP 客户端
ort = "1.16"               # ONNX Runtime
pyo3 = "0.20"              # Python binding
```

### GUI 技术栈

**方案一：Tauri (推荐)**
```toml
[dependencies]
tauri = { version = "1.5", features = ["shell-open"] }
tauri-plugin-fs-extra = "1.0"
```

**方案二：Dioxus**
```toml
[dependencies]
dioxus = "0.4"
dioxus-desktop = "0.4"
```

## 七、性能优化策略

### 1. 内存优化
- 使用 `Vec<f32>` 存储音频数据，避免不必要的转换
- 大文件流式处理，避免一次性加载到内存
- 零拷贝传递：使用引用和切片

### 2. 计算优化
- SIMD 指令加速：使用 `std::simd` 或 `packed_simd`
- 多线程并行：`rayon` 并行迭代器
- 缓存友好：连续内存访问模式

### 3. I/O 优化
- 缓冲读写：`BufReader` / `BufWriter`
- 异步 I/O：`tokio` 用于网络请求
- 内存映射：`memmap2` 用于超大文件

## 八、测试策略

### 单元测试
- 每个算法函数都有对应测试
- 边界条件测试
- 错误处理测试

### 集成测试
- 完整的音频处理 Pipeline
- 播放器功能测试
- 文件格式兼容性测试

### 性能测试
- 使用 `criterion` 进行基准测试
- 大文件处理性能测试
- 内存使用分析

### 测试音频资源
```
tests/
  fixtures/
    test_mono_44100.wav
    test_stereo_48000.wav
    test_silence.wav
    test_sine_440hz.wav
```

## 九、文档规范

### 代码文档
- 所有公开 API 必须有文档注释
- 使用 `cargo doc` 生成文档
- 提供使用示例

### 用户文档
- README.md：项目介绍和快速开始
- DESIGN.md：架构设计文档 (本文档)
- AGENTS.md：AI Agent 开发指南
- API.md：API 参考文档

### 开发文档
- CONTRIBUTING.md：贡献指南
- CHANGELOG.md：版本变更记录

## 十、社区与生态

### 开源协议
- 双协议：MIT OR Apache-2.0
- 开放友好，鼓励商业使用

### 项目目标
- 成为 Rust 音频处理的标准库
- 提供完整的音频工作站解决方案
- 构建活跃的开发者社区

---

**最后更新**：2026-01-12  
**维护者**：hal  
**版本**：v0.1.0-alpha
