use crate::config::FunAsrConfig;
use voice_input_core::{Result, Transcript, VoiceInputError};

#[derive(Debug, Clone)]
pub struct FunAsrRequest {
    pub audio_bytes: Vec<u8>,
    pub config: FunAsrConfig,
}

pub trait FunAsrRunner {
    fn transcribe(&self, request: FunAsrRequest) -> Result<String>;
}

pub(crate) fn invalid_runner_error(message: impl Into<String>) -> VoiceInputError {
    VoiceInputError::Transcription(message.into())
}

pub(crate) fn to_transcript(text: String) -> Transcript {
    Transcript::new(text)
}

