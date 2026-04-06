use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager, InputMethodHost};

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

pub trait LocalRuntimeMetadata {
    fn label(&self) -> &str;
}

pub struct LocalVoiceInputRuntime<M> {
    controller: AppController,
    metadata: M,
}

impl<M> LocalVoiceInputRuntime<M> {
    pub fn new(
        config: LocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        host: Box<dyn InputMethodHost>,
        metadata: M,
    ) -> Self {
        let transcriber = LocalFunAsrTranscriber::new(config.asr, runner);
        let controller =
            AppController::new(config.app, hotkeys, recorder, Box::new(transcriber), host);

        Self {
            controller,
            metadata,
        }
    }

    pub fn controller(&self) -> &AppController {
        &self.controller
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn metadata(&self) -> &M {
        &self.metadata
    }
}
