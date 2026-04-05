use std::sync::{Arc, Mutex};

#[cfg(target_os = "macos")]
use core_foundation::base::{CFGetTypeID, CFRange, CFRelease, CFTypeRef, TCFType};
#[cfg(target_os = "macos")]
use core_foundation::string::{CFString, CFStringRef};
use voice_input_core::{Result, VoiceInputError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacImeEvent {
    StartComposition,
    UpdatePreedit(String),
    ShowRecordingIndicator,
    ClearRecordingIndicator,
    CommitText(String),
    CancelComposition,
    EndComposition,
}

impl std::fmt::Display for MacImeEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StartComposition => write!(f, "开始输入"),
            Self::UpdatePreedit(text) => write!(f, "更新预编辑：{text}"),
            Self::ShowRecordingIndicator => write!(f, "显示录音标记"),
            Self::ClearRecordingIndicator => write!(f, "清除录音标记"),
            Self::CommitText(text) => write!(f, "提交文本：{text}"),
            Self::CancelComposition => write!(f, "取消输入"),
            Self::EndComposition => write!(f, "结束输入"),
        }
    }
}

pub trait MacImeBridge {
    fn start_composition(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn show_recording_indicator(&self) -> Result<()>;
    fn clear_recording_indicator(&self) -> Result<()>;
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

pub struct UnwiredMacImeBridge;

impl MacImeBridge for UnwiredMacImeBridge {
    fn start_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS 注入桥接尚未接入".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS 注入桥接尚未接入".to_string(),
        ))
    }

    fn show_recording_indicator(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS 注入桥接尚未接入".to_string(),
        ))
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS 注入桥接尚未接入".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS 注入桥接尚未接入".to_string(),
        ))
    }

    fn cancel_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS 注入桥接尚未接入".to_string(),
        ))
    }

    fn end_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "macOS 注入桥接尚未接入".to_string(),
        ))
    }
}

#[cfg(target_os = "macos")]
pub struct ClipboardMacImeBridge {
    recording_indicator_len: Arc<Mutex<usize>>,
}

#[cfg(target_os = "macos")]
impl Default for ClipboardMacImeBridge {
    fn default() -> Self {
        Self {
            recording_indicator_len: Arc::new(Mutex::new(0)),
        }
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

    fn show_recording_indicator(&self) -> Result<()> {
        let mut guard = self.recording_indicator_len.lock().map_err(|_| {
            VoiceInputError::Injection("锁定 macOS 录音标记状态失败".to_string())
        })?;
        if *guard > 0 {
            return Ok(());
        }

        if accessibility_commit_text(RECORDING_MARKER)? {
            *guard = RECORDING_MARKER.chars().count();
            return Ok(());
        }

        if unicode_key_event_commit_text(RECORDING_MARKER)? {
            *guard = RECORDING_MARKER.chars().count();
            return Ok(());
        }

        paste_text_and_restore_clipboard(RECORDING_MARKER)?;
        *guard = RECORDING_MARKER.chars().count();
        Ok(())
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        let marker_len = {
            let guard = self.recording_indicator_len.lock().map_err(|_| {
                VoiceInputError::Injection("锁定 macOS 录音标记状态失败".to_string())
            })?;
            *guard
        };

        if marker_len == 0 {
            return Ok(());
        }

        send_backspace_events(marker_len)?;

        let mut guard = self.recording_indicator_len.lock().map_err(|_| {
            VoiceInputError::Injection("锁定 macOS 录音标记状态失败".to_string())
        })?;
        *guard = 0;
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        if accessibility_commit_text(text)? {
            return Ok(());
        }

        if unicode_key_event_commit_text(text)? {
            return Ok(());
        }

        paste_text_and_restore_clipboard(text)
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

    fn show_recording_indicator(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "clipboard 提交只支持 macOS".to_string(),
        ))
    }

    fn clear_recording_indicator(&self) -> Result<()> {
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

#[cfg(target_os = "macos")]
pub(crate) fn paste_text_and_restore_clipboard(text: &str) -> Result<()> {
    use cocoa::appkit::{NSPasteboard, NSPasteboardTypeString};
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSString;
    use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
    use std::thread;
    use std::time::Duration;

    unsafe {
        let pasteboard = NSPasteboard::generalPasteboard(nil);
        let previous_text: id = pasteboard.stringForType(NSPasteboardTypeString);
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

        if previous_text != nil {
            thread::sleep(Duration::from_millis(120));
            let _: i64 = pasteboard.clearContents();
            let _: bool = pasteboard.setString_forType(previous_text, NSPasteboardTypeString);
        }
    }

    Ok(())
}

#[cfg(target_os = "macos")]
const RECORDING_MARKER: &str = "●";

#[cfg(target_os = "macos")]
fn send_backspace_events(count: usize) -> Result<()> {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    if count == 0 {
        return Ok(());
    }

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState)
        .map_err(|_| VoiceInputError::Injection("创建键盘事件源失败".to_string()))?;

    for _ in 0..count {
        let key_down = CGEvent::new_keyboard_event(source.clone(), 0x33, true)
            .map_err(|_| VoiceInputError::Injection("创建退格键事件失败".to_string()))?;
        key_down.post(CGEventTapLocation::HID);

        let key_up = CGEvent::new_keyboard_event(source.clone(), 0x33, false)
            .map_err(|_| VoiceInputError::Injection("创建退格键事件失败".to_string()))?;
        key_up.post(CGEventTapLocation::HID);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn accessibility_commit_text(text: &str) -> Result<bool> {
    use std::convert::TryFrom;
    use std::os::raw::c_void;

    type AXError = i32;
    type AXUIElementRef = *const c_void;
    type AXValueRef = *const c_void;
    type AXValueType = u32;
    type Boolean = u8;

    const K_AX_VALUE_TYPE_CF_RANGE: AXValueType = 4;
    const AX_SUCCESS: AXError = 0;

    fn ax_focused_ui_element_attribute() -> CFString {
        CFString::from_static_string("AXFocusedUIElement")
    }

    fn ax_selected_text_range_attribute() -> CFString {
        CFString::from_static_string("AXSelectedTextRange")
    }

    fn ax_value_attribute() -> CFString {
        CFString::from_static_string("AXValue")
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> Boolean;
        fn AXUIElementCreateSystemWide() -> AXUIElementRef;
        fn AXUIElementCopyAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: *mut CFTypeRef,
        ) -> AXError;
        fn AXUIElementSetAttributeValue(
            element: AXUIElementRef,
            attribute: CFStringRef,
            value: CFTypeRef,
        ) -> AXError;
        fn AXUIElementIsAttributeSettable(
            element: AXUIElementRef,
            attribute: CFStringRef,
            settable: *mut Boolean,
        ) -> AXError;
        fn AXValueCreate(the_type: AXValueType, value_ptr: *const c_void) -> AXValueRef;
        fn AXValueGetValue(
            value: AXValueRef,
            the_type: AXValueType,
            value_ptr: *mut c_void,
        ) -> Boolean;
    }

    unsafe fn copy_attribute(element: AXUIElementRef, attribute: CFStringRef) -> Option<CFTypeRef> {
        let mut value: CFTypeRef = std::ptr::null();
        let status = AXUIElementCopyAttributeValue(element, attribute, &mut value);
        if status == AX_SUCCESS && !value.is_null() {
            Some(value)
        } else {
            None
        }
    }

    unsafe fn selected_range(element: AXUIElementRef) -> Option<CFRange> {
        let range_attribute = ax_selected_text_range_attribute();
        let value = copy_attribute(element, range_attribute.as_concrete_TypeRef())?;
        let mut range = CFRange {
            location: 0,
            length: 0,
        };
        let ok = AXValueGetValue(
            value as AXValueRef,
            K_AX_VALUE_TYPE_CF_RANGE,
            &mut range as *mut _ as *mut c_void,
        );
        CFRelease(value);
        if ok != 0 {
            Some(range)
        } else {
            None
        }
    }

    unsafe fn focused_ui_element() -> Option<AXUIElementRef> {
        if AXIsProcessTrusted() == 0 {
            return None;
        }

        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }

        let focused_attribute = ax_focused_ui_element_attribute();
        let focused = copy_attribute(system, focused_attribute.as_concrete_TypeRef())
            .map(|value| value as AXUIElementRef);
        CFRelease(system as CFTypeRef);
        focused
    }

    unsafe fn utf16_to_byte_index(text: &str, utf16_offset: usize) -> Option<usize> {
        let mut consumed = 0usize;
        for (byte_index, ch) in text.char_indices() {
            if consumed == utf16_offset {
                return Some(byte_index);
            }
            consumed += ch.len_utf16();
            if consumed > utf16_offset {
                return None;
            }
        }

        if consumed == utf16_offset {
            Some(text.len())
        } else {
            None
        }
    }

    unsafe fn accessibility_insert_into_focused_element(
        element: AXUIElementRef,
        text: &str,
    ) -> Result<bool> {
        let value_attribute = ax_value_attribute();
        let range_attribute = ax_selected_text_range_attribute();
        let mut settable: Boolean = 0;
        let status = AXUIElementIsAttributeSettable(
            element,
            value_attribute.as_concrete_TypeRef(),
            &mut settable as *mut Boolean,
        );
        if status != AX_SUCCESS || settable == 0 {
            return Ok(false);
        }

        let raw_value = match copy_attribute(element, value_attribute.as_concrete_TypeRef()) {
            Some(value) => value,
            None => return Ok(false),
        };

        if CFGetTypeID(raw_value) != CFString::type_id() {
            CFRelease(raw_value);
            return Ok(false);
        }

        let current_value = CFString::wrap_under_create_rule(raw_value as CFStringRef);
        let current_text = current_value.to_string();
        let current_utf16_len = current_value.char_len() as usize;
        let selection = selected_range(element).unwrap_or(CFRange {
            location: current_utf16_len as isize,
            length: 0,
        });

        let start_utf16 = usize::try_from(selection.location)
            .ok()
            .unwrap_or(current_utf16_len);
        let length_utf16 = usize::try_from(selection.length).ok().unwrap_or(0);
        let start_byte = match utf16_to_byte_index(&current_text, start_utf16) {
            Some(index) => index,
            None => return Ok(false),
        };
        let end_byte =
            match utf16_to_byte_index(&current_text, start_utf16.saturating_add(length_utf16)) {
                Some(index) => index,
                None => return Ok(false),
            };

        let mut composed = String::with_capacity(current_text.len() + text.len());
        composed.push_str(&current_text[..start_byte]);
        composed.push_str(text);
        composed.push_str(&current_text[end_byte..]);

        let new_value = CFString::new(&composed);
        let status = AXUIElementSetAttributeValue(
            element,
            value_attribute.as_concrete_TypeRef(),
            new_value.as_CFTypeRef(),
        );
        if status != AX_SUCCESS {
            return Ok(false);
        }

        let inserted_utf16 = text.encode_utf16().count() as isize;
        let caret = CFRange {
            location: selection.location + inserted_utf16,
            length: 0,
        };
        let caret_value = AXValueCreate(
            K_AX_VALUE_TYPE_CF_RANGE,
            &caret as *const CFRange as *const c_void,
        );
        if !caret_value.is_null() {
            let _ = AXUIElementSetAttributeValue(
                element,
                range_attribute.as_concrete_TypeRef(),
                caret_value as CFTypeRef,
            );
            CFRelease(caret_value as CFTypeRef);
        }

        Ok(true)
    }

    unsafe {
        let element = match focused_ui_element() {
            Some(element) => element,
            None => return Ok(false),
        };

        let result = accessibility_insert_into_focused_element(element, text);
        CFRelease(element as CFTypeRef);
        result
    }
}

#[cfg(target_os = "macos")]
fn unicode_key_event_commit_text(text: &str) -> Result<bool> {
    use core_graphics::event::{CGEvent, CGEventTapLocation};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
        Ok(source) => source,
        Err(_) => return Ok(false),
    };

    let event = match CGEvent::new_keyboard_event(source, 0, true) {
        Ok(event) => event,
        Err(_) => return Ok(false),
    };

    event.set_string(text);
    event.post(CGEventTapLocation::HID);
    Ok(true)
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

    fn show_recording_indicator(&self) -> Result<()> {
        self.push(MacImeEvent::ShowRecordingIndicator)
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        self.push(MacImeEvent::ClearRecordingIndicator)
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
