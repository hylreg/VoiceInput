use std::sync::{Arc, Mutex};

use voice_input_core::{Result, VoiceInputError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowsImeEvent {
    StartComposition,
    UpdatePreedit(String),
    CommitText(String),
    CancelComposition,
    EndComposition,
}

impl std::fmt::Display for WindowsImeEvent {
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

pub trait WindowsImeBridge {
    fn start_composition(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

pub struct UnwiredWindowsImeBridge;

impl WindowsImeBridge for UnwiredWindowsImeBridge {
    fn start_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Windows 注入桥接尚未接入".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Windows 注入桥接尚未接入".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Windows 注入桥接尚未接入".to_string(),
        ))
    }

    fn cancel_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Windows 注入桥接尚未接入".to_string(),
        ))
    }

    fn end_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Windows 注入桥接尚未接入".to_string(),
        ))
    }
}

#[cfg(target_os = "windows")]
pub struct ClipboardWindowsImeBridge;

#[cfg(target_os = "windows")]
impl Default for ClipboardWindowsImeBridge {
    fn default() -> Self {
        Self
    }
}

#[cfg(target_os = "windows")]
impl WindowsImeBridge for ClipboardWindowsImeBridge {
    fn start_composition(&self) -> Result<()> {
        Ok(())
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }

        if send_unicode_text(text).is_ok() {
            return Ok(());
        }

        paste_text_via_clipboard(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
pub struct ClipboardWindowsImeBridge;

#[cfg(not(target_os = "windows"))]
impl Default for ClipboardWindowsImeBridge {
    fn default() -> Self {
        Self
    }
}

#[cfg(not(target_os = "windows"))]
impl WindowsImeBridge for ClipboardWindowsImeBridge {
    fn start_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 Windows".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 Windows".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 Windows".to_string(),
        ))
    }

    fn cancel_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 Windows".to_string(),
        ))
    }

    fn end_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 Windows".to_string(),
        ))
    }
}

#[derive(Clone, Default)]
pub struct MockWindowsImeBridge {
    events: Arc<Mutex<Vec<WindowsImeEvent>>>,
}

impl MockWindowsImeBridge {
    pub fn events(&self) -> Vec<WindowsImeEvent> {
        self.events.lock().expect("模拟 Windows 桥接锁").clone()
    }

    fn push(&self, event: WindowsImeEvent) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| VoiceInputError::Injection("记录 Windows 桥接事件失败".to_string()))?
            .push(event);
        Ok(())
    }
}

impl WindowsImeBridge for MockWindowsImeBridge {
    fn start_composition(&self) -> Result<()> {
        self.push(WindowsImeEvent::StartComposition)
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.push(WindowsImeEvent::UpdatePreedit(text.to_string()))
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.push(WindowsImeEvent::CommitText(text.to_string()))
    }

    fn cancel_composition(&self) -> Result<()> {
        self.push(WindowsImeEvent::CancelComposition)
    }

    fn end_composition(&self) -> Result<()> {
        self.push(WindowsImeEvent::EndComposition)
    }
}

#[cfg(target_os = "windows")]
fn send_unicode_text(text: &str) -> Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    };

    let mut inputs = Vec::with_capacity(text.encode_utf16().count() * 2);
    for unit in text.encode_utf16() {
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
        inputs.push(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: 0,
                    wScan: unit,
                    dwFlags: KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        });
    }

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(VoiceInputError::Injection(format!(
            "Windows Unicode 注入失败：期望发送 {} 个事件，实际发送 {sent} 个",
            inputs.len()
        )));
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn paste_text_via_clipboard(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| VoiceInputError::Injection(format!("打开 Windows 剪贴板失败：{e}")))?;
    let previous_text = clipboard.get_text().ok();
    clipboard
        .set_text(text.to_string())
        .map_err(|e| VoiceInputError::Injection(format!("写入 Windows 剪贴板失败：{e}")))?;

    send_ctrl_v().map_err(|err| {
        if let Some(previous) = previous_text.as_ref() {
            let _ = clipboard.set_text(previous.clone());
        }
        err
    })?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    if let Some(previous) = previous_text {
        clipboard
            .set_text(previous)
            .map_err(|e| VoiceInputError::Injection(format!("恢复 Windows 剪贴板失败：{e}")))?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn send_ctrl_v() -> Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL,
    };

    const VIRTUAL_KEY_V: u16 = b'V' as u16;

    let inputs = [
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY_V,
                    wScan: 0,
                    dwFlags: 0,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY_V,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VK_CONTROL,
                    wScan: 0,
                    dwFlags: KEYEVENTF_KEYUP,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];

    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            size_of::<INPUT>() as i32,
        )
    };
    if sent != inputs.len() as u32 {
        return Err(VoiceInputError::Injection(format!(
            "Windows 剪贴板粘贴失败：期望发送 {} 个事件，实际发送 {sent} 个",
            inputs.len()
        )));
    }

    Ok(())
}
