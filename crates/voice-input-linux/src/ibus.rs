#[cfg(feature = "ibus")]
use std::process::Command;
use std::sync::{Arc, Mutex};
#[cfg(feature = "ibus")]
use std::thread;
#[cfg(feature = "ibus")]
use std::time::{Duration, Instant};

use crate::backend::LinuxBackendKind;
use voice_input_core::{Result, VoiceInputError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IbusEngineEvent {
    StartComposition,
    UpdatePreedit(String),
    CommitText(String),
    CancelComposition,
    EndComposition,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IbusEngineSpec {
    pub engine_name: String,
    pub object_path: String,
    pub service_name: String,
}

impl Default for IbusEngineSpec {
    fn default() -> Self {
        Self {
            engine_name: "voice-input".to_string(),
            object_path: "/com/example/VoiceInput/Engine".to_string(),
            service_name: "voice-input".to_string(),
        }
    }
}

pub trait IbusEngineBridge {
    fn start_composition(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

#[cfg(feature = "ibus")]
pub struct IbusClientBridge {
    #[allow(dead_code)]
    spec: IbusEngineSpec,
    events: Arc<Mutex<Vec<IbusEngineEvent>>>,
}

#[cfg(feature = "ibus")]
impl IbusClientBridge {
    pub fn try_new(spec: IbusEngineSpec) -> Result<Self> {
        // 不连接 IBus 总线、不创建输入上下文。
        // VoiceInput 不是注册的 IBus 引擎，所有 IBus D-Bus
        // 交互都会干扰目标应用的输入法光标状态。
        Ok(Self {
            spec,
            events: Arc::new(Mutex::new(Vec::new())),
        })
    }
}

#[cfg(feature = "ibus")]
impl IbusEngineBridge for IbusClientBridge {
    fn start_composition(&self) -> Result<()> {
        // 不创建 IBus 上下文、不调用任何 IBus D-Bus 方法。
        // VoiceInput 不是注册的 IBus 引擎，任何 D-Bus 交互都会
        // 干扰目标应用的输入法状态和光标显示。

        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            eprintln!("[VOICEINPUT_DEBUG {now}] IBus start_composition (no-op)");
        }

        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::StartComposition);
        }

        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            eprintln!("[VOICEINPUT_DEBUG {now}] IBus update_preedit len={} (no-op)", text.len());
        }

        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::UpdatePreedit(text.to_string()));
        }

        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::CommitText(text.to_string()));
        }

        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            let preview: String = text.chars().take(20).collect();
            eprintln!("[VOICEINPUT_DEBUG {now}] IBus commit_text → insert_text \"{preview}\" (no-op)");
        }

        if let Err(err) = insert_text_into_active_window(text, None) {
            return Err(VoiceInputError::Injection(format!(
                "Linux 文本提交失败：{err}"
            )));
        }

        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            eprintln!("[VOICEINPUT_DEBUG {now}] IBus cancel_composition (no-op)");
        }

        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::CancelComposition);
        }

        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            eprintln!("[VOICEINPUT_DEBUG {now}] IBus end_composition (no-op)");
        }

        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::EndComposition);
        }

        Ok(())
    }
}

#[cfg(not(feature = "ibus"))]
pub struct IbusBackend {
    spec: IbusEngineSpec,
    bridge: Box<dyn IbusEngineBridge>,
}

#[cfg(feature = "ibus")]
pub struct IbusBackend {
    spec: IbusEngineSpec,
    bridge: Box<dyn IbusEngineBridge>,
}

impl IbusBackend {
    pub fn new(spec: IbusEngineSpec) -> Self {
        Self::new_real(spec)
    }

    pub fn new_with_bridge(spec: IbusEngineSpec, bridge: Box<dyn IbusEngineBridge>) -> Self {
        Self { spec, bridge }
    }

    pub fn spec(&self) -> &IbusEngineSpec {
        &self.spec
    }
}

#[cfg(feature = "ibus")]
impl IbusBackend {
    pub fn new_real(spec: IbusEngineSpec) -> Self {
        let bridge: Box<dyn IbusEngineBridge> = match IbusClientBridge::try_new(spec.clone()) {
            Ok(client) => Box::new(client),
            Err(_) => Box::new(UnwiredIbusBridge),
        };

        Self { spec, bridge }
    }
}

#[cfg(not(feature = "ibus"))]
impl IbusBackend {
    pub fn new_real(spec: IbusEngineSpec) -> Self {
        Self {
            spec,
            bridge: Box::new(UnwiredIbusBridge),
        }
    }
}

pub struct UnwiredIbusBridge;

impl IbusEngineBridge for UnwiredIbusBridge {
    fn start_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "IBus 桥接尚未接入原生绑定".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "IBus 桥接尚未接入原生绑定".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "IBus 桥接尚未接入原生绑定".to_string(),
        ))
    }

    fn cancel_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "IBus 桥接尚未接入原生绑定".to_string(),
        ))
    }

    fn end_composition(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "IBus 桥接尚未接入原生绑定".to_string(),
        ))
    }
}

#[derive(Clone, Default)]
pub struct MockIbusBridge {
    events: Arc<Mutex<Vec<IbusEngineEvent>>>,
}

impl MockIbusBridge {
    pub fn events(&self) -> Vec<IbusEngineEvent> {
        self.events.lock().expect("模拟 IBus 桥接锁").clone()
    }

    fn push(&self, event: IbusEngineEvent) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| VoiceInputError::Injection("记录 IBus 事件失败".to_string()))?
            .push(event);
        Ok(())
    }
}

impl IbusEngineBridge for MockIbusBridge {
    fn start_composition(&self) -> Result<()> {
        self.push(IbusEngineEvent::StartComposition)
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.push(IbusEngineEvent::UpdatePreedit(text.to_string()))
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.push(IbusEngineEvent::CommitText(text.to_string()))
    }

    fn cancel_composition(&self) -> Result<()> {
        self.push(IbusEngineEvent::CancelComposition)
    }

    fn end_composition(&self) -> Result<()> {
        self.push(IbusEngineEvent::EndComposition)
    }
}

impl crate::backend::LinuxBackend for IbusBackend {
    fn kind(&self) -> LinuxBackendKind {
        LinuxBackendKind::IBus
    }

    fn start(&self) -> Result<()> {
        self.bridge.start_composition()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.bridge.update_preedit(text)
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.bridge.commit_text(text)
    }

    fn cancel(&self) -> Result<()> {
        self.bridge.cancel_composition()
    }

    fn stop(&self) -> Result<()> {
        self.bridge.end_composition()
    }
}

#[cfg(feature = "ibus")]
macro_rules! debug_xdotool {
    ($label:expr, $cmd:expr, $args:expr) => {{
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = crate::ibus::debug_timestamp();
            eprintln!(
                "[VOICEINPUT_DEBUG {now}] {} → args={:?}",
                $label,
                $args,
            );
        }
        let result = $cmd;
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = crate::ibus::debug_timestamp();
            match &result {
                Ok(status) => eprintln!("[VOICEINPUT_DEBUG {now}] {} ← exit={}", $label, status.success()),
                Err(e) => eprintln!("[VOICEINPUT_DEBUG {now}] {} ← err={}", $label, e),
            }
        }
        result
    }};
}

#[cfg(feature = "ibus")]
pub fn debug_timestamp() -> String {
    let elapsed = DEBUG_START.elapsed();
    format!(
        "{}.{:03}s",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    )
}

#[cfg(feature = "ibus")]
static DEBUG_START: std::sync::LazyLock<Instant> =
    std::sync::LazyLock::new(Instant::now);

/// 在光标处插入录音指示符 ●，同时保存当前剪贴板。
/// 返回的 guard 持有 Clipboard 对象，确保还原后剪贴板内容
/// 存活到整个录音周期结束，不被剪贴板管理器丢弃。
#[cfg(feature = "ibus")]
pub fn insert_indicator_and_save_clipboard() -> Result<ClipboardRestoreGuard> {
    let saved = arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok());

    insert_text_into_active_window("●", None)?;

    // 立即还原用户剪贴板，避免 ● 残留在剪贴板中
    let clipboard = if let Some(ref text) = saved {
        let mut c = arboard::Clipboard::new()
            .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
        c.set_text(text.clone())
            .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))?;
        Some(c)
    } else {
        None
    };

    Ok(ClipboardRestoreGuard {
        _clipboard: clipboard,
    })
}

/// 持有剪贴板还原逻辑的守卫。
/// `_clipboard` 字段在整个录音周期内保持 Clipboard 连接存活，
/// 确保剪贴板管理器在足够长的时间内能同步内容。
pub struct ClipboardRestoreGuard {
    #[allow(dead_code)]
    _clipboard: Option<arboard::Clipboard>,
}

#[cfg(feature = "ibus")]
pub fn insert_text_into_active_window(text: &str, window_id: Option<&str>) -> Result<()> {
    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        let preview: String = text.chars().take(20).collect();
        eprintln!("[VOICEINPUT_DEBUG {now}] insert_text len={} is_ascii={} preview=\"{preview}\" win={window_id:?}", text.len(), text.chars().all(|c| c.is_ascii()));
    }

    // xdotool type 无法正确处理中文等多字节字符，对纯 ASCII 文本才
    // 优先使用打字方式（避免覆盖剪贴板），对非 ASCII 文本直接走剪贴板粘贴。
    let is_ascii = text.chars().all(|c| c.is_ascii());
    if is_ascii && type_text_in_active_window(text, window_id).is_ok() {
        return Ok(());
    }

    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))?;

    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] insert_text clipboard set, sleeping 40ms");
    }

    thread::sleep(Duration::from_millis(40));

    for shortcut in [
        ["key", "--clearmodifiers", "Shift+Insert"],
        ["key", "--clearmodifiers", "ctrl+v"],
    ] {
        let status = debug_xdotool!(
            "insert_text paste",
            Command::new("xdotool")
                .args(shortcut)
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            shortcut
        )?;

        if status.success() {
            return Ok(());
        }
    }

    // 粘贴失败时回退到先聚焦窗口再试
    if let Some(id) = window_id {
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            eprintln!("[VOICEINPUT_DEBUG {now}] insert_text paste failed, trying windowfocus {id}");
        }
        let _ = debug_xdotool!(
            "insert_text windowfocus (retry)",
            Command::new("xdotool")
                .args(["windowfocus", "--sync", id])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["windowfocus", "--sync", id]
        );
        thread::sleep(Duration::from_millis(40));

        for shortcut in [
            ["key", "--clearmodifiers", "Shift+Insert"],
            ["key", "--clearmodifiers", "ctrl+v"],
        ] {
            let status = debug_xdotool!(
                "insert_text paste (retry)",
                Command::new("xdotool")
                    .args(shortcut)
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                shortcut
            )?;

            if status.success() {
                return Ok(());
            }
        }
    }

    Err(VoiceInputError::Injection(
        "xdotool 粘贴失败：Shift+Insert 和 ctrl+v 都未成功".to_string(),
    ))
}

#[cfg(feature = "ibus")]
pub fn type_text_in_active_window(text: &str, window_id: Option<&str>) -> Result<()> {
    // 调用方已确保窗口是活动窗口，
    // 不要在此处调用 focus_window——xdotool windowfocus --sync 会抢占焦点，
    // 导致目标应用光标闪烁或消失。

    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] xdotool type text_len={} win={window_id:?}", text.len());
    }

    let status = debug_xdotool!(
        "xdotool type",
        Command::new("xdotool")
            .args(["type", "--clearmodifiers", "--delay", "0", text])
            .status()
            .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
        &["type", "--clearmodifiers", "--delay", "0", text]
    )?;

    if !status.success() {
        // 如果直接打字失败（例如 Wayland），回退到用 windowfocus 聚焦后再试
        if let Some(id) = window_id {
            let _ = debug_xdotool!(
                "xdotool type windowfocus (retry)",
                Command::new("xdotool")
                    .args(["windowfocus", "--sync", id])
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                &["windowfocus", "--sync", id]
            );
            thread::sleep(Duration::from_millis(40));
        }

        let status = debug_xdotool!(
            "xdotool type (retry)",
            Command::new("xdotool")
                .args(["type", "--clearmodifiers", "--delay", "0", text])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["type", "--clearmodifiers", "--delay", "0", text]
        )?;

        if !status.success() {
            return Err(VoiceInputError::Injection(format!(
                "xdotool 输入失败，退出码：{status}"
            )));
        }
    }

    Ok(())
}

#[cfg(feature = "ibus")]
#[allow(dead_code)]
pub fn backspace_in_active_window(count: usize, window_id: Option<&str>) -> Result<()> {
    // 不预先调用 focus_window——调用方已确保窗口是活动窗口。
    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] backspace count={count} win={window_id:?}");
    }

    for _i in 0..count {
        let status = debug_xdotool!(
            "backspace",
            Command::new("xdotool")
                .args(["key", "--clearmodifiers", "BackSpace"])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["key", "--clearmodifiers", "BackSpace"]
        )?;

        if !status.success() {
            // 退格失败（如 Wayland），先聚焦再重试一次
            if let Some(id) = window_id {
                let _ = debug_xdotool!(
                    "backspace windowfocus (retry)",
                    Command::new("xdotool")
                        .args(["windowfocus", "--sync", id])
                        .status()
                        .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                    &["windowfocus", "--sync", id]
                );
                thread::sleep(Duration::from_millis(40));
            }

            let status = debug_xdotool!(
                "backspace (retry)",
                Command::new("xdotool")
                    .args(["key", "--clearmodifiers", "BackSpace"])
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                &["key", "--clearmodifiers", "BackSpace"]
            )?;

            if !status.success() {
                return Err(VoiceInputError::Injection(format!(
                    "xdotool 退格失败，退出码：{status}"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(not(feature = "ibus"))]
pub fn insert_indicator_and_save_clipboard() -> Result<ClipboardRestoreGuard> {
    Ok(ClipboardRestoreGuard { _clipboard: None })
}

#[cfg(not(feature = "ibus"))]
#[allow(dead_code)]
pub fn type_text_in_active_window(_text: &str, _window_id: Option<&str>) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "ibus"))]
#[allow(dead_code)]
pub fn backspace_in_active_window(_count: usize, _window_id: Option<&str>) -> Result<()> {
    Ok(())
}
