use crate::backend::{backend_from_kind, LinuxBackend, LinuxBackendKind};
use crate::session::LinuxCompositionSession;
use voice_input_core::{InputMethodHost, Result};

#[derive(Debug, Clone)]
pub struct LinuxHostConfig {
    pub backend: LinuxBackendKind,
    pub service_name: String,
}

impl Default for LinuxHostConfig {
    fn default() -> Self {
        Self {
            backend: LinuxBackendKind::Fcitx5,
            service_name: "voice-input".to_string(),
        }
    }
}

pub struct LinuxInputMethodHost {
    config: LinuxHostConfig,
    backend: Box<dyn LinuxBackend>,
    session: std::cell::RefCell<LinuxCompositionSession>,
}

impl LinuxInputMethodHost {
    pub fn new(config: LinuxHostConfig) -> Self {
        let backend_kind = config.backend;
        Self::new_with_backend(config, backend_from_kind(backend_kind))
    }

    pub fn new_with_backend(
        config: LinuxHostConfig,
        backend: Box<dyn LinuxBackend>,
    ) -> Self {
        let session = LinuxCompositionSession::new(config.service_name.clone());

        Self {
            config,
            backend,
            session: std::cell::RefCell::new(session),
        }
    }

    pub fn backend_kind(&self) -> LinuxBackendKind {
        self.config.backend
    }
}

impl InputMethodHost for LinuxInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.backend.start()?;
        self.session.borrow_mut().start();
        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.backend.update_preedit(text)?;
        self.session.borrow_mut().update(text);
        Ok(())
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.backend.commit_text(text)?;
        self.session.borrow_mut().commit(text);
        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        self.backend.cancel()?;
        self.session.borrow_mut().cancel();
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        self.backend.stop()
    }
}
