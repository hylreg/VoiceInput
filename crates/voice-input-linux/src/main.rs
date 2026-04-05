use std::env;
use std::path::PathBuf;

use voice_input_asr::{FunAsrConfig, PythonFunAsrRunner};
use voice_input_core::{AppConfig, MockHotkeyManager, VoiceInputError};
use voice_input_linux::{
    FileAudioRecorder, LinuxBackend, LinuxBackendKind, LinuxHostConfig, LinuxLocalVoiceInput,
    LinuxLocalVoiceInputConfig,
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

    let backend = match build_backend(args.backend) {
        Ok(backend) => backend,
        Err(err) => {
            eprintln!("{err}");
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

    let pipeline = LinuxLocalVoiceInput::new(
        LinuxLocalVoiceInputConfig {
            app: AppConfig::default(),
            host: LinuxHostConfig {
                backend: args.backend,
                service_name: "voice-input".to_string(),
            },
            asr: asr_config,
        },
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(args.audio_file)),
        Box::new(asr_runner),
        backend,
    );

    match pipeline.run_once() {
        Ok(text) => {
            println!("识别结果：{text}");
            println!("Linux 后端：{:?}", pipeline.backend_kind());
            println!("服务名：{}", pipeline.service_name());
        }
        Err(err) => {
            eprintln!("Linux 本地管线失败：{err}");
            std::process::exit(1);
        }
    }
}

#[derive(Debug)]
struct Args {
    audio_file: PathBuf,
    backend: LinuxBackendKind,
}

impl Args {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut audio_file = None;
        let mut backend = LinuxBackendKind::IBus;
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
                "--backend" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| String::from("缺少 --backend 的值"))?;
                    backend = parse_backend(&value)?;
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

        Ok(Self {
            audio_file,
            backend,
        })
    }
}

fn parse_backend(value: &str) -> Result<LinuxBackendKind, String> {
    match value.to_ascii_lowercase().as_str() {
        "ibus" => Ok(LinuxBackendKind::IBus),
        "fcitx5" | "fcitx" => Ok(LinuxBackendKind::Fcitx5),
        other => Err(format!("不支持的 Linux 后端：{other}")),
    }
}

fn build_backend(kind: LinuxBackendKind) -> Result<Box<dyn LinuxBackend>, VoiceInputError> {
    match kind {
        LinuxBackendKind::IBus => build_ibus_backend(),
        LinuxBackendKind::Fcitx5 => Err(VoiceInputError::Injection(
            "Fcitx5 路径还没有接入原生绑定，请先使用 --backend ibus".to_string(),
        )),
    }
}

#[cfg(feature = "ibus")]
fn build_ibus_backend() -> Result<Box<dyn LinuxBackend>, VoiceInputError> {
    Ok(Box::new(voice_input_linux::IbusBackend::new(
        voice_input_linux::IbusEngineSpec::default(),
    )))
}

#[cfg(not(feature = "ibus"))]
fn build_ibus_backend() -> Result<Box<dyn LinuxBackend>, VoiceInputError> {
    Err(VoiceInputError::Injection(
        "当前构建未启用 IBus 支持，请使用 `cargo run -p voice-input-linux --features ibus -- --audio-file ...`"
            .to_string(),
    ))
}

fn print_usage() {
    eprintln!(
        "用法：cargo run -p voice-input-linux --features ibus -- --audio-file /path/to/audio.wav [--backend ibus|fcitx5]"
    );
}
