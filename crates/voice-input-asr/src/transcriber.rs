use crate::config::FunAsrConfig;
use crate::runner::{FunAsrRequest, FunAsrRunner};
use voice_input_core::{Result, Transcript, Transcriber};

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
}

impl Transcriber for LocalFunAsrTranscriber {
    fn transcribe(&self, audio: &[u8]) -> Result<Transcript> {
        let text = self.runner.transcribe(FunAsrRequest {
            audio_bytes: audio.to_vec(),
            config: self.config.clone(),
        })?;

        Ok(Transcript::new(text))
    }
}

