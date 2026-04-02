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
        let mut command = if python_bin == "uv" {
            let mut command = Command::new(python_bin);
            command.arg("run").arg("--").arg("python").arg("-c").arg(PYTHON_WORKER_SCRIPT);
            command
        } else {
            let mut command = Command::new(python_bin);
            command.arg("-c").arg(PYTHON_WORKER_SCRIPT);
            command
        };

        command
            .arg(&config.model_dir)
            .arg(&config.remote_code)
            .arg(&config.device)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut child = command
            .spawn()
            .map_err(|e| VoiceInputError::Transcription(format!("启动 FunASR worker 失败：{e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| VoiceInputError::Transcription("获取 FunASR worker stdin 失败".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| VoiceInputError::Transcription("获取 FunASR worker stdout 失败".to_string()))?;
        let mut stdout = BufReader::new(stdout);

        let mut ready_line = String::new();
        let read = stdout
            .read_line(&mut ready_line)
            .map_err(|e| VoiceInputError::Transcription(format!("等待 FunASR worker 就绪失败：{e}")))?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "FunASR worker 启动后没有返回就绪信号".to_string(),
            ));
        }

        let ready = serde_json::from_str::<serde_json::Value>(ready_line.trim())
            .map_err(|e| VoiceInputError::Transcription(format!("解析 FunASR worker 就绪信号失败：{e}")))?;
        if ready.get("ready").and_then(|value| value.as_bool()) != Some(true) {
            return Err(VoiceInputError::Transcription(format!(
                "FunASR worker 就绪信号异常：{}",
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
        let payload = serde_json::json!({
            "audio_path": audio_path,
            "language": request.config.language,
            "itn": request.config.itn,
            "hotwords": request.config.hotwords,
        });
        serde_json::to_writer(&mut self.stdin, &payload)
            .map_err(|e| VoiceInputError::Transcription(format!("写入 FunASR worker 请求失败：{e}")))?;
        self.stdin
            .write_all(b"\n")
            .map_err(|e| VoiceInputError::Transcription(format!("发送 FunASR worker 请求失败：{e}")))?;
        self.stdin
            .flush()
            .map_err(|e| VoiceInputError::Transcription(format!("刷新 FunASR worker 请求失败：{e}")))?;

        let mut response = String::new();
        let read = self
            .stdout
            .read_line(&mut response)
            .map_err(|e| VoiceInputError::Transcription(format!("读取 FunASR worker 响应失败：{e}")))?;
        if read == 0 {
            return Err(VoiceInputError::Transcription(
                "FunASR worker 已退出".to_string(),
            ));
        }

        let json: serde_json::Value = serde_json::from_str(response.trim())
            .map_err(|e| VoiceInputError::Transcription(format!("解析 FunASR worker 响应失败：{e}")))?;
        if let Some(error) = json.get("error").and_then(|value| value.as_str()) {
            return Err(VoiceInputError::Transcription(format!("FunASR worker 返回错误：{error}")));
        }

        let text = json
            .get("text")
            .and_then(|value| value.as_str())
            .ok_or_else(|| VoiceInputError::Transcription("FunASR worker 响应缺少 text".to_string()))?;
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
                .map_err(|_| VoiceInputError::Transcription("锁定 FunASR worker 失败".to_string()))?;
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
                        "通过 uv 启动 FunASR Python 进程失败：{e}"
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
                    VoiceInputError::Transcription(format!("启动 FunASR Python 进程失败：{e}"))
                })?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceInputError::Transcription(format!(
                "FunASR 进程失败：{stderr}"
            )));
        }

        let text = String::from_utf8(output.stdout).map_err(|e| {
            VoiceInputError::Transcription(format!("FunASR 输出不是有效的 UTF-8：{e}"))
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
