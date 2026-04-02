use crate::backend::{LinuxBackend, LinuxBackendKind};
use crate::host::{LinuxHostConfig, LinuxInputMethodHost};
use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager};

#[derive(Debug, Clone)]
pub struct LinuxLocalVoiceInputConfig {
    pub app: AppConfig,
    pub host: LinuxHostConfig,
    pub asr: FunAsrConfig,
}

impl Default for LinuxLocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            app: AppConfig::default(),
            host: LinuxHostConfig::default(),
            asr: FunAsrConfig::default(),
        }
    }
}

pub struct LinuxLocalVoiceInput {
    controller: AppController,
    backend_kind: LinuxBackendKind,
    service_name: String,
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
        let transcriber = LocalFunAsrTranscriber::new(config.asr, runner);
        let controller = AppController::new(
            config.app,
            hotkeys,
            recorder,
            Box::new(transcriber),
            Box::new(host),
        );

        Self {
            controller,
            backend_kind,
            service_name,
        }
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn backend_kind(&self) -> LinuxBackendKind {
        self.backend_kind
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}
