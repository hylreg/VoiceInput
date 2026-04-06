use crate::bridge::{UnwiredWindowsImeBridge, WindowsImeBridge};
use voice_input_core::{InputMethodHost, Result};
use voice_input_runtime::{CompositionDriver, StatefulInputMethodHost};

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
    inner: StatefulInputMethodHost<WindowsHostDriver>,
}

struct WindowsHostDriver {
    bridge: Box<dyn WindowsImeBridge>,
}

impl WindowsInputMethodHost {
    pub fn new(config: WindowsHostConfig) -> Self {
        Self::new_with_bridge(config, Box::new(UnwiredWindowsImeBridge))
    }

    pub fn new_with_bridge(config: WindowsHostConfig, bridge: Box<dyn WindowsImeBridge>) -> Self {
        Self {
            config,
            inner: StatefulInputMethodHost::new(WindowsHostDriver { bridge }),
        }
    }

    pub fn app_id(&self) -> &str {
        &self.config.app_id
    }
}

impl CompositionDriver for WindowsHostDriver {
    fn start_composition(&self) -> Result<()> {
        self.bridge.start_composition()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.bridge.update_preedit(text)
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

impl InputMethodHost for WindowsInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.inner.start_composition()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.inner.update_preedit(text)
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
