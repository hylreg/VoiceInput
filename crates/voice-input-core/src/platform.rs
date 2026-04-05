use crate::error::{Result, VoiceInputError};
use crate::ime::Transcript;
use std::sync::{Arc, Mutex};

pub trait HotkeyManager {
    fn register_global_hotkey(&self, hotkey: &str) -> Result<()>;
}

pub trait AudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>>;
}

pub trait Transcriber {
    fn transcribe(&self, audio: &[u8]) -> Result<Transcript>;
}

pub trait TextInjector {
    fn inject(&self, text: &str) -> Result<()>;
}

pub trait InputMethodHost {
    fn start_composition(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn show_recording_indicator(&self) -> Result<()> {
        Ok(())
    }
    fn clear_recording_indicator(&self) -> Result<()> {
        Ok(())
    }
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

pub struct MockHotkeyManager;
pub struct MockAudioRecorder;
pub struct MockTranscriber;
pub struct MockTextInjector;
#[derive(Clone, Default)]
pub struct MockInputMethodHost {
    events: Arc<Mutex<Vec<String>>>,
}

impl MockInputMethodHost {
    pub fn events(&self) -> Vec<String> {
        self.events.lock().expect("模拟宿主锁").clone()
    }

    fn push(&self, value: impl Into<String>) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| VoiceInputError::Injection("记录输入法事件失败".to_string()))?
            .push(value.into());
        Ok(())
    }
}

impl HotkeyManager for MockHotkeyManager {
    fn register_global_hotkey(&self, hotkey: &str) -> Result<()> {
        println!("已注册全局热键：{hotkey}");
        Ok(())
    }
}

impl AudioRecorder for MockAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        Ok("模拟音频数据".as_bytes().to_vec())
    }
}

impl Transcriber for MockTranscriber {
    fn transcribe(&self, audio: &[u8]) -> Result<Transcript> {
        if audio.is_empty() {
            return Err(VoiceInputError::Transcription("音频数据为空".to_string()));
        }

        Ok(Transcript {
            partials: vec![
                "你好".to_string(),
                "来自语音".to_string(),
                "来自语音输入".to_string(),
            ],
            final_text: "来自语音输入".to_string(),
        })
    }
}

impl TextInjector for MockTextInjector {
    fn inject(&self, text: &str) -> Result<()> {
        println!("已注入文本：{text}");
        Ok(())
    }
}

impl InputMethodHost for MockInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.push("开始输入")
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.push(format!("更新预编辑：{text}"))
    }

    fn show_recording_indicator(&self) -> Result<()> {
        self.push("显示录音标记")
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        self.push("清除录音标记")
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.push(format!("提交文本：{text}"))
    }

    fn cancel_composition(&self) -> Result<()> {
        self.push("取消输入")
    }

    fn end_composition(&self) -> Result<()> {
        self.push("结束输入")
    }
}
