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

    pub fn config(&self) -> &FunAsrConfig {
        &self.config
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
                "FunASR 没有返回识别文本，请检查麦克风输入、录音时长或环境噪声".to_string(),
            ));
        }

        Ok(Transcript::new(text))
    }
}
