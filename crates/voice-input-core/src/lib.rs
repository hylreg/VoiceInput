mod config;
mod controller;
mod error;
mod ime;
mod platform;

pub use config::AppConfig;
pub use controller::AppController;
pub use error::{Result, VoiceInputError};
pub use ime::Transcript;
pub use platform::{
    AudioRecorder, HotkeyManager, InputMethodHost, MockAudioRecorder, MockHotkeyManager,
    MockInputMethodHost, MockTranscriber, Transcriber,
};
