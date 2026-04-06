use crate::bridge::WindowsImeBridge;
use crate::host::{WindowsHostConfig, WindowsInputMethodHost};
use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager};

#[derive(Debug, Clone)]
pub struct WindowsLocalVoiceInputConfig {
    pub app: AppConfig,
    pub host: WindowsHostConfig,
    pub asr: FunAsrConfig,
}

impl Default for WindowsLocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            app: AppConfig::default(),
            host: WindowsHostConfig::default(),
            asr: FunAsrConfig::from_env(),
        }
    }
}

pub struct WindowsLocalVoiceInput {
    controller: AppController,
    app_id: String,
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
        let transcriber = LocalFunAsrTranscriber::new(config.asr, runner);
        let controller = AppController::new(
            config.app,
            hotkeys,
            recorder,
            Box::new(transcriber),
            Box::new(host),
        );

        Self { controller, app_id }
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }
}
