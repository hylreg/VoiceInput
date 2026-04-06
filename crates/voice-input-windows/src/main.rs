use std::env;
use std::path::PathBuf;

use voice_input_asr::{FunAsrConfig, PythonFunAsrRunner};
use voice_input_core::{AppConfig, MockHotkeyManager};
use voice_input_runtime::LocalVoiceInputConfig;
use voice_input_windows::{
    ClipboardWindowsImeBridge, FileAudioRecorder, WindowsHostConfig, WindowsLocalVoiceInput,
    WindowsLocalVoiceInputConfig,
};

fn main() {
    let args = match Args::parse(env::args().collect()) {
        Ok(args) => args,
        Err(message) => {
            if message == "help" {
                print_usage();
                std::process::exit(0);
            }
            eprintln!("{message}");
            print_usage();
            std::process::exit(2);
        }
    };

    let asr_config = FunAsrConfig::from_env();
    let asr_runner = match PythonFunAsrRunner::connect(asr_config.clone()) {
        Ok(runner) => runner,
        Err(err) => {
            eprintln!("预加载 ASR 模型失败：{err}");
            std::process::exit(1);
        }
    };

    let pipeline = WindowsLocalVoiceInput::new(
        WindowsLocalVoiceInputConfig {
            runtime: LocalVoiceInputConfig {
                app: AppConfig::default(),
                asr: asr_config,
            },
            host: WindowsHostConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(args.audio_file)),
        Box::new(asr_runner),
        Box::new(ClipboardWindowsImeBridge),
    );

    match pipeline.run_once() {
        Ok(text) => {
            println!("识别结果：{text}");
            println!("Windows App ID：{}", pipeline.app_id());
            println!("提交方式：Windows Unicode 注入，失败时回退剪贴板粘贴");
        }
        Err(err) => {
            eprintln!("Windows 本地管线失败：{err}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug)]
struct Args {
    audio_file: PathBuf,
}

impl Args {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut audio_file = None;
        let mut iter = args.into_iter();
        let _bin = iter.next();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--audio-file" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| String::from("缺少 --audio-file 的值"))?;
                    audio_file = Some(PathBuf::from(value));
                }
                "--help" | "-h" => {
                    return Err(String::from("help"));
                }
                other => {
                    return Err(format!("不支持的参数：{other}"));
                }
            }
        }

        let audio_file = audio_file.ok_or_else(|| String::from("缺少必需参数 --audio-file"))?;
        Ok(Self { audio_file })
    }
}

fn print_usage() {
    eprintln!("用法：cargo run -p voice-input-windows -- --audio-file /path/to/audio.wav");
}
