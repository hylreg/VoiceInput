use crate::bridge::{MacImeBridge, UnwiredMacImeBridge};
use voice_input_core::{InputMethodHost, Result};
use voice_input_runtime::{CompositionDriver, StatefulInputMethodHost};

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
    inner: StatefulInputMethodHost<MacHostDriver>,
}

struct MacHostDriver {
    bridge: Box<dyn MacImeBridge>,
}

impl MacInputMethodHost {
    pub fn new(config: MacHostConfig) -> Self {
        Self::new_with_bridge(config, Box::new(UnwiredMacImeBridge))
    }

    pub fn new_with_bridge(config: MacHostConfig, bridge: Box<dyn MacImeBridge>) -> Self {
        Self {
            config,
            inner: StatefulInputMethodHost::new(MacHostDriver { bridge }),
        }
    }

    pub fn bundle_id(&self) -> &str {
        &self.config.bundle_id
    }
}

impl CompositionDriver for MacHostDriver {
    fn start_composition(&self) -> Result<()> {
        self.bridge.start_composition()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.bridge.update_preedit(text)
    }

    fn show_recording_indicator(&self) -> Result<()> {
        self.bridge.show_recording_indicator()
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        self.bridge.clear_recording_indicator()
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.bridge.commit_text(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        self.bridge.cancel_composition()
    }

    fn end_composition(&self) -> Result<()> {
        self.bridge.end_composition()
    }
}

impl InputMethodHost for MacInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.inner.start_composition()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.inner.update_preedit(text)
    }

    fn show_recording_indicator(&self) -> Result<()> {
        self.inner.show_recording_indicator()
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        self.inner.clear_recording_indicator()
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.inner.commit_text(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        self.inner.cancel_composition()
    }

    fn end_composition(&self) -> Result<()> {
        self.inner.end_composition()
    }
}
