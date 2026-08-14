use voice_input_asr::MockFunAsrRunner;
use voice_input_core::{AppConfig, MockAudioRecorder, MockHotkeyManager};
use voice_input_linux::{LinuxLiveAppConfig, LinuxLocalVoiceInput, LocalVoiceInputConfig};

#[test]
fn live_app_defaults_to_double_alt_hotkey() {
    let config = LinuxLiveAppConfig::default();

    assert_eq!(config.app.activation_hotkey, "DoubleAlt");
}

#[test]
fn local_voice_input_wires_linux_host_and_asr_pipeline() {
    let runner = MockFunAsrRunner {
        transcript: "来自 Linux".to_string(),
        ..Default::default()
    };
    let pipeline = LinuxLocalVoiceInput::new(
        LocalVoiceInputConfig {
            app: AppConfig::default(),
            asr: voice_input_asr::FunAsrConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(runner),
        "voice-input",
    );

    let text = pipeline.run_once().expect("pipeline should succeed");
    assert_eq!(text, "来自 Linux");
}
