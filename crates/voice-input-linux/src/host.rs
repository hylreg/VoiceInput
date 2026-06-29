use std::cell::RefCell;

use crate::backend::{backend_from_kind, LinuxBackend, LinuxBackendKind};
use voice_input_core::{CompositionState, InputMethodHost, Result};

// ── Inlined from voice-input-runtime::host ──

pub trait CompositionDriver {
    fn start_composition(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn show_recording_indicator(&self) -> Result<()> {
        Ok(())
    }
    fn clear_recording_indicator(&self) -> Result<()> {
        Ok(())
    }
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

pub struct StatefulInputMethodHost<D> {
    driver: D,
    state: RefCell<CompositionState>,
}

impl<D> StatefulInputMethodHost<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            state: RefCell::new(CompositionState::default()),
        }
    }
}

impl<D> InputMethodHost for StatefulInputMethodHost<D>
where
    D: CompositionDriver,
{
    fn start_composition(&self) -> Result<()> {
        self.driver.start_composition()?;
        self.state.borrow_mut().start();
        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.driver.update_preedit(text)?;
        self.state.borrow_mut().update(text);
        Ok(())
    }

    fn show_recording_indicator(&self) -> Result<()> {
        self.driver.show_recording_indicator()
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        self.driver.clear_recording_indicator()
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.driver.commit_text(text)?;
        self.state.borrow_mut().commit(text);
        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        self.driver.cancel_composition()?;
        self.state.borrow_mut().cancel();
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        self.driver.end_composition()
    }
}

// ── Linux-specific host ──

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
    inner: StatefulInputMethodHost<LinuxHostDriver>,
}

struct LinuxHostDriver {
    backend: Box<dyn LinuxBackend>,
}

impl LinuxInputMethodHost {
    pub fn new(config: LinuxHostConfig) -> Self {
        let backend_kind = config.backend;
        Self::new_with_backend(config, backend_from_kind(backend_kind))
    }

    pub fn new_with_backend(config: LinuxHostConfig, backend: Box<dyn LinuxBackend>) -> Self {
        Self {
            config,
            inner: StatefulInputMethodHost::new(LinuxHostDriver { backend }),
        }
    }

    pub fn backend_kind(&self) -> LinuxBackendKind {
        self.config.backend
    }
}

impl CompositionDriver for LinuxHostDriver {
    fn start_composition(&self) -> Result<()> {
        self.backend.start()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.backend.update_preedit(text)
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.backend.commit_text(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        self.backend.cancel()
    }

    fn end_composition(&self) -> Result<()> {
        self.backend.stop()
    }
}

impl InputMethodHost for LinuxInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.inner.start_composition()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.inner.update_preedit(text)
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.inner.commit_text(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        self.inner.cancel_composition()
    }

    fn end_composition(&self) -> Result<()> {
        self.inner.end_composition()
    }
}
