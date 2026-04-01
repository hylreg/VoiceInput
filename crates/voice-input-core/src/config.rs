#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranscriptionMode {
    Local,
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InsertionMode {
    ClipboardPaste,
    Accessibility,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub activation_hotkey: String,
    pub transcription_mode: TranscriptionMode,
    pub insertion_mode: InsertionMode,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            activation_hotkey: "Ctrl+Shift+Space".to_string(),
            transcription_mode: TranscriptionMode::Local,
            insertion_mode: InsertionMode::ClipboardPaste,
        }
    }
}
