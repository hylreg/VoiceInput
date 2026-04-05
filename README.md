# VoiceInput

一个跨平台语音输入法项目。

## 当前形态

这个仓库现在把应用建模成一个 IME runtime：

1. 开始 composition
2. 将部分转写流式写入 preedit
3. 提交最终文本
4. 出错时干净地取消

共享 Rust core 负责这条流程，各个平台宿主负责把它翻译成原生输入法 API。macOS 这条线已经能完成热键触发、录音、转写和文本注入的闭环。

## 平台目标

- macOS: 常驻 app + Accessibility / Unicode / 剪贴板注入
- Windows: `TSF` 和 `COM`
- Linux: `IBus` 或 `Fcitx5`

## 已实现内容

- 共享 IME 状态机和 transcript 模型
- composition 生命周期处理，支持失败时 cancel
- 用于开发和测试的 mock host 与转写管线
- macOS host crate，负责常驻 app 的桥接边界
- macOS 本地语音输入管线，已接入 `LocalFunAsrTranscriber`
- macOS smoke：`scripts/voiceinput.sh macos smoke --audio-file testdata/smoke.wav`
- macOS 常驻菜单栏 app：`scripts/voiceinput.sh macos install`
- `voice-input-macos-app` 负责常驻菜单栏入口
- 两个入口共用同一套实时运行时
- 建议使用 WAV/PCM 音频做 smoke 测试；仓库里已经提供了 `testdata/smoke.wav`
- Linux host crate，拆分了 IBus/Fcitx5 后端与 IBus bridge 层
- Linux IBus 路径绑定到 `ibus` crate + D-Bus 抽象
- IBus bridge 已接上真实 `ibus` crate API
- 本地 ASR 默认使用 `Qwen/Qwen3-ASR-0.6B`
- 模型部署脚本：[`scripts/deploy_funasr_model.py`](./scripts/deploy_funasr_model.py)
- Python 依赖使用本地 `.venv` 和 `uv` 管理
- macOS smoke 路径默认使用 `uv run`
- 在 macOS 上自动检测 MPS，在 Linux/Windows 上自动检测 NVIDIA CUDA

## 脚本入口

仓库里只需要记这三样：

- `scripts/voiceinput.sh`：统一入口
- `config/voiceinput.env`：仓库级配置模板
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

1. `scripts/voiceinput.sh macos smoke --audio-file testdata/smoke.wav`
2. `scripts/voiceinput.sh linux smoke --audio-file testdata/smoke.wav`
3. 需要时直接看终端日志

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
6. macOS 默认通过常驻 app 注入文本

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
4. `scripts/voiceinput.sh linux smoke --audio-file testdata/smoke.wav`
5. `scripts/voiceinput.sh linux install`
6. Linux 默认热键现在和 macOS 一样，是 `Ctrl+Shift+Space`
7. 如果要切模型，可以加 `--model qwen` 或 `--model qwen-0.6b`
8. `--backend` 只影响 Linux 宿主后端
