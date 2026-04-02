use std::env;
use std::path::PathBuf;

use voice_input_asr::{FunAsrConfig, PythonFunAsrRunner};
use voice_input_core::{AppConfig, MockHotkeyManager};
use voice_input_macos::{
    FileAudioRecorder, MacHostConfig, MacLocalVoiceInput, MacLocalVoiceInputConfig,
    MockMacImeBridge,
};

fn main() {
    let audio_path = match parse_audio_path(env::args().collect()) {
        Ok(path) => path,
        Err(message) => {
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    let bridge = MockMacImeBridge::default();
    let bridge_for_output = bridge.clone();
    let asr_runner = match voice_input_asr::PythonFunAsrRunner::connect(FunAsrConfig::default()) {
        Ok(runner) => runner,
        Err(err) => {
            eprintln!("预加载 FunASR 模型失败：{err}");
            std::process::exit(1);
        }
    };

    let pipeline = MacLocalVoiceInput::new(
        MacLocalVoiceInputConfig {
            app: AppConfig::default(),
            host: MacHostConfig::default(),
            asr: FunAsrConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(audio_path)),
        Box::new(asr_runner),
        Box::new(bridge),
    );

    match pipeline.run_once() {
        Ok(text) => {
            println!("识别结果：{text}");
            println!("应用标识：{}", pipeline.host_bundle_id());
            let events = bridge_for_output
                .events()
                .into_iter()
                .map(|event| event.to_string())
                .collect::<Vec<_>>()
                .join("，");
            println!("输入法事件：{events}");
        }
        Err(err) => {
            eprintln!("macOS 本地管线失败：{err}");
            std::process::exit(1);
        }
    }
}

fn parse_audio_path(args: Vec<String>) -> Result<PathBuf, String> {
    let mut iter = args.into_iter();
    let _bin = iter.next();

    while let Some(arg) = iter.next() {
        if arg == "--audio-file" {
            let value = iter
                .next()
                .ok_or_else(|| String::from("缺少 --audio-file 的值"))?;
            return Ok(PathBuf::from(value));
        }
    }

    Err(String::from("缺少必需参数 --audio-file"))
}

fn print_usage() {
    eprintln!("用法：uv run -- cargo run -p voice-input-macos -- --audio-file /path/to/audio.wav");
}
