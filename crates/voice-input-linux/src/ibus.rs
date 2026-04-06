#[cfg(feature = "ibus")]
use std::process::Command;
use std::sync::{Arc, Mutex};
#[cfg(feature = "ibus")]
use std::thread;
#[cfg(feature = "ibus")]
use std::time::Duration;

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
    spec: IbusEngineSpec,
    bus: ibus::Bus,
    context: std::cell::RefCell<Option<ibus::InputContext>>,
    tokens: std::cell::RefCell<Vec<ibus::dbus::channel::Token>>,
    events: Arc<Mutex<Vec<IbusEngineEvent>>>,
}

#[cfg(feature = "ibus")]
impl IbusClientBridge {
    pub fn try_new(spec: IbusEngineSpec) -> Result<Self> {
        let bus = ibus::Bus::new()
            .map_err(|e| VoiceInputError::Injection(format!("连接 IBus 总线失败：{e}")))?;

        Ok(Self {
            spec,
            bus,
            context: std::cell::RefCell::new(None),
            tokens: std::cell::RefCell::new(Vec::new()),
            events: Arc::new(Mutex::new(Vec::new())),
        })
    }

    fn ensure_context(&self) -> Result<()> {
        if self.context.borrow().is_some() {
            return Ok(());
        }

        let context = self
            .bus
            .create_input_context(&self.spec.engine_name)
            .map_err(|e| VoiceInputError::Injection(format!("创建 IBus 输入上下文失败：{e}")))?;

        context.set_capabilities(
            ibus::Capabilites::PREEDIT_TEXT
                | ibus::Capabilites::FOCUS
                | ibus::Capabilites::SURROUNDING_TEXT,
        );

        let events = Arc::clone(&self.events);
        let update_token = context
            .on_update_preedit_text(move |signal, _, _| {
                if let Ok(mut lock) = events.lock() {
                    lock.push(IbusEngineEvent::UpdatePreedit(signal.text.into_string()));
                }
                ibus::AfterCallback::Keep
            })
            .map_err(|e| VoiceInputError::Injection(format!("订阅 IBus 预编辑更新失败：{e}")))?;

        let events = Arc::clone(&self.events);
        let commit_token = context
            .on_commit_text(move |signal, _, _| {
                if let Ok(mut lock) = events.lock() {
                    lock.push(IbusEngineEvent::CommitText(signal.text.into_string()));
                }
                ibus::AfterCallback::Keep
            })
            .map_err(|e| VoiceInputError::Injection(format!("订阅 IBus 提交事件失败：{e}")))?;

        let events = Arc::clone(&self.events);
        let show_token = context
            .on_show_preedit_text(move |_, _| {
                if let Ok(mut lock) = events.lock() {
                    lock.push(IbusEngineEvent::StartComposition);
                }
                ibus::AfterCallback::Keep
            })
            .map_err(|e| {
                VoiceInputError::Injection(format!("订阅 IBus 显示预编辑事件失败：{e}"))
            })?;

        let events = Arc::clone(&self.events);
        let hide_token = context
            .on_hide_preedit_text(move |_, _| {
                if let Ok(mut lock) = events.lock() {
                    lock.push(IbusEngineEvent::EndComposition);
                }
                ibus::AfterCallback::Keep
            })
            .map_err(|e| {
                VoiceInputError::Injection(format!("订阅 IBus 隐藏预编辑事件失败：{e}"))
            })?;

        self.tokens
            .borrow_mut()
            .extend([update_token, commit_token, show_token, hide_token]);
        *self.context.borrow_mut() = Some(context);
        Ok(())
    }
}

#[cfg(feature = "ibus")]
impl IbusEngineBridge for IbusClientBridge {
    fn start_composition(&self) -> Result<()> {
        self.ensure_context()?;
        let context_binding = self.context.borrow();
        let context = context_binding.as_ref().expect("IBus 上下文已初始化");
        context
            .focus_in()
            .map_err(|e| VoiceInputError::Injection(format!("IBus focus_in 失败：{e}")))?;

        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::StartComposition);
        }

        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.ensure_context()?;
        let context_binding = self.context.borrow();
        let context = context_binding.as_ref().expect("IBus 上下文已初始化");
        context
            .set_surrounding_text(text.to_string(), text.len() as u32, text.len() as u32)
            .map_err(|e| {
                VoiceInputError::Injection(format!("IBus set_surrounding_text 失败：{e}"))
            })?;

        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::UpdatePreedit(text.to_string()));
        }

        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.ensure_context()?;
        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::CommitText(text.to_string()));
        }

        let context_binding = self.context.borrow();
        let context = context_binding.as_ref().expect("IBus 上下文已初始化");
        context
            .reset()
            .map_err(|e| VoiceInputError::Injection(format!("提交后 IBus reset 失败：{e}")))?;

        if let Err(err) = insert_text_into_active_window(text) {
            return Err(VoiceInputError::Injection(format!(
                "Linux 文本提交失败：{err}"
            )));
        }

        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        self.ensure_context()?;
        if let Ok(mut lock) = self.events.lock() {
            lock.push(IbusEngineEvent::CancelComposition);
        }
        let context_binding = self.context.borrow();
        let context = context_binding.as_ref().expect("IBus 上下文已初始化");
        context
            .reset()
            .map_err(|e| VoiceInputError::Injection(format!("取消后 IBus reset 失败：{e}")))?;

        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        self.ensure_context()?;
        let context_binding = self.context.borrow();
        let context = context_binding.as_ref().expect("IBus 上下文已初始化");
        context
            .focus_out()
            .map_err(|e| VoiceInputError::Injection(format!("IBus focus_out 失败：{e}")))?;

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
pub fn insert_text_into_active_window(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))?;

    thread::sleep(Duration::from_millis(40));

    for shortcut in [
        ["key", "--clearmodifiers", "Shift+Insert"],
        ["key", "--clearmodifiers", "ctrl+v"],
    ] {
        let status = Command::new("xdotool")
            .args(shortcut)
            .status()
            .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}")))?;

        if status.success() {
            return Ok(());
        }
    }

    Err(VoiceInputError::Injection(
        "xdotool 粘贴失败：Shift+Insert 和 ctrl+v 都未成功".to_string(),
    ))
}

#[cfg(feature = "ibus")]
pub fn backspace_in_active_window(count: usize) -> Result<()> {
    for _ in 0..count {
        let status = Command::new("xdotool")
            .args(["key", "--clearmodifiers", "BackSpace"])
            .status()
            .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}")))?;

        if !status.success() {
            return Err(VoiceInputError::Injection(format!(
                "xdotool 退格失败，退出码：{status}"
            )));
        }
    }

    Ok(())
}

#[cfg(not(feature = "ibus"))]
pub fn backspace_in_active_window(_count: usize) -> Result<()> {
    Ok(())
}
