# VoiceInput

Linux 语音输入法项目。

## 架构

- `voice-input-core`：纯业务状态机和 trait 定义
- `voice-input-asr`：ASR 配置、runner、transcriber
- `voice-input-audio`：文件录音、PCM/WAV 公共处理
- `voice-input-linux`：Linux 平台实现、CLI 入口、常驻托盘

## Linux 快速开始

1. Ubuntu 20.04 上先装 `build-essential`、`pkg-config`、`libdbus-1-dev`、`libibus-1.0-dev`、`python3`、`python3-venv`、`python3-pip`
2. 如果要让 Rust 侧录音后端也可用，再补 `libasound2-dev` 和 `portaudio19-dev`
3. 如果要用 Linux 全局热键监听，再补 `libx11-dev`
4. `scripts/voiceinput.sh bootstrap`
5. `cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus`
6. `scripts/voiceinput.sh linux install`
7. Linux 默认热键是双击 Ctrl
8. 如果要切模型，可以加 `--model qwen` 或 `--model qwen-0.6b`
9. `--backend` 只影响 Linux 宿主后端
10. 常驻版也可直接走 `cargo run -p voice-input-linux --features ibus -- live --backend ibus`

## 命令入口

- `cargo run -p voice-input-linux --features ibus -- <smoke|live> [args]`：统一 CLI 入口
- `scripts/voiceinput.sh ...`：环境准备、模型部署、安装/常驻入口

## Smoke 流程

```bash
cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus
```

## Live 流程

```bash
cargo run -p voice-input-linux --features ibus -- live --backend ibus
```

可额外传 `--double-ctrl-window-ms` 和 `--silence-stop-ms`。

## 脚本入口

- `scripts/voiceinput.sh`：统一入口
- `config/models.json`：模型 catalog 单一来源
- `config/voiceinput.env`：由 catalog 生成的仓库级默认配置
- `scripts/voiceinput_config.sh`：共享 helper

如果要切默认模型：

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

## 模型

项目支持三种 ASR 模型，通过 `scripts/voiceinput.sh model <模型名>` 切换，或传 `--model <模型名>` 给 bootstrap/install/smoke 子命令。

### FunASR (`funasr`)

- **模型**：[FunAudioLLM/Fun-ASR-Nano-2512](https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512)
- **后端**：FunASR
- **参数量**：~100MB，Nano 级别
- **特点**：最轻量，启动快，内存占用低，适合低配机器或对延迟敏感的场景。中文识别精度中上，适合日常语音输入。
- **适用场景**：资源受限的设备、快速测试、不需要最高精度的日常使用。

### Qwen3-ASR 1.7B (`qwen`)

- **模型**：[Qwen/Qwen3-ASR-1.7B](https://www.modelscope.cn/collections/Qwen/Qwen3-ASR)
- **后端**：Qwen
- **参数量**：1.7B
- **特点**：精度最高的模型，中文识别效果最好，支持多种语言。需要更多显存/内存，加载时间较长。
- **适用场景**：对识别精度要求高的场景，有 GPU 或充足内存的机器。

### Qwen3-ASR 0.6B (`qwen-0.6b`，默认)

- **模型**：[Qwen/Qwen3-ASR-0.6B](https://www.modelscope.cn/collections/Qwen/Qwen3-ASR)
- **后端**：Qwen
- **参数量**：0.6B
- **特点**：Qwen3-ASR 的小型化版本，精度接近 1.7B 但资源占用显著更低，启动更快。当前默认模型。
- **适用场景**：多数用户的推荐选择，在精度和资源消耗之间取得良好平衡。

### 切换模型

```bash
# 切换到 Qwen3-ASR 1.7B（最高精度）
scripts/voiceinput.sh model qwen

# 切换到 FunASR（最轻量）
scripts/voiceinput.sh model funasr

# 切换到 Qwen3-ASR 0.6B（默认，平衡之选）
scripts/voiceinput.sh model qwen-0.6b
```

切换后运行 `scripts/voiceinput.sh bootstrap` 下载对应模型，再 `scripts/voiceinput.sh linux install` 更新常驻服务。

## 模型部署

1. `scripts/voiceinput.sh bootstrap`
2. `scripts/voiceinput.sh bootstrap --audio-file testdata/smoke.wav`
3. 一键部署会先安装依赖，再下载模型

## Linux 安装

### 一键安装（含开机自启）

```bash
scripts/voiceinput.sh linux install
```

这一条命令会依次完成：安装系统依赖 → 准备 Python 环境 → 下载模型 → 编译 release 二进制 → 设置 systemd 用户服务 → 启动常驻托盘。安装完成后语音输入法已在运行，**重启后也会自动启动**。

常用选项：

```bash
# 指定模型（默认 qwen-0.6b）
scripts/voiceinput.sh linux install --model qwen

# 不设置开机自启
scripts/voiceinput.sh linux install --no-autostart

# 安装但不启动（只编译 + 注册服务，不下发 start）
scripts/voiceinput.sh linux install --no-launch

# 安装后立即跑 smoke 验证
scripts/voiceinput.sh linux install --audio-file testdata/smoke.wav
```

### 服务管理

安装完成后通过 systemd 用户服务管理：

```bash
# 启动
systemctl --user start voice-input.service
# 停止
systemctl --user stop voice-input.service
# 查看运行状态
systemctl --user status voice-input.service
# 查看实时日志
journalctl --user -u voice-input.service -f
# 禁用开机自启（保留编译产物）
systemctl --user disable voice-input.service
```

### 手动启动（调试用）

改代码后无需完整重装，可直接编译并启动：

```bash
cargo run -p voice-input-linux --features ibus -- live --backend ibus
```

也可直接用安装时生成的启动脚本（会自动加载模型配置）：

```bash
~/.local/bin/voice-input
```

### 卸载

```bash
scripts/voiceinput.sh linux uninstall
```

移除 systemd 服务、启动脚本，并停用开机自启。
