# VoiceInput

一个跨平台语音输入法项目。

## 当前形态

这个仓库现在把应用建模成一个 IME runtime：

1. 开始 composition
2. 将部分转写流式写入 preedit
3. 提交最终文本
4. 出错时干净地取消

共享 Rust core 负责这条流程。各个平台宿主负责把它翻译成原生输入法 API。macOS 这条线现在已经补出一个可运行的闭环：全局热键触发、麦克风录音、本地 ASR 转写，并把结果注入到当前前台应用。

## 平台目标

- macOS: 常驻 app + Accessibility / Unicode / 剪贴板注入
- Windows: `TSF` 和 `COM`
- Linux: `IBus` 或 `Fcitx5`

## 为什么这样设计

真正的输入法行为是平台原生能力。跨平台项目更适合这样分层：

- 共享 core 负责 dictation 状态、转写流和错误恢复
- 每个系统提供一个原生宿主，把 composition 和 commit 事件传进去
- 识别后端保持可替换

## 已实现内容

- 共享 IME 状态机和 transcript 模型
- composition 生命周期处理，支持失败时 cancel
- 用于开发和测试的 mock host 与转写管线
- macOS host crate，负责常驻 app 的桥接边界
- macOS 本地语音输入管线，已接入 `LocalFunAsrTranscriber`
- macOS smoke：`scripts/voiceinput.sh macos smoke --audio-file testdata/smoke.wav`
- macOS 常驻菜单栏 app：`scripts/voiceinput.sh macos install`
- `voice-input-macos-app` 负责常驻菜单栏入口
- 两个入口都会启动同一套实时运行时：按热键开始录音，再按一次停止并提交文本
- 建议使用 WAV/PCM 音频做 smoke 测试；仓库里已经提供了 `testdata/smoke.wav`
- Linux host crate，拆分了 IBus/Fcitx5 后端与 IBus bridge 层
- Linux IBus 路径绑定到 `ibus` crate + D-Bus 抽象，而不是 `glib`
- IBus bridge 已使用真实 `ibus` crate 调用：`Bus`、`InputContext`、focus、surrounding text、reset、signal subscriptions
- 本地 ASR 默认使用 `Qwen/Qwen3-ASR-0.6B`，缓存到 `./models/Qwen/Qwen3-ASR-0.6B`
- 也兼容 `FunAudioLLM/Fun-ASR-Nano-2512` 和 `Qwen/Qwen3-ASR-1.7B`，可通过 `VOICEINPUT_ASR_MODEL_ID=...` 切换，缓存默认目录分别是 `./models/FunAudioLLM/Fun-ASR-Nano-2512` 和 `./models/Qwen/Qwen3-ASR-1.7B`
- 模型部署脚本：[`scripts/deploy_funasr_model.py`](./scripts/deploy_funasr_model.py)（支持 `--backend funasr|qwen`）
- Python 依赖使用本地 `.venv` 和 `uv` 管理
- macOS smoke 路径默认使用 `uv run`
- 在 macOS 上自动检测 MPS，在 Linux/Windows 上自动检测 NVIDIA CUDA；默认不自动安装 CUDA，但可通过 `--install-cuda` 触发安装 CUDA 版 PyTorch wheels
- 部署脚本提供 `--install-cuda` 选项，用于 NVIDIA 机器安装 CUDA 版 PyTorch wheels

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
6. 也可以直接运行一键部署：`scripts/voiceinput.sh bootstrap`
7. 如果要切模型，可以传入 `--model qwen` 或 `--model qwen-0.6b`
8. 如果同时想跑 smoke，可以传入 `--audio-file testdata/smoke.wav`
9. 默认会使用阿里云 PyPI 镜像；如果要改源，可以先设置 `UV_DEFAULT_INDEX`
10. 依赖已经拆成 `scripts/requirements-asr-base.txt` 和 `scripts/requirements-asr-runtime.txt`，`scripts/requirements-asr.txt` 只是组合入口
11. 默认 ASR 配置来自 `config/voiceinput.env`，仓库当前默认写入的是 `Qwen/Qwen3-ASR-0.6B`；选模型时可以用 `scripts/voiceinput.sh ... --model ...`，要把默认写回仓库配置时用 `scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b>`
12. 如果想用统一入口，可以直接运行 `scripts/voiceinput.sh bootstrap`

## Smoke 流程

1. `scripts/voiceinput.sh macos smoke --audio-file testdata/smoke.wav`
2. `scripts/voiceinput.sh linux smoke --audio-file testdata/smoke.wav`
3. 需要时直接看终端日志

## 模型部署

1. `scripts/voiceinput.sh bootstrap`
2. 一键部署并跑 smoke：`scripts/voiceinput.sh bootstrap --audio-file testdata/smoke.wav`
3. 一键部署会先安装 `requirements-asr-base.txt` 和 `requirements-asr-runtime.txt`，再下载模型，这样 Mac 上可以正确检测到 MPS

## macOS 常驻 app

1. 一键启动：`scripts/voiceinput.sh macos install`
2. 如果要安装时切模型，可以传入 `--model qwen` 或 `--model qwen-0.6b`
3. 开发一键启动：`scripts/voiceinput.sh macos dev-install`
4. 如果同时想跑 smoke，可以加 `--audio-file testdata/smoke.wav`
5. 首次运行前建议授予“麦克风”和“辅助功能”权限，否则热键监听或录音可能失败
6. macOS 默认通过常驻 app 把识别结果注入到当前前台应用；不再默认走系统级输入法注册流程

日常调试时，优先用下面这条链路：

1. 改代码
2. 运行 `scripts/voiceinput.sh macos dev-install`
3. 直接在前台应用里验证输入效果

如果 macOS app 没有正常启动或不能输入，按下面顺序排查：

1. 先确认 `.venv` 和 Rust 工具链都已准备好
2. 如果启动失败，重新跑 `scripts/voiceinput.sh macos install`
3. 如果还没有输入效果，先确认“麦克风”和“辅助功能”权限
4. 仍然异常时，优先看终端里的启动日志，确认没有编译失败或录音失败

## 还缺什么

1. 增加更完整的 macOS 注入适配器
2. 用真正的 TSF 实现替换 Windows mock host
3. 用真实 native bindings 补齐 Fcitx5 路径
4. 增加更稳定的 macOS 热键和录音适配器

## Linux 快速开始

1. Ubuntu 20.04 上先安装 `build-essential`、`pkg-config`、`libdbus-1-dev`、`libibus-1.0-dev`、`python3`、`python3-venv`、`python3-pip`
2. 如果要让 Rust 侧录音后端也可用，再补 `libasound2-dev` 和 `portaudio19-dev`
3. 如果要用 Linux 全局热键监听，再补 `libx11-dev`
4. 先准备好 `.venv` 和本地 ASR 模型
5. 运行 `scripts/voiceinput.sh linux smoke --audio-file testdata/smoke.wav`
6. 直接用 `scripts/voiceinput.sh linux install` 启动常驻版
7. 如果要切模型，可以加 `--model qwen` 或 `--model qwen-0.6b`
8. `--backend` 只影响 Linux 常驻版 / smoke 的宿主后端（IBus 或 Fcitx5）
9. 当前 Linux 这条线优先支持 IBus，Fcitx5 还保留为后续 native bindings 的路线
