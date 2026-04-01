use voice_input_core::{
    AppConfig, AppController, MockAudioRecorder, MockHotkeyManager, MockInputMethodHost,
    MockTranscriber,
};

fn main() {
    let controller = AppController::new(
        AppConfig::default(),
        Box::new(MockHotkeyManager),
        Box::new(MockAudioRecorder),
        Box::new(MockTranscriber),
        Box::new(MockInputMethodHost::default()),
    );

    match controller.run_demo() {
        Ok(text) => {
            println!("语音输入管线完成：{text}");
        }
        Err(err) => {
            eprintln!("语音输入管线失败：{err}");
            std::process::exit(1);
        }
    }
}
