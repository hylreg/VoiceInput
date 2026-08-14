#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub activation_hotkey: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            activation_hotkey: "Ctrl+Shift+Space".to_string(),
        }
    }
}
