use std::io::{BufRead, BufReader, Write};
use std::path::Path;
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

const PYTHON_QWEN_SCRIPT: &str = r#"
import contextlib
import json
import sys

import torch
from transformers.utils import logging as hf_logging
from qwen_asr import Qwen3ASRModel

model_dir = sys.argv[1]
audio_path = sys.argv[2]
device = sys.argv[3]
language = sys.argv[4]

hf_logging.set_verbosity_error()

if language in ("", "auto", "automatic", "自动"):
    language = None

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

        if let Some(worker) = &self.worker {
            let mut worker = worker
                .lock()
                .map_err(|_| VoiceInputError::Transcription("锁定 ASR worker 失败".to_string()))?;
            return worker.transcribe(audio_file.path(), &request);
        }

        if request.config.is_qwen() {
            let qwen_language = request
                .config
                .qwen_language()
                .unwrap_or_else(|| request.config.language.clone());
            let output = if self.python_bin == "uv" {
                Command::new(&self.python_bin)
                    .env("TRANSFORMERS_VERBOSITY", "error")
                    .arg("run")
                    .arg("--")
                    .arg("python")
                    .arg("-c")
                    .arg(PYTHON_QWEN_SCRIPT)
                    .arg(&request.config.model_dir)
                    .arg(audio_file.path())
                    .arg(&request.config.device)
                    .arg(&qwen_language)
                    .output()
                    .map_err(|e| {
                        VoiceInputError::Transcription(format!(
                            "通过 uv 启动 Qwen ASR Python 进程失败：{e}"
                        ))
                    })?
            } else {
                Command::new(&self.python_bin)
                    .env("TRANSFORMERS_VERBOSITY", "error")
                    .arg("-c")
                    .arg(PYTHON_QWEN_SCRIPT)
                    .arg(&request.config.model_dir)
                    .arg(audio_file.path())
                    .arg(&request.config.device)
                    .arg(&qwen_language)
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
                    VoiceInputError::Transcription(format!("通过 uv 启动 ASR Python 进程失败：{e}"))
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

#[cfg(test)]
mod tests {
    use super::{read_ready_json, FunAsrRequest, PythonFunAsrRunner, PythonFunAsrWorker};
    use crate::runner::FunAsrRunner;
    use std::io::BufReader;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};

    #[test]
    fn qwen_transcribe_prefers_reused_worker_when_available() {
        let worker = spawn_test_worker();
        let runner = PythonFunAsrRunner {
            python_bin: "python3".to_string(),
            worker: Some(Arc::new(Mutex::new(worker))),
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
