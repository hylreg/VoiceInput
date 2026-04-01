use std::fs;
use std::path::PathBuf;

use voice_input_core::{AudioRecorder, Result, VoiceInputError};

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

impl AudioRecorder for FileAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        fs::read(&self.path).map_err(|e| {
            VoiceInputError::Audio(format!(
                "读取音频文件失败 {}：{e}",
                self.path.display()
            ))
        })
    }
}
