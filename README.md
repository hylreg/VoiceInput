# VoiceInput

一个跨平台语音输入法项目。

## 当前形态

这个仓库现在把应用建模成一个 IME runtime：

1. 开始 composition
2. 将部分转写流式写入 preedit
3. 提交最终文本
4. 出错时干净地取消

共享 Rust core 负责这条流程。各个平台宿主负责把它翻译成原生输入法 API。macOS 这条线现在已经补出一个可运行的闭环：全局热键触发、麦克风录音、本地 Fun-ASR 转写，系统级入口优先通过 `InputMethodKit` 提交，找不到活跃 controller 时再回退到剪贴板。

## 平台目标

- macOS: `InputMethodKit`
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
- macOS host crate，保留 `InputMethodKit` 风格的桥接边界
- macOS 本地语音输入管线，已接入 `LocalFunAsrTranscriber`
- macOS smoke binary: `cargo run -p voice-input-macos -- --audio-file /path/to/audio.wav`
- macOS 系统级 IME 入口: `cargo run -p voice-input-macos --bin voice-input-macos-ime`
- macOS 常驻菜单栏 app: `cargo run -p voice-input-macos --bin voice-input-macos-app`
- `voice-input-macos-ime` 负责系统级入口，默认不显示菜单栏图标，并优先使用 `InputMethodKit`
- `voice-input-macos-app` 负责常驻菜单栏入口
- 两个入口都会启动同一套实时运行时：按热键开始录音，再按一次停止并提交文本
- 建议使用 WAV/PCM 音频做 smoke 测试
- Linux host crate，拆分了 IBus/Fcitx5 后端与 IBus bridge 层
- Linux IBus 路径绑定到 `ibus` crate + D-Bus 抽象，而不是 `glib`
- IBus bridge 已使用真实 `ibus` crate 调用：`Bus`、`InputContext`、focus、surrounding text、reset、signal subscriptions
- 本地 ASR 默认使用 ModelScope 上的 `FunAudioLLM/Fun-ASR-Nano-2512`，缓存到 `./models/FunAudioLLM/Fun-ASR-Nano-2512`
- 模型部署脚本：[`scripts/deploy_funasr_model.py`](./scripts/deploy_funasr_model.py)
- Python 依赖使用本地 `.venv` 和 `uv` 管理
- macOS smoke 路径默认使用 `uv run`
- 在 macOS 上自动检测 MPS，在 Linux/Windows 上自动检测 NVIDIA CUDA，但不会自动安装 CUDA
- 部署脚本提供 `--install-cuda` 选项，用于 NVIDIA 机器安装 CUDA 版 PyTorch wheels

## Python 环境

1. `uv venv .venv`
2. `uv pip install -r scripts/requirements-asr-base.txt`
3. `uv pip install -r scripts/requirements-asr-runtime.txt`
4. `source .venv/bin/activate`
5. 或者直接使用 `uv run`
6. 也可以直接运行一键部署脚本：`scripts/bootstrap.sh`
7. 如果同时想跑 smoke，可以传入 `--audio-file /path/to/audio.wav`
8. 默认会使用阿里云 PyPI 镜像；如果要改源，可以先设置 `UV_DEFAULT_INDEX`
9. 依赖已经拆成 `scripts/requirements-asr-base.txt` 和 `scripts/requirements-asr-runtime.txt`，`scripts/requirements-asr.txt` 只是组合入口

## Smoke 流程

1. `scripts/run_macos_smoke.sh --audio-file /path/to/audio.wav`
2. 或者 `uv run -- cargo run -p voice-input-macos -- --audio-file /path/to/audio.wav`

## 模型部署

1. `uv run -- python scripts/deploy_funasr_model.py --skip-existing`
2. 或者先执行 `scripts/setup_python_env.sh`，再运行同样的命令
3. 或者直接执行 `scripts/bootstrap.sh`
4. 一键部署并跑 smoke：`scripts/bootstrap.sh --audio-file /path/to/audio.wav`
5. 一键部署会先安装 `requirements-asr-base.txt` 和 `requirements-asr-runtime.txt`，再下载模型，这样 Mac 上可以正确检测到 MPS

## macOS 系统级 IME

1. 一键安装：`scripts/install_macos_input_method.sh`
2. 只打包：`scripts/package_macos_input_method.sh`
3. 安装脚本会把生成的 `dist/VoiceInput.app` 复制到 `~/Library/Input Methods/`
4. 重新登录或重启输入法服务
5. 系统输入法列表里选择 VoiceInput
6. 首次运行前建议授予“麦克风”和“辅助功能”权限，否则热键监听或录音可能失败
7. 系统级入口优先走 `InputMethodKit` 提交，剪贴板只作为兜底

## 还缺什么

1. 用真正的 `InputMethodKit` 实现替换 macOS mock host
2. 用真正的 TSF 实现替换 Windows mock host
3. 用真实 native bindings 替换 IBus bridge placeholder
4. 增加真正的 macOS 热键和录音适配器
