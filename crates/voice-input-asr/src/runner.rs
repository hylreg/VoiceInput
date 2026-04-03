use crate::config::FunAsrConfig;
use voice_input_core::Result;

#[derive(Debug, Clone)]
pub struct FunAsrRequest {
    pub audio_bytes: Vec<u8>,
    pub config: FunAsrConfig,
}

pub trait FunAsrRunner {
    fn transcribe(&self, request: FunAsrRequest) -> Result<String>;
}

pub trait FunAsrStreamingRunner {
    fn stream_chunk(&self, samples: &[i16], sample_rate: u32, is_final: bool) -> Result<String>;
}
