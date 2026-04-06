use voice_input_asr::{FunAsrConfig, MockFunAsrRunner};
use voice_input_core::{MockAudioRecorder, MockHotkeyManager};
use voice_input_macos::{
    MacHostConfig, MacImeEvent, MacLocalVoiceInput, MacLocalVoiceInputConfig, MockMacImeBridge,
};
use voice_input_runtime::LocalVoiceInputConfig;

#[test]
fn local_mac_pipeline_uses_funasr_and_drives_ime_events() {
    let bridge = MockMacImeBridge::default();
    let bridge_for_assertions = bridge.clone();
    let runner = MockFunAsrRunner {
        transcript: "本地语音输入".to_string(),
        ..Default::default()
    };
    let calls = runner.calls.clone();
    let pipeline = MacLocalVoiceInput::new(
        MacLocalVoiceInputConfig {
            runtime: LocalVoiceInputConfig {
                app: voice_input_core::AppConfig::default(),
                asr: FunAsrConfig::default(),
            },
            host: MacHostConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(runner),
        Box::new(bridge),
    );

    let text = pipeline.run_once().expect("pipeline should succeed");

    assert_eq!(text, "本地语音输入");
    assert_eq!(
        bridge_for_assertions.events(),
        vec![
            MacImeEvent::StartComposition,
            MacImeEvent::ShowRecordingIndicator,
            MacImeEvent::ClearRecordingIndicator,
            MacImeEvent::UpdatePreedit("本地语音输入".to_string()),
            MacImeEvent::CommitText("本地语音输入".to_string()),
            MacImeEvent::EndComposition,
        ]
    );

    let recorded = calls.lock().expect("calls lock").clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].source_url,
        "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
    );
    assert_eq!(recorded[0].model_id, "FunAudioLLM/Fun-ASR-Nano-2512");
}
