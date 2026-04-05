# 原生 IME 方案

这个项目应该按“共享 Rust core + 三个平台宿主”的方式来实现。

## 共享 core 负责什么

- 热键处理
- 录音会话控制
- 把部分转写结果更新到 preedit
- 提交最终文本
- 取消并清理 composition

## macOS

使用 `InputMethodKit`。

原生宿主应该负责：

- 接收系统输入法管线里的按键事件
- 通过类似 `setMarkedText` 的行为更新 composition
- 通过原生的 insert / commit API 提交文本

仓库里现在已经有一个 macOS host crate，用来隔离这层桥接。它现在已经能跑出一条可用闭环：全局热键触发、麦克风录音、本地 Fun-ASR 转写，然后优先通过 `InputMethodKit` / Accessibility 注入把文本送到当前光标，最后才会回退到剪贴板。后面如果要更原生，还可以继续把提交层完全收束到真正的 `InputMethodKit` 事件。

现在 macOS crate 还带了一个 smoke binary，会读取本地音频文件并通过本地 Fun-ASR 模型转写：

- `cargo run -p voice-input-macos -- --audio-file /path/to/audio.wav`
- 这个 smoke 路径建议使用 WAV/PCM 输入
- 推荐命令是 `uv run -- cargo run -p voice-input-macos -- --audio-file /path/to/audio.wav`
- 也可以用 `scripts/voiceinput.sh macos smoke --audio-file /path/to/audio.wav`
- 实时运行时已经支持热键开始/停止录音，并在结束后提交文本

此外，仓库里还加了一个系统级 IME 入口：

- `cargo run -p voice-input-macos --bin voice-input-macos-ime`
- `cargo run -p voice-input-macos --bin voice-input-macos-app`
- `voice-input-macos-ime` 是系统级入口，默认不显示菜单栏图标
- `voice-input-macos-app` 是常驻菜单栏入口
- 这两个入口当前都会启动同一套实时运行时
- 当前提交动作优先走 `InputMethodKit` 和 Accessibility 注入，Unicode 事件作为次级兜底，剪贴板只作为最后兜底

Python 环境：

- 用 `uv` 管理 ASR 依赖
- 用 `uv venv .venv` 创建本地虚拟环境
- 先用 `uv pip install -r scripts/requirements-asr-base.txt` 安装模型下载依赖
- 再用 `uv pip install -r scripts/requirements-asr-runtime.txt` 安装运行时依赖
- 在运行部署脚本或任何 Python ASR 命令之前，先 `source .venv/bin/activate`
- Rust 侧的 FunASR runner 会优先使用 `uv run`，然后回退到 `.venv/bin/python`，最后才是 `python3`
- 默认使用阿里云 PyPI 镜像；如果你有自己的镜像源，可以设置 `UV_DEFAULT_INDEX`
- 依赖分成两层：`requirements-asr-base.txt` 负责模型下载，`requirements-asr-runtime.txt` 负责 FunASR 运行时，`requirements-asr.txt` 只是组合入口
- 首次跑实时运行时之前，最好给应用授予“麦克风”和“辅助功能”权限

## Windows

使用 `TSF`。

原生宿主应该负责：

- 实现 text service
- 暴露 COM 对象用于 composition
- 通过 TSF API 更新 composition 字符串并提交最终文本

## Linux

使用 `IBus` 或 `Fcitx5`。

原生宿主应该负责：

- 作为 input method engine service 运行
- 把 preedit 文本推给 engine context
- 把最终文本提交到当前焦点窗口

仓库里现在已经有 Linux host crate，并把后端拆成了 IBus / Fcitx5 两层。剩下的工作是把后端 trait 绑定到真实的 IBus / Fcitx5 API。

### IBus 依赖选择

- IBus 这条线优先用 `ibus` crate 作为 Rust 绑定层
- 这个 crate 已经建模了 `Bus`、`Input Context`、`Commit Text Signal`、`Update Preedit Text Signal`，并且 reexport 了 `dbus`
- 不要把 `glib` 拉进 IME core，除非后面你要加 GTK / GObject 风格的 UI
- `glib` 更适合作为未来 UI 层依赖，而不是 core 依赖

### 当前 IBus 侧已经接上的真实 crate API

- `Bus::new`
- `Bus::create_input_context`
- `InputContext::set_capabilities`
- `InputContext::focus_in`
- `InputContext::set_surrounding_text`
- `InputContext::reset`
- `InputContext::focus_out`
- `InputContext::on_update_preedit_text`
- `InputContext::on_commit_text`
- `InputContext::on_show_preedit_text`
- `InputContext::on_hide_preedit_text`

对 IBus 来说，桥接层应该对应官方文档里的 engine 生命周期，尤其是：

- `ibus_engine_update_preedit_text`
- `ibus_engine_commit_text`
- composition cleanup

## 本地 ASR 来源

- ModelScope 模型页：`https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512`
- 默认本地缓存目录：`./models/FunAudioLLM/Fun-ASR-Nano-2512`
- 也兼容 Qwen 模型：`Qwen/Qwen3-ASR-1.7B`，缓存到 `./models/Qwen/Qwen3-ASR-1.7B`
- 仓库级默认配置文件是 [`config/voiceinput.env`](../config/voiceinput.env)，里面放了 FunASR 和 Qwen 两个可切换模板；默认值会先从这里读取，如果要换文件，可以设置 `VOICEINPUT_CONFIG_FILE`；命令行参数和显式环境变量仍然可以覆盖
- 统一入口是 [`scripts/voiceinput.sh`](../scripts/voiceinput.sh)，比如 `scripts/voiceinput.sh bootstrap`、`scripts/voiceinput.sh macos install`
- 旧脚本现在主要是兼容壳，方便你继续使用原来的命令名
- `voice-input-asr` 里的 Python runner 会根据 `FunAsrConfig` 的 `backend` 选择 FunASR 或 Qwen 路径；FunASR 会使用 `remote_code`、`device`、`language` 和 `itn`，Qwen 会优先使用 `model_id`、`device` 和 `language`
- 用 [`scripts/deploy_funasr_model.py`](../scripts/deploy_funasr_model.py) 把模型下载到本地缓存目录，`--backend qwen` 会下载 `Qwen/Qwen3-ASR-1.7B`
- Python 依赖见 [`scripts/requirements-asr-base.txt`](../scripts/requirements-asr-base.txt) 和 [`scripts/requirements-asr-runtime.txt`](../scripts/requirements-asr-runtime.txt)

### 推荐部署步骤

1. `scripts/voiceinput.sh bootstrap`
2. 如果想在部署后直接验证，可以传入 `--audio-file /path/to/audio.wav`
3. 或者手动执行 `uv venv .venv`
4. `uv pip install -r scripts/requirements-asr-base.txt`
5. `uv pip install -r scripts/requirements-asr-runtime.txt`
6. `uv run -- python scripts/deploy_funasr_model.py --skip-existing`
7. 如果要部署 Qwen 模型，可以运行 `uv run -- python scripts/deploy_funasr_model.py --backend qwen --model-id Qwen/Qwen3-ASR-1.7B --skip-existing`
8. 确认模型目录存在于 `./models/FunAudioLLM/Fun-ASR-Nano-2512`，或者 Qwen 的 `./models/Qwen/Qwen3-ASR-1.7B`
9. `scripts/voiceinput.sh bootstrap` 内部会先装 base 和 runtime，再部署模型，这样 Mac 上能正确检测 MPS
10. `scripts/voiceinput.sh bootstrap --model qwen` 会直接走 Qwen 的下载和部署路径，`--backend qwen` 也兼容
11. 统一入口可写成 `scripts/voiceinput.sh bootstrap --model qwen`

### 系统级安装

1. `scripts/voiceinput.sh macos install`
2. 如果只想打包，不安装，可以运行 `scripts/voiceinput.sh macos package`
3. 日常调试刷新可以运行 `scripts/voiceinput.sh macos reinstall`
4. 如果要把已安装的 VoiceInput 注册进 pluginkit，并把它写进当前用户的输入法偏好，可以运行 `scripts/voiceinput.sh macos enable`
5. 如果想一条命令完成打包 + 刷新 + 启用，可以运行 `scripts/voiceinput.sh macos dev-install`
6. 如果要看系统到底有没有登记这个输入法，可以运行 `scripts/voiceinput.sh macos dump-state`
7. 统一入口可写成 `scripts/voiceinput.sh macos install`
8. 安装脚本会把 `dist/VoiceInput.app` 复制到 `~/Library/Input Methods/`
9. 这个包现在包含一个容器 app 和一个 `Contents/PlugIns/VoiceInput.appex` extension
10. 安装和调试刷新子命令都会自动执行启用步骤
11. 重新登录或重启输入法服务
12. 系统输入法列表里选择 VoiceInput
13. 首次运行前建议授予“麦克风”和“辅助功能”权限

如果系统输入法列表里找不到 `VoiceInput`，先按这个顺序检查：

1. `ls ~/Library/Input\ Methods/VoiceInput.app`
2. 如果文件不存在，说明安装没有完成，重新运行安装脚本
3. 如果文件存在，先注销再登录一次，或者重启系统输入法相关服务
4. 如果 bundle 是从下载包或外部目录拷贝来的，再执行：`xattr -dr com.apple.quarantine ~/Library/Input\ Methods/VoiceInput.app`
5. 仍然没有出现时，回看安装日志里是否有编译、复制或权限错误
6. 运行 `scripts/voiceinput.sh macos dump-state` 看 `VoiceInput` 是否已经进入 TIS 列表，以及 extension 是否存在

### GPU 处理

- 部署脚本会在 Linux / Windows 上自动检测 NVIDIA GPU
- 如果检测到 NVIDIA，就把 inference hint 设为 `cuda`
- 在 macOS 上，会检测 PyTorch 的 MPS 支持，存在时选择 `mps`
- 默认不会自动安装 CUDA；如果你传了 `--install-cuda`，并且机器上检测到 NVIDIA GPU，脚本会尝试安装 CUDA 版 PyTorch wheels
- 如果不传 `--install-cuda`，你仍然需要在运行推理的机器上准备好 CUDA 版 PyTorch 和匹配的 NVIDIA 驱动 / runtime

### Smoke 路径

- `uv run -- cargo run -p voice-input-macos -- --audio-file /path/to/audio.wav`
- 或者 `scripts/voiceinput.sh macos smoke --audio-file /path/to/audio.wav`
- 或者直接 `scripts/voiceinput.sh bootstrap --audio-file /path/to/audio.wav`

## 推荐推进顺序

1. 先把 core 状态机定死
2. 再做一个平台宿主跑通端到端
3. 然后把宿主边界推广到其他平台
4. 最后补转写后端的完整集成

## Linux 运行建议

Ubuntu 20.04 上优先用 IBus 跑通最小闭环。

建议先准备这些系统依赖：

- `build-essential`
- `pkg-config`
- `libdbus-1-dev`
- `libibus-1.0-dev`
- `python3`
- `python3-venv`
- `python3-pip`
- `libx11-dev`

如果还要让 Rust 侧录音后端可用，再补：

- `libasound2-dev`
- `portaudio19-dev`

然后可以先跑：

```bash
cargo run -p voice-input-linux --features ibus -- --audio-file /path/to/audio.wav
```

或者：

```bash
scripts/voiceinput.sh linux smoke --audio-file /path/to/audio.wav
```

常驻版可以这样启动：

```bash
cargo run -p voice-input-linux --bin voice-input-linux-app -- --backend ibus
```

启动后会常驻在托盘里，菜单提供状态、停止当前录音和退出；平时还是用全局热键开始录音。

如果你是直接用 `cargo run`，记得加上 `--features ibus`，否则 IBus 会退回成未绑定的占位实现。

如果想要真正的一键启动，可以直接用：

```bash
scripts/voiceinput.sh linux install
```

默认会先跑 Linux bootstrap，自动安装缺失的 Ubuntu 20.04 常用依赖，然后启动常驻托盘版；传入 `--audio-file` 时会改成 smoke 模式。
如果要在安装时切到 Qwen，可以传入 `--model qwen`；这个参数会原样透传给 `scripts/voiceinput.sh bootstrap`。
统一入口也可以这样用：`scripts/voiceinput.sh linux install`

当前 Linux 目标是先把转写结果通过 IBus 宿主送进当前焦点窗口。Fcitx5 还保留为后续单独接 native bindings 的路线。
