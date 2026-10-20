# 模型 Provider 配置

Phonon 的 TTS(语音合成)、STT(语音识别)、LLM(播客脚本生成)都可以通过配置切换模型。
内置 MiniMax / Qwen / GLM / StepFun 等预置，也支持任何 OpenAI 兼容的自部署服务
(vLLM、Ollama、Kokoro-FastAPI、speaches 等)。

> 本文前半部分是使用说明，后半部分是设计记录(为什么这样做、哪些没做)。

## 快速开始

不写配置文件也能用：按优先级取第一个设置了 API key 的厂商。

| 能力 | 自动选择顺序(看环境变量) | 都没有时 |
|---|---|---|
| TTS | minimax → qwen → glm → stepfun → openai → elevenlabs | edge(免费，无需 key) |
| STT | minimax → qwen → glm → stepfun → openai | 报错 |
| LLM | minimax → qwen → glm → stepfun → deepseek → openai | 报错 |

| 预置 | API key 环境变量 | TTS | STT | LLM |
|---|---|---|---|---|
| `minimax` | `MINIMAX_API_KEY` | speech-2.8-hd | asr-1.0(带时间戳) | MiniMax-M3.1-Flash |
| `qwen` | `DASHSCOPE_API_KEY` | qwen3-tts-flash | qwen-audio-3.0-asr-flash(带时间戳) | qwen3.8-flash |
| `glm` | `ZHIPU_API_KEY` | glm-tts | glm-asr(带时间戳，单次 ≤30 s,自动切分) | glm-4.7 |
| `stepfun` | `STEP_API_KEY` | step-tts-2 | stepaudio-2.5-asr(**无时间戳**) | step-3.7-flash |
| `deepseek` | `DEEPSEEK_API_KEY` | — | — | deepseek-flash |
| `openai` | `OPENAI_API_KEY` | tts-1 | whisper-1(带时间戳) | gpt-4o-mini |
| `elevenlabs` | `ELEVENLABS_API_KEY` | eleven_multilingual_v2 | — | — |
| `edge` | 无 | Edge 在线 TTS | — | — |

## 配置文件

查找顺序:`--config <path>` → 环境变量 `PHONON_CONFIG` → `./phonon.toml` →
`~/.config/phonon/config.toml`。每一节都可省略，省略的节走上面的自动选择。

```toml
[tts]
provider = "qwen"          # 预置名，或下面 [providers.*] 里自定义的名字
model = "qwen3-tts-flash"  # 可选，默认取预置
voice = "Cherry"           # 可选，默认取预置
max_chars = 600            # 可选，单次请求最大字符数，超过自动按句切分

[stt]
provider = "glm"
max_secs = 30              # 可选，单次请求最大音频时长，超过自动切分

[llm]
provider = "deepseek"
model = "deepseek-flash"
# 原样合并进 chat/completions 请求体，用于关闭思考等厂商参数
extra = { thinking = { type = "disabled" } }

# 自部署 / 其他厂商:声明协议和地址即可
[providers.local-tts]
protocol = "openai"                    # openai | minimax | qwen | edge | elevenlabs
base_url = "http://127.0.0.1:8880/v1"
api_key_env = "LOCAL_TTS_KEY"          # 可选;不填则不发送鉴权头
```

自定义 provider 没有默认模型和音色，需要在 `[tts]` / `[stt]` / `[llm]` 里写 `model`(TTS 还要 `voice`)。

### 自部署服务

| 服务 | 能力 | 配置 |
|---|---|---|
| vLLM / Ollama / llama.cpp server | LLM | `protocol = "openai"`,`base_url = "http://host:port/v1"` |
| Kokoro-FastAPI / LocalAI | TTS | 同上 |
| speaches(faster-whisper-server)/ LocalAI | STT | 同上 |
| whisper.cpp `whisper-server` | STT | 启动时加 `--inference-path /v1/audio/transcriptions` |
| CosyVoice 官方 server、GPT-SoVITS | TTS | 非 OpenAI 协议，暂不支持(见下文"不做") |

## 设计记录

### 要解决的问题

调用方今天只能在代码里 `OpenAiTts::new()` / `MiniMaxTts::new()` 选死一个 provider;
CLI 的 `--provider` 只有 edge / openai,server 写死 Edge。国内厂商除 MiniMax TTS 外都没接。

### 厂商实测(2026-10-20,真实请求)

- **OpenAI 兼容**:StepFun / GLM 的 TTS;MiniMax / GLM / StepFun 的 STT(multipart `/audio/transcriptions`);
  五家 LLM 全部兼容，且都接受 `response_format: json_object`。
- **非兼容**:Qwen TTS(DashScope 原生，返回音频 URL 需二次下载)、Qwen STT(DashScope 原生，词级时间戳)、
  MiniMax TTS(已实现)。
- **差异点**(都要处理，否则请求失败):
  - GLM TTS 不支持 mp3 → OpenAI 协议的输出格式改为可配，默认 wav。
  - StepFun STT 拒绝 `verbose_json`,也不返回时间戳;GLM STT 不需 `verbose_json` 就返回 segments。
  - GLM STT 单次 ≤30 s(超过返回 400 code 1214)→ 按时长切分;只有 `glm-asr` 返回 segments,
    `glm-asr-2512` 只返回文本。
  - MiniMax STT 请求体与 OpenAI 相同，但路径是 `/speech_to_text` → 路径可配。
  - TTS 单次长度上限:Qwen 600 字、GLM 1024、StepFun 1000 → 按句切分。
    Qwen 预置取 200 而非 600:同一段 594 字的文本三次合成，时长分别为 153、176、248 s,
    一次长链测试中达到 627 s(正常约 125 s);396 字长出 8–24%;198 字四次都在 40–41 s,
    与短句按字数折算的 41 s 一致。GLM / StepFun / MiniMax 在 990 字时偏差 ≤6%,而且两次结果一致。
  - 当前 LLM 默认开启思考：小 `max_tokens` 下 content 为空;MiniMax-M2 把 `<think>` 放进 content
    → 预置里带关闭思考的参数，解析前剥掉 `<think>…</think>` 和代码围栏。
  - Qwen / CosyVoice 返回的 WAV 头部长度是占位值，hound 打不开;我们的 `decode_audio` 走 symphonia,
    五家 WAV 实测都能正确解码，不需要处理。

### 结构

- **协议 = 实现**:`openai`、`minimax`、`qwen`、`edge`、`elevenlabs` 五种 TTS 协议，
  `openai`、`qwen` 两种 STT 协议,LLM 只有 `openai` 一种。厂商只是"协议 + 地址 + key 变量 + 默认模型"的数据。
- **运行期选择用 enum,不用 dyn**:`AnyTts` / `AnyStt` 包住已有实现并实现同一 trait。
  这同时满足了 v0.7.3"AI trait RPITIT → dyn"一项的重估条件(server 按配置切换 provider),
  但 enum 已足够，trait 不改。
- **配置解析在 phonon-ai**(`config` 模块),产出可直接用的 provider;
  LLM 产出 `ChatEndpoint { client, model, extra }`,由 phonon-podcast 的脚本生成器消费。
- **切分放在 `AnyTts` / `AnyStt`**:上限是"某个配置的 provider"的属性，不是协议的属性。

### 不做(当前版本)

- **音乐 / 音效生成接入国内厂商**:MiniMax `music_generation` 对当前 key 返回 410(code 2153,停止新用户);
  Qwen Fun-Music 返回 403;GLM 无此 API;StepFun 无公开文档。没有能验证的接口。
  重估条件:`curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $MINIMAX_API_KEY" -H 'Content-Type: application/json' -d '{"model":"music-2.0","prompt":"calm piano","lyrics":"[Instrumental]"}' https://api.minimaxi.com/v1/music_generation` 输出 200
- **CosyVoice 官方 server / GPT-SoVITS 协议**:没有使用方，也没有部署可测。
  重估条件：有人提供一个可访问的部署地址
- **MCP 暴露 AI 工具**:沿用既有决定(留给下游项目)
