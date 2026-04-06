use voice_input_core::CompositionState;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsCompositionSession {
    pub state: CompositionState,
}
