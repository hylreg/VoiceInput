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

## 模型部署

1. `scripts/voiceinput.sh bootstrap`
2. `scripts/voiceinput.sh bootstrap --audio-file testdata/smoke.wav`
3. 一键部署会先安装依赖，再下载模型

## Linux 安装

1. `scripts/voiceinput.sh linux install`
2. 如果要安装时切模型，可以传入 `--model qwen` 或 `--model qwen-0.6b`
3. 默认会设置 systemd 开机自启，可使用 `--no-autostart` 跳过
4. `scripts/voiceinput.sh linux uninstall` 可移除常驻版及开机自启

日常调试链路：

1. 改代码
2. 运行 `scripts/voiceinput.sh linux install`
3. 直接在前台应用里验证输入效果
