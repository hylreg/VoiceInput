use voice_input_core::InputMethodHost;
use voice_input_linux::{
    IbusEngineEvent, LinuxBackendKind, LinuxCompositionSession, LinuxHostConfig,
    LinuxInputMethodHost, MockIbusBridge, MockLinuxBackend,
};

#[test]
fn session_tracks_composition_state() {
    let mut session = LinuxCompositionSession::new("voice-input");
    session.start();
    session.update("hello");
    session.commit("hello world");

    assert!(!session.state.inner.active);
    assert_eq!(session.state.inner.committed_text, "hello world");
    assert_eq!(session.backend, "voice-input");
}

#[test]
fn host_uses_configured_backend() {
    let host = LinuxInputMethodHost::new(LinuxHostConfig {
        backend: LinuxBackendKind::IBus,
        service_name: "voice-input".to_string(),
    });

    assert_eq!(host.backend_kind(), LinuxBackendKind::IBus);
}

#[test]
fn host_forwards_events_to_backend_and_session() {
    let backend = MockLinuxBackend::new(LinuxBackendKind::Fcitx5);
    let backend_for_assertions = backend.clone();
    let host = LinuxInputMethodHost::new_with_backend(
        LinuxHostConfig {
            backend: LinuxBackendKind::Fcitx5,
            service_name: "voice-input".to_string(),
        },
        Box::new(backend),
    );

    host.start_composition().expect("start composition");
    host.update_preedit("hello").expect("update preedit");
    host.commit_text("hello world").expect("commit text");
    host.end_composition().expect("end composition");

    assert_eq!(
        backend_for_assertions.events(),
        vec!["开始输入", "更新预编辑：hello", "提交文本：hello world", "结束输入"]
    );
}

#[test]
fn ibus_backend_records_ibus_style_events() {
    let bridge = MockIbusBridge::default();
    let bridge_for_assertions = bridge.clone();
    let host = LinuxInputMethodHost::new_with_backend(
        LinuxHostConfig {
            backend: LinuxBackendKind::IBus,
            service_name: "voice-input".to_string(),
        },
        Box::new(voice_input_linux::IbusBackend::new_with_bridge(
            voice_input_linux::IbusEngineSpec::default(),
            Box::new(bridge),
        )),
    );

    host.start_composition().expect("start composition");
    host.update_preedit("hello").expect("update preedit");
    host.commit_text("hello world").expect("commit text");
    host.end_composition().expect("end composition");

    assert_eq!(
        bridge_for_assertions.events(),
        vec![
            IbusEngineEvent::StartComposition,
            IbusEngineEvent::UpdatePreedit("hello".to_string()),
            IbusEngineEvent::CommitText("hello world".to_string()),
            IbusEngineEvent::EndComposition,
        ]
    );
}
