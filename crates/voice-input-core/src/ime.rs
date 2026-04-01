#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub partials: Vec<String>,
    pub final_text: String,
}

impl Transcript {
    pub fn new(final_text: impl Into<String>) -> Self {
        let final_text = final_text.into();
        Self {
            partials: vec![final_text.clone()],
            final_text,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationEvent {
    CompositionStarted,
    CompositionUpdated(String),
    CompositionCommitted(String),
    CompositionCanceled,
    CompositionEnded,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CompositionState {
    pub active: bool,
    pub preedit: String,
    pub committed_text: String,
}

impl CompositionState {
    pub fn start(&mut self) {
        self.active = true;
        self.preedit.clear();
    }

    pub fn update(&mut self, text: impl Into<String>) {
        self.preedit = text.into();
    }

    pub fn commit(&mut self, text: impl Into<String>) {
        self.committed_text = text.into();
        self.preedit.clear();
        self.active = false;
    }

    pub fn cancel(&mut self) {
        self.preedit.clear();
        self.active = false;
    }
}
