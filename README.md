# VoiceInput

一个跨平台语音输入法项目。

## 当前形态

这个仓库现在把应用建模成一个 IME runtime：

1. 开始 composition
2. 将部分转写流式写入 preedit
3. 提交最终文本
4. 出错时干净地取消

共享 Rust core 负责这条流程，各个平台宿主负责把它翻译成原生输入法 API。macOS 这条线已经能完成热键触发、录音、转写和文本注入的闭环；Windows 现在先补上了本地 ASR + 文本直接注入 / 剪贴板回退的兼容路径。

目前 workspace 已经开始按共享层和平台层拆分：

- `voice-input-core`：纯业务状态机和错误模型
- `voice-input-asr`：ASR 配置、runner、transcriber
- `voice-input-audio`：文件录音、PCM/WAV 公共处理
- `voice-input-runtime`：共享 local runtime、stateful host、live job/session 状态和常驻入口 helper
- `voice-input-macos` / `voice-input-linux` / `voice-input-windows`：平台桥接与常驻 runtime
- `voice-input-cli`：统一 smoke/live CLI 入口

## 平台目标

- macOS: 常驻 app + Accessibility / Unicode / 剪贴板注入
- Windows: `TSF` 和 `COM`，当前先提供直接注入 + 剪贴板回退兼容层
- Linux: `IBus` 或 `Fcitx5`

## 已实现内容

- 共享 IME 状态机和 transcript 模型
- composition 生命周期处理，支持失败时 cancel
- 用于开发和测试的 mock host 与转写管线
- macOS host crate，负责常驻 app 的桥接边界
- macOS 本地语音输入管线，已接入 `LocalFunAsrTranscriber`
- macOS smoke：`cargo run -p voice-input-cli -- smoke macos --audio-file testdata/smoke.wav`
- macOS 常驻菜单栏 app：`scripts/voiceinput.sh macos install`
- `voice-input-macos-app` 负责常驻菜单栏入口
- 两个入口共用同一套实时运行时
- 建议使用 WAV/PCM 音频做 smoke 测试；仓库里已经提供了 `testdata/smoke.wav`
- Linux host crate，拆分了 IBus/Fcitx5 后端与 IBus bridge 层
- Linux IBus 路径绑定到 `ibus` crate + D-Bus 抽象
- IBus bridge 已接上真实 `ibus` crate API
- Windows host crate，已提供全局热键、麦克风录音、文本直接注入、剪贴板回退和 smoke 路径
- Windows smoke：`cargo run -p voice-input-cli -- smoke windows --audio-file testdata/smoke.wav`
- Windows 常驻 app：`scripts/voiceinput.sh windows install`
- 本地 ASR 默认使用 `Qwen/Qwen3-ASR-0.6B`
- 模型部署脚本：[`scripts/deploy_funasr_model.py`](./scripts/deploy_funasr_model.py)
- Python 依赖使用本地 `.venv` 和 `uv` 管理
- macOS smoke 路径默认使用 `uv run`
- 在 macOS 上自动检测 MPS，在 Linux/Windows 上自动检测 NVIDIA CUDA

## 命令入口

日常开发优先记两层：

- `cargo run -p voice-input-cli -- <smoke|live> <macos|linux|windows> ...`：统一 CLI 入口
- `scripts/voiceinput.sh ...`：环境准备、模型部署、安装/常驻入口

## 脚本入口

仓库里只需要记这三样：

- `scripts/voiceinput.sh`：统一入口
- `config/models.json`：模型 catalog 单一来源
- `config/voiceinput.env`：由 catalog 生成的仓库级默认配置
- `scripts/voiceinput_config.sh`：共享 helper

如果要切默认模型，用：

```bash
scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b>
```

## Python 环境

1. `uv venv .venv`
2. `uv pip install -r scripts/requirements-asr-base.txt`
3. `uv pip install -r scripts/requirements-asr-runtime.txt`
4. `source .venv/bin/activate`
5. 或者直接使用 `uv run`
6. `scripts/voiceinput.sh bootstrap`
7. 如果要切模型，可以传入 `--model qwen` 或 `--model qwen-0.6b`
8. 如果同时想跑 smoke，可以传入 `--audio-file testdata/smoke.wav`

## Smoke 流程

1. `cargo run -p voice-input-cli -- smoke macos --audio-file testdata/smoke.wav`
2. `cargo run -p voice-input-cli --features linux-ibus-smoke -- smoke linux --audio-file testdata/smoke.wav --backend ibus`
3. `cargo run -p voice-input-cli -- smoke windows --audio-file testdata/smoke.wav`
4. 需要时直接看终端日志

## Live 流程

1. `cargo run -p voice-input-cli -- live macos`
2. `cargo run -p voice-input-cli --features linux-ibus-smoke -- live linux --backend ibus`
3. `cargo run -p voice-input-cli -- live windows`
4. Linux 可额外传 `--double-ctrl-window-ms` 和 `--silence-stop-ms`

## 模型部署

1. `scripts/voiceinput.sh bootstrap`
2. `scripts/voiceinput.sh bootstrap --audio-file testdata/smoke.wav`
3. 一键部署会先安装依赖，再下载模型

## macOS 常驻 app

1. `scripts/voiceinput.sh macos install`
2. 如果要安装时切模型，可以传入 `--model qwen` 或 `--model qwen-0.6b`
3. `scripts/voiceinput.sh macos dev-install`
4. 如果同时想跑 smoke，可以加 `--audio-file testdata/smoke.wav`
5. 首次运行前建议授予“麦克风”和“辅助功能”权限
6. macOS 默认通过统一 CLI `live macos` 启动常驻路径

日常调试时，优先用下面这条链路：

1. 改代码
2. 运行 `scripts/voiceinput.sh macos dev-install`
3. 直接在前台应用里验证输入效果

如果 macOS app 没有正常启动或不能输入，按下面顺序排查：

1. 先确认 `.venv` 和 Rust 工具链都已准备好
2. 如果启动失败，重新跑 `scripts/voiceinput.sh macos install`
3. 如果还没有输入效果，先确认“麦克风”和“辅助功能”权限
4. 仍然异常时，优先看终端里的启动日志，确认没有编译失败或录音失败

## Linux 快速开始

1. Ubuntu 20.04 上先装 `build-essential`、`pkg-config`、`libdbus-1-dev`、`libibus-1.0-dev`、`python3`、`python3-venv`、`python3-pip`
2. 如果要让 Rust 侧录音后端也可用，再补 `libasound2-dev` 和 `portaudio19-dev`
3. 如果要用 Linux 全局热键监听，再补 `libx11-dev`
4. `cargo run -p voice-input-cli --features linux-ibus-smoke -- smoke linux --audio-file testdata/smoke.wav --backend ibus`
5. `scripts/voiceinput.sh linux install`
6. Linux 默认热键现在和 macOS 一样，是 `Ctrl+Shift+Space`
7. 如果要切模型，可以加 `--model qwen` 或 `--model qwen-0.6b`
8. `--backend` 只影响 Linux 宿主后端
9. 常驻版也可直接走 `cargo run -p voice-input-cli --features linux-ibus-smoke -- live linux --backend ibus`

## Windows 快速开始

1. 先准备 Rust、Python 和 `uv`
2. `scripts/voiceinput.sh bootstrap`
3. `cargo run -p voice-input-cli -- smoke windows --audio-file testdata/smoke.wav`
4. `scripts/voiceinput.sh windows install`
5. Windows 默认热键是 `Ctrl+Shift+Space`
6. 当前 Windows 路径会优先直接输入文本，失败时回退到系统剪贴板粘贴
7. `TSF/COM` 原生输入法宿主还没接入，当前常驻版还是热键驱动的兼容层
8. 常驻版也可直接走 `cargo run -p voice-input-cli -- live windows`
