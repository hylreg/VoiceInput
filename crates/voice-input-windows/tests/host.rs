use voice_input_core::InputMethodHost;
use voice_input_windows::{
    MockWindowsImeBridge, WindowsHostConfig, WindowsImeEvent, WindowsInputMethodHost,
};

#[test]
fn windows_host_drives_bridge_events() {
    let bridge = MockWindowsImeBridge::default();
    let bridge_for_assertions = bridge.clone();
    let host =
        WindowsInputMethodHost::new_with_bridge(WindowsHostConfig::default(), Box::new(bridge));

    host.start_composition().expect("start");
    host.update_preedit("hello").expect("update");
    host.commit_text("hello world").expect("commit");
    host.end_composition().expect("end");

    assert_eq!(
        bridge_for_assertions.events(),
        vec![
            WindowsImeEvent::StartComposition,
            WindowsImeEvent::UpdatePreedit("hello".to_string()),
            WindowsImeEvent::CommitText("hello world".to_string()),
            WindowsImeEvent::EndComposition,
        ]
    );
}
