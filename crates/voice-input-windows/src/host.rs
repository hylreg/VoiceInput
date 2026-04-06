use crate::bridge::{UnwiredWindowsImeBridge, WindowsImeBridge};
use crate::session::WindowsCompositionSession;
use voice_input_core::{InputMethodHost, Result};

#[derive(Debug, Clone)]
pub struct WindowsHostConfig {
    pub app_id: String,
    pub service_name: String,
}

impl Default for WindowsHostConfig {
    fn default() -> Self {
        Self {
            app_id: "com.example.voiceinput.windows".to_string(),
            service_name: "voice-input".to_string(),
        }
    }
}

pub struct WindowsInputMethodHost {
    config: WindowsHostConfig,
    bridge: Box<dyn WindowsImeBridge>,
    session: std::cell::RefCell<WindowsCompositionSession>,
}

impl WindowsInputMethodHost {
    pub fn new(config: WindowsHostConfig) -> Self {
        Self::new_with_bridge(config, Box::new(UnwiredWindowsImeBridge))
    }

    pub fn new_with_bridge(config: WindowsHostConfig, bridge: Box<dyn WindowsImeBridge>) -> Self {
        Self {
            config,
            bridge,
            session: std::cell::RefCell::new(WindowsCompositionSession::default()),
        }
    }

    pub fn app_id(&self) -> &str {
        &self.config.app_id
    }
}

impl InputMethodHost for WindowsInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.bridge.start_composition()?;
        self.session.borrow_mut().state.start();
        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.bridge.update_preedit(text)?;
        self.session.borrow_mut().state.update(text);
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.bridge.commit_text(text)?;
        self.session.borrow_mut().state.commit(text);
        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        self.bridge.cancel_composition()?;
        self.session.borrow_mut().state.cancel();
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        self.bridge.end_composition()
    }
}
