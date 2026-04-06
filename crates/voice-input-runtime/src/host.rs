use std::cell::RefCell;

use voice_input_core::{CompositionState, InputMethodHost, Result};

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

    pub fn state(&self) -> CompositionState {
        self.state.borrow().clone()
    }

    pub fn driver(&self) -> &D {
        &self.driver
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
