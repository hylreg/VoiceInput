use std::env;
use std::path::PathBuf;

use voice_input_runtime::{
    parse_audio_file_with_optional_backend_arg, parse_required_audio_file_arg,
};

pub fn run() -> i32 {
    run_with_args(env::args().collect())
}

pub fn run_with_args(args: Vec<String>) -> i32 {
    let command = match Command::parse(args) {
        Ok(command) => command,
        Err(ParseOutcome::Help(message)) => {
            eprintln!("{message}");
            return 0;
        }
        Err(ParseOutcome::Error(message)) => {
            eprintln!("{message}");
            eprintln!("{}", usage());
            return 2;
        }
    };

    let result = match command {
        Command::SmokeMacos { audio_file } => voice_input_macos::run_smoke(audio_file),
        Command::SmokeLinux {
            audio_file,
            backend,
        } => voice_input_linux::run_smoke(audio_file, backend),
        Command::SmokeWindows { audio_file } => voice_input_windows::run_smoke(audio_file),
        Command::LiveMacos => {
            voice_input_macos::run_live_app(voice_input_macos::MacLiveAppConfig::default())
                .map_err(|err| format!("macOS 常驻应用启动失败：{err}"))
        }
        Command::LiveLinux {
            backend,
            double_ctrl_window_ms,
            silence_stop_ms,
        } => voice_input_linux::run_live_with_args(voice_input_linux::LinuxLiveArgs {
            backend,
            double_ctrl_window_ms,
            silence_stop_ms,
        }),
        Command::LiveWindows => {
            voice_input_windows::run_live_app(voice_input_windows::WindowsLiveAppConfig::default())
                .map_err(|err| format!("Windows 常驻应用启动失败：{err}"))
        }
    };

    match result {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

enum Command {
    SmokeMacos {
        audio_file: PathBuf,
    },
    SmokeLinux {
        audio_file: PathBuf,
        backend: voice_input_linux::LinuxBackendKind,
    },
    SmokeWindows {
        audio_file: PathBuf,
    },
    LiveMacos,
    LiveLinux {
        backend: voice_input_linux::LinuxBackendKind,
        double_ctrl_window_ms: Option<u64>,
        silence_stop_ms: Option<u64>,
    },
    LiveWindows,
}

enum ParseOutcome {
    Help(String),
    Error(String),
}

impl Command {
    fn parse(args: Vec<String>) -> Result<Self, ParseOutcome> {
        let mut iter = args.into_iter();
        let _bin = iter.next();

        let Some(top) = iter.next() else {
            return Err(ParseOutcome::Error("缺少子命令".to_string()));
        };

        if matches!(top.as_str(), "--help" | "-h" | "help") {
            return Err(ParseOutcome::Help(usage()));
        }

        let Some(platform) = iter.next() else {
            return Err(ParseOutcome::Error("缺少平台子命令".to_string()));
        };

        match top.to_ascii_lowercase().as_str() {
            "smoke" => match platform.to_ascii_lowercase().as_str() {
                "macos" => {
                    let audio_file = parse_audio_file(iter.collect())?;
                    Ok(Self::SmokeMacos { audio_file })
                }
                "linux" => {
                    let (audio_file, backend) = parse_linux_smoke_args(iter.collect())?;
                    Ok(Self::SmokeLinux {
                        audio_file,
                        backend,
                    })
                }
                "windows" => {
                    let audio_file = parse_audio_file(iter.collect())?;
                    Ok(Self::SmokeWindows { audio_file })
                }
                other => Err(ParseOutcome::Error(format!("不支持的平台：{other}"))),
            },
            "live" => match platform.to_ascii_lowercase().as_str() {
                "macos" => {
                    parse_no_args(iter.collect())?;
                    Ok(Self::LiveMacos)
                }
                "linux" => {
                    let args = parse_linux_live_args(iter.collect())?;
                    Ok(Self::LiveLinux {
                        backend: args.backend,
                        double_ctrl_window_ms: args.double_ctrl_window_ms,
                        silence_stop_ms: args.silence_stop_ms,
                    })
                }
                "windows" => {
                    parse_no_args(iter.collect())?;
                    Ok(Self::LiveWindows)
                }
                other => Err(ParseOutcome::Error(format!("不支持的平台：{other}"))),
            },
            other => Err(ParseOutcome::Error(format!("不支持的子命令：{other}"))),
        }
    }
}

fn parse_audio_file(args: Vec<String>) -> Result<PathBuf, ParseOutcome> {
    let mut forwarded = vec!["voice-input-cli-smoke".to_string()];
    forwarded.extend(args);
    parse_required_audio_file_arg(forwarded).map_err(map_arg_parse_error)
}

fn parse_linux_smoke_args(
    args: Vec<String>,
) -> Result<(PathBuf, voice_input_linux::LinuxBackendKind), ParseOutcome> {
    let mut forwarded = vec!["voice-input-cli-smoke-linux".to_string()];
    forwarded.extend(args);
    parse_audio_file_with_optional_backend_arg(
        forwarded,
        voice_input_linux::LinuxBackendKind::IBus,
        voice_input_linux::parse_backend_kind,
    )
    .map_err(map_arg_parse_error)
}

fn parse_linux_live_args(
    args: Vec<String>,
) -> Result<voice_input_linux::LinuxLiveArgs, ParseOutcome> {
    let mut forwarded = vec!["voice-input-cli-live-linux".to_string()];
    forwarded.extend(args);
    voice_input_linux::parse_live_args(forwarded).map_err(|message| {
        if message == "help" {
            ParseOutcome::Help(usage())
        } else {
            ParseOutcome::Error(message)
        }
    })
}

fn parse_no_args(args: Vec<String>) -> Result<(), ParseOutcome> {
    if args.is_empty() {
        Ok(())
    } else if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        Err(ParseOutcome::Help(usage()))
    } else {
        Err(ParseOutcome::Error(format!("不支持的参数：{}", args[0])))
    }
}

fn map_arg_parse_error(message: String) -> ParseOutcome {
    if message == "help" {
        ParseOutcome::Help(usage())
    } else {
        ParseOutcome::Error(message)
    }
}

fn usage() -> String {
    "用法：cargo run -p voice-input-cli -- <smoke|live> <macos|linux|windows> [args]\nsmoke: cargo run -p voice-input-cli -- smoke macos --audio-file testdata/smoke.wav\nlive: cargo run -p voice-input-cli -- live windows\nLinux smoke IBus: cargo run -p voice-input-cli --features linux-ibus-smoke -- smoke linux --audio-file testdata/smoke.wav --backend ibus\nLinux live IBus: cargo run -p voice-input-cli --features linux-ibus-smoke -- live linux --backend ibus [--double-ctrl-window-ms 300] [--silence-stop-ms 1500]".to_string()
}
