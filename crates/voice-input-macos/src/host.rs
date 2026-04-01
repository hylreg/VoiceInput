use crate::bridge::{MacImeBridge, UnwiredMacImeBridge};
use crate::session::MacCompositionSession;
use voice_input_core::{InputMethodHost, Result};

#[derive(Debug, Clone)]
pub struct MacHostConfig {
    pub bundle_id: String,
    pub service_name: String,
}

impl Default for MacHostConfig {
    fn default() -> Self {
        Self {
            bundle_id: "com.example.voiceinput".to_string(),
            service_name: "voice-input".to_string(),
        }
    }
}

pub struct MacInputMethodHost {
    config: MacHostConfig,
    bridge: Box<dyn MacImeBridge>,
    session: std::cell::RefCell<MacCompositionSession>,
}

impl MacInputMethodHost {
    pub fn new(config: MacHostConfig) -> Self {
        Self::new_with_bridge(config, Box::new(UnwiredMacImeBridge))
    }

    pub fn new_with_bridge(config: MacHostConfig, bridge: Box<dyn MacImeBridge>) -> Self {
        Self {
            config,
            bridge,
            session: std::cell::RefCell::new(MacCompositionSession::default()),
        }
    }

    pub fn bundle_id(&self) -> &str {
        &self.config.bundle_id
    }
}

impl InputMethodHost for MacInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.bridge.start_composition()?;
        self.session.borrow_mut().inner.start();
        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.bridge.update_preedit(text)?;
        self.session.borrow_mut().inner.update(text);
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.bridge.commit_text(text)?;
        self.session.borrow_mut().inner.commit(text);
        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        self.bridge.cancel_composition()?;
        self.session.borrow_mut().inner.cancel();
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        self.bridge.end_composition()
    }
}

