use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager};

use crate::host::LinuxInputMethodHost;

// ── Inlined from voice-input-runtime::local ──

#[derive(Debug, Clone)]
pub struct LocalVoiceInputConfig {
    pub app: AppConfig,
    pub asr: FunAsrConfig,
}

impl Default for LocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            app: AppConfig::default(),
            asr: FunAsrConfig::from_env(),
        }
    }
}

pub fn build_local_python_runtime_config(
) -> voice_input_core::Result<(LocalVoiceInputConfig, Box<dyn FunAsrRunner>)> {
    let config = LocalVoiceInputConfig::default();
    let runner = PythonFunAsrRunner::connect(config.asr.clone())?;
    Ok((config, Box::new(runner)))
}

// ── Linux-specific local voice input ──

pub struct LinuxLocalVoiceInput {
    controller: AppController,
    service_name: String,
}

impl LinuxLocalVoiceInput {
    pub fn new(
        config: LocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        service_name: impl Into<String>,
    ) -> Self {
        let service_name = service_name.into();
        let transcriber = LocalFunAsrTranscriber::new(config.asr, runner);
        let host = LinuxInputMethodHost::new(service_name.clone());
        let controller = AppController::new(
            config.app,
            hotkeys,
            recorder,
            Box::new(transcriber),
            Box::new(host),
        );

        Self {
            controller,
            service_name,
        }
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}
