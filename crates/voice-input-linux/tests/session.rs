use std::path::Path;

use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, MockFunAsrRunner};
use voice_input_linux::{transcribe_file, LinuxLiveAppConfig};

#[test]
fn live_app_defaults_to_double_ctrl_hotkey() {
    let config = LinuxLiveAppConfig::default();

    assert_eq!(config.app.activation_hotkey, "DoubleCtrl");
}

#[test]
fn smoke_transcribes_test_audio_file() {
    let runner = MockFunAsrRunner {
        transcript: "来自 Linux".to_string(),
        ..Default::default()
    };
    let transcriber = LocalFunAsrTranscriber::new(FunAsrConfig::default(), Box::new(runner));
    let audio_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/smoke.wav");

    let text = transcribe_file(&audio_path, &transcriber).expect("transcribe file");
    assert_eq!(text, "来自 Linux");
}
