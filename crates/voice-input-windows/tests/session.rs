use voice_input_asr::MockFunAsrRunner;
use voice_input_core::{AppConfig, MockAudioRecorder, MockHotkeyManager};
use voice_input_runtime::LocalVoiceInputConfig;
use voice_input_windows::{
    MockWindowsImeBridge, WindowsHostConfig, WindowsImeEvent, WindowsLocalVoiceInput,
    WindowsLocalVoiceInputConfig,
};

#[test]
fn local_voice_input_wires_windows_host_and_asr_pipeline() {
    let bridge = MockWindowsImeBridge::default();
    let bridge_for_assertions = bridge.clone();
    let runner = MockFunAsrRunner {
        transcript: "来自 Windows".to_string(),
        ..Default::default()
    };
    let pipeline = WindowsLocalVoiceInput::new(
        WindowsLocalVoiceInputConfig {
            runtime: LocalVoiceInputConfig {
                app: AppConfig::default(),
                asr: voice_input_asr::FunAsrConfig::default(),
            },
            host: WindowsHostConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(runner),
        Box::new(bridge),
    );

    let text = pipeline.run_once().expect("pipeline should succeed");
    assert_eq!(text, "来自 Windows");
    assert_eq!(
        bridge_for_assertions.events(),
        vec![
            WindowsImeEvent::StartComposition,
            WindowsImeEvent::UpdatePreedit("来自 Windows".to_string()),
            WindowsImeEvent::CommitText("来自 Windows".to_string()),
            WindowsImeEvent::EndComposition,
        ]
    );
}
