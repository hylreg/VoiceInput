use std::sync::{Arc, Mutex};

use crate::ibus::{IbusBackend, IbusEngineSpec};
use voice_input_core::{Result, VoiceInputError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBackendKind {
    IBus,
    Fcitx5,
}

pub trait LinuxBackend {
    fn kind(&self) -> LinuxBackendKind;
    fn start(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel(&self) -> Result<()>;
    fn stop(&self) -> Result<()>;
}

pub struct Fcitx5Backend;
#[derive(Clone)]
pub struct MockLinuxBackend {
    events: Arc<Mutex<Vec<String>>>,
    kind: LinuxBackendKind,
}

impl MockLinuxBackend {
    pub fn new(kind: LinuxBackendKind) -> Self {
        Self {
            events: Arc::default(),
            kind,
        }
    }

    pub fn events(&self) -> Vec<String> {
        self.events.lock().expect("模拟 Linux 后端锁").clone()
    }

    fn push(&self, event: impl Into<String>) -> Result<()> {
        self.events
            .lock()
            .map_err(|_| VoiceInputError::Injection("记录 Linux 后端事件失败".to_string()))?
            .push(event.into());
        Ok(())
    }
}

impl Default for MockLinuxBackend {
    fn default() -> Self {
        Self::new(LinuxBackendKind::Fcitx5)
    }
}

impl LinuxBackend for Fcitx5Backend {
    fn kind(&self) -> LinuxBackendKind {
        LinuxBackendKind::Fcitx5
    }

    fn start(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Fcitx5 后端尚未接入原生绑定".to_string(),
        ))
    }

    fn update_preedit(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Fcitx5 后端尚未接入原生绑定".to_string(),
        ))
    }

    fn commit_text(&self, _text: &str) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Fcitx5 后端尚未接入原生绑定".to_string(),
        ))
    }

    fn cancel(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Fcitx5 后端尚未接入原生绑定".to_string(),
        ))
    }

    fn stop(&self) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Fcitx5 后端尚未接入原生绑定".to_string(),
        ))
    }
}

impl LinuxBackend for MockLinuxBackend {
    fn kind(&self) -> LinuxBackendKind {
        self.kind
    }

    fn start(&self) -> Result<()> {
        self.push("开始输入")
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.push(format!("更新预编辑：{text}"))
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.push(format!("提交文本：{text}"))
    }

    fn cancel(&self) -> Result<()> {
        self.push("取消输入")
    }

    fn stop(&self) -> Result<()> {
        self.push("结束输入")
    }
}

pub fn backend_from_kind(kind: LinuxBackendKind) -> Box<dyn LinuxBackend> {
    match kind {
        LinuxBackendKind::IBus => Box::new(IbusBackend::new(IbusEngineSpec::default())),
        LinuxBackendKind::Fcitx5 => Box::new(Fcitx5Backend),
    }
}
