use crate::bridge::WindowsImeBridge;
use crate::host::{WindowsHostConfig, WindowsInputMethodHost};
use voice_input_asr::FunAsrRunner;
use voice_input_core::{AudioRecorder, HotkeyManager};
use voice_input_runtime::{LocalRuntimeMetadata, LocalVoiceInputConfig, LocalVoiceInputRuntime};

#[derive(Debug, Clone)]
pub struct WindowsLocalVoiceInputConfig {
    pub runtime: LocalVoiceInputConfig,
    pub host: WindowsHostConfig,
}

impl Default for WindowsLocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            runtime: LocalVoiceInputConfig::default(),
            host: WindowsHostConfig::default(),
        }
    }
}

pub struct WindowsLocalVoiceInput {
    inner: LocalVoiceInputRuntime<WindowsLocalMetadata>,
}

#[derive(Debug, Clone)]
pub struct WindowsLocalMetadata {
    pub app_id: String,
}

impl WindowsLocalVoiceInput {
    pub fn new(
        config: WindowsLocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        bridge: Box<dyn WindowsImeBridge>,
    ) -> Self {
        let host = WindowsInputMethodHost::new_with_bridge(config.host, bridge);
        let app_id = host.app_id().to_string();
        let inner = LocalVoiceInputRuntime::new(
            config.runtime,
            hotkeys,
            recorder,
            runner,
            Box::new(host),
            WindowsLocalMetadata { app_id },
        );

        Self { inner }
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.inner.run_once()
    }

    pub fn app_id(&self) -> &str {
        &self.inner.metadata().app_id
    }
}

impl LocalRuntimeMetadata for WindowsLocalMetadata {
    fn label(&self) -> &str {
        &self.app_id
    }
}
