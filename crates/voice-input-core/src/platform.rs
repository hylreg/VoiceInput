use crate::audio::RecordedAudio;
use crate::error::Result;

pub trait AudioRecorder {
    fn record_once(&self) -> Result<RecordedAudio>;
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
