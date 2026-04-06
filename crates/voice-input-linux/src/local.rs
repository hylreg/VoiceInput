use crate::backend::{LinuxBackend, LinuxBackendKind};
use crate::host::{LinuxHostConfig, LinuxInputMethodHost};
use voice_input_asr::FunAsrRunner;
use voice_input_core::{AudioRecorder, HotkeyManager};
use voice_input_runtime::{LocalRuntimeMetadata, LocalVoiceInputConfig, LocalVoiceInputRuntime};

#[derive(Debug, Clone)]
pub struct LinuxLocalVoiceInputConfig {
    pub runtime: LocalVoiceInputConfig,
    pub host: LinuxHostConfig,
}

impl Default for LinuxLocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            runtime: LocalVoiceInputConfig::default(),
            host: LinuxHostConfig::default(),
        }
    }
}

pub struct LinuxLocalVoiceInput {
    inner: LocalVoiceInputRuntime<LinuxLocalMetadata>,
}

#[derive(Debug, Clone)]
pub struct LinuxLocalMetadata {
    pub backend_kind: LinuxBackendKind,
    pub service_name: String,
}

impl LinuxLocalVoiceInput {
    pub fn new(
        config: LinuxLocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        backend: Box<dyn LinuxBackend>,
    ) -> Self {
        let backend_kind = backend.kind();
        let service_name = config.host.service_name.clone();
        let host = LinuxInputMethodHost::new_with_backend(config.host, backend);
        let inner = LocalVoiceInputRuntime::new(
            config.runtime,
            hotkeys,
            recorder,
            runner,
            Box::new(host),
            LinuxLocalMetadata {
                backend_kind,
                service_name,
            },
        );

        Self { inner }
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.inner.run_once()
    }

    pub fn backend_kind(&self) -> LinuxBackendKind {
        self.inner.metadata().backend_kind
    }

    pub fn service_name(&self) -> &str {
        &self.inner.metadata().service_name
    }
}

impl LocalRuntimeMetadata for LinuxLocalMetadata {
    fn label(&self) -> &str {
        &self.service_name
    }
}
