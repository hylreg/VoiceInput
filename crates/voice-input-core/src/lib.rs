mod config;
mod controller;
mod error;
mod ime;
mod platform;

pub use config::{AppConfig, InsertionMode, TranscriptionMode};
pub use controller::AppController;
pub use error::{Result, VoiceInputError};
pub use ime::{CompositionState, DictationEvent, Transcript};
pub use platform::{
    AudioRecorder, HotkeyManager, InputMethodHost, MockAudioRecorder, MockHotkeyManager,
    MockInputMethodHost, MockTextInjector, MockTranscriber, TextInjector, Transcriber,
};
