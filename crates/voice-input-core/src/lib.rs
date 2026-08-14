mod config;
mod error;
mod platform;

pub use config::AppConfig;
pub use error::{Result, VoiceInputError};
pub use platform::{AudioRecorder, InputMethodHost, MockTranscriber, Transcriber};
