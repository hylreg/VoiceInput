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
