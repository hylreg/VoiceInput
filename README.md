# VoiceInput

Linux 语音输入法：双击 Alt 开始/停止录音，本地 ASR 转写后自动注入活动窗口。

## 架构

- `voice-input-core`：错误类型、trait 定义、`RecordedAudio` 公共类型
- `voice-input-asr`：ASR 配置（catalog 编译期嵌入）、常驻 Python worker
- `voice-input-audio`：WAV/PCM 处理、文件录音
- `voice-input-linux`：Linux 平台实现、CLI 入口、常驻托盘

## 快速开始

```bash
# 一键安装：装系统依赖 → 建 Python 环境 → 下载模型 → 编译 → systemd 自启 → 启动托盘
scripts/voiceinput.sh linux install

# 指定模型（默认 qwen-0.6b）
scripts/voiceinput.sh linux install --model qwen

# 不设开机自启 / 只装不启动
scripts/voiceinput.sh linux install --no-autostart
scripts/voiceinput.sh linux install --no-launch

# 只部署环境并跑 smoke 验证（不安装服务、不启动托盘）
scripts/voiceinput.sh linux install --audio-file testdata/smoke.wav
```

系统依赖（Ubuntu 20.04）：`build-essential`、`python3`、`python3-venv`、`python3-pip`，以及 ASR 所需的 Python ≥ 3.12（20.04 默认 3.8，需手动安装 `python3.12`）。`pkg-config`、`libdbus-1-dev`、`libibus-1.0-dev`、`libx11-dev`、`libasound2-dev`、`portaudio19-dev` 六个编译库由安装脚本自动补齐。

默认热键：**双击 Alt**（可配 `--activation-hotkey DoubleCtrl`）。

## 命令入口

```bash
# smoke：音频文件 → 转写 → 注入活动窗口
cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus

# live：常驻托盘，热键触发录音
cargo run -p voice-input-linux --features ibus -- live --backend ibus \
  [--activation-hotkey DoubleAlt] [--double-press-window-ms 300] [--silence-stop-ms 1500]
```

直接 `cargo run` 前需先 `scripts/voiceinput.sh bootstrap` 准备 Python 环境与模型（见下文 Python 环境）。

`--backend` 只接受 `ibus`（其他值报错提示）。

## 模型

通过 `scripts/voiceinput.sh model <模型名>` 切换，或给 bootstrap/install/smoke 传 `--model`：

| 模型 | 规模 | 特点 |
|---|---|---|
| `funasr`（FunASR Nano） | ~100MB | 最轻量，启动快，低配机器 |
| `qwen-0.6b`（Qwen3-ASR-0.6B，默认） | 0.6B | 精度/资源平衡，多数用户推荐 |
| `qwen`（Qwen3-ASR-1.7B） | 1.7B | 最高精度，需要 GPU 或充足内存 |

切换后重新运行 `scripts/voiceinput.sh linux install` 部署（会自动下载模型并更新服务）。

模型 catalog 单一来源：`config/models.json`（编译期嵌入 Rust 侧）；`config/voiceinput.env` 是由 catalog 生成的仓库级默认配置（脚本维护）。

## 服务管理

```bash
systemctl --user start voice-input.service    # start 可换 stop / status
systemctl --user disable voice-input.service  # 取消开机自启（保留安装）
journalctl --user -u voice-input.service -f
scripts/voiceinput.sh linux uninstall   # 移除服务与自启
```

## Python 环境（开发）

```bash
uv venv .venv
uv pip install -r scripts/requirements-asr-base.txt -r scripts/requirements-asr-runtime.txt
scripts/voiceinput.sh bootstrap [--audio-file testdata/smoke.wav]
```
