use voice_input_core::CompositionState;

#[derive(Debug, Clone, Default)]
pub struct MacCompositionSession {
    pub inner: CompositionState,
}
