use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub enum VoiceInputError {
    Hotkey(String),
    Audio(String),
    Transcription(String),
    Injection(String),
}

impl Display for VoiceInputError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Hotkey(msg) => write!(f, "热键错误：{msg}"),
            Self::Audio(msg) => write!(f, "音频错误：{msg}"),
            Self::Transcription(msg) => write!(f, "转写错误：{msg}"),
            Self::Injection(msg) => write!(f, "注入错误：{msg}"),
        }
    }
}

impl Error for VoiceInputError {}

pub type Result<T> = std::result::Result<T, VoiceInputError>;
