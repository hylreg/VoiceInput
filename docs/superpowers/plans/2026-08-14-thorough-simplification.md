# VoiceInput 彻底精简 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在外部行为（CLI 参数、环境变量、默认值、systemd 安装/托盘流程）不变的前提下删除死代码、坍缩三层 IME 抽象为单一实现、合并 smoke/live 双流水线、统一模型 catalog 与热键实现。

**Architecture:** 4-crate workspace 保持不变。core 只剩错误类型 + 3 个 trait + `RecordedAudio`；asr 用 `include_str!` 嵌入 `config/models.json`，只保留常驻 worker 协议；audio 统一 PCM 形态；linux 删除 backend/ibus 桥接层，`LinuxInputMethodHost` 直接实现 `InputMethodHost`，smoke 改为直接流程。

**Tech Stack:** Rust 2021, cpal, ksni, arboard, device_query, xdotool, hound, serde, bash

**Spec:** `docs/superpowers/specs/2026-08-14-thorough-simplification-design.md`

---

### Task 0: 提交 spec 修订（Transcript/update_preedit/mock 删除）

**Files:**
- Modify: `docs/superpowers/specs/2026-08-14-thorough-simplification-design.md`

spec 已在会话中修订完成（Transcript 删除、Transcriber 返回 String、MockAudioRecorder/MockInputMethodHost 删除、InputMethodHost 精简为 4 方法），改动已在磁盘上。直接提交。

- [ ] **Step 1: 提交 spec 修订**

```bash
git add docs/superpowers/specs/2026-08-14-thorough-simplification-design.md
git commit -m "docs: 修订精简 spec——Transcript/update_preedit/无消费者 mock 一并删除"
```

---

### Task 1: core 零引用死类型清理

删除 core 中零引用的类型。注意：`HotkeyManager`、`MockHotkeyManager`、`AppController`、`MockAudioRecorder`、`MockInputMethodHost` 仍被 linux 侧使用（Task 4/5 删除），本任务不动。

**Files:**
- Modify: `crates/voice-input-core/src/config.rs`
- Modify: `crates/voice-input-core/src/ime.rs`
- Modify: `crates/voice-input-core/src/platform.rs`
- Modify: `crates/voice-input-core/src/lib.rs`

- [ ] **Step 1: config.rs 删除 InsertionMode/TranscriptionMode 和 AppConfig 的未读字段**

替换 `crates/voice-input-core/src/config.rs` 全部内容：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub activation_hotkey: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            activation_hotkey: "Ctrl+Shift+Space".to_string(),
        }
    }
}
```

- [ ] **Step 2: ime.rs 只保留 Transcript**

替换 `crates/voice-input-core/src/ime.rs` 全部内容（删除 `CompositionState`、`DictationEvent`）：

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub partials: Vec<String>,
    pub final_text: String,
}

impl Transcript {
    pub fn new(final_text: impl Into<String>) -> Self {
        let final_text = final_text.into();
        Self {
            partials: vec![final_text.clone()],
            final_text,
        }
    }
}
```

（Transcript 本身在 Task 5 删除。）

- [ ] **Step 3: platform.rs 删除 TextInjector 和 MockTextInjector**

在 `crates/voice-input-core/src/platform.rs` 中删除以下三处：

```rust
pub trait TextInjector {
    fn inject(&self, text: &str) -> Result<()>;
}
```

```rust
pub struct MockTextInjector;
```

```rust
impl TextInjector for MockTextInjector {
    fn inject(&self, text: &str) -> Result<()> {
        println!("已注入文本：{text}");
        Ok(())
    }
}
```

- [ ] **Step 4: lib.rs 同步 re-exports**

替换 `crates/voice-input-core/src/lib.rs` 全部内容：

```rust
mod config;
mod controller;
mod error;
mod ime;
mod platform;

pub use config::AppConfig;
pub use controller::AppController;
pub use error::{Result, VoiceInputError};
pub use ime::Transcript;
pub use platform::{
    AudioRecorder, HotkeyManager, InputMethodHost, MockAudioRecorder, MockHotkeyManager,
    MockInputMethodHost, MockTranscriber, Transcriber,
};
```

- [ ] **Step 5: 编译验证**

Run: `cargo build --workspace && cargo test --workspace`
Expected: 全绿（被删类型无消费者）

- [ ] **Step 6: 提交**

```bash
git add crates/voice-input-core/
git commit -m "refactor(voice-input-core): 删除零引用的 CompositionState/DictationEvent/TextInjector 及 AppConfig 未读字段"
```

---

### Task 2: asr 精简——单一 catalog + 单一 worker 协议

**Files:**
- Modify: `crates/voice-input-asr/src/config.rs`
- Modify: `crates/voice-input-asr/src/funasr.rs`
- Modify: `crates/voice-input-asr/src/transcriber.rs`

- [ ] **Step 1: config.rs 改为编译期嵌入 catalog**

替换 `crates/voice-input-asr/src/config.rs` 全部内容：

```rust
use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrBackend {
    FunAsr,
    QwenAsr,
}

impl AsrBackend {
    pub fn from_model_id(model_id: &str) -> Self {
        let normalized = model_id.trim().to_ascii_lowercase();
        if normalized.contains("qwen/qwen3-asr") {
            Self::QwenAsr
        } else {
            Self::FunAsr
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FunAsr => "funasr",
            Self::QwenAsr => "qwen",
        }
    }
}

impl Default for AsrBackend {
    fn default() -> Self {
        Self::FunAsr
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ModelCatalog {
    aliases: HashMap<String, String>,
    models: HashMap<String, ModelSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelSpec {
    backend: String,
    model_id: String,
    source_url: String,
    model_dir: String,
    #[serde(default)]
    remote_code: String,
}

/// 编译期嵌入 config/models.json，模型配置单一来源。
static MODEL_CATALOG: OnceLock<ModelCatalog> = OnceLock::new();

fn load_catalog() -> &'static ModelCatalog {
    MODEL_CATALOG.get_or_init(|| {
        let json = include_str!("../../../config/models.json");
        serde_json::from_str(json).expect("config/models.json should be valid")
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunAsrConfig {
    pub backend: AsrBackend,
    pub model_id: String,
    pub source_url: String,
    pub model_dir: PathBuf,
    pub remote_code: PathBuf,
    pub device: String,
    pub language: String,
    pub itn: bool,
    pub hotwords: Vec<String>,
}

impl Default for FunAsrConfig {
    fn default() -> Self {
        Self::funasr_default()
    }
}

impl FunAsrConfig {
    fn from_spec(spec: &ModelSpec) -> Self {
        Self {
            backend: match spec.backend.as_str() {
                "qwen" => AsrBackend::QwenAsr,
                _ => AsrBackend::FunAsr,
            },
            model_id: spec.model_id.clone(),
            source_url: spec.source_url.clone(),
            model_dir: PathBuf::from(&spec.model_dir),
            remote_code: PathBuf::from(&spec.remote_code),
            device: "auto".to_string(),
            language: "中文".to_string(),
            itn: true,
            hotwords: Vec::new(),
        }
    }

    /// 按别名查 catalog。catalog 是编译期嵌入的，三个内置模型必然存在。
    fn model_spec_by_alias(alias: &str) -> Option<ModelSpec> {
        let catalog = load_catalog();
        let normalized = catalog.aliases.get(&alias.trim().to_ascii_lowercase())?;
        catalog.models.get(normalized).cloned()
    }

    pub fn funasr_default() -> Self {
        let spec = Self::model_spec_by_alias("funasr").expect("catalog 缺少 funasr");
        Self::from_spec(&spec)
    }

    pub fn qwen3_asr_1_7b_default() -> Self {
        let spec = Self::model_spec_by_alias("qwen").expect("catalog 缺少 qwen");
        Self::from_spec(&spec)
    }

    pub fn qwen3_asr_0_6b_default() -> Self {
        let spec = Self::model_spec_by_alias("qwen-0.6b").expect("catalog 缺少 qwen-0.6b");
        Self::from_spec(&spec)
    }

    pub fn for_model_id(model_id: impl Into<String>) -> Self {
        let model_id = model_id.into();
        match AsrBackend::from_model_id(&model_id) {
            AsrBackend::FunAsr => {
                let mut config = Self::funasr_default();
                config.model_id = model_id;
                config
            }
            AsrBackend::QwenAsr => {
                let mut config = if model_id
                    .to_ascii_lowercase()
                    .contains("qwen/qwen3-asr-0.6b")
                {
                    Self::qwen3_asr_0_6b_default()
                } else {
                    Self::qwen3_asr_1_7b_default()
                };
                config.model_id = model_id;
                config
            }
        }
    }

    pub fn from_env() -> Self {
        let mut config = if let Ok(model_id) = env::var("VOICEINPUT_ASR_MODEL_ID") {
            Self::for_model_id(model_id)
        } else {
            Self::default()
        };

        if let Ok(model_name) = env::var("VOICEINPUT_ASR_MODEL") {
            if let Some(spec) = Self::model_spec_by_alias(&model_name) {
                config = Self::from_spec(&spec);
            }
        }

        if let Ok(backend) = env::var("VOICEINPUT_ASR_BACKEND") {
            match backend.trim().to_ascii_lowercase().as_str() {
                "funasr" => {
                    config.backend = AsrBackend::FunAsr;
                }
                "qwen" | "qwen3" | "qwen-asr" => {
                    config.backend = AsrBackend::QwenAsr;
                }
                _ => {}
            }
        }

        if let Ok(model_id) = env::var("VOICEINPUT_ASR_MODEL_ID") {
            config.model_id = model_id;
        }
        if let Ok(source_url) = env::var("VOICEINPUT_ASR_SOURCE_URL") {
            config.source_url = source_url;
        }
        if let Ok(model_dir) = env::var("VOICEINPUT_ASR_MODEL_DIR") {
            config.model_dir = PathBuf::from(model_dir);
        }
        if let Ok(remote_code) = env::var("VOICEINPUT_ASR_REMOTE_CODE") {
            config.remote_code = PathBuf::from(remote_code);
        }
        if let Ok(device) = env::var("VOICEINPUT_ASR_DEVICE") {
            config.device = device;
        }
        if let Ok(language) = env::var("VOICEINPUT_ASR_LANGUAGE") {
            config.language = language;
        }
        if let Ok(itn) = env::var("VOICEINPUT_ASR_ITN") {
            config.itn = !matches!(
                itn.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no"
            );
        }
        if let Ok(hotwords) = env::var("VOICEINPUT_ASR_HOTWORDS") {
            config.hotwords = hotwords
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect();
        }

        if config.model_id.trim().is_empty() {
            config.model_id = match config.backend {
                AsrBackend::FunAsr => "FunAudioLLM/Fun-ASR-Nano-2512".to_string(),
                AsrBackend::QwenAsr => "Qwen/Qwen3-ASR-1.7B".to_string(),
            };
        }

        config
    }

    pub fn is_qwen(&self) -> bool {
        matches!(self.backend, AsrBackend::QwenAsr)
    }

    pub fn qwen_language(&self) -> Option<String> {
        let language = self.language.trim();
        if language.is_empty()
            || language.eq_ignore_ascii_case("auto")
            || language.eq_ignore_ascii_case("automatic")
            || language.eq_ignore_ascii_case("自动")
        {
            return None;
        }

        let lower = language.to_ascii_lowercase();
        let normalized = match lower.as_str() {
            "中文" | "zh" | "zh-cn" | "zh-hans" | "chinese" => "Chinese",
            "英文" | "en" | "english" => "English",
            "日文" | "ja" | "japanese" => "Japanese",
            "韩文" | "ko" | "korean" => "Korean",
            "粤语" | "cantonese" => "Cantonese",
            "法语" | "french" => "French",
            "德语" | "german" => "German",
            "西班牙语" | "spanish" => "Spanish",
            "葡萄牙语" | "portuguese" => "Portuguese",
            _ => language,
        };

        Some(normalized.to_string())
    }
}
```

- [ ] **Step 2: funasr.rs 删除一次性脚本路径，worker 非 Option**

替换 `crates/voice-input-asr/src/funasr.rs` 全部内容（删除 `PYTHON_SCRIPT`、`PYTHON_QWEN_SCRIPT` 一次性脚本、`serde_json_like_array` 手写 JSON 转义、`python_bin` 字段与 `Default` impl；`connect` 里 `resolve_python_bin()` 的优先级解析逻辑原样保留）：

```rust
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use tempfile::NamedTempFile;

use crate::config::FunAsrConfig;
use crate::runner::{FunAsrRequest, FunAsrRunner};
use voice_input_core::{Result, VoiceInputError};

const PYTHON_WORKER_SCRIPT: &str = r#"
import json
import os
import sys
import contextlib
from funasr import AutoModel
import torch

model_dir = sys.argv[1]
remote_code = sys.argv[2]
device = sys.argv[3]

if remote_code:
    candidate = remote_code
    if not os.path.isabs(candidate):
        local_candidate = os.path.join(model_dir, candidate)
        if os.path.exists(local_candidate):
            candidate = local_candidate
    if os.path.isdir(candidate):
        candidate = os.path.join(candidate, "model.py")
    if os.path.exists(os.path.join(model_dir, "model.py")):
        candidate = os.path.join(model_dir, "model.py")
    remote_code = candidate

if device == "auto":
    if torch.cuda.is_available():
        device = "cuda"
    elif hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
        device = "mps"
    else:
        device = "cpu"

with contextlib.redirect_stdout(sys.stderr):
    model = AutoModel(
        model=model_dir,
        trust_remote_code=True,
        remote_code=remote_code,
        device=device,
        disable_update=True,
        log_level="ERROR",
    )

print(json.dumps({"ready": True}), flush=True)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue

    if line == "__quit__":
        break

    try:
        request = json.loads(line)
        audio_path = request["audio_path"]
        language = request["language"]
        itn = request["itn"]
        hotwords = request["hotwords"]

        with contextlib.redirect_stdout(sys.stderr):
            res = model.generate(
                input=[audio_path],
                cache={},
                batch_size=1,
                hotwords=hotwords,
                language=language,
                itn=itn,
            )

        text = res[0]["text"].strip()
        print(json.dumps({"text": text}), flush=True)
    except Exception as exc:
        print(json.dumps({"error": str(exc)}), flush=True)
"#;

const PYTHON_QWEN_WORKER_SCRIPT: &str = r#"
import contextlib
import json
import os
import sys

import torch
from transformers.utils import logging as hf_logging
from qwen_asr import Qwen3ASRModel

model_dir = sys.argv[1]
device = sys.argv[2]

hf_logging.set_verbosity_error()

if device == "cuda":
    device_map = "cuda:0"
    dtype = torch.bfloat16
else:
    device_map = "cpu"
    dtype = torch.float32


def sanitize_generation_config(model):
    generation_config = getattr(model, "generation_config", None)
    if generation_config is None:
        return

    temperature = getattr(generation_config, "temperature", None)
    if temperature is not None:
        try:
            generation_config.temperature = None
        except Exception:
            pass

    pad_token_id = getattr(generation_config, "pad_token_id", None)
    if pad_token_id is None:
        eos_token_id = getattr(generation_config, "eos_token_id", None)
        if eos_token_id is None:
            eos_token_id = getattr(getattr(model, "config", None), "eos_token_id", None)
        if eos_token_id is not None:
            try:
                generation_config.pad_token_id = eos_token_id
            except Exception:
                pass
            try:
                model.config.pad_token_id = eos_token_id
            except Exception:
                pass

    try:
        generation_config.validate(is_init=False)
    except Exception:
        pass

with contextlib.redirect_stdout(sys.stderr):
    model = Qwen3ASRModel.from_pretrained(
        model_dir,
        device_map=device_map,
        dtype=dtype,
        max_inference_batch_size=1,
    )
    sanitize_generation_config(model)

print(json.dumps({"ready": True}), flush=True)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue

    if line == "__quit__":
        break

    try:
        request = json.loads(line)
        audio_path = request["audio_path"]
        language = request.get("language")
        if language in ("", None, "auto", "automatic", "自动"):
            language = None

        with contextlib.redirect_stdout(sys.stderr):
            res = model.transcribe(
                audio=audio_path,
                context="",
                language=language,
                return_time_stamps=False,
            )

        text = ""
        if isinstance(res, list) and res:
            first = res[0]
            text = getattr(first, "text", "")
            if not text and isinstance(first, dict):
                text = str(first.get("text", ""))
        elif isinstance(res, dict):
            text = str(res.get("text", ""))
        elif hasattr(res, "text"):
            text = str(getattr(res, "text"))

        print(json.dumps({"text": text.strip()}), flush=True)
    except Exception as exc:
        print(json.dumps({"error": str(exc)}), flush=True)
"#;

fn python_command(python_bin: &str, script: &str) -> Command {
    if python_bin == "uv" {
        let mut command = Command::new(python_bin);
        command
            .arg("run")
            .arg("--")
            .arg("python")
            .arg("-c")
            .arg(script);
        command
    } else {
        let mut command = Command::new(python_bin);
        command.arg("-c").arg(script);
        command
    }
}

fn read_ready_json<R: BufRead>(
    reader: &mut R,
    empty_message: &str,
    wait_error_message: &str,
) -> Result<serde_json::Value> {
    let mut line = String::new();

    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| VoiceInputError::Transcription(format!("{wait_error_message}：{e}")))?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(empty_message.to_string()));
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };

        if json.get("ready").and_then(|value| value.as_bool()) == Some(true) {
            return Ok(json);
        }
    }
}

/// 按优先级解析 Python 解释器：PYTHON_BIN 环境变量 > uv > .venv > python3。
fn resolve_python_bin() -> String {
    if let Ok(python_bin) = std::env::var("PYTHON_BIN") {
        return python_bin;
    }

    if std::env::var("VOICEINPUT_USE_UV")
        .map(|v| v != "0")
        .unwrap_or(true)
        && which::which("uv").is_ok()
    {
        return "uv".to_string();
    }

    let venv_python = PathBuf::from(".venv/bin/python");
    if venv_python.exists() {
        return venv_python.to_string_lossy().to_string();
    }

    "python3".to_string()
}

pub struct PythonFunAsrRunner {
    worker: Arc<Mutex<PythonFunAsrWorker>>,
}

struct PythonFunAsrWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl PythonFunAsrRunner {
    pub fn connect(config: FunAsrConfig) -> Result<Self> {
        let python_bin = resolve_python_bin();
        let worker = PythonFunAsrWorker::spawn(&python_bin, &config)?;
        Ok(Self {
            worker: Arc::new(Mutex::new(worker)),
        })
    }
}

impl PythonFunAsrWorker {
    fn spawn(python_bin: &str, config: &FunAsrConfig) -> Result<Self> {
        let script = if config.is_qwen() {
            PYTHON_QWEN_WORKER_SCRIPT
        } else {
            PYTHON_WORKER_SCRIPT
        };

        let mut command = python_command(python_bin, script);
        if config.is_qwen() {
            command
                .arg(&config.model_dir)
                .arg(&config.device)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
        } else {
            command
                .arg(&config.model_dir)
                .arg(&config.remote_code)
                .arg(&config.device)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
        }

        let mut child = command
            .spawn()
            .map_err(|e| VoiceInputError::Transcription(format!("启动 ASR worker 失败：{e}")))?;

        let stdin = child.stdin.take().ok_or_else(|| {
            VoiceInputError::Transcription("获取 ASR worker stdin 失败".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            VoiceInputError::Transcription("获取 ASR worker stdout 失败".to_string())
        })?;
        let mut stdout = BufReader::new(stdout);

        let _ready = read_ready_json(
            &mut stdout,
            "ASR worker 启动后没有返回就绪信号",
            "等待 ASR worker 就绪失败",
        )?;

        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    fn transcribe(&mut self, audio_path: &Path, request: &FunAsrRequest) -> Result<String> {
        let payload = if request.config.is_qwen() {
            serde_json::json!({
                "audio_path": audio_path,
                "language": request.config.qwen_language(),
            })
        } else {
            serde_json::json!({
                "audio_path": audio_path,
                "language": request.config.language,
                "itn": request.config.itn,
                "hotwords": request.config.hotwords,
            })
        };
        serde_json::to_writer(&mut self.stdin, &payload).map_err(|e| {
            VoiceInputError::Transcription(format!("写入 ASR worker 请求失败：{e}"))
        })?;
        self.stdin.write_all(b"\n").map_err(|e| {
            VoiceInputError::Transcription(format!("发送 ASR worker 请求失败：{e}"))
        })?;
        self.stdin.flush().map_err(|e| {
            VoiceInputError::Transcription(format!("刷新 ASR worker 请求失败：{e}"))
        })?;

        let mut response = String::new();
        let read = self.stdout.read_line(&mut response).map_err(|e| {
            VoiceInputError::Transcription(format!("读取 ASR worker 响应失败：{e}"))
        })?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "ASR worker 已退出".to_string(),
            ));
        }

        let json: serde_json::Value = serde_json::from_str(response.trim()).map_err(|e| {
            VoiceInputError::Transcription(format!("解析 ASR worker 响应失败：{e}"))
        })?;
        if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
            return Err(VoiceInputError::Transcription(format!(
                "ASR worker 返回错误：{error}"
            )));
        }

        let text = json
            .get("text")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                VoiceInputError::Transcription("ASR worker 响应缺少 text".to_string())
            })?;
        Ok(text.trim().to_string())
    }

    fn shutdown(&mut self) {
        let _ = self.stdin.write_all(b"__quit__\n");
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl FunAsrRunner for PythonFunAsrRunner {
    fn transcribe(&self, request: FunAsrRequest) -> Result<String> {
        let mut audio_file = NamedTempFile::new()
            .map_err(|e| VoiceInputError::Transcription(format!("创建临时音频文件失败：{e}")))?;
        audio_file
            .write_all(&request.audio_bytes)
            .map_err(|e| VoiceInputError::Transcription(format!("写入临时音频文件失败：{e}")))?;

        let mut worker = self.worker.lock().map_err(|_| {
            VoiceInputError::Transcription("锁定 ASR worker 失败".to_string())
        })?;
        worker.transcribe(audio_file.path(), &request)
    }
}

impl Drop for PythonFunAsrWorker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{read_ready_json, PythonFunAsrRunner, PythonFunAsrWorker};
    use crate::runner::{FunAsrRequest, FunAsrRunner};
    use std::io::BufReader;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};

    #[test]
    fn qwen_transcribe_prefers_reused_worker_when_available() {
        let worker = spawn_test_worker();
        let runner = PythonFunAsrRunner {
            worker: Arc::new(Mutex::new(worker)),
        };

        let transcript = runner
            .transcribe(FunAsrRequest {
                audio_bytes: b"fake wav bytes".to_vec(),
                config: crate::config::FunAsrConfig::qwen3_asr_0_6b_default(),
            })
            .expect("worker-backed qwen transcription should succeed");

        assert_eq!(transcript, "worker-ok");
    }

    fn spawn_test_worker() -> PythonFunAsrWorker {
        let script = r#"
import json
import sys

print("funasr version: test-noise", flush=True)
print(json.dumps({"ready": True}), flush=True)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    if line == "__quit__":
        break
    json.loads(line)
    print(json.dumps({"text": "worker-ok"}), flush=True)
    break
"#;

        let mut child = Command::new("python3")
            .arg("-c")
            .arg(script)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("spawn test worker");

        let stdin = child.stdin.take().expect("test worker stdin");
        let stdout = child.stdout.take().expect("test worker stdout");
        let mut stdout = BufReader::new(stdout);

        let ready = read_ready_json(
            &mut stdout,
            "test worker did not report ready",
            "read worker ready line failed",
        )
        .expect("parse worker ready line");
        assert_eq!(
            ready.get("ready").and_then(|value| value.as_bool()),
            Some(true)
        );

        PythonFunAsrWorker {
            child,
            stdin,
            stdout,
        }
    }
}

#[derive(Clone, Default)]
pub struct MockFunAsrRunner {
    pub transcript: String,
    pub calls: Arc<Mutex<Vec<FunAsrConfig>>>,
}

impl FunAsrRunner for MockFunAsrRunner {
    fn transcribe(&self, request: FunAsrRequest) -> Result<String> {
        self.calls
            .lock()
            .map_err(|_| VoiceInputError::Transcription("记录 FunASR 调用失败".to_string()))?
            .push(request.config);

        Ok(self.transcript.clone())
    }
}
```

- [ ] **Step 3: transcriber.rs 删除 config() accessor，错误消息去 FunASR 化**

替换 `crates/voice-input-asr/src/transcriber.rs` 全部内容（`Transcriber` trait 仍返回 `Transcript`，Task 5 才切换为 String——本任务保持编译绿色）：

```rust
use crate::config::FunAsrConfig;
use crate::runner::{FunAsrRequest, FunAsrRunner};
use voice_input_core::{Result, Transcriber, Transcript, VoiceInputError};

pub struct LocalFunAsrTranscriber {
    config: FunAsrConfig,
    runner: Box<dyn FunAsrRunner>,
}

impl LocalFunAsrTranscriber {
    pub fn new(config: FunAsrConfig, runner: Box<dyn FunAsrRunner>) -> Self {
        Self { config, runner }
    }

    pub fn transcribe_allow_empty(&self, audio: &[u8]) -> Result<String> {
        self.runner.transcribe(FunAsrRequest {
            audio_bytes: audio.to_vec(),
            config: self.config.clone(),
        })
    }
}

impl Transcriber for LocalFunAsrTranscriber {
    fn transcribe(&self, audio: &[u8]) -> Result<Transcript> {
        let text = self.transcribe_allow_empty(audio)?;

        if text.trim().is_empty() {
            return Err(VoiceInputError::Transcription(
                "ASR 没有返回识别文本，请检查麦克风输入、录音时长或环境噪声".to_string(),
            ));
        }

        Ok(Transcript::new(text))
    }
}
```

- [ ] **Step 4: 编译测试验证**

Run: `cargo build -p voice-input-asr && cargo test -p voice-input-asr`
Expected: 全绿（asr 测试无需改动，catalog 断言值不变）

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-asr/
git commit -m "refactor(voice-input-asr): catalog 编译期嵌入、删除一次性脚本路径、worker 非 Option"
```

---

### Task 3: linux 零引用死代码小清理

**Files:**
- Modify: `crates/voice-input-linux/src/local.rs`
- Modify: `crates/voice-input-linux/src/live_cli.rs`
- Modify: `crates/voice-input-linux/src/tray.rs`
- Modify: `crates/voice-input-linux/src/ibus.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Modify: `crates/voice-input-linux/src/main.rs`

- [ ] **Step 1: 删除 parse_required_audio_file_arg**

在 `crates/voice-input-linux/src/local.rs` 中删除整个函数（`pub fn parse_required_audio_file_arg` 到其结尾 `}`，即「缺少必需参数 --audio-file」的 `Err` 行之后的 `}` 止）。

- [ ] **Step 2: 删除 print_live_usage**

在 `crates/voice-input-linux/src/live_cli.rs` 中删除整个函数（`pub fn print_live_usage()` 到其结尾 `}`，共 5 行）。

- [ ] **Step 3: 删除 LinuxTrayHandle::request_quit**

在 `crates/voice-input-linux/src/tray.rs` 中删除：

```rust
        pub fn request_quit(&self) {
            self.quit_requested.store(true, Ordering::SeqCst);
            let _ = self.handle.update(|tray| {
                tray.quit_requested.store(true, Ordering::SeqCst);
            });
        }
```

- [ ] **Step 4: 删除 capture_active_window（两个 cfg 版本）**

在 `crates/voice-input-linux/src/ibus.rs` 中删除 `#[cfg(feature = "ibus")]` 版本的 `capture_active_window` 函数（含 `#[allow(dead_code)]`，约 26 行）和 `#[cfg(not(feature = "ibus"))]` 版本（约 5 行）。

- [ ] **Step 5: ClipboardRestoreGuard 删 saved 字段和空 Drop**

在 `crates/voice-input-linux/src/ibus.rs` 中，将 `#[cfg(feature = "ibus")]` 版本替换：

```rust
    Ok(ClipboardRestoreGuard {
        _clipboard: clipboard,
        saved,
    })
```

为：

```rust
    Ok(ClipboardRestoreGuard {
        _clipboard: clipboard,
    })
```

将：

```rust
pub struct ClipboardRestoreGuard {
    #[allow(dead_code)]
    _clipboard: Option<arboard::Clipboard>,
    #[allow(dead_code)]
    saved: Option<String>,
}

impl Drop for ClipboardRestoreGuard {
    fn drop(&mut self) {
        // _clipboard 在此处 drop，录音周期结束。
        // 由于 Clipboard 已经持有了足够长时间（通常数秒），
        // 剪贴板管理器有充足时间同步还原后的内容。
    }
}
```

替换为：

```rust
pub struct ClipboardRestoreGuard {
    #[allow(dead_code)]
    _clipboard: Option<arboard::Clipboard>,
}
```

将 `#[cfg(not(feature = "ibus"))]` 版本中的：

```rust
    Ok(ClipboardRestoreGuard {
        _clipboard: None,
        saved: None,
    })
```

替换为：

```rust
    Ok(ClipboardRestoreGuard { _clipboard: None })
```

- [ ] **Step 6: lib.rs 删 re-export**

在 `crates/voice-input-linux/src/lib.rs` 中，将：

```rust
pub use local::{
    build_local_python_runtime_config, LinuxLocalVoiceInput, LinuxLocalVoiceInputConfig,
    LocalVoiceInputConfig, parse_audio_file_with_optional_backend_arg,
    parse_required_audio_file_arg,
};
```

替换为：

```rust
pub use local::{
    build_local_python_runtime_config, LinuxLocalVoiceInput, LinuxLocalVoiceInputConfig,
    LocalVoiceInputConfig, parse_audio_file_with_optional_backend_arg,
};
```

将：

```rust
pub use live_cli::{parse_live_args, print_live_usage, run_live_with_args, LinuxLiveArgs};
```

替换为：

```rust
pub use live_cli::{parse_live_args, run_live_with_args, LinuxLiveArgs};
```

- [ ] **Step 7: main.rs usage 字符串修正（参数名 + 默认热键）**

在 `crates/voice-input-linux/src/main.rs` 的 `usage()` 中，将：

```rust
        "live:  cargo run -p voice-input-linux --features ibus -- live [--backend ibus] [--activation-hotkey DoubleCtrl] [--double-ctrl-window-ms 300] [--silence-stop-ms 1500]\n",
```

替换为：

```rust
        "live:  cargo run -p voice-input-linux --features ibus -- live [--backend ibus] [--activation-hotkey DoubleAlt] [--double-press-window-ms 300] [--silence-stop-ms 1500]\n",
```

- [ ] **Step 8: 编译验证**

Run: `cargo build -p voice-input-linux --features ibus && cargo test -p voice-input-linux`
Expected: 全绿（可能有 dead_code warning，Task 4/5 消除）

- [ ] **Step 9: 提交**

```bash
git add crates/voice-input-linux/
git commit -m "refactor(voice-input-linux): 删除零引用函数与 ClipboardRestoreGuard 冗余字段，修正 usage 参数名"
```

---

### Task 4: IME 三层抽象坍缩为单一实现

本任务一步到位：backend.rs 删除的同时改写 main.rs 的 smoke 解析（否则 main.rs 引用已删除的 `LinuxBackendKind`/`parse_backend_kind`，无法编译）。`--backend` CLI 参数保留，仅校验取值。

**Files:**
- Modify: `crates/voice-input-linux/src/host.rs`（重写）
- Delete: `crates/voice-input-linux/src/backend.rs`
- Modify: `crates/voice-input-linux/src/ibus.rs`（精简为注入工具）
- Modify: `crates/voice-input-linux/src/local.rs`（适配）
- Modify: `crates/voice-input-linux/src/smoke.rs`（适配）
- Modify: `crates/voice-input-linux/src/main.rs`（smoke 自解析）
- Modify: `crates/voice-input-linux/src/live_cli.rs`（backend 参数校验化）
- Modify: `crates/voice-input-linux/src/runtime.rs`（LinuxHostConfig 扁平化）
- Modify: `crates/voice-input-linux/src/lib.rs`
- Modify: `crates/voice-input-linux/Cargo.toml`（删 ibus 依赖）
- Modify: `crates/voice-input-linux/tests/session.rs`

- [ ] **Step 1: host.rs 重写为直接实现**

替换 `crates/voice-input-linux/src/host.rs` 全部内容（`update_preedit` 暂留 no-op，Task 5 随 core trait 精简一并删除）：

```rust
use voice_input_core::{InputMethodHost, Result};

/// Linux 文本提交宿主：直接通过 xdotool 注入活动窗口。
/// composition 相关方法为 no-op——VoiceInput 不是注册的 IBus 引擎，
/// 任何 IBus D-Bus 交互都会干扰目标应用的输入法光标状态。
pub struct LinuxInputMethodHost {
    service_name: String,
}

impl LinuxInputMethodHost {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}

impl InputMethodHost for LinuxInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        Ok(())
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        crate::ibus::insert_text_into_active_window(text, None)
    }

    fn cancel_composition(&self) -> Result<()> {
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        Ok(())
    }
}
```

- [ ] **Step 2: 删除 backend.rs**

```bash
rm crates/voice-input-linux/src/backend.rs
```

- [ ] **Step 3: ibus.rs 精简为注入工具模块**

替换 `crates/voice-input-linux/src/ibus.rs` 全部内容（删除 `IbusEngineEvent`、`IbusEngineSpec`、`IbusEngineBridge`、`IbusClientBridge`、`UnwiredIbusBridge`、`MockIbusBridge`、`IbusBackend` 及 `impl LinuxBackend`；保留注入工具与 debug 宏）：

```rust
#[cfg(feature = "ibus")]
use std::process::Command;
#[cfg(feature = "ibus")]
use std::thread;
#[cfg(feature = "ibus")]
use std::time::{Duration, Instant};

use voice_input_core::{Result, VoiceInputError};

/// 在光标处插入录音指示符 ●，同时保存当前剪贴板。
/// 返回的 guard 持有 Clipboard 对象，确保还原后剪贴板内容
/// 存活到整个录音周期结束，不被剪贴板管理器丢弃。
#[cfg(feature = "ibus")]
pub fn insert_indicator_and_save_clipboard() -> Result<ClipboardRestoreGuard> {
    let saved = arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok());

    insert_text_into_active_window("●", None)?;

    // 立即还原用户剪贴板，避免 ● 残留在剪贴板中
    let clipboard = if let Some(ref text) = saved {
        let mut c = arboard::Clipboard::new()
            .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
        c.set_text(text.clone())
            .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))?;
        Some(c)
    } else {
        None
    };

    Ok(ClipboardRestoreGuard {
        _clipboard: clipboard,
    })
}

/// 持有剪贴板还原逻辑的守卫。
/// `_clipboard` 字段在整个录音周期内保持 Clipboard 连接存活，
/// 确保剪贴板管理器在足够长的时间内能同步内容。
pub struct ClipboardRestoreGuard {
    #[allow(dead_code)]
    _clipboard: Option<arboard::Clipboard>,
}

#[cfg(feature = "ibus")]
pub fn insert_text_into_active_window(text: &str, window_id: Option<&str>) -> Result<()> {
    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        let preview: String = text.chars().take(20).collect();
        eprintln!("[VOICEINPUT_DEBUG {now}] insert_text len={} is_ascii={} preview=\"{preview}\" win={window_id:?}", text.len(), text.chars().all(|c| c.is_ascii()));
    }

    // xdotool type 无法正确处理中文等多字节字符，对纯 ASCII 文本才
    // 优先使用打字方式（避免覆盖剪贴板），对非 ASCII 文本直接走剪贴板粘贴。
    let is_ascii = text.chars().all(|c| c.is_ascii());
    if is_ascii && type_text_in_active_window(text, window_id).is_ok() {
        return Ok(());
    }

    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))?;

    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] insert_text clipboard set, sleeping 40ms");
    }

    thread::sleep(Duration::from_millis(40));

    for shortcut in [
        ["key", "--clearmodifiers", "Shift+Insert"],
        ["key", "--clearmodifiers", "ctrl+v"],
    ] {
        let status = debug_xdotool!(
            "insert_text paste",
            Command::new("xdotool")
                .args(shortcut)
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            shortcut
        )?;

        if status.success() {
            return Ok(());
        }
    }

    // 粘贴失败时回退到先聚焦窗口再试
    if let Some(id) = window_id {
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            eprintln!("[VOICEINPUT_DEBUG {now}] insert_text paste failed, trying windowfocus {id}");
        }
        let _ = debug_xdotool!(
            "insert_text windowfocus (retry)",
            Command::new("xdotool")
                .args(["windowfocus", "--sync", id])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["windowfocus", "--sync", id]
        );
        thread::sleep(Duration::from_millis(40));

        for shortcut in [
            ["key", "--clearmodifiers", "Shift+Insert"],
            ["key", "--clearmodifiers", "ctrl+v"],
        ] {
            let status = debug_xdotool!(
                "insert_text paste (retry)",
                Command::new("xdotool")
                    .args(shortcut)
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                shortcut
            )?;

            if status.success() {
                return Ok(());
            }
        }
    }

    Err(VoiceInputError::Injection(
        "xdotool 粘贴失败：Shift+Insert 和 ctrl+v 都未成功".to_string(),
    ))
}

#[cfg(feature = "ibus")]
pub fn type_text_in_active_window(text: &str, window_id: Option<&str>) -> Result<()> {
    // 调用方已确保窗口是活动窗口，不要在此处调用 focus_window——
    // xdotool windowfocus --sync 会抢占焦点，导致目标应用光标闪烁或消失。

    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] xdotool type text_len={} win={window_id:?}", text.len());
    }

    let status = debug_xdotool!(
        "xdotool type",
        Command::new("xdotool")
            .args(["type", "--clearmodifiers", "--delay", "0", text])
            .status()
            .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
        &["type", "--clearmodifiers", "--delay", "0", text]
    )?;

    if !status.success() {
        // 如果直接打字失败（例如 Wayland），回退到用 windowfocus 聚焦后再试
        if let Some(id) = window_id {
            let _ = debug_xdotool!(
                "xdotool type windowfocus (retry)",
                Command::new("xdotool")
                    .args(["windowfocus", "--sync", id])
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                &["windowfocus", "--sync", id]
            );
            thread::sleep(Duration::from_millis(40));
        }

        let status = debug_xdotool!(
            "xdotool type (retry)",
            Command::new("xdotool")
                .args(["type", "--clearmodifiers", "--delay", "0", text])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["type", "--clearmodifiers", "--delay", "0", text]
        )?;

        if !status.success() {
            return Err(VoiceInputError::Injection(format!(
                "xdotool 输入失败，退出码：{status}"
            )));
        }
    }

    Ok(())
}

#[cfg(feature = "ibus")]
pub fn backspace_in_active_window(count: usize, window_id: Option<&str>) -> Result<()> {
    // 不预先调用 focus_window——调用方已确保窗口是活动窗口。
    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] backspace count={count} win={window_id:?}");
    }

    for _i in 0..count {
        let status = debug_xdotool!(
            "backspace",
            Command::new("xdotool")
                .args(["key", "--clearmodifiers", "BackSpace"])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["key", "--clearmodifiers", "BackSpace"]
        )?;

        if !status.success() {
            // 退格失败（如 Wayland），先聚焦再重试一次
            if let Some(id) = window_id {
                let _ = debug_xdotool!(
                    "backspace windowfocus (retry)",
                    Command::new("xdotool")
                        .args(["windowfocus", "--sync", id])
                        .status()
                        .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                    &["windowfocus", "--sync", id]
                );
                thread::sleep(Duration::from_millis(40));
            }

            let status = debug_xdotool!(
                "backspace (retry)",
                Command::new("xdotool")
                    .args(["key", "--clearmodifiers", "BackSpace"])
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                &["key", "--clearmodifiers", "BackSpace"]
            )?;

            if !status.success() {
                return Err(VoiceInputError::Injection(format!(
                    "xdotool 退格失败，退出码：{status}"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(not(feature = "ibus"))]
pub fn insert_indicator_and_save_clipboard() -> Result<ClipboardRestoreGuard> {
    Ok(ClipboardRestoreGuard { _clipboard: None })
}

#[cfg(not(feature = "ibus"))]
#[allow(dead_code)]
pub fn type_text_in_active_window(_text: &str, _window_id: Option<&str>) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "ibus"))]
#[allow(dead_code)]
pub fn backspace_in_active_window(_count: usize, _window_id: Option<&str>) -> Result<()> {
    Ok(())
}

#[cfg(feature = "ibus")]
macro_rules! debug_xdotool {
    ($label:expr, $cmd:expr, $args:expr) => {{
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = crate::ibus::debug_timestamp();
            eprintln!(
                "[VOICEINPUT_DEBUG {now}] {} → args={:?}",
                $label,
                $args,
            );
        }
        let result = $cmd;
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = crate::ibus::debug_timestamp();
            match &result {
                Ok(status) => eprintln!("[VOICEINPUT_DEBUG {now}] {} ← exit={}", $label, status.success()),
                Err(e) => eprintln!("[VOICEINPUT_DEBUG {now}] {} ← err={}", $label, e),
            }
        }
        result
    }};
}

#[cfg(feature = "ibus")]
pub fn debug_timestamp() -> String {
    let elapsed = DEBUG_START.elapsed();
    format!(
        "{}.{:03}s",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    )
}

#[cfg(feature = "ibus")]
static DEBUG_START: std::sync::LazyLock<Instant> =
    std::sync::LazyLock::new(Instant::now);
```

- [ ] **Step 4: Cargo.toml 删除 ibus crate 依赖**

在 `crates/voice-input-linux/Cargo.toml` 中：

```toml
[features]
default = []
ibus = ["dep:ibus"]
```

替换为：

```toml
[features]
default = []
ibus = []
```

并删除：

```toml
ibus = { version = "0.2", optional = true }
```

（feature 名保留——scripts/README 大量使用 `--features ibus`，只当作 cfg 开关。）

- [ ] **Step 5: local.rs 适配（backend 参数消失，HostConfig 扁平化）**

替换 `crates/voice-input-linux/src/local.rs` 全部内容（此文件在 Task 5 整个删除，这里只保持中间态编译）：

```rust
use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager};

use crate::host::LinuxInputMethodHost;

// ── Inlined from voice-input-runtime::local ──

#[derive(Debug, Clone)]
pub struct LocalVoiceInputConfig {
    pub app: AppConfig,
    pub asr: FunAsrConfig,
}

impl Default for LocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            app: AppConfig::default(),
            asr: FunAsrConfig::from_env(),
        }
    }
}

pub fn build_local_python_runtime_config(
) -> voice_input_core::Result<(LocalVoiceInputConfig, Box<dyn FunAsrRunner>)> {
    let config = LocalVoiceInputConfig::default();
    let runner = PythonFunAsrRunner::connect(config.asr.clone())?;
    Ok((config, Box::new(runner)))
}

// ── Linux-specific local voice input ──

pub struct LinuxLocalVoiceInput {
    controller: AppController,
    service_name: String,
}

impl LinuxLocalVoiceInput {
    pub fn new(
        config: LocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        service_name: impl Into<String>,
    ) -> Self {
        let service_name = service_name.into();
        let transcriber = LocalFunAsrTranscriber::new(config.asr, runner);
        let host = LinuxInputMethodHost::new(service_name.clone());
        let controller = AppController::new(
            config.app,
            hotkeys,
            recorder,
            Box::new(transcriber),
            Box::new(host),
        );

        Self {
            controller,
            service_name,
        }
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}
```

- [ ] **Step 6: smoke.rs 适配（单参数，删 build_backend）**

替换 `crates/voice-input-linux/src/smoke.rs` 全部内容（Task 5 会改为直接流程，这里先让它在无 backend 层下编译）：

```rust
use std::path::PathBuf;

use voice_input_core::MockHotkeyManager;

use crate::local::build_local_python_runtime_config;
use crate::{FileAudioRecorder, LinuxLocalVoiceInput};

pub fn run_smoke(audio_path: PathBuf) -> Result<(), String> {
    let (runtime_config, asr_runner) =
        build_local_python_runtime_config().map_err(|err| format!("预加载 ASR 模型失败：{err}"))?;

    let pipeline = LinuxLocalVoiceInput::new(
        runtime_config,
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(audio_path)),
        asr_runner,
        "voice-input",
    );

    let text = pipeline
        .run_once()
        .map_err(|err| format!("Linux 本地管线失败：{err}"))?;

    println!("识别结果：{text}");
    println!("服务名：{}", pipeline.service_name());
    Ok(())
}
```

- [ ] **Step 7: main.rs——smoke 自解析、--backend 只校验**

替换 `crates/voice-input-linux/src/main.rs` 全部内容：

```rust
use std::env;
use std::path::PathBuf;

use voice_input_linux::{parse_live_args, run_live_with_args, run_smoke};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();
    let command = match parse_command(args) {
        Ok(cmd) => cmd,
        Err(ParseOutcome::Help(msg)) => {
            eprintln!("{msg}");
            return 0;
        }
        Err(ParseOutcome::Error(msg)) => {
            eprintln!("{msg}");
            eprintln!("{}", usage());
            return 2;
        }
    };

    let result = match command {
        Command::Smoke { audio_file } => run_smoke(audio_file),
        Command::Live(args) => run_live_with_args(args),
    };

    match result {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("{msg}");
            1
        }
    }
}

enum Command {
    Smoke { audio_file: PathBuf },
    Live(voice_input_linux::LinuxLiveArgs),
}

enum ParseOutcome {
    Help(String),
    Error(String),
}

fn parse_command(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut iter = args.into_iter();
    let _bin = iter.next();

    let Some(subcommand) = iter.next() else {
        return Err(ParseOutcome::Error("缺少子命令".to_string()));
    };

    if matches!(subcommand.as_str(), "--help" | "-h" | "help") {
        return Err(ParseOutcome::Help(usage()));
    }

    match subcommand.to_ascii_lowercase().as_str() {
        "smoke" => parse_smoke_args(iter.collect()),
        "live" => parse_live_subcommand(iter.collect()),
        other => Err(ParseOutcome::Error(format!("不支持的子命令：{other}"))),
    }
}

fn parse_smoke_args(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut iter = args.into_iter();
    let mut audio_file = None;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--audio-file" => {
                let value = iter
                    .next()
                    .ok_or_else(|| ParseOutcome::Error("缺少 --audio-file 的值".to_string()))?;
                audio_file = Some(PathBuf::from(value));
            }
            "--backend" => {
                let value = iter
                    .next()
                    .ok_or_else(|| ParseOutcome::Error("缺少 --backend 的值".to_string()))?;
                voice_input_linux::validate_backend(&value).map_err(ParseOutcome::Error)?;
            }
            "--help" | "-h" => return Err(ParseOutcome::Help(usage())),
            other => return Err(ParseOutcome::Error(format!("不支持的参数：{other}"))),
        }
    }

    let audio_file = audio_file
        .ok_or_else(|| ParseOutcome::Error("缺少必需参数 --audio-file".to_string()))?;
    Ok(Command::Smoke { audio_file })
}

fn parse_live_subcommand(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut forwarded = vec!["voice-input-linux-live".to_string()];
    forwarded.extend(args);
    let live_args = parse_live_args(forwarded).map_err(|msg| {
        if msg == "help" {
            ParseOutcome::Help(usage())
        } else {
            ParseOutcome::Error(msg)
        }
    })?;
    Ok(Command::Live(live_args))
}

fn usage() -> String {
    concat!(
        "用法：cargo run -p voice-input-linux -- <smoke|live> [args]\n",
        "\n",
        "smoke: cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav [--backend ibus]\n",
        "live:  cargo run -p voice-input-linux --features ibus -- live [--backend ibus] [--activation-hotkey DoubleAlt] [--double-press-window-ms 300] [--silence-stop-ms 1500]\n",
    )
    .to_string()
}
```

- [ ] **Step 8: live_cli.rs——--backend 参数保留但只做校验**

在 `crates/voice-input-linux/src/live_cli.rs` 中：

1. 将 import 行：

```rust
use crate::{
    run_live_app, settings_path, LinuxAppSettings, LinuxBackendKind, LinuxHostConfig,
    LinuxLiveAppConfig,
};
```

替换为：

```rust
use crate::{run_live_app, settings_path, LinuxAppSettings, LinuxLiveAppConfig};
```

2. `LinuxLiveArgs` 删除 `backend` 字段：

```rust
#[derive(Debug, Clone)]
pub struct LinuxLiveArgs {
    pub activation_hotkey: Option<String>,
    pub double_press_window_ms: Option<u64>,
    pub silence_stop_ms: Option<u64>,
}

impl Default for LinuxLiveArgs {
    fn default() -> Self {
        Self {
            activation_hotkey: None,
            double_press_window_ms: None,
            silence_stop_ms: None,
        }
    }
}
```

3. `run_live_with_args` 中，将：

```rust
    if args.backend == LinuxBackendKind::Fcitx5 {
        return Err("Fcitx5 常驻路径还没有接入原生绑定，请先使用 --backend ibus".to_string());
    }

    #[cfg(not(feature = "ibus"))]
    if args.backend == LinuxBackendKind::IBus {
        return Err(
            "当前构建未启用 IBus 支持，请改用 `cargo run -p voice-input-linux --features ibus --bin voice-input-linux-app -- --backend ibus`"
                .to_string(),
        );
    }

    let mut config = LinuxLiveAppConfig {
        host: LinuxHostConfig {
            backend: args.backend,
            service_name: "voice-input".to_string(),
        },
        max_recording_duration: Duration::from_secs(30),
        double_press_window: Duration::from_millis(effective_window_ms),
        silence_stop_timeout: Duration::from_millis(effective_silence_stop_ms),
        ..Default::default()
    };
```

替换为：

```rust
    #[cfg(not(feature = "ibus"))]
    return Err("当前构建未启用 IBus 支持，请使用 --features ibus 重新构建".to_string());

    let mut config = LinuxLiveAppConfig {
        max_recording_duration: Duration::from_secs(30),
        double_press_window: Duration::from_millis(effective_window_ms),
        silence_stop_timeout: Duration::from_millis(effective_silence_stop_ms),
        ..Default::default()
    };
```

4. `parse_live_args` 的 `--backend` 分支中，将：

```rust
                parsed.backend = parse_backend(&value)?;
```

替换为：

```rust
                validate_backend(&value)?;
```

5. 将 `parse_backend` 替换为 `validate_backend`（pub，main.rs smoke 也用它）：

```rust
pub fn validate_backend(value: &str) -> Result<(), String> {
    match value.to_ascii_lowercase().as_str() {
        "ibus" => Ok(()),
        "fcitx5" | "fcitx" => Err(
            "Fcitx5 路径还没有接入原生绑定，请使用 --backend ibus".to_string(),
        ),
        other => Err(format!("不支持的 Linux 后端：{other}（仅支持 ibus）")),
    }
}
```

- [ ] **Step 9: runtime.rs——LinuxHostConfig 扁平化为 service_name**

在 `crates/voice-input-linux/src/runtime.rs` 中：

1. `LinuxLiveAppConfig` 删除 `pub host: LinuxHostConfig`，增加 `pub service_name: String`：

```rust
#[derive(Debug, Clone)]
pub struct LinuxLiveAppConfig {
    pub app: AppConfig,
    pub asr: FunAsrConfig,
    pub max_recording_duration: Duration,
    pub double_press_window: Duration,
    pub silence_stop_timeout: Duration,
    pub show_status_item: bool,
    pub service_name: String,
}

impl Default for LinuxLiveAppConfig {
    fn default() -> Self {
        let mut app = AppConfig::default();
        app.activation_hotkey = "DoubleAlt".to_string();

        Self {
            app,
            asr: FunAsrConfig::from_env(),
            max_recording_duration: Duration::from_secs(30),
            double_press_window: Duration::from_millis(300),
            silence_stop_timeout: Duration::from_millis(1500),
            show_status_item: true,
            service_name: "voice-input".to_string(),
        }
    }
}
```

2. import 中删除：

```rust
use crate::backend::LinuxBackendKind;
use crate::host::{LinuxHostConfig, LinuxInputMethodHost};
```

替换为：

```rust
use crate::host::LinuxInputMethodHost;
```

3. `run_live_app` 中，将：

```rust
    let host = LinuxInputMethodHost::new(config.host.clone());
```

替换为：

```rust
    let host = LinuxInputMethodHost::new(config.service_name.clone());
```

将：

```rust
        let tray = spawn_linux_tray(LinuxTrayConfig::new(
            config.host.service_name.clone(),
```

替换为：

```rust
        let tray = spawn_linux_tray(LinuxTrayConfig::new(
            config.service_name.clone(),
```

- [ ] **Step 10: lib.rs 更新 re-exports**

替换 `crates/voice-input-linux/src/lib.rs` 全部内容：

```rust
mod host;
mod hotkey;
mod ibus;
mod live;
mod live_cli;
mod local;
mod recorder;
mod runtime;
mod settings;
mod smoke;
mod tray;

pub use host::LinuxInputMethodHost;
pub use hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
pub use live::{LiveJobHandle, LiveJobState};
pub use live_cli::{parse_live_args, run_live_with_args, validate_backend, LinuxLiveArgs};
pub use local::{build_local_python_runtime_config, LinuxLocalVoiceInput, LocalVoiceInputConfig};
pub use recorder::{FileAudioRecorder, LinuxMicAudioRecorder};
pub use runtime::{run_live_app, LinuxLiveAppConfig};
pub use settings::{settings_path, LinuxAppSettings};
pub use smoke::run_smoke;
pub use tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
```

- [ ] **Step 11: session.rs 删除转发层测试，适配 local 测试**

在 `crates/voice-input-linux/tests/session.rs` 中：

1. 删除三个测试：`host_uses_configured_backend`、`host_forwards_events_to_backend_and_session`、`ibus_backend_records_ibus_style_events`（转发层已删除）。

2. `local_voice_input_wires_linux_host_and_asr_pipeline` 适配新签名（backend 参数消失），替换为：

```rust
#[test]
fn local_voice_input_wires_linux_host_and_asr_pipeline() {
    let runner = MockFunAsrRunner {
        transcript: "来自 Linux".to_string(),
        ..Default::default()
    };
    let pipeline = LinuxLocalVoiceInput::new(
        LocalVoiceInputConfig {
            app: AppConfig::default(),
            asr: voice_input_asr::FunAsrConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(runner),
        "voice-input",
    );

    let text = pipeline.run_once().expect("pipeline should succeed");
    assert_eq!(text, "来自 Linux");
}
```

3. import 替换为：

```rust
use voice_input_asr::MockFunAsrRunner;
use voice_input_core::{AppConfig, MockAudioRecorder, MockHotkeyManager};
use voice_input_linux::{LinuxLiveAppConfig, LinuxLocalVoiceInput, LocalVoiceInputConfig};
```

（`live_app_defaults_to_double_alt_hotkey` 测试保留不动。）

- [ ] **Step 12: 编译测试验证**

Run: `cargo build -p voice-input-linux --features ibus && cargo test -p voice-input-linux`
Expected: 全绿

- [ ] **Step 13: 提交**

```bash
git add crates/voice-input-linux/
git commit -m "refactor(voice-input-linux): 坍缩 IME 三层抽象，删除 backend/ibus 桥接层，HostConfig 扁平化"
```

---

### Task 5: 双流水线合并——删除 AppController，smoke 直接流程

**Files:**
- Delete: `crates/voice-input-core/src/controller.rs`
- Delete: `crates/voice-input-core/tests/controller.rs`
- Modify: `crates/voice-input-core/src/platform.rs`
- Delete: `crates/voice-input-core/src/ime.rs`
- Modify: `crates/voice-input-core/src/lib.rs`
- Modify: `crates/voice-input-asr/src/transcriber.rs`
- Modify: `crates/voice-input-asr/tests/funasr.rs`
- Delete: `crates/voice-input-linux/src/local.rs`
- Modify: `crates/voice-input-linux/src/smoke.rs`（重写为直接流程）
- Modify: `crates/voice-input-linux/src/host.rs`（trait 4 方法）
- Modify: `crates/voice-input-linux/src/runtime.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Modify: `crates/voice-input-linux/tests/session.rs`

- [ ] **Step 1: 删除 core/controller.rs 与 core/tests/controller.rs**

```bash
rm crates/voice-input-core/src/controller.rs crates/voice-input-core/tests/controller.rs
```

- [ ] **Step 2: core platform.rs——删 HotkeyManager/MockHotkeyManager/MockAudioRecorder/MockInputMethodHost/update_preedit**

替换 `crates/voice-input-core/src/platform.rs` 全部内容：

```rust
use crate::error::Result;

pub trait AudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>>;
}

pub trait Transcriber {
    fn transcribe(&self, audio: &[u8]) -> Result<String>;
}

pub trait InputMethodHost {
    fn start_composition(&self) -> Result<()>;
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

#[derive(Clone, Default)]
pub struct MockTranscriber {
    pub transcript: String,
}

impl Transcriber for MockTranscriber {
    fn transcribe(&self, _audio: &[u8]) -> Result<String> {
        Ok(self.transcript.clone())
    }
}
```

- [ ] **Step 3: core ime.rs 删除（Transcript 无消费者）**

```bash
rm crates/voice-input-core/src/ime.rs
```

- [ ] **Step 4: core lib.rs**

替换 `crates/voice-input-core/src/lib.rs` 全部内容：

```rust
mod config;
mod error;
mod platform;

pub use config::AppConfig;
pub use error::{Result, VoiceInputError};
pub use platform::{AudioRecorder, InputMethodHost, MockTranscriber, Transcriber};
```

- [ ] **Step 5: asr Transcriber 实现切 String + 测试断言更新**

`crates/voice-input-asr/src/transcriber.rs` 中，将 import：

```rust
use voice_input_core::{Result, Transcriber, Transcript, VoiceInputError};
```

替换为：

```rust
use voice_input_core::{Result, Transcriber, VoiceInputError};
```

将：

```rust
impl Transcriber for LocalFunAsrTranscriber {
    fn transcribe(&self, audio: &[u8]) -> Result<Transcript> {
        let text = self.transcribe_allow_empty(audio)?;

        if text.trim().is_empty() {
            return Err(VoiceInputError::Transcription(
                "ASR 没有返回识别文本，请检查麦克风输入、录音时长或环境噪声".to_string(),
            ));
        }

        Ok(Transcript::new(text))
    }
}
```

替换为：

```rust
impl Transcriber for LocalFunAsrTranscriber {
    fn transcribe(&self, audio: &[u8]) -> Result<String> {
        let text = self.transcribe_allow_empty(audio)?;

        if text.trim().is_empty() {
            return Err(VoiceInputError::Transcription(
                "ASR 没有返回识别文本，请检查麦克风输入、录音时长或环境噪声".to_string(),
            ));
        }

        Ok(text)
    }
}
```

`crates/voice-input-asr/tests/funasr.rs` 中，将 import：

```rust
use voice_input_core::{Transcriber, Transcript};
```

替换为：

```rust
use voice_input_core::Transcriber;
```

将：

```rust
    assert_eq!(transcript, Transcript::new("你好，世界"));
```

替换为：

```rust
    assert_eq!(transcript, "你好，世界");
```

- [ ] **Step 6: 删除 linux local.rs**

```bash
rm crates/voice-input-linux/src/local.rs
```

- [ ] **Step 7: smoke.rs 重写为直接流程**

替换 `crates/voice-input-linux/src/smoke.rs` 全部内容：

```rust
use std::path::{Path, PathBuf};

use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{AudioRecorder, InputMethodHost, Transcriber};

use crate::host::LinuxInputMethodHost;
use crate::recorder::FileAudioRecorder;

/// 读取 WAV 文件并转写，返回识别文本。独立纯函数，便于测试。
pub fn transcribe_file(
    audio_path: &Path,
    transcriber: &LocalFunAsrTranscriber,
) -> voice_input_core::Result<String> {
    let audio = FileAudioRecorder::new(audio_path).record_once()?;
    transcriber.transcribe(&audio)
}

pub fn run_smoke(audio_path: PathBuf) -> Result<(), String> {
    if cfg!(not(feature = "ibus")) {
        return Err("当前构建未启用 IBus 支持，请使用 --features ibus 重新构建".to_string());
    }

    let asr = FunAsrConfig::from_env();
    let runner = PythonFunAsrRunner::connect(asr.clone())
        .map_err(|err| format!("预加载 ASR 模型失败：{err}"))?;
    let transcriber = LocalFunAsrTranscriber::new(asr, Box::new(runner));

    let text = transcribe_file(&audio_path, &transcriber)
        .map_err(|err| format!("Linux smoke 管线失败：{err}"))?;

    let host = LinuxInputMethodHost::new("voice-input");
    host.start_composition()
        .map_err(|err| format!("开始输入失败：{err}"))?;
    host.commit_text(&text)
        .map_err(|err| format!("提交文本失败：{err}"))?;
    host.end_composition()
        .map_err(|err| format!("结束输入失败：{err}"))?;

    println!("识别结果：{text}");
    Ok(())
}
```

（本任务的 `transcribe_file` 里 `record_once` 返回 `Vec<u8>`；Task 6 统一 PCM 后改为解码 + `write_pcm_wav` 重编码。）

- [ ] **Step 8: host.rs 适配 4 方法 trait**

在 `crates/voice-input-linux/src/host.rs` 的 `impl InputMethodHost for LinuxInputMethodHost` 中删除 `update_preedit` 方法（service_name 字段与访问器保留——宿主身份标识）：

```rust
impl InputMethodHost for LinuxInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        crate::ibus::insert_text_into_active_window(text, None)
            .map_err(|e| VoiceInputError::Injection(format!("Linux 文本提交失败：{e}")))
    }

    fn cancel_composition(&self) -> Result<()> {
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        Ok(())
    }
}
```

- [ ] **Step 9: runtime.rs 删除 _job 参数**

在 `crates/voice-input-linux/src/runtime.rs` 中：

1. import：

```rust
use crate::live::{print_live_ready, LiveJobHandle, LiveJobState};
```

替换为：

```rust
use crate::live::{print_live_ready, LiveJobState};
```

2. `run_recording_cycle` 签名删除 `_job: LiveJobHandle` 参数（删除签名最后一行参数）。

3. 调用点：

```rust
        let Some(job) = LiveJobState::try_acquire(&active) else {
            continue;
        };

        if run_recording_cycle(
            &recorder,
            &host,
            &transcriber,
            config.silence_stop_timeout,
            tray.as_ref(),
            &watcher,
            job,
        )? {
```

替换为：

```rust
        let Some(_job) = LiveJobState::try_acquire(&active) else {
            continue;
        };

        if run_recording_cycle(
            &recorder,
            &host,
            &transcriber,
            config.silence_stop_timeout,
            tray.as_ref(),
            &watcher,
        )? {
```

（`_job` 仍持有 `LiveJobState` 守卫，函数返回后释放。）

- [ ] **Step 10: lib.rs 更新**

替换 `crates/voice-input-linux/src/lib.rs` 全部内容：

```rust
mod host;
mod hotkey;
mod ibus;
mod live;
mod live_cli;
mod recorder;
mod runtime;
mod settings;
mod smoke;
mod tray;

pub use hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
pub use host::LinuxInputMethodHost;
pub use live::LiveJobState;
pub use live_cli::{parse_live_args, run_live_with_args, validate_backend, LinuxLiveArgs};
pub use recorder::{FileAudioRecorder, LinuxMicAudioRecorder};
pub use runtime::{run_live_app, LinuxLiveAppConfig};
pub use settings::{settings_path, LinuxAppSettings};
pub use smoke::{run_smoke, transcribe_file};
pub use tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
```

- [ ] **Step 11: session.rs 测试适配**

替换 `crates/voice-input-linux/tests/session.rs` 全部内容：

```rust
use std::path::Path;

use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, MockFunAsrRunner};
use voice_input_linux::{transcribe_file, LinuxLiveAppConfig};

#[test]
fn live_app_defaults_to_double_alt_hotkey() {
    let config = LinuxLiveAppConfig::default();

    assert_eq!(config.app.activation_hotkey, "DoubleAlt");
}

#[test]
fn smoke_transcribes_test_audio_file() {
    let runner = MockFunAsrRunner {
        transcript: "来自 Linux".to_string(),
        ..Default::default()
    };
    let transcriber = LocalFunAsrTranscriber::new(FunAsrConfig::default(), Box::new(runner));
    let audio_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/smoke.wav");

    let text = transcribe_file(&audio_path, &transcriber).expect("transcribe file");
    assert_eq!(text, "来自 Linux");
}
```

- [ ] **Step 12: 编译测试验证**

Run: `cargo build --workspace && cargo test --workspace`
Expected: 全绿

- [ ] **Step 13: 提交**

```bash
git add crates/
git commit -m "refactor(voice-input-linux): 删除 AppController 双流水线，smoke 改为直接流程"
```

---

### Task 6: PCM 统一——RecordedAudio + WAV 解码 + 峰值 VAD

**Files:**
- Create: `crates/voice-input-core/src/audio.rs`
- Modify: `crates/voice-input-core/src/platform.rs`
- Modify: `crates/voice-input-core/src/lib.rs`
- Modify: `crates/voice-input-audio/src/file.rs`
- Modify: `crates/voice-input-audio/src/silence.rs`
- Modify: `crates/voice-input-audio/src/lib.rs`
- Modify: `crates/voice-input-linux/src/recorder.rs`
- Modify: `crates/voice-input-linux/src/runtime.rs`
- Modify: `crates/voice-input-linux/src/smoke.rs`

- [ ] **Step 1: core 新增 RecordedAudio**

创建 `crates/voice-input-core/src/audio.rs`：

```rust
/// 已录制的单声道 PCM 音频。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedAudio {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}
```

- [ ] **Step 2: core platform.rs 的 AudioRecorder 返回 RecordedAudio**

在 `crates/voice-input-core/src/platform.rs` 中，将：

```rust
use crate::error::Result;

pub trait AudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>>;
}
```

替换为：

```rust
use crate::audio::RecordedAudio;
use crate::error::Result;

pub trait AudioRecorder {
    fn record_once(&self) -> Result<RecordedAudio>;
}
```

- [ ] **Step 3: core lib.rs 导出**

替换 `crates/voice-input-core/src/lib.rs` 全部内容：

```rust
mod audio;
mod config;
mod error;
mod platform;

pub use audio::RecordedAudio;
pub use config::AppConfig;
pub use error::{Result, VoiceInputError};
pub use platform::{AudioRecorder, InputMethodHost, MockTranscriber, Transcriber};
```

- [ ] **Step 4: audio file.rs 用 hound 解码 WAV**

替换 `crates/voice-input-audio/src/file.rs` 全部内容：

```rust
use std::path::PathBuf;

use voice_input_core::{AudioRecorder, RecordedAudio, Result, VoiceInputError};

#[derive(Debug, Clone)]
pub struct FileAudioRecorder {
    path: PathBuf,
}

impl FileAudioRecorder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

fn average_to_mono(samples: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }

    samples
        .chunks(channels)
        .map(|frame| {
            let sum = frame.iter().map(|s| i32::from(*s)).sum::<i32>();
            (sum / frame.len() as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

impl AudioRecorder for FileAudioRecorder {
    fn record_once(&self) -> Result<RecordedAudio> {
        let reader = hound::WavReader::open(&self.path).map_err(|e| {
            VoiceInputError::Audio(format!("读取音频文件失败 {}：{e}", self.path.display()))
        })?;
        let spec = reader.spec();
        let sample_rate = spec.sample_rate;
        let channels = usize::from(spec.channels.max(1));

        let samples: Vec<i16> = match spec.sample_format {
            hound::SampleFormat::Int => reader
                .into_samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| VoiceInputError::Audio(format!("解析 WAV 采样失败：{e}")))?,
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| VoiceInputError::Audio(format!("解析 WAV 采样失败：{e}")))?
                .iter()
                .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect(),
        };

        Ok(RecordedAudio {
            samples: average_to_mono(&samples, channels),
            sample_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FileAudioRecorder;
    use crate::wav::write_pcm_wav;
    use voice_input_core::AudioRecorder;

    #[test]
    fn decodes_pcm_wav_back_to_mono_samples() {
        let wav = write_pcm_wav(&[100, -100, 300], 16000).expect("write wav");
        let path = std::env::temp_dir().join(format!(
            "voiceinput-test-{}-decode.wav",
            std::process::id()
        ));
        std::fs::write(&path, &wav).expect("write temp wav");

        let recorder = FileAudioRecorder::new(&path);
        let audio = recorder.record_once().expect("decode wav");

        std::fs::remove_file(&path).ok();

        assert_eq!(audio.samples, vec![100, -100, 300]);
        assert_eq!(audio.sample_rate, 16000);
    }
}
```

- [ ] **Step 5: audio silence.rs 新增峰值检测**

在 `crates/voice-input-audio/src/silence.rs` 末尾追加：

```rust
/// 峰值检测：任一采样绝对值超过阈值即认为有语音。
/// 与 has_voice_activity 的 RMS 语义不同——峰值对单个尖峰更敏感，
/// 用于整段录音的最终语音判定（对抗 AGC 底噪）。
pub fn has_peak_above(samples: &[i16], threshold: i16) -> bool {
    samples.iter().any(|s| s.abs() > threshold)
}

#[cfg(test)]
mod tests {
    use super::{has_peak_above, has_voice_activity};

    #[test]
    fn peak_detection_flags_loud_samples() {
        assert!(has_peak_above(&[0, 900, 0], 800));
        assert!(!has_peak_above(&[0, 800, 0], 800));
        assert!(!has_peak_above(&[], 800));
    }

    #[test]
    fn rms_detection_flags_sustained_energy() {
        assert!(has_voice_activity(&[500; 100]));
        assert!(!has_voice_activity(&[10; 100]));
    }
}
```

- [ ] **Step 6: audio lib.rs 导出**

替换 `crates/voice-input-audio/src/lib.rs` 全部内容：

```rust
mod file;
mod pcm;
mod silence;
mod wav;

pub use file::FileAudioRecorder;
pub use pcm::{push_mono_i16_f32, push_mono_i16_i16, push_mono_i16_u16};
pub use silence::{has_peak_above, has_voice_activity};
pub use wav::write_pcm_wav;
```

- [ ] **Step 7: recorder.rs 返回 RecordedAudio**

在 `crates/voice-input-linux/src/recorder.rs` 中：

1. 将：

```rust
use voice_input_audio::{
    has_voice_activity, push_mono_i16_f32, push_mono_i16_i16, push_mono_i16_u16, write_pcm_wav,
};
use voice_input_core::{AudioRecorder, Result, VoiceInputError};
```

替换为：

```rust
use voice_input_audio::{
    has_voice_activity, push_mono_i16_f32, push_mono_i16_i16, push_mono_i16_u16,
};
use voice_input_core::{AudioRecorder, RecordedAudio, Result, VoiceInputError};
```

（`pub use voice_input_audio::FileAudioRecorder;` 行保留不动。）

2. `record_once_with_chunks` 签名与结尾。将：

```rust
    ) -> Result<Vec<u8>>
    where
        F: FnMut(u32, Vec<i16>, bool),
    {
```

替换为：

```rust
    ) -> Result<RecordedAudio>
    where
        F: FnMut(u32, Vec<i16>, bool),
    {
```

将结尾：

```rust
        let duration_secs = captured.len() as f32 / sample_rate as f32;
        println!(
            "录音完成：{} 个采样，约 {:.2} 秒",
            captured.len(),
            duration_secs
        );

        write_pcm_wav(&captured, sample_rate)
    }
}
```

替换为：

```rust
        let duration_secs = captured.len() as f32 / sample_rate as f32;
        println!(
            "录音完成：{} 个采样，约 {:.2} 秒",
            captured.len(),
            duration_secs
        );

        Ok(RecordedAudio {
            samples: captured,
            sample_rate,
        })
    }
}
```

3. `AudioRecorder` trait impl 同步更新。将：

```rust
impl AudioRecorder for LinuxMicAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        self.record_once_with_chunks(
            Duration::from_millis(0),
            Duration::from_millis(0),
            Arc::new(AtomicBool::new(true)),
            |_, _, _| {},
        )
    }
}
```

替换为：

```rust
impl AudioRecorder for LinuxMicAudioRecorder {
    fn record_once(&self) -> Result<RecordedAudio> {
        self.record_once_with_chunks(
            Duration::from_millis(0),
            Duration::from_millis(0),
            Arc::new(AtomicBool::new(true)),
            |_, _, _| {},
        )
    }
}
```

- [ ] **Step 8: runtime.rs 用 PCM 做 VAD、调用点编码 WAV**

在 `crates/voice-input-linux/src/runtime.rs` 中：

1. 删除 `wav_has_voice_activity` 函数（含注释，从 `/// 从 WAV 字节中扫描 "data" chunk` 到函数结尾 `}`）。

2. import 增加：

```rust
use voice_input_audio::{has_peak_above, write_pcm_wav};
```

（放在 `use voice_input_core::{AppConfig, InputMethodHost, Result, VoiceInputError};` 之后。）

3. `run_recording_cycle` 中，将：

```rust
        Ok(audio_data) => {
            // 从 WAV 中扫描 "data" chunk 起点，正确跳过 RIFF/WAVE/fmt 头。
            // 之前硬编码 44 字节偏移导致 WAV 头被误当音频数据，
            // RIFF 标识符的字节值转成 i16 远超阈值，任何录音都被判有语音。
            let has_voice = wav_has_voice_activity(&audio_data);

            if !has_voice {
                eprintln!("录音中未检测到有效语音，跳过转写");
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                return Ok(false);
            }

            let transcript = transcriber
                .transcribe_allow_empty(&audio_data)?
                .trim()
                .to_string();
```

替换为：

```rust
        Ok(audio) => {
            // 峰值 > 800（约满量程 2.4%）认为有有效语音，
            // 对抗 AGC 底噪，避免对噪声幻觉转写。
            if !has_peak_above(&audio.samples, 800) {
                eprintln!("录音中未检测到有效语音，跳过转写");
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                return Ok(false);
            }

            let wav = write_pcm_wav(&audio.samples, audio.sample_rate)?;
            let transcript = transcriber
                .transcribe_allow_empty(&wav)?
                .trim()
                .to_string();
```

- [ ] **Step 9: smoke.rs transcribe_file 适配 PCM 形态**

在 `crates/voice-input-linux/src/smoke.rs` 中，将 import：

```rust
use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{AudioRecorder, InputMethodHost, Transcriber};
```

替换为：

```rust
use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_audio::write_pcm_wav;
use voice_input_core::{AudioRecorder, InputMethodHost, Transcriber};
```

将 `transcribe_file`：

```rust
pub fn transcribe_file(
    audio_path: &Path,
    transcriber: &LocalFunAsrTranscriber,
) -> voice_input_core::Result<String> {
    let audio = FileAudioRecorder::new(audio_path).record_once()?;
    transcriber.transcribe(&audio)
}
```

替换为：

```rust
pub fn transcribe_file(
    audio_path: &Path,
    transcriber: &LocalFunAsrTranscriber,
) -> voice_input_core::Result<String> {
    let audio = FileAudioRecorder::new(audio_path).record_once()?;
    let wav = write_pcm_wav(&audio.samples, audio.sample_rate)?;
    transcriber.transcribe(&wav)
}
```

- [ ] **Step 10: 编译测试验证**

Run: `cargo build --workspace && cargo test --workspace`
Expected: 全绿（含新的 audio 解码 round-trip 测试与峰值检测测试）

- [ ] **Step 11: 提交**

```bash
git add crates/
git commit -m "refactor(voice-input-audio): 统一 PCM 形态——RecordedAudio、WAV 解码、峰值 VAD 语义保持"
```

---

### Task 7: hotkey 统一——enum 建模 + 双击循环合一

**Files:**
- Modify: `crates/voice-input-linux/src/hotkey.rs`

- [ ] **Step 1: 重写 hotkey.rs**

替换 `crates/voice-input-linux/src/hotkey.rs` 全部内容：

```rust
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::live::LiveJobState;
use device_query::{DeviceQuery, DeviceState, Keycode};
use voice_input_core::{Result, VoiceInputError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierKey {
    Ctrl,
    Alt,
}

impl ModifierKey {
    fn label(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
        }
    }

    fn is_held(self, keys: &[Keycode]) -> bool {
        match self {
            Self::Ctrl => has_any(keys, &[Keycode::LControl, Keycode::RControl]),
            Self::Alt => has_any(keys, &[Keycode::LAlt, Keycode::RAlt]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyKind {
    DoublePress(ModifierKey),
    Combo {
        key: Keycode,
        control: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxHotkeySpec {
    kind: HotkeyKind,
}

impl LinuxHotkeySpec {
    pub fn parse(spec: &str) -> Result<Self> {
        let mut double_press: Option<ModifierKey> = None;
        let mut combo = HotkeyKind::Combo {
            key: Keycode::Space,
            control: false,
            shift: false,
            alt: false,
            meta: false,
        };

        for token in spec
            .split('+')
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            match token.to_ascii_lowercase().as_str() {
                "longctrl" | "long-ctrl" | "long_ctrl" | "doublectrl" | "double-ctrl"
                | "double_ctrl" | "doublectrlstrict" | "double-ctrl-strict"
                | "double_ctrl_strict" => {
                    double_press = Some(ModifierKey::Ctrl);
                }
                "doublealt" | "double-alt" | "double_alt" => {
                    double_press = Some(ModifierKey::Alt);
                }
                "ctrl" | "control" => set_combo_modifier(&mut combo, ModifierFlag::Control),
                "shift" => set_combo_modifier(&mut combo, ModifierFlag::Shift),
                "alt" | "option" => set_combo_modifier(&mut combo, ModifierFlag::Alt),
                "cmd" | "command" | "meta" => set_combo_modifier(&mut combo, ModifierFlag::Meta),
                "space" => set_combo_key(&mut combo, Keycode::Space),
                "tab" => set_combo_key(&mut combo, Keycode::Tab),
                "enter" | "return" => set_combo_key(&mut combo, Keycode::Enter),
                "esc" | "escape" => set_combo_key(&mut combo, Keycode::Escape),
                "delete" | "backspace" => set_combo_key(&mut combo, Keycode::Delete),
                "f1" => set_combo_key(&mut combo, Keycode::F1),
                "f2" => set_combo_key(&mut combo, Keycode::F2),
                "f3" => set_combo_key(&mut combo, Keycode::F3),
                "f4" => set_combo_key(&mut combo, Keycode::F4),
                "f5" => set_combo_key(&mut combo, Keycode::F5),
                "f6" => set_combo_key(&mut combo, Keycode::F6),
                "f7" => set_combo_key(&mut combo, Keycode::F7),
                "f8" => set_combo_key(&mut combo, Keycode::F8),
                "f9" => set_combo_key(&mut combo, Keycode::F9),
                "f10" => set_combo_key(&mut combo, Keycode::F10),
                "f11" => set_combo_key(&mut combo, Keycode::F11),
                "f12" => set_combo_key(&mut combo, Keycode::F12),
                other if other.len() == 1 => {
                    set_combo_key(&mut combo, keycode_from_token(other.chars().next().unwrap())?)
                }
                other => {
                    return Err(VoiceInputError::Hotkey(format!(
                        "不支持的热键片段：{other}"
                    )));
                }
            }
        }

        Ok(Self {
            kind: double_press.map(HotkeyKind::DoublePress).unwrap_or(combo),
        })
    }

    pub fn kind(&self) -> HotkeyKind {
        self.kind
    }

    pub fn matches(&self, keys: &[Keycode]) -> bool {
        match self.kind {
            HotkeyKind::DoublePress(ModifierKey::Ctrl) => is_ctrl_only(keys),
            HotkeyKind::DoublePress(ModifierKey::Alt) => is_alt_only(keys),
            HotkeyKind::Combo {
                key,
                control,
                shift,
                alt,
                meta,
            } => {
                if !keys.contains(&key) {
                    return false;
                }

                if control && !has_any(keys, &[Keycode::LControl, Keycode::RControl]) {
                    return false;
                }
                if shift && !has_any(keys, &[Keycode::LShift, Keycode::RShift]) {
                    return false;
                }
                if alt
                    && !has_any(
                        keys,
                        &[
                            Keycode::LAlt,
                            Keycode::RAlt,
                            Keycode::LOption,
                            Keycode::ROption,
                        ],
                    )
                {
                    return false;
                }
                if meta
                    && !has_any(
                        keys,
                        &[
                            Keycode::LMeta,
                            Keycode::RMeta,
                            Keycode::Command,
                            Keycode::RCommand,
                        ],
                    )
                {
                    return false;
                }

                true
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ModifierFlag {
    Control,
    Shift,
    Alt,
    Meta,
}

fn set_combo_modifier(combo: &mut HotkeyKind, flag: ModifierFlag) {
    if let HotkeyKind::Combo {
        control,
        shift,
        alt,
        meta,
        ..
    } = combo
    {
        match flag {
            ModifierFlag::Control => *control = true,
            ModifierFlag::Shift => *shift = true,
            ModifierFlag::Alt => *alt = true,
            ModifierFlag::Meta => *meta = true,
        }
    }
}

fn set_combo_key(combo: &mut HotkeyKind, key: Keycode) {
    if let HotkeyKind::Combo {
        key: combo_key, ..
    } = combo
    {
        *combo_key = key;
    }
}

fn keycode_from_token(token: char) -> Result<Keycode> {
    let key = match token.to_ascii_lowercase() {
        'a' => Keycode::A,
        'b' => Keycode::B,
        'c' => Keycode::C,
        'd' => Keycode::D,
        'e' => Keycode::E,
        'f' => Keycode::F,
        'g' => Keycode::G,
        'h' => Keycode::H,
        'i' => Keycode::I,
        'j' => Keycode::J,
        'k' => Keycode::K,
        'l' => Keycode::L,
        'm' => Keycode::M,
        'n' => Keycode::N,
        'o' => Keycode::O,
        'p' => Keycode::P,
        'q' => Keycode::Q,
        'r' => Keycode::R,
        's' => Keycode::S,
        't' => Keycode::T,
        'u' => Keycode::U,
        'v' => Keycode::V,
        'w' => Keycode::W,
        'x' => Keycode::X,
        'y' => Keycode::Y,
        'z' => Keycode::Z,
        '0' => Keycode::Key0,
        '1' => Keycode::Key1,
        '2' => Keycode::Key2,
        '3' => Keycode::Key3,
        '4' => Keycode::Key4,
        '5' => Keycode::Key5,
        '6' => Keycode::Key6,
        '7' => Keycode::Key7,
        '8' => Keycode::Key8,
        '9' => Keycode::Key9,
        other => {
            return Err(VoiceInputError::Hotkey(format!(
                "不支持的单字符热键：{other}"
            )));
        }
    };

    Ok(key)
}

fn has_any(keys: &[Keycode], candidates: &[Keycode]) -> bool {
    candidates.iter().any(|candidate| keys.contains(candidate))
}

fn is_ctrl_only(keys: &[Keycode]) -> bool {
    !keys.is_empty()
        && keys
            .iter()
            .all(|key| matches!(key, Keycode::LControl | Keycode::RControl))
}

fn is_alt_only(keys: &[Keycode]) -> bool {
    !keys.is_empty()
        && keys
            .iter()
            .all(|key| matches!(key, Keycode::LAlt | Keycode::RAlt))
}

pub struct LinuxHotkeyWatcher {
    receiver: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl LinuxHotkeyWatcher {
    pub fn spawn(
        spec: LinuxHotkeySpec,
        active: Arc<LiveJobState>,
        recorder: crate::recorder::LinuxMicAudioRecorder,
        double_press_window: Duration,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);

        let handle = thread::spawn(move || {
            let device = DeviceState::new();
            let mut last_trigger_at: Option<Instant> = None;
            let mut last_release: Option<Instant> = None;
            let mut was_held = false;
            let mut latched = false;
            const TRIGGER_COOLDOWN: Duration = Duration::from_millis(800);

            while !stop_for_thread.load(Ordering::SeqCst) {
                let keys = device.get_keys();

                match spec.kind() {
                    HotkeyKind::DoublePress(modifier) => {
                        // 使用 has_any 而不是 is_*_only 检测修饰键状态。
                        // is_*_only 会在组合键（如 Ctrl+C）释放 C 时产生虚假的
                        // 上升沿（因为 Ctrl 再次变成"单独按下"），导致误触发。
                        let held = modifier.is_held(&keys);
                        let now = Instant::now();
                        let in_cooldown = last_trigger_at
                            .map(|last| now.duration_since(last) <= TRIGGER_COOLDOWN)
                            .unwrap_or(false);

                        if held && !was_held {
                            // 修饰键刚按下
                            if !in_cooldown {
                                if let Some(release_time) = last_release {
                                    if now.duration_since(release_time) <= double_press_window {
                                        // 两次按下间隔在窗口内 → 触发
                                        let label = modifier.label();
                                        if active.is_active() {
                                            if recorder.is_recording() {
                                                eprintln!(
                                                    "检测到双击 {label} 停止热键，正在结束录音..."
                                                );
                                                recorder.stop();
                                            }
                                        } else {
                                            eprintln!(
                                                "检测到双击 {label} 开始热键，正在启动录音..."
                                            );
                                            let _ = sender.send(());
                                        }
                                        last_trigger_at = Some(now);
                                        last_release = None;
                                    }
                                }
                            }
                        } else if !held && was_held {
                            // 修饰键刚释放
                            last_release = Some(now);
                        }

                        was_held = held;
                    }
                    HotkeyKind::Combo { .. } => {
                        // 组合热键（Ctrl+Shift+Space 等）
                        let pressed = spec.matches(&keys);

                        if pressed && !latched {
                            let now = Instant::now();
                            let recently_triggered = last_trigger_at
                                .map(|last| now.duration_since(last) <= TRIGGER_COOLDOWN)
                                .unwrap_or(false);
                            if recently_triggered {
                                latched = true;
                                continue;
                            }

                            if active.is_active() {
                                if recorder.is_recording() {
                                    eprintln!("检测到停止热键，正在结束录音...");
                                    recorder.stop();
                                }
                            } else {
                                eprintln!("检测到开始热键，正在启动录音...");
                                let _ = sender.send(());
                            }
                            last_trigger_at = Some(now);
                            latched = true;
                        } else if !pressed {
                            latched = false;
                        }
                    }
                }

                thread::sleep(Duration::from_millis(25));
            }
        });

        Ok(Self {
            receiver,
            stop,
            handle: Some(handle),
        })
    }

    pub fn wait_for_trigger(&self) -> Result<()> {
        self.receiver
            .recv()
            .map_err(|_| VoiceInputError::Hotkey("热键监听已停止".to_string()))
    }

    pub fn wait_for_trigger_timeout(&self, timeout: Duration) -> Result<bool> {
        match self.receiver.recv_timeout(timeout) {
            Ok(_) => Ok(true),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(false),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(VoiceInputError::Hotkey("热键监听已停止".to_string()))
            }
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for LinuxHotkeyWatcher {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combo_hotkey() {
        let spec = LinuxHotkeySpec::parse("Ctrl+Shift+Space").expect("parse hotkey");
        assert!(spec.matches(&[Keycode::Space, Keycode::LControl, Keycode::LShift]));
        assert!(!spec.matches(&[Keycode::Space, Keycode::LControl]));
    }

    #[test]
    fn parses_double_ctrl_hotkey() {
        let spec = LinuxHotkeySpec::parse("DoubleCtrl").expect("parse hotkey");
        assert_eq!(spec.kind(), HotkeyKind::DoublePress(ModifierKey::Ctrl));
        assert!(spec.matches(&[Keycode::LControl]));
        assert!(spec.matches(&[Keycode::RControl]));
        assert!(spec.matches(&[Keycode::LControl, Keycode::RControl]));
        assert!(!spec.matches(&[Keycode::Space]));
        assert!(!spec.matches(&[Keycode::LControl, Keycode::Space]));
    }

    #[test]
    fn parses_double_alt_hotkey() {
        let spec = LinuxHotkeySpec::parse("DoubleAlt").expect("parse hotkey");
        assert_eq!(spec.kind(), HotkeyKind::DoublePress(ModifierKey::Alt));
        assert!(spec.matches(&[Keycode::LAlt]));
        assert!(spec.matches(&[Keycode::RAlt]));
        assert!(!spec.matches(&[Keycode::Space]));
        assert!(!spec.matches(&[Keycode::LAlt, Keycode::Space]));
    }
}
```

（删除了冗余的 `parses_mac_like_default_hotkey_for_linux_runtime`（与 `parses_combo_hotkey` 重复）和 `parses_double_ctrl_strict_hotkey`（strict 只是解析别名，无独立行为）。）

- [ ] **Step 2: 编译测试验证**

Run: `cargo build --workspace && cargo test --workspace`
Expected: 全绿（`runtime.rs` 的 `describe_activation_hotkey` 按字符串比较，不依赖 spec 内部字段，无需改动）

- [ ] **Step 3: 提交**

```bash
git add crates/voice-input-linux/src/hotkey.rs
git commit -m "refactor(voice-input-linux): 热键规格 enum 建模，双击 Ctrl/Alt 监听循环合一"
```

---

### Task 8: scripts 精简——删除 dev-streaming，bootstrap 复用 smoke

**Files:**
- Delete: `scripts/funasr_stream_server.py`
- Modify: `scripts/voiceinput.sh`

- [ ] **Step 1: 删除流式服务脚本**

```bash
rm scripts/funasr_stream_server.py
```

- [ ] **Step 2: voiceinput.sh 删除 dev-streaming 函数**

删除整个 `voiceinput_linux_dev_streaming_impl()` 函数（从 `voiceinput_linux_dev_streaming_impl() {` 到其结尾 `}`，即 `usage() {` 之前，约 161 行，含其中传给 live 的过期参数 `--double-ctrl-window-ms`）。

- [ ] **Step 3: voiceinput.sh 删除 dispatch 分支**

在 case 块中删除：

```bash
  linux-dev)
    voiceinput_linux_dev_streaming_impl "$@"
    ;;
  linux-dev-streaming)
    voiceinput_linux_dev_streaming_impl "$@"
    ;;
```

- [ ] **Step 4: voiceinput.sh usage 更新**

在 `usage()` 的 heredoc 中删除两行：

```
  linux dev              启动 Linux 开发常驻服务
  linux dev-streaming    启动 Linux FunASR 流式开发服务
```

- [ ] **Step 5: bootstrap 复用 smoke 实现**

在 `voiceinput_bootstrap_impl` 中，将：

```bash
  if [[ -n "$smoke_audio_file" ]]; then
    echo "正在运行 Linux smoke"
    uv run -- cargo run -p voice-input-linux --features ibus -- smoke --audio-file "$smoke_audio_file"
  fi
```

替换为：

```bash
  if [[ -n "$smoke_audio_file" ]]; then
    echo "正在运行 Linux smoke"
    voiceinput_linux_smoke_impl --audio-file "$smoke_audio_file"
  fi
```

- [ ] **Step 6: bash 语法校验与冒烟**

Run: `bash -n scripts/voiceinput.sh && bash -n scripts/voiceinput_config.sh`
Expected: 无输出（语法正确）

Run: `scripts/voiceinput.sh --help`
Expected: usage 输出中不含 dev-streaming

- [ ] **Step 7: 提交**

```bash
git add scripts/
git commit -m "refactor(scripts): 删除无人读取的 dev-streaming 子命令，bootstrap 复用 smoke 实现"
```

---

### Task 9: README 重写精简

**Files:**
- Modify: `README.md`

- [ ] **Step 1: README 替换为精简版**

替换 `README.md` 全部内容：

````markdown
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

# 安装后立即跑 smoke 验证
scripts/voiceinput.sh linux install --audio-file testdata/smoke.wav
```

系统依赖（Ubuntu 20.04）：`build-essential`、`pkg-config`、`libdbus-1-dev`、`libibus-1.0-dev`、`python3`、`python3-venv`、`python3-pip`；可选 `libasound2-dev`、`portaudio19-dev`（Rust 录音后端）、`libx11-dev`（全局热键）。

默认热键：**双击 Alt**（可配 `--activation-hotkey DoubleCtrl`）。

## 命令入口

```bash
# smoke：音频文件 → 转写 → 注入活动窗口
cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus

# live：常驻托盘，热键触发录音
cargo run -p voice-input-linux --features ibus -- live --backend ibus \
  [--activation-hotkey DoubleAlt] [--double-press-window-ms 300] [--silence-stop-ms 1500]
```

`--backend` 只接受 `ibus`（其他值报错提示）。

## 模型

通过 `scripts/voiceinput.sh model <模型名>` 切换，或给 bootstrap/install/smoke 传 `--model`：

| 模型 | 参数量 | 特点 |
|---|---|---|
| `funasr`（FunASR Nano） | ~100MB | 最轻量，启动快，低配机器 |
| `qwen-0.6b`（Qwen3-ASR-0.6B，默认） | 0.6B | 精度/资源平衡，多数用户推荐 |
| `qwen`（Qwen3-ASR-1.7B） | 1.7B | 最高精度，需要 GPU 或充足内存 |

切换后运行 `scripts/voiceinput.sh bootstrap` 下载模型，再 `linux install` 更新服务。

模型 catalog 单一来源：`config/models.json`（编译期嵌入 Rust 侧）；`config/voiceinput.env` 是由 catalog 生成的仓库级默认配置（脚本维护）。

## 服务管理

```bash
systemctl --user start|stop|status voice-input.service
journalctl --user -u voice-input.service -f
scripts/voiceinput.sh linux uninstall   # 移除服务与自启
```

## Python 环境（开发）

```bash
uv venv .venv
uv pip install -r scripts/requirements-asr-base.txt -r scripts/requirements-asr-runtime.txt
scripts/voiceinput.sh bootstrap [--audio-file testdata/smoke.wav]
```
````

- [ ] **Step 2: 提交**

```bash
git add README.md
git commit -m "docs: README 精简重写——修正默认热键与参数名，合并重复章节"
```

---

### Task 10: 最终验证

**Files:** 无（仅验证）

- [ ] **Step 1: 全量构建与测试**

Run: `cargo build --workspace && cargo test --workspace && cargo build -p voice-input-linux --features ibus`
Expected: 全绿

- [ ] **Step 2: clippy**

Run: `cargo clippy --workspace --features ibus`
Expected: 无 warning（若有，修复后重跑）

- [ ] **Step 3: 真实 smoke 端到端**

Run: `cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus`
Expected: 预加载 ASR 模型成功，输出「识别结果：<文本>」。若 `.venv`/models 缺失则跳过并说明。

- [ ] **Step 4: 脚本入口冒烟**

Run: `scripts/voiceinput.sh --help` 与 `bash -n scripts/voiceinput.sh`
Expected: 无 dev-streaming，语法正确

- [ ] **Step 5: 统计与收尾**

```bash
git status --short
wc -l crates/*/src/*.rs scripts/*.sh README.md | tail -1
```

确认工作区只剩预期改动；对比行数（预期从 ~6500 降到 ~4500 左右）。

- [ ] **Step 6: 提交收尾**

```bash
git add -A
git commit -m "refactor: 彻底精简收尾——验证通过"
```

（若 Step 3 发现真实 smoke 行为异常，先修复再提交。）

---

## Self-Review 记录

- **Spec coverage**：spec 的 core/asr/audio/linux/scripts/README/测试/验证各节均有对应任务（Task 1/2/4/5/6/7/8/9/10；Task 3 为小清理补充；Task 0 为 spec 修订）。spec「明确不做的」三项（不合并 crate、不加 `--purge`、不改默认值）在计划中均未涉及。
- **Placeholder scan**：无 TBD/TODO；所有代码步骤均给出完整代码。
- **Type consistency（已对照真实代码逐一核验）**：
  - `RecordedAudio` 在 Task 6 Step 1 定义，Step 4/7/8/9 使用字段名 `samples`/`sample_rate` 一致；recorder.rs 的 `AudioRecorder` trait impl 同步更新（Step 7.3）。
  - `LinuxLiveArgs` 在 Task 4 Step 8 删除 `backend` 字段；Task 4 Step 7 的 main.rs 使用一致。
  - `validate_backend` 在 Task 4 Step 8 定义为 pub；Task 4 Step 7 的 main.rs 引用一致。
  - `HotkeyKind`/`ModifierKey` 在 Task 7 定义，`spec.kind()`/`modifier.is_held()`/`modifier.label()` 使用一致。
  - Task 4 中间态的 `LinuxLocalVoiceInput::new(config: LocalVoiceInputConfig, …, service_name)` 与 smoke.rs（Step 6）、session.rs（Step 11）调用一致；`run_demo` 真实存在于 controller.rs。
  - Task 5 后 `LiveJobHandle` 不再导出（lib.rs Step 10），runtime.rs Step 9 同步删除 import。
  - 已消除顺序耦合：Task 2 的 transcriber 保持返回 `Transcript`（Task 5 Step 5 才切 String），Task 4 一步内同时删除 backend.rs 与改写 main.rs，每个任务结束时编译绿色。
- **真实性核验**：所有「替换」步骤的 old_string 均与当前磁盘代码逐字一致（本计划编写时已逐一 Read 核验：config.rs/ime.rs/platform.rs/lib.rs/controller.rs/local.rs/smoke.rs/main.rs/live_cli.rs/runtime.rs/host.rs/ibus.rs/backend.rs/tray.rs/recorder.rs/hotkey.rs/session.rs/funasr.rs/transcriber.rs/runner.rs/file.rs/silence.rs/wav.rs/pcm.rs/Cargo.toml/models.json/voiceinput.sh/README.md）。
- **Task 4 执行记录（commit 669973a + 3bfbf96，两阶段审查后补记）**：
  - Step 3 的 ibus.rs 代码块中 `debug_xdotool!`/`debug_timestamp`/`DEBUG_START` 位于文件末尾——Rust `macro_rules!` 必须先于使用，执行时移至文件顶部（内容不变）。
  - Step 3 的 `#[cfg(not(feature = "ibus"))]` 块缺少 `insert_text_into_active_window` 存根，而 host.rs 无条件调用它，无 feature 构建无法编译——执行时补齐与其余两个存根一致的 no-op 存根。
  - 质量审查追加修复（3bfbf96）：`run_smoke` 无 ibus feature 时明确报错（已同步写入 Task 5 Step 7 代码块；Task 6 Step 9 只改 import 与 `transcribe_file`，不触碰 `run_smoke`）；补 `validate_backend`（6 例）与 `parse_smoke_args`（4 例）单测；`host.commit_text` 恢复「Linux 文本提交失败：{err}」错误上下文（已同步写入 Task 5 Step 8 代码块）。
- **Task 5 执行记录（commit c514f5b，两阶段审查后补记）**：
  - Step 10 的 lib.rs 代码块遗漏 `pub use host::LinuxInputMethodHost;`——host 模块私有，缺少该行会让 Step 8 要求保留的 `service_name` 字段/访问器产生 dead_code 警告。执行时已补回（上方代码块已同步修正）。
  - Step 11 的测试路径 `"../testdata/smoke.wav"` 有误（CARGO_MANIFEST_DIR 指向 crates/voice-input-linux，`../testdata` 解析为 crates/testdata，不存在）；实际路径为 `"../../testdata/smoke.wav"`（上方代码块已同步修正）。
  - 质量审查 Minor（记录待 Task 10 收尾评估）：smoke.rs `transcribe_file` 文档注释「纯函数」措辞宜改（函数有文件 I/O）；host.rs `service_name` 访问器与 core `MockTranscriber` 保留但零调用者（计划要求保留，后续任务评估去留）。
- **Task 6 执行记录（commit c55c208 + 8ebb210，两阶段审查后补记）**：
  - 无 spec 缺陷；Step 4/5 代码块与磁盘逐字匹配，一次通过规范审查。
  - 质量审查追加修复（8ebb210）：(1) file.rs 测试块 rustfmt 折行（仓库存在历史 fmt 漂移，仅局部修正本任务新代码，未全仓跑 cargo fmt）；(2) 测试临时 WAV 加 `TempFileGuard` Drop guard，`record_once().expect` panic 时不泄漏 /tmp 文件；(3) `has_peak_above` 改用 `unsigned_abs() > threshold as u16`，修复 `i16::MIN` 采样在 debug 构建 panic / release 构建误判静音的边界缺陷（负阈值经 `as u16` 回绕的语义与旧代码 `abs() > 负值` 恒真一致，实际唯一调用点传 800，无影响）。
  - 审查者建议跳过并记录（不阻塞）：`average_to_mono` 与 pcm.rs `push_mono_i16_i16` 的 downmix 数学重复（超出本任务文件清单，未抽公共 helper）；runtime.rs `write_pcm_wav(...)?` 使编码错误从日志记录改为向上传播（Cursor 写入不可失败，理论性）。
  - 审查者可选 Nit 未修（其标注「不需要再修」）：file.rs 测试末尾 `std::fs::remove_file(&path).ok()` 与 guard 的 Drop 重复，可直接删该行；`has_peak_above` 未补 i16::MIN 回归测试（可加 `assert!(has_peak_above(&[i16::MIN], 800))`）。
- **Task 7 执行记录（commit e8ac422，审查后补记）**：
  - **spec 缺陷（计划代码块编译不过）**：Step 1 的 `kind: double_press.unwrap_or(combo)` 类型错误——`double_press` 是 `Option<ModifierKey>`，`unwrap_or` 期望 `Option<HotkeyKind>`。执行时最小修复为 `double_press.map(HotkeyKind::DoublePress).unwrap_or(combo)`（上方代码块已同步修正）；语义：双击 token 优先于组合 token，与旧代码 `matches()` 先查 `double_ctrl`/`double_alt` 的优先级一致。
  - Step 1 代码块另有 2 处 rustfmt 漂移（单字符 match 臂折行、`set_combo_key` if-let），执行时仅对 hotkey.rs 单文件跑 rustfmt（未全仓 fmt，保留 funasr.rs/ibus.rs 历史漂移）。
  - 测试数从 27 降为 25（删除 2 个冗余测试），全绿。
  - 质量审查追加修复（e31fb69）：(1) `ModifierKey`/`HotkeyKind` 收敛为 `pub(crate)`、`kind()` 为 `pub(crate)`（原计划代码块的 `pub` 在私有模块中构成死表面；完全私有会触发 `private_interfaces` lint，故取 pub(crate)）；(2) `matches()` 补 doc comment 说明 DoublePress 分支严格语义与监听循环 has_any 的有意差异；(3) 补 2 个测试：全部双击别名变体（9 个 Ctrl + 3 个 Alt）与优先级（双击覆盖组合、后者覆盖前者）。
  - 审查者 Nit 跳过并记录：`set_combo_modifier`/`set_combo_key` 对 DoublePress 静默 no-op 的形状问题（建议 ComboSpec builder，超范围）；`matches()` doc comment「解析校验」措辞无对应调用点，宜改「测试」——后者归入 Task 10 收尾。
- **Task 8 执行记录（commit f14a3d2，两阶段审查后补记）**：
  - 无 spec 缺陷，一次通过规范审查（删除范围逐行核验：py 224 行、函数 161 行、case 6 行、usage 2 行；唯一新增行为 bootstrap 替换行）。
  - 质量审查 Minor 记录待 Task 10 评估（不阻塞，计划 Step 5 明确指定的替换）：bootstrap --audio-file 复用 `voiceinput_linux_smoke_impl` 后经由其 dpkg-only 的 `voiceinput_ensure_linux_dev_deps`——非 Debian 系统会无条件 exit 2（旧内联 cargo 调用至少会尝试构建）；Debian 上 bootstrap 可能触发 sudo apt-get（旧路径不会）。目标平台为 Ubuntu 系，且对全新环境该预检反而更完善；manager-aware 改造属行为增强，超出「精简」范围。
  - 质量审查 Nit 记录：历史设计文档 `docs/superpowers/specs/2026-06-29-linux-only-refactor-design.md:98` 仍列 `linux dev`/`linux dev-streaming`（历史文档，不改）。
  - 磁盘上 `scripts/__pycache__/funasr_stream_server.cpython-38.pyc` 为被删文件的陈旧编译产物（gitignored、未被提交、惰性无害），Task 10 顺手从磁盘删除。
