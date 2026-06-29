# 纯 Linux 语音输入法重构

日期：2026-06-29

## 目标

移除 macOS 和 Windows 平台支持，将项目精简为纯 Linux 语音输入法。删除 `voice-input-runtime` 和 `voice-input-cli` crate，将其逻辑合并到 `voice-input-linux` 中，使 `voice-input-linux` 成为唯一的二进制目标 crate。

## Crate 变更总览

| Crate | 动作 | 说明 |
|---|---|---|
| `voice-input-linux` | **保留+膨胀** | 吸收 runtime、cli，成为唯一二进制目标 |
| `voice-input-core` | 保留 | 纯状态机和 trait 定义，无平台依赖 |
| `voice-input-asr` | 保留 | ASR 配置/runner/transcriber |
| `voice-input-audio` | 保留 | 音频文件/PCM/WAV 工具 |
| `voice-input-runtime` | **删除** | host/local/live 逻辑合并到 linux |
| `voice-input-cli` | **删除** | CLI 入口合并到 linux |
| `voice-input-macos` | **删除** | 整个 crate |
| `voice-input-windows` | **删除** | 整个 crate |

## voice-input-linux 合并后模块

```
crates/voice-input-linux/src/
  main.rs          ← 统一二进制入口（原 cli main + linux main）
  lib.rs           ← 精简 re-export
  backend.rs       ← IBus/Fcitx5 后端（保留）
  host.rs          ← 合并 runtime::host 的 CompositionDriver + StatefulInputMethodHost
  hotkey.rs        ← 全局热键监听（保留）
  ibus.rs          ← IBus 桥接（保留）
  recorder.rs      ← 音频录制（保留）
  session.rs       ← 组合会话状态（保留）
  settings.rs      ← 应用设置/持久化（保留）
  tray.rs          ← 系统托盘（保留）
  live.rs          ← 合并 runtime::live 的实时循环/任务调度
  local.rs         ← 合并 runtime::local 的本地运行时配置/LocalVoiceInputRuntime
  smoke.rs         ← 合并 cli::smoke 的 smoke 测试逻辑
```

### 模块职责

**main.rs** — 唯一的二进制入口点。解析 CLI 参数（smoke/live 子命令 + 平台无关参数），分发到 `run_smoke` 或 `run_live_with_args`。

**lib.rs** — 对外暴露的公共类型。精简到只保留 `voice-input-cli` 原本需要的那些 re-export。

**host.rs** — 将 `voice-input-runtime::host` 的 `CompositionDriver` trait 和 `StatefulInputMethodHost` 移入，去掉泛型多层包装。Linux 下只有一个 driver 实现 (`LinuxInputMethodHost`)，不需要 trait 抽象间接层。

**live.rs** — 从 `voice-input-runtime::live` 移入实时语音输入的事件循环、任务排队、流式预览逻辑。这些函数原本通过 runtime crate 间接调用 linux crate 的组件，合并后可以直接调用。

**local.rs** — 从 `voice-input-runtime::local` 移入 `LocalVoiceInputRuntime`、`LocalVoiceInputConfig` 及相关的参数解析函数。去掉 `LocalRuntimeMetadata` trait（原来为多平台设计，合并后不需要抽象间接层）。

**smoke.rs** — 原来在 `voice-input-cli::smoke` 和 `voice-input-linux::smoke` 中各有一份，合并为一份。

## 依赖关系

```
voice-input-linux
  ├── voice-input-core     （IME 状态机、trait 定义）
  ├── voice-input-asr      （ASR 转写）
  ├── voice-input-audio    （音频处理）
  └── 外部 crate（ibus、cpal、arboard、ksni 等）

voice-input-asr
  └── voice-input-core

voice-input-audio
  （无内部依赖）
```

`voice-input-linux` 不再依赖 `voice-input-runtime` 和 `voice-input-cli`，原本通过这两个 crate 间接使用的类型（`CompositionDriver`、`StatefulInputMethodHost`、`LocalVoiceInputRuntime` 等）直接定义或使用。

## 工作区 Cargo.toml

```toml
[workspace]
members = [
    "crates/voice-input-core",
    "crates/voice-input-linux",
    "crates/voice-input-asr",
    "crates/voice-input-audio",
]
resolver = "2"
```

## 脚本变更

`scripts/voiceinput.sh` 移除以下子命令：
- `macos install` / `macos package` / `macos smoke` / `macos dev-install`
- `windows install` / `windows smoke`
- 对应的 `voiceinput_macos_*_impl` 和 `voiceinput_windows_*_impl` 函数

保留的子命令：
- `bootstrap` — 环境准备、Python 依赖、模型部署
- `model` — 切换默认模型
- `linux install` / `linux uninstall` — 安装/卸载常驻版 + systemd 自启
- `linux smoke` — 冒烟测试
- `linux dev` / `linux dev-streaming` — 开发模式（FunASR 流式服务）

`voiceinput_run_cli_linux` 函数不再需要 `--features linux-ibus-smoke` flag（合并后 feature 可能不再需要，或直接成为默认行为）。

## README 变更

移除：
- macOS 常驻 app 章节
- Windows 快速开始章节
- 所有 macos/windows 的 smoke/live 命令示例
- macOS app 打包相关内容

保留并更新：
- Linux 快速开始
- 命令入口（CLI 子命令只保留 linux）
- 模型部署
- Python 环境
- Smoke/Live 流程（仅 Linux）

## 编译入口

重构后唯一的编译/运行方式：

```bash
# Smoke 测试
cargo run -p voice-input-linux -- smoke --audio-file testdata/smoke.wav --backend ibus

# Live 常驻
cargo run -p voice-input-linux -- live --backend ibus

# 脚本封装
scripts/voiceinput.sh linux smoke --audio-file testdata/smoke.wav
scripts/voiceinput.sh linux install
```

## 不在范围内

- 不改变 IBus/Fcitx5 后端的行为逻辑
- 不修改 FunASR/Python 环境部署流程
- 不改动 `voice-input-core`、`voice-input-asr`、`voice-input-audio` 的公开 API（除了移除 runtime 依赖的引用）
- 不添加新功能

## 风险点

1. **编译中断**：合并后 `voice-input-linux` 的 `Cargo.toml` 依赖需要仔细更新，确保所有原来通过 runtime/cli 间接获得的类型都能正确解析
2. **IBus feature flag**：目前 `linux-ibus-smoke` feature 在 cli crate 中控制。合并后需评估是保留 feature 还是直接设为默认行为
3. **脚本兼容**：`voiceinput.sh` 中的 `voiceinput_run_cli_linux` 函数当前硬编码了 `--features linux-ibus-smoke`，合并后需要更新
