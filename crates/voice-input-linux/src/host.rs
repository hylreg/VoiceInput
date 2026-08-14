use voice_input_core::{InputMethodHost, Result, VoiceInputError};

/// Linux 文本提交宿主：直接通过 xdotool 注入活动窗口。
/// composition 相关方法为 no-op——VoiceInput 不是注册的 IBus 引擎，
/// 任何 IBus D-Bus 交互都会干扰目标应用的输入法光标状态。
pub struct LinuxInputMethodHost {
    service_name: String,
}

impl LinuxInputMethodHost {
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
        }
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}

impl InputMethodHost for LinuxInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        crate::ibus::insert_text_into_active_window(text, None)
            .map_err(|e| VoiceInputError::Injection(format!("Linux 文本提交失败：{e}")))
    }

    fn cancel_composition(&self) -> Result<()> {
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        Ok(())
    }
}
