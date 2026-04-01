use voice_input_core::CompositionState;

#[derive(Debug, Clone, Default)]
pub struct LinuxCompositionState {
    pub inner: CompositionState,
}

#[derive(Debug, Clone)]
pub struct LinuxCompositionSession {
    pub backend: String,
    pub state: LinuxCompositionState,
}

impl LinuxCompositionSession {
    pub fn new(backend: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            state: LinuxCompositionState::default(),
        }
    }

    pub fn start(&mut self) {
        self.state.inner.start();
    }

    pub fn update(&mut self, text: impl Into<String>) {
        self.state.inner.update(text);
    }

    pub fn commit(&mut self, text: impl Into<String>) {
        self.state.inner.commit(text);
    }

    pub fn cancel(&mut self) {
        self.state.inner.cancel();
    }
}

