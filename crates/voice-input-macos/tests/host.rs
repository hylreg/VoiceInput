use voice_input_core::InputMethodHost;
use voice_input_macos::{MacHostConfig, MacInputMethodHost, MockMacImeBridge};

#[test]
fn mac_host_drives_bridge_events() {
    let bridge = MockMacImeBridge::default();
    let bridge_for_assertions = bridge.clone();
    let host = MacInputMethodHost::new_with_bridge(MacHostConfig::default(), Box::new(bridge));

    host.start_composition().expect("start");
    host.show_recording_indicator().expect("show recording");
    host.clear_recording_indicator().expect("clear recording");
    host.update_preedit("hello").expect("update");
    host.commit_text("hello world").expect("commit");
    host.end_composition().expect("end");

    assert_eq!(
        bridge_for_assertions.events(),
        vec![
            voice_input_macos::MacImeEvent::StartComposition,
            voice_input_macos::MacImeEvent::ShowRecordingIndicator,
            voice_input_macos::MacImeEvent::ClearRecordingIndicator,
            voice_input_macos::MacImeEvent::UpdatePreedit("hello".to_string()),
            voice_input_macos::MacImeEvent::CommitText("hello world".to_string()),
            voice_input_macos::MacImeEvent::EndComposition,
        ]
    );
}
