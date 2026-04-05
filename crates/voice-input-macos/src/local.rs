use crate::bridge::MacImeBridge;
use crate::host::{MacHostConfig, MacInputMethodHost};
use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager};

pub struct MacLocalVoiceInputConfig {
    pub app: AppConfig,
    pub host: MacHostConfig,
    pub asr: FunAsrConfig,
}

impl Default for MacLocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            app: AppConfig::default(),
            host: MacHostConfig::default(),
            asr: FunAsrConfig::from_env(),
        }
    }
}

pub struct MacLocalVoiceInput {
    controller: AppController,
    bundle_id: String,
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
            bundle_id,
        }
    }

    pub fn controller(&self) -> &AppController {
        &self.controller
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn host_bundle_id(&self) -> &str {
        &self.bundle_id
    }
}
