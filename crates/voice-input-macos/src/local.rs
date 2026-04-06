use crate::bridge::MacImeBridge;
use crate::host::{MacHostConfig, MacInputMethodHost};
use voice_input_asr::FunAsrRunner;
use voice_input_core::{AudioRecorder, HotkeyManager};
use voice_input_runtime::{LocalRuntimeMetadata, LocalVoiceInputConfig, LocalVoiceInputRuntime};

pub struct MacLocalVoiceInputConfig {
    pub runtime: LocalVoiceInputConfig,
    pub host: MacHostConfig,
}

impl Default for MacLocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            runtime: LocalVoiceInputConfig::default(),
            host: MacHostConfig::default(),
        }
    }
}

pub struct MacLocalVoiceInput {
    inner: LocalVoiceInputRuntime<MacLocalMetadata>,
}

#[derive(Debug, Clone)]
pub struct MacLocalMetadata {
    pub bundle_id: String,
}

impl MacLocalVoiceInput {
    pub fn new(
        config: MacLocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        bridge: Box<dyn MacImeBridge>,
    ) -> Self {
        let host = MacInputMethodHost::new_with_bridge(config.host, bridge);
        let bundle_id = host.bundle_id().to_string();
        let inner = LocalVoiceInputRuntime::new(
            config.runtime,
            hotkeys,
            recorder,
            runner,
            Box::new(host),
            MacLocalMetadata { bundle_id },
        );

        Self { inner }
    }

    pub fn controller(&self) -> &voice_input_core::AppController {
        self.inner.controller()
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.inner.run_once()
    }

    pub fn host_bundle_id(&self) -> &str {
        &self.inner.metadata().bundle_id
    }
}

impl LocalRuntimeMetadata for MacLocalMetadata {
    fn label(&self) -> &str {
        &self.bundle_id
    }
}
