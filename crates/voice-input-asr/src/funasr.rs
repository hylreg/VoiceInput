use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};

use tempfile::NamedTempFile;

use crate::config::FunAsrConfig;
use crate::runner::{FunAsrRequest, FunAsrRunner};
use voice_input_core::{Result, VoiceInputError};

const PYTHON_SCRIPT: &str = r#"
import json
import os
import sys
from funasr import AutoModel
import torch

model_dir = sys.argv[1]
remote_code = sys.argv[2]
audio_path = sys.argv[3]
device = sys.argv[4]
language = sys.argv[5]
itn = sys.argv[6] == "true"
hotwords = json.loads(sys.argv[7])

if device == "auto":
    if torch.cuda.is_available():
        device = "cuda"
    elif hasattr(torch.backends, "mps") and torch.backends.mps.is_available():
        device = "mps"
    else:
        device = "cpu"

model = AutoModel(
    model=model_dir,
    trust_remote_code=True,
    remote_code=remote_code,
    device=device,
)

res = model.generate(
    input=[audio_path],
    cache={},
    batch_size=1,
    hotwords=hotwords,
    language=language,
    itn=itn,
)

text = res[0]["text"]
print(text)
"#;

pub struct PythonFunAsrRunner {
    python_bin: String,
}

impl Default for PythonFunAsrRunner {
    fn default() -> Self {
        if let Ok(python_bin) = std::env::var("PYTHON_BIN") {
            return Self { python_bin };
        }

        if std::env::var("VOICEINPUT_USE_UV").map(|v| v != "0").unwrap_or(true) {
            if which::which("uv").is_ok() {
                return Self {
                    python_bin: "uv".to_string(),
                };
            }
        }

        let uv_python = Path::new(".venv/bin/python");
        if uv_python.exists() {
            return Self {
                python_bin: uv_python.to_string_lossy().to_string(),
            };
        }

        Self {
            python_bin: "python3".to_string(),
        }
    }
}

impl PythonFunAsrRunner {
    pub fn new(python_bin: impl Into<String>) -> Self {
        Self {
            python_bin: python_bin.into(),
        }
    }
}

impl FunAsrRunner for PythonFunAsrRunner {
    fn transcribe(&self, request: FunAsrRequest) -> Result<String> {
        let mut audio_file = NamedTempFile::new()
            .map_err(|e| VoiceInputError::Transcription(format!("创建临时音频文件失败：{e}")))?;
        audio_file
            .write_all(&request.audio_bytes)
            .map_err(|e| VoiceInputError::Transcription(format!("写入临时音频文件失败：{e}")))?;

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
                .map_err(|e| VoiceInputError::Transcription(format!("通过 uv 启动 FunASR Python 进程失败：{e}")))?
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
                .map_err(|e| VoiceInputError::Transcription(format!("启动 FunASR Python 进程失败：{e}")))?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VoiceInputError::Transcription(format!(
                "FunASR 进程失败：{stderr}"
            )));
        }

        let text = String::from_utf8(output.stdout)
            .map_err(|e| VoiceInputError::Transcription(format!("FunASR 输出不是有效的 UTF-8：{e}")))?;
        Ok(text.trim().to_string())
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
