use std::sync::{Arc, Mutex};

use voice_input_core::{Result, VoiceInputError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacImeEvent {
    StartComposition,
    UpdatePreedit(String),
    CommitText(String),
    CancelComposition,
    EndComposition,
}

impl std::fmt::Display for MacImeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StartComposition => write!(f, "开始输入"),
            Self::UpdatePreedit(text) => write!(f, "更新预编辑：{text}"),
            Self::CommitText(text) => write!(f, "提交文本：{text}"),
            Self::CancelComposition => write!(f, "取消输入"),
            Self::EndComposition => write!(f, "结束输入"),
        }
    }
}

pub trait MacImeBridge {
    fn start_composition(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

pub struct UnwiredMacImeBridge;

impl MacImeBridge for UnwiredMacImeBridge {
    fn start_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS InputMethodKit 桥接尚未接入".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS InputMethodKit 桥接尚未接入".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS InputMethodKit 桥接尚未接入".to_string(),
        ))
    }

    fn cancel_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS InputMethodKit 桥接尚未接入".to_string(),
        ))
    }

    fn end_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS InputMethodKit 桥接尚未接入".to_string(),
        ))
    }
}

#[cfg(target_os = "macos")]
pub struct ClipboardMacImeBridge;

#[cfg(target_os = "macos")]
impl Default for ClipboardMacImeBridge {
    fn default() -> Self {
        Self
    }
}

#[cfg(target_os = "macos")]
impl MacImeBridge for ClipboardMacImeBridge {
    fn start_composition(&self) -> Result<()> {
        Ok(())
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
        use cocoa::base::nil;
        use cocoa::foundation::NSString;
        use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
        use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

        unsafe {
            let pasteboard = NSPasteboard::generalPasteboard(nil);
            let _: i64 = pasteboard.clearContents();
            let string = NSString::alloc(nil).init_str(text);
            let _: bool = pasteboard.setString_forType(string, NSPasteboardTypeString);

            let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
                .map_err(|_| VoiceInputError::Injection("创建键盘事件源失败".to_string()))?;

            let key_down = CGEvent::new_keyboard_event(source.clone(), 0x09, true)
                .map_err(|_| VoiceInputError::Injection("创建粘贴快捷键失败".to_string()))?;
            key_down.set_flags(CGEventFlags::CGEventFlagCommand);
            key_down.post(CGEventTapLocation::HID);

            let key_up = CGEvent::new_keyboard_event(source, 0x09, false)
                .map_err(|_| VoiceInputError::Injection("创建粘贴快捷键失败".to_string()))?;
            key_up.set_flags(CGEventFlags::CGEventFlagCommand);
            key_up.post(CGEventTapLocation::HID);
        }

        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
pub struct ClipboardMacImeBridge;

#[cfg(not(target_os = "macos"))]
impl Default for ClipboardMacImeBridge {
    fn default() -> Self {
        Self
    }
}

#[cfg(not(target_os = "macos"))]
impl MacImeBridge for ClipboardMacImeBridge {
    fn start_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 macOS".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 macOS".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 macOS".to_string(),
        ))
    }

    fn cancel_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 macOS".to_string(),
        ))
    }

    fn end_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 macOS".to_string(),
        ))
    }
}

#[derive(Clone, Default)]
pub struct MockMacImeBridge {
    events: Arc<Mutex<Vec<MacImeEvent>>>,
}

impl MockMacImeBridge {
    pub fn events(&self) -> Vec<MacImeEvent> {
        self.events.lock().expect("macOS 桥接锁").clone()
    }

    fn push(&self, event: MacImeEvent) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| VoiceInputError::Injection("记录 macOS IME 事件失败".to_string()))?
            .push(event);
        Ok(())
    }
}

impl MacImeBridge for MockMacImeBridge {
    fn start_composition(&self) -> Result<()> {
        self.push(MacImeEvent::StartComposition)
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.push(MacImeEvent::UpdatePreedit(text.to_string()))
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.push(MacImeEvent::CommitText(text.to_string()))
    }

    fn cancel_composition(&self) -> Result<()> {
        self.push(MacImeEvent::CancelComposition)
    }

    fn end_composition(&self) -> Result<()> {
        self.push(MacImeEvent::EndComposition)
    }
}
