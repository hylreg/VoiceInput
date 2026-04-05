use std::io::{BufRead, BufReader, Cursor, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
#[cfg(unix)]
use std::{os::unix::net::UnixStream, path::PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use hound::WavReader;
use tempfile::NamedTempFile;

use crate::config::FunAsrConfig;
use crate::runner::{FunAsrRequest, FunAsrRunner, FunAsrStreamingRunner};
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

const PYTHON_SCRIPT: &str = r#"
import json
import os
import sys
import contextlib
from funasr import AutoModel
import torch

model_dir = sys.argv[1]
remote_code = sys.argv[2]
audio_path = sys.argv[3]
device = sys.argv[4]
language = sys.argv[5]
itn = sys.argv[6] == "true"
hotwords = json.loads(sys.argv[7])

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

    res = model.generate(
        input=[audio_path],
        cache={},
        batch_size=1,
        hotwords=hotwords,
        language=language,
        itn=itn,
    )

text = res[0]["text"].strip()
print(text)
"#;

const PYTHON_STREAM_SCRIPT: &str = r#"
import base64
import contextlib
import json
import os
import sys
import tempfile
import wave

import numpy as np
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

pending_samples = np.array([], dtype=np.float32)
preview_window_seconds = 6
sample_rate = 16000

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue

    if line == "__quit__":
        break

    try:
        request = json.loads(line)
        action = request.get("action", "chunk")

        if action == "reset":
            pending_samples = np.array([], dtype=np.float32)
            print(json.dumps({"ok": True}), flush=True)
            continue

        if action != "chunk":
            print(json.dumps({"error": f"unknown action: {action}"}), flush=True)
            continue

        pcm = base64.b64decode(request["pcm_b64"])
        new_samples = np.frombuffer(pcm, dtype=np.int16).astype(np.float32) / 32768.0
        pending_samples = np.concatenate([pending_samples, new_samples])
        preview_window_seconds = int(request.get("preview_window_seconds", preview_window_seconds))
        sample_rate = int(request.get("sample_rate", sample_rate))
        is_final = request.get("is_final", False)

        if is_final:
            inference_samples = pending_samples
            pending_samples = np.array([], dtype=np.float32)
        else:
            preview_window_samples = max(int(preview_window_seconds * sample_rate), sample_rate)
            inference_samples = pending_samples[-preview_window_samples:]

        text = ""
        if inference_samples.size > 0:
            int16_samples = np.clip(inference_samples * 32768.0, -32768, 32767).astype(np.int16)
            with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
                wav_path = tmp.name

            try:
                with wave.open(wav_path, "wb") as wav_file:
                    wav_file.setnchannels(1)
                    wav_file.setsampwidth(2)
                    wav_file.setframerate(sample_rate)
                    wav_file.writeframes(int16_samples.tobytes())

                with contextlib.redirect_stdout(sys.stderr):
                    res = model.generate(
                        input=[wav_path],
                        cache={},
                        batch_size=1,
                        hotwords=request.get("hotwords", []),
                        language=request.get("language"),
                        itn=request.get("itn", True),
                    )

                if isinstance(res, list) and res and isinstance(res[0], dict):
                    text = str(res[0].get("text", "")).strip()
            finally:
                try:
                    os.unlink(wav_path)
                except OSError:
                    pass

        print(json.dumps({"text": text, "is_final": is_final}), flush=True)
    except Exception as exc:
        print(json.dumps({"error": str(exc)}), flush=True)
"#;

const PYTHON_QWEN_WORKER_SCRIPT: &str = r#"
import contextlib
import json
import os
import sys

import torch
from qwen_asr import Qwen3ASRModel

model_dir = sys.argv[1]
device = sys.argv[2]

if device == "cuda":
    device_map = "cuda:0"
    dtype = torch.bfloat16
else:
    device_map = "cpu"
    dtype = torch.float32

with contextlib.redirect_stdout(sys.stderr):
    model = Qwen3ASRModel.from_pretrained(
        model_dir,
        device_map=device_map,
        dtype=dtype,
        max_inference_batch_size=1,
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

const PYTHON_QWEN_SCRIPT: &str = r#"
import contextlib
import json
import sys

import torch
from qwen_asr import Qwen3ASRModel

model_dir = sys.argv[1]
audio_path = sys.argv[2]
device = sys.argv[3]
language = sys.argv[4]

if language in ("", "auto", "automatic", "自动"):
    language = None

if device == "cuda":
    device_map = "cuda:0"
    dtype = torch.bfloat16
else:
    device_map = "cpu"
    dtype = torch.float32

with contextlib.redirect_stdout(sys.stderr):
    model = Qwen3ASRModel.from_pretrained(
        model_dir,
        device_map=device_map,
        dtype=dtype,
        max_inference_batch_size=1,
    )
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

print(text.strip())
"#;

fn python_command(python_bin: &str, script: &str) -> Command {
    if python_bin == "uv" {
        let mut command = Command::new(python_bin);
        command.arg("run").arg("--").arg("python").arg("-c").arg(script);
        command
    } else {
        let mut command = Command::new(python_bin);
        command.arg("-c").arg(script);
        command
    }
}

pub struct PythonFunAsrRunner {
    python_bin: String,
    worker: Option<Arc<Mutex<PythonFunAsrWorker>>>,
}

struct PythonFunAsrWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Default for PythonFunAsrRunner {
    fn default() -> Self {
        if let Ok(python_bin) = std::env::var("PYTHON_BIN") {
            return Self {
                python_bin,
                worker: None,
            };
        }

        if std::env::var("VOICEINPUT_USE_UV")
            .map(|v| v != "0")
            .unwrap_or(true)
        {
            if which::which("uv").is_ok() {
                return Self {
                    python_bin: "uv".to_string(),
                    worker: None,
                };
            }
        }

        let uv_python = Path::new(".venv/bin/python");
        if uv_python.exists() {
            return Self {
                python_bin: uv_python.to_string_lossy().to_string(),
                worker: None,
            };
        }

        Self {
            python_bin: "python3".to_string(),
            worker: None,
        }
    }
}

impl PythonFunAsrRunner {
    pub fn new(python_bin: impl Into<String>) -> Self {
        Self {
            python_bin: python_bin.into(),
            worker: None,
        }
    }

    pub fn connect(config: FunAsrConfig) -> Result<Self> {
        let runner = Self::default();
        let worker = PythonFunAsrWorker::spawn(&runner.python_bin, &config)?;
        let python_bin = runner.python_bin.clone();

        Ok(Self {
            python_bin,
            worker: Some(Arc::new(Mutex::new(worker))),
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

        let mut ready_line = String::new();
        let read = stdout.read_line(&mut ready_line).map_err(|e| {
            VoiceInputError::Transcription(format!("等待 ASR worker 就绪失败：{e}"))
        })?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "ASR worker 启动后没有返回就绪信号".to_string(),
            ));
        }

        let ready = serde_json::from_str::<serde_json::Value>(ready_line.trim()).map_err(|e| {
            VoiceInputError::Transcription(format!("解析 ASR worker 就绪信号失败：{e}"))
        })?;
        if ready.get("ready").and_then(|value| value.as_bool()) != Some(true) {
            return Err(VoiceInputError::Transcription(format!(
                "ASR worker 就绪信号异常：{}",
                ready
            )));
        }

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

        if request.config.is_qwen() {
            let output = if self.python_bin == "uv" {
                Command::new(&self.python_bin)
                    .arg("run")
                    .arg("--")
                    .arg("python")
                    .arg("-c")
                    .arg(PYTHON_QWEN_SCRIPT)
                    .arg(&request.config.model_dir)
                    .arg(audio_file.path())
                    .arg(&request.config.device)
                    .arg(&request.config.language)
                    .output()
                    .map_err(|e| {
                        VoiceInputError::Transcription(format!(
                            "通过 uv 启动 Qwen ASR Python 进程失败：{e}"
                        ))
                    })?
            } else {
                Command::new(&self.python_bin)
                    .arg("-c")
                    .arg(PYTHON_QWEN_SCRIPT)
                    .arg(&request.config.model_dir)
                    .arg(audio_file.path())
                    .arg(&request.config.device)
                    .arg(&request.config.language)
                    .output()
                    .map_err(|e| {
                        VoiceInputError::Transcription(format!(
                            "启动 Qwen ASR Python 进程失败：{e}"
                        ))
                    })?
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(VoiceInputError::Transcription(format!(
                    "Qwen ASR 进程失败：{stderr}"
                )));
            }

            let text = String::from_utf8(output.stdout).map_err(|e| {
                VoiceInputError::Transcription(format!("Qwen ASR 输出不是有效的 UTF-8：{e}"))
            })?;
            return Ok(text.trim().to_string());
        }

        if let Some(worker) = &self.worker {
            let mut worker = worker.lock().map_err(|_| {
                VoiceInputError::Transcription("锁定 ASR worker 失败".to_string())
            })?;
            return worker.transcribe(audio_file.path(), &request);
        }

        let hotwords_json = serde_json_like_array(&request.config.hotwords);
        let output = if self.python_bin == "uv" {
            Command::new(&self.python_bin)
                .arg("run")
                .arg("--")
                .arg("python")
                .arg("-c")
                .arg(PYTHON_SCRIPT)
                .arg(&request.config.model_dir)
                .arg(&request.config.remote_code)
                .arg(audio_file.path())
                .arg(&request.config.device)
                .arg(&request.config.language)
                .arg(if request.config.itn { "true" } else { "false" })
                .arg(hotwords_json)
                .output()
                .map_err(|e| {
                    VoiceInputError::Transcription(format!(
                        "通过 uv 启动 ASR Python 进程失败：{e}"
                    ))
                })?
        } else {
            Command::new(&self.python_bin)
                .arg("-c")
                .arg(PYTHON_SCRIPT)
                .arg(&request.config.model_dir)
                .arg(&request.config.remote_code)
                .arg(audio_file.path())
                .arg(&request.config.device)
                .arg(&request.config.language)
                .arg(if request.config.itn { "true" } else { "false" })
                .arg(hotwords_json)
                .output()
                .map_err(|e| {
                    VoiceInputError::Transcription(format!("启动 ASR Python 进程失败：{e}"))
                })?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceInputError::Transcription(format!(
                "ASR 进程失败：{stderr}"
            )));
        }

        let text = String::from_utf8(output.stdout).map_err(|e| {
            VoiceInputError::Transcription(format!("ASR 输出不是有效的 UTF-8：{e}"))
        })?;
        Ok(text.trim().to_string())
    }
}

impl Drop for PythonFunAsrRunner {
    fn drop(&mut self) {
        if let Some(worker) = &self.worker {
            if let Ok(mut worker) = worker.lock() {
                worker.shutdown();
            }
        }
    }
}

pub struct PythonFunAsrStreamingRunner {
    worker: Arc<Mutex<PythonFunAsrStreamingWorker>>,
    config: FunAsrConfig,
}

struct PythonFunAsrStreamingWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl PythonFunAsrStreamingRunner {
    pub fn connect(config: FunAsrConfig) -> Result<Self> {
        if config.is_qwen() {
            return Err(VoiceInputError::Transcription(
                "Qwen/Qwen3-ASR-1.7B 目前不支持 FunASR 流式调试服务".to_string(),
            ));
        }
        let runner = PythonFunAsrRunner::default();
        let worker = PythonFunAsrStreamingWorker::spawn(&runner.python_bin, &config)?;

        Ok(Self {
            worker: Arc::new(Mutex::new(worker)),
            config,
        })
    }

    pub fn stream_chunk(
        &self,
        samples: &[i16],
        sample_rate: u32,
        is_final: bool,
    ) -> Result<String> {
        let mut worker = self.worker.lock().map_err(|_| {
            VoiceInputError::Transcription("锁定 FunASR 流式 worker 失败".to_string())
        })?;
        worker.stream_chunk(samples, sample_rate, is_final, &self.config)
    }
}

impl PythonFunAsrStreamingWorker {
    fn spawn(python_bin: &str, config: &FunAsrConfig) -> Result<Self> {
        let mut command = if python_bin == "uv" {
            let mut command = Command::new(python_bin);
            command
                .arg("run")
                .arg("--")
                .arg("python")
                .arg("-c")
                .arg(PYTHON_STREAM_SCRIPT);
            command
        } else {
            let mut command = Command::new(python_bin);
            command.arg("-c").arg(PYTHON_STREAM_SCRIPT);
            command
        };

        command
            .arg(&config.model_dir)
            .arg(&config.remote_code)
            .arg(&config.device)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command.spawn().map_err(|e| {
            VoiceInputError::Transcription(format!("启动 FunASR 流式 worker 失败：{e}"))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            VoiceInputError::Transcription("获取 FunASR 流式 worker stdin 失败".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            VoiceInputError::Transcription("获取 FunASR 流式 worker stdout 失败".to_string())
        })?;
        let mut stdout = BufReader::new(stdout);

        let mut ready_line = String::new();
        let read = stdout.read_line(&mut ready_line).map_err(|e| {
            VoiceInputError::Transcription(format!("等待 FunASR 流式 worker 就绪失败：{e}"))
        })?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "FunASR 流式 worker 启动后没有返回就绪信号".to_string(),
            ));
        }

        let ready = serde_json::from_str::<serde_json::Value>(ready_line.trim()).map_err(|e| {
            VoiceInputError::Transcription(format!("解析 FunASR 流式 worker 就绪信号失败：{e}"))
        })?;
        if ready.get("ready").and_then(|value| value.as_bool()) != Some(true) {
            return Err(VoiceInputError::Transcription(format!(
                "FunASR 流式 worker 就绪信号异常：{}",
                ready
            )));
        }

        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    fn stream_chunk(
        &mut self,
        samples: &[i16],
        sample_rate: u32,
        is_final: bool,
        config: &FunAsrConfig,
    ) -> Result<String> {
        let normalized = resample_pcm16(samples, sample_rate, 16_000);
        let pcm_bytes = pcm16_to_bytes(&normalized);
        let payload = serde_json::json!({
            "action": "chunk",
            "pcm_b64": BASE64.encode(pcm_bytes),
            "language": config.language,
            "itn": config.itn,
            "hotwords": config.hotwords,
            "is_final": is_final,
        });

        serde_json::to_writer(&mut self.stdin, &payload).map_err(|e| {
            VoiceInputError::Transcription(format!("写入 FunASR 流式 worker 请求失败：{e}"))
        })?;
        self.stdin.write_all(b"\n").map_err(|e| {
            VoiceInputError::Transcription(format!("发送 FunASR 流式 worker 请求失败：{e}"))
        })?;
        self.stdin.flush().map_err(|e| {
            VoiceInputError::Transcription(format!("刷新 FunASR 流式 worker 请求失败：{e}"))
        })?;

        let mut response = String::new();
        let read = self.stdout.read_line(&mut response).map_err(|e| {
            VoiceInputError::Transcription(format!("读取 FunASR 流式 worker 响应失败：{e}"))
        })?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "FunASR 流式 worker 已退出".to_string(),
            ));
        }

        let json: serde_json::Value = serde_json::from_str(response.trim()).map_err(|e| {
            VoiceInputError::Transcription(format!("解析 FunASR 流式 worker 响应失败：{e}"))
        })?;
        if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
            return Err(VoiceInputError::Transcription(format!(
                "FunASR 流式 worker 返回错误：{error}"
            )));
        }

        let text = json
            .get("text")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                VoiceInputError::Transcription("FunASR 流式 worker 响应缺少 text".to_string())
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

impl Drop for PythonFunAsrStreamingRunner {
    fn drop(&mut self) {
        if let Ok(mut worker) = self.worker.lock() {
            worker.shutdown();
        }
    }
}

impl FunAsrStreamingRunner for PythonFunAsrStreamingRunner {
    fn stream_chunk(&self, samples: &[i16], sample_rate: u32, is_final: bool) -> Result<String> {
        PythonFunAsrStreamingRunner::stream_chunk(self, samples, sample_rate, is_final)
    }
}

#[cfg(unix)]
pub struct SocketFunAsrStreamingRunner {
    stream: Arc<Mutex<SocketFunAsrStreamingConnection>>,
    config: FunAsrConfig,
}

#[cfg(unix)]
struct SocketFunAsrStreamingConnection {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
}

#[cfg(unix)]
impl SocketFunAsrStreamingRunner {
    pub fn connect(socket_path: impl Into<PathBuf>, config: FunAsrConfig) -> Result<Self> {
        if config.is_qwen() {
            return Err(VoiceInputError::Transcription(
                "Qwen/Qwen3-ASR-1.7B 目前不支持 FunASR 开发调试 socket".to_string(),
            ));
        }
        let socket_path = socket_path.into();
        let stream = UnixStream::connect(&socket_path).map_err(|e| {
            VoiceInputError::Transcription(format!(
                "连接 FunASR 开发调试 socket 失败 {}：{e}",
                socket_path.display()
            ))
        })?;
        let reader = stream
            .try_clone()
            .map(BufReader::new)
            .map_err(|e| VoiceInputError::Transcription(format!("克隆 FunASR socket 失败：{e}")))?;

        let mut connection = SocketFunAsrStreamingConnection {
            writer: stream,
            reader,
        };
        connection.wait_ready()?;

        Ok(Self {
            stream: Arc::new(Mutex::new(connection)),
            config,
        })
    }

    pub fn stream_chunk(
        &self,
        samples: &[i16],
        sample_rate: u32,
        is_final: bool,
    ) -> Result<String> {
        let mut connection = self.stream.lock().map_err(|_| {
            VoiceInputError::Transcription("锁定 FunASR 开发调试 socket 失败".to_string())
        })?;
        connection.stream_chunk(samples, sample_rate, is_final, &self.config)
    }
}

#[cfg(unix)]
impl FunAsrStreamingRunner for SocketFunAsrStreamingRunner {
    fn stream_chunk(&self, samples: &[i16], sample_rate: u32, is_final: bool) -> Result<String> {
        SocketFunAsrStreamingRunner::stream_chunk(self, samples, sample_rate, is_final)
    }
}

impl FunAsrRunner for SocketFunAsrStreamingRunner {
    fn transcribe(&self, request: FunAsrRequest) -> Result<String> {
        let (samples, sample_rate) = wav_bytes_to_pcm16(&request.audio_bytes)?;
        self.stream_chunk(&samples, sample_rate, true)
    }
}

#[cfg(unix)]
impl SocketFunAsrStreamingConnection {
    fn wait_ready(&mut self) -> Result<()> {
        let mut ready_line = String::new();
        let read = self.reader.read_line(&mut ready_line).map_err(|e| {
            VoiceInputError::Transcription(format!("等待 FunASR 开发调试服务就绪失败：{e}"))
        })?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "FunASR 开发调试服务启动后没有返回就绪信号".to_string(),
            ));
        }

        let ready = serde_json::from_str::<serde_json::Value>(ready_line.trim()).map_err(|e| {
            VoiceInputError::Transcription(format!("解析 FunASR 开发调试服务就绪信号失败：{e}"))
        })?;
        if ready.get("ready").and_then(|value| value.as_bool()) != Some(true) {
            return Err(VoiceInputError::Transcription(format!(
                "FunASR 开发调试服务就绪信号异常：{}",
                ready
            )));
        }

        Ok(())
    }

    fn stream_chunk(
        &mut self,
        samples: &[i16],
        sample_rate: u32,
        is_final: bool,
        config: &FunAsrConfig,
    ) -> Result<String> {
        let normalized = resample_pcm16(samples, sample_rate, 16_000);
        let pcm_bytes = pcm16_to_bytes(&normalized);
        let payload = serde_json::json!({
            "action": "chunk",
            "pcm_b64": BASE64.encode(pcm_bytes),
            "language": config.language,
            "itn": config.itn,
            "hotwords": config.hotwords,
            "is_final": is_final,
        });

        serde_json::to_writer(&mut self.writer, &payload).map_err(|e| {
            VoiceInputError::Transcription(format!("写入 FunASR 开发调试请求失败：{e}"))
        })?;
        self.writer.write_all(b"\n").map_err(|e| {
            VoiceInputError::Transcription(format!("发送 FunASR 开发调试请求失败：{e}"))
        })?;
        self.writer.flush().map_err(|e| {
            VoiceInputError::Transcription(format!("刷新 FunASR 开发调试请求失败：{e}"))
        })?;

        let mut response = String::new();
        let read = self.reader.read_line(&mut response).map_err(|e| {
            VoiceInputError::Transcription(format!("读取 FunASR 开发调试响应失败：{e}"))
        })?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "FunASR 开发调试服务已断开".to_string(),
            ));
        }

        let json: serde_json::Value = serde_json::from_str(response.trim()).map_err(|e| {
            VoiceInputError::Transcription(format!("解析 FunASR 开发调试响应失败：{e}"))
        })?;
        if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
            return Err(VoiceInputError::Transcription(format!(
                "FunASR 开发调试服务返回错误：{error}"
            )));
        }

        let text = json
            .get("text")
            .and_then(|value| value.as_str())
            .ok_or_else(|| {
                VoiceInputError::Transcription("FunASR 开发调试响应缺少 text".to_string())
            })?;
        Ok(text.trim().to_string())
    }
}

fn resample_pcm16(samples: &[i16], input_rate: u32, output_rate: u32) -> Vec<i16> {
    if samples.is_empty() || input_rate == 0 || input_rate == output_rate {
        return samples.to_vec();
    }

    let ratio = output_rate as f64 / input_rate as f64;
    let output_len = ((samples.len() as f64) * ratio).round().max(1.0) as usize;
    let last_index = samples.len().saturating_sub(1);
    let mut output = Vec::with_capacity(output_len);

    for index in 0..output_len {
        let source_pos = (index as f64) / ratio;
        let left = source_pos.floor() as usize;
        let frac = source_pos - left as f64;
        let left_index = left.min(last_index);
        let right_index = (left_index + 1).min(last_index);

        let value = if left_index == right_index {
            samples[left_index] as f64
        } else {
            let left_value = samples[left_index] as f64;
            let right_value = samples[right_index] as f64;
            left_value + (right_value - left_value) * frac
        };

        output.push(value.round().clamp(i16::MIN as f64, i16::MAX as f64) as i16);
    }

    output
}

fn pcm16_to_bytes(samples: &[i16]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    bytes
}

fn wav_bytes_to_pcm16(audio_bytes: &[u8]) -> Result<(Vec<i16>, u32)> {
    let reader = WavReader::new(Cursor::new(audio_bytes))
        .map_err(|e| VoiceInputError::Transcription(format!("解析 WAV 音频失败：{e}")))?;
    let spec = reader.spec();

    if spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(VoiceInputError::Transcription(format!(
            "仅支持单声道 16-bit PCM WAV，当前格式：channels={} bits_per_sample={} sample_format={:?}",
            spec.channels, spec.bits_per_sample, spec.sample_format
        )));
    }

    let mut samples = Vec::new();
    for sample in reader.into_samples::<i16>() {
        samples.push(
            sample
                .map_err(|e| VoiceInputError::Transcription(format!("读取 WAV 采样失败：{e}")))?,
        );
    }

    Ok((samples, spec.sample_rate))
}

#[cfg(test)]
mod tests {
    use super::resample_pcm16;

    #[test]
    fn resample_pcm16_keeps_matching_rate_unchanged() {
        let samples = vec![0i16, 1024, -1024, 2048];
        assert_eq!(resample_pcm16(&samples, 16_000, 16_000), samples);
    }

    #[test]
    fn resample_pcm16_expands_shorter_input() {
        let samples = vec![0i16, 1000];
        let resampled = resample_pcm16(&samples, 8_000, 16_000);
        assert_eq!(resampled.len(), 4);
        assert_eq!(resampled.first().copied(), Some(0));
        assert_eq!(resampled.last().copied(), Some(1000));
    }
}

fn serde_json_like_array(values: &[String]) -> String {
    let mut encoded = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            encoded.push(',');
        }
        encoded.push('"');
        for ch in value.chars() {
            match ch {
                '"' => encoded.push_str("\\\""),
                '\\' => encoded.push_str("\\\\"),
                '\n' => encoded.push_str("\\n"),
                '\r' => encoded.push_str("\\r"),
                '\t' => encoded.push_str("\\t"),
                other => encoded.push(other),
            }
        }
        encoded.push('"');
    }
    encoded.push(']');
    encoded
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
