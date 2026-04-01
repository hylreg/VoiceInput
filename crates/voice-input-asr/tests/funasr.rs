use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, MockFunAsrRunner};
use voice_input_core::{Transcriber, Transcript};

#[test]
fn local_transcriber_uses_local_model_config() {
    let runner = MockFunAsrRunner {
        transcript: "你好，世界".to_string(),
        ..Default::default()
    };
    let calls = runner.calls.clone();
    let transcriber = LocalFunAsrTranscriber::new(FunAsrConfig::default(), Box::new(runner));

    let transcript = transcriber
        .transcribe(b"fake wav bytes")
        .expect("transcription should succeed");

    assert_eq!(transcript, Transcript::new("你好，世界"));
    let recorded = calls.lock().expect("calls lock").clone();
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        recorded[0].model_dir,
        std::path::PathBuf::from("./models/FunAudioLLM/Fun-ASR-Nano-2512")
    );
    assert_eq!(
        recorded[0].source_url,
        "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
    );
    assert_eq!(recorded[0].device, "auto");
    assert_eq!(recorded[0].language, "中文");
}
