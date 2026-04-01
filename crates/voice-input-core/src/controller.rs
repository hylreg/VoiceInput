use crate::config::AppConfig;
use crate::error::Result;
use crate::platform::{AudioRecorder, HotkeyManager, InputMethodHost, Transcriber};

pub struct AppController {
    pub config: AppConfig,
    hotkeys: Box<dyn HotkeyManager>,
    recorder: Box<dyn AudioRecorder>,
    transcriber: Box<dyn Transcriber>,
    ime: Box<dyn InputMethodHost>,
}

impl AppController {
    pub fn new(
        config: AppConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        transcriber: Box<dyn Transcriber>,
        ime: Box<dyn InputMethodHost>,
    ) -> Self {
        Self {
            config,
            hotkeys,
            recorder,
            transcriber,
            ime,
        }
    }

    pub fn process_once(&self) -> Result<String> {
        self.ime.start_composition()?;
        let outcome = (|| {
            let audio = self.recorder.record_once()?;
            let transcript = self.transcriber.transcribe(&audio)?;

            for partial in &transcript.partials {
                self.ime.update_preedit(partial)?;
            }

            self.ime.commit_text(&transcript.final_text)?;
            Ok(transcript.final_text)
        })();

        match outcome {
            Ok(text) => {
                self.ime.end_composition()?;
                Ok(text)
            }
            Err(err) => {
                let _ = self.ime.cancel_composition();
                let _ = self.ime.end_composition();
                Err(err)
            }
        }
    }

    pub fn run_demo(&self) -> Result<String> {
        self.hotkeys
            .register_global_hotkey(&self.config.activation_hotkey)?;

        self.process_once()
    }
}
