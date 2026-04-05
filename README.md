# VoiceInput

一个跨平台语音输入法项目。

## 当前形态

这个仓库现在把应用建模成一个 IME runtime：

1. 开始 composition
2. 将部分转写流式写入 preedit
3. 提交最终文本
4. 出错时干净地取消

共享 Rust core 负责这条流程。各个平台宿主负责把它翻译成原生输入法 API。macOS 这条线现在已经补出一个可运行的闭环：全局热键触发、麦克风录音、本地 Fun-ASR 转写，系统级入口优先通过 `InputMethodKit` 提交，找不到活跃 controller 时先尝试 Accessibility 注入，再退到 Unicode 事件输入，最后才回退到剪贴板。

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
- macOS smoke binary: `cargo run -p voice-input-macos -- --audio-file testdata/smoke.wav`
- macOS 系统级 IME 入口: `cargo run -p voice-input-macos --bin voice-input-macos-ime`
- macOS 常驻菜单栏 app: `cargo run -p voice-input-macos --bin voice-input-macos-app`
- `voice-input-macos-ime` 负责系统级入口，默认不显示菜单栏图标，并优先使用 `InputMethodKit`
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

仓库里的脚本现在只需要记三层：

- `scripts/voiceinput.sh`：统一入口，优先推荐用子命令调用
- `config/voiceinput.env`：仓库级配置模板，默认写入 Qwen 0.6B，也可以用 `VOICEINPUT_CONFIG_FILE` 指向其他文件
- `scripts/macos_input_method_common.sh` 和 `scripts/voiceinput_config.sh`：少量共享 helper
- `scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b>`：把仓库默认模型写回 `config/voiceinput.env`

旧脚本保留为兼容壳，方便你继续用原来的命令名，但不再作为推荐路径。

## Python 环境

1. `uv venv .venv`
2. `uv pip install -r scripts/requirements-asr-base.txt`
3. `uv pip install -r scripts/requirements-asr-runtime.txt`
4. `source .venv/bin/activate`
5. 或者直接使用 `uv run`
6. 也可以直接运行一键部署脚本：`scripts/voiceinput.sh bootstrap`
7. 如果要切到 Qwen 1.7B，可以传入 `--model qwen`；如果要切到 Qwen 0.6B，可以传入 `--model qwen-0.6b`，`--backend qwen` 也兼容
8. 如果同时想跑 smoke，可以传入 `--audio-file testdata/smoke.wav`
9. 默认会使用阿里云 PyPI 镜像；如果要改源，可以先设置 `UV_DEFAULT_INDEX`
10. 依赖已经拆成 `scripts/requirements-asr-base.txt` 和 `scripts/requirements-asr-runtime.txt`，`scripts/requirements-asr.txt` 只是组合入口
11. 默认 ASR 配置来自 `config/voiceinput.env`，仓库当前默认写入的是 `Qwen/Qwen3-ASR-0.6B`；真正选模型时可以用 `scripts/voiceinput.sh ... --model ...`，要把默认写回仓库配置时用 `scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b>`
12. 如果想用统一入口，可以直接运行 `scripts/voiceinput.sh bootstrap`

## Smoke 流程

1. `scripts/voiceinput.sh macos smoke --audio-file testdata/smoke.wav`
2. 或者 `uv run -- cargo run -p voice-input-macos -- --audio-file testdata/smoke.wav`
3. Linux live app 可以用 `cargo run -p voice-input-linux --features ibus --bin voice-input-linux-app -- --backend ibus`
4. Linux smoke 仍然可以用 `scripts/voiceinput.sh linux smoke --audio-file testdata/smoke.wav`，需要切 Qwen 时可加 `--model qwen`，切 0.6B 时可加 `--model qwen-0.6b`
5. Linux 一键版可以用 `scripts/voiceinput.sh linux install`

## 模型部署

1. `uv run -- python scripts/deploy_funasr_model.py --skip-existing`
2. 或者先执行 `scripts/setup_python_env.sh`，再运行同样的命令
3. 或者直接执行 `scripts/voiceinput.sh bootstrap`
4. 一键部署并跑 smoke：`scripts/voiceinput.sh bootstrap --audio-file testdata/smoke.wav`
5. 一键部署会先安装 `requirements-asr-base.txt` 和 `requirements-asr-runtime.txt`，再下载模型，这样 Mac 上可以正确检测到 MPS
6. 统一入口可以写成 `scripts/voiceinput.sh bootstrap --audio-file testdata/smoke.wav`

## macOS 系统级 IME

1. 一键安装：`scripts/voiceinput.sh macos install`
2. 如果要安装时直接切到 Qwen，可以传入 `--backend qwen`；如果要安装时直接切到 Qwen 0.6B，可以传入 `--model qwen-0.6b`
3. 只打包：`scripts/voiceinput.sh macos package`
4. 调试刷新：`scripts/voiceinput.sh macos reinstall`
5. 启用当前输入法：`scripts/voiceinput.sh macos enable`
6. 开发一键刷新：`scripts/voiceinput.sh macos dev-install`
7. 导出当前状态：`scripts/voiceinput.sh macos dump-state`
8. 安装脚本会把生成的 `dist/VoiceInput.app` 复制到 `~/Library/Input Methods/`
9. 这个包现在包含一个容器 app 和一个 `Contents/PlugIns/VoiceInput.appex` extension
10. 安装和调试刷新子命令都会自动执行启用步骤
11. 重新登录或重启输入法服务
12. 系统输入法列表里选择 VoiceInput
13. 首次运行前建议授予“麦克风”和“辅助功能”权限，否则热键监听或录音可能失败
14. 系统级入口优先走 `InputMethodKit` 提交；无活跃 controller 时优先尝试 Accessibility 注入，再退到 Unicode 事件输入，最后才回退到剪贴板

日常调试时，优先用下面这条链路：

1. 改代码
2. 运行 `scripts/voiceinput.sh macos dev-install`
3. 回到系统设置里切换或重新启用 `VoiceInput`

如果系统输入法列表里还是找不到 `VoiceInput`，按下面顺序排查：

1. 先确认 bundle 已经真的安装到了用户目录：`ls ~/Library/Input\ Methods/VoiceInput.app`
2. 如果这里不存在，说明安装脚本还没有成功完成，重新跑 `scripts/voiceinput.sh macos install`
3. 如果 bundle 已存在，先注销当前 macOS 会话再回来，或者重启一次输入法相关服务
4. 如果你是从下载包、zip 或外部目录复制过来的，再检查是否带了隔离属性：`xattr -dr com.apple.quarantine ~/Library/Input\ Methods/VoiceInput.app`
5. 仍然看不到时，优先看终端里的安装日志，确认没有编译失败或复制失败
6. 进一步排查时，运行 `scripts/voiceinput.sh macos dump-state` 看它有没有出现在 HIToolbox、TIS 和 extension 里

## 还缺什么

1. 用真正的 `InputMethodKit` 实现替换 macOS mock host
2. 用真正的 TSF 实现替换 Windows mock host
3. 用真实 native bindings 补齐 Fcitx5 路径
4. 增加真正的 macOS 热键和录音适配器

## Linux 快速开始

1. Ubuntu 20.04 上先安装 `build-essential`、`pkg-config`、`libdbus-1-dev`、`libibus-1.0-dev`、`python3`、`python3-venv`、`python3-pip`
2. 如果要让 Rust 侧录音后端也可用，再补 `libasound2-dev` 和 `portaudio19-dev`
3. 如果要用 Linux 全局热键监听，再补 `libx11-dev`
4. 先准备好 `.venv` 和本地 ASR 模型
5. 运行 `scripts/voiceinput.sh linux smoke --audio-file testdata/smoke.wav`
6. 如果要切到 Qwen，可以加 `--model qwen`；要切 0.6B 可以加 `--model qwen-0.6b`
7. 或者直接运行 `cargo run -p voice-input-linux --features ibus -- --audio-file testdata/smoke.wav`
8. 或者启动常驻版：`cargo run -p voice-input-linux --features ibus --bin voice-input-linux-app -- --backend ibus`
9. 常驻版会在托盘显示状态项，热键开始/停止录音，托盘菜单里也有停止和退出
10. 或者直接用一键脚本：`scripts/voiceinput.sh linux install`
11. 这个一键脚本会自动安装 Ubuntu 20.04 常用的 Linux 编译依赖，然后启动常驻版
12. 如果要在安装时切到 Qwen，可以传入 `--model qwen`；如果要切到 Qwen 0.6B，可以传入 `--model qwen-0.6b`
13. `--backend` 仍然只影响 Linux 常驻版 / smoke 的宿主后端（IBus 或 Fcitx5）
14. 当前 Linux 这条线优先支持 IBus，Fcitx5 还保留为后续 native bindings 的路线
15. 统一入口也可以这样用：`scripts/voiceinput.sh linux install`
