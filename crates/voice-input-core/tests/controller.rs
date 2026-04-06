use voice_input_core::{
    AppConfig, AppController, MockAudioRecorder, MockHotkeyManager, MockInputMethodHost,
    MockTranscriber, Result, Transcriber, Transcript,
};

#[test]
fn demo_pipeline_returns_transcribed_text() {
    let controller = AppController::new(
        AppConfig::default(),
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(MockTranscriber),
        Box::new(MockInputMethodHost::default()),
    );

    let result = controller.run_demo().expect("pipeline should succeed");
    assert_eq!(result, "来自语音输入");
}

#[test]
fn demo_pipeline_drives_ime_composition_events() {
    let ime = MockInputMethodHost::default();
    let controller = AppController::new(
        AppConfig::default(),
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(MockTranscriber),
        Box::new(ime.clone()),
    );

    controller.run_demo().expect("pipeline should succeed");

    assert_eq!(
        ime.events(),
        vec![
            "开始输入",
            "显示录音标记",
            "清除录音标记",
            "更新预编辑：你好",
            "更新预编辑：来自语音",
            "更新预编辑：来自语音输入",
            "提交文本：来自语音输入",
            "结束输入",
        ]
    );
}

struct FailingTranscriber;

impl Transcriber for FailingTranscriber {
    fn transcribe(&self, _audio: &[u8]) -> Result<Transcript> {
        Err(voice_input_core::VoiceInputError::Transcription(
            "simulated failure".to_string(),
        ))
    }
}

#[test]
fn demo_pipeline_cancels_composition_on_failure() {
    let ime = MockInputMethodHost::default();
    let controller = AppController::new(
        AppConfig::default(),
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(FailingTranscriber),
        Box::new(ime.clone()),
    );

    let err = controller.run_demo().expect_err("pipeline should fail");
    assert!(matches!(
        err,
        voice_input_core::VoiceInputError::Transcription(_)
    ));
    assert_eq!(
        ime.events(),
        vec![
            "开始输入",
            "显示录音标记",
            "清除录音标记",
            "取消输入",
            "结束输入"
        ]
    );
}
