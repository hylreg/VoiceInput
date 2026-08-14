use std::env;
use std::path::PathBuf;

use voice_input_linux::{parse_live_args, run_live_with_args, run_smoke};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();
    let command = match parse_command(args) {
        Ok(cmd) => cmd,
        Err(ParseOutcome::Help(msg)) => {
            eprintln!("{msg}");
            return 0;
        }
        Err(ParseOutcome::Error(msg)) => {
            eprintln!("{msg}");
            eprintln!("{}", usage());
            return 2;
        }
    };

    let result = match command {
        Command::Smoke { audio_file } => run_smoke(audio_file),
        Command::Live(args) => run_live_with_args(args),
    };

    match result {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("{msg}");
            1
        }
    }
}

enum Command {
    Smoke { audio_file: PathBuf },
    Live(voice_input_linux::LinuxLiveArgs),
}

enum ParseOutcome {
    Help(String),
    Error(String),
}

fn parse_command(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut iter = args.into_iter();
    let _bin = iter.next();

    let Some(subcommand) = iter.next() else {
        return Err(ParseOutcome::Error("缺少子命令".to_string()));
    };

    if matches!(subcommand.as_str(), "--help" | "-h" | "help") {
        return Err(ParseOutcome::Help(usage()));
    }

    match subcommand.to_ascii_lowercase().as_str() {
        "smoke" => parse_smoke_args(iter.collect()),
        "live" => parse_live_subcommand(iter.collect()),
        other => Err(ParseOutcome::Error(format!("不支持的子命令：{other}"))),
    }
}

fn parse_smoke_args(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut iter = args.into_iter();
    let mut audio_file = None;

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--audio-file" => {
                let value = iter
                    .next()
                    .ok_or_else(|| ParseOutcome::Error("缺少 --audio-file 的值".to_string()))?;
                audio_file = Some(PathBuf::from(value));
            }
            "--backend" => {
                let value = iter
                    .next()
                    .ok_or_else(|| ParseOutcome::Error("缺少 --backend 的值".to_string()))?;
                voice_input_linux::validate_backend(&value).map_err(ParseOutcome::Error)?;
            }
            "--help" | "-h" => return Err(ParseOutcome::Help(usage())),
            other => return Err(ParseOutcome::Error(format!("不支持的参数：{other}"))),
        }
    }

    let audio_file = audio_file
        .ok_or_else(|| ParseOutcome::Error("缺少必需参数 --audio-file".to_string()))?;
    Ok(Command::Smoke { audio_file })
}

fn parse_live_subcommand(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut forwarded = vec!["voice-input-linux-live".to_string()];
    forwarded.extend(args);
    let live_args = parse_live_args(forwarded).map_err(|msg| {
        if msg == "help" {
            ParseOutcome::Help(usage())
        } else {
            ParseOutcome::Error(msg)
        }
    })?;
    Ok(Command::Live(live_args))
}

fn usage() -> String {
    concat!(
        "用法：cargo run -p voice-input-linux -- <smoke|live> [args]\n",
        "\n",
        "smoke: cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav [--backend ibus]\n",
        "live:  cargo run -p voice-input-linux --features ibus -- live [--backend ibus] [--activation-hotkey DoubleAlt] [--double-press-window-ms 300] [--silence-stop-ms 1500]\n",
    )
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_smoke_args_reads_audio_file_path() {
        let cmd = match parse_smoke_args(vec![
            "--audio-file".to_string(),
            "x.wav".to_string(),
        ]) {
            Ok(cmd) => cmd,
            Err(_) => panic!("smoke 参数应当解析成功"),
        };
        match cmd {
            Command::Smoke { audio_file } => assert_eq!(audio_file, PathBuf::from("x.wav")),
            _ => panic!("expected Smoke command"),
        }
    }

    #[test]
    fn parse_smoke_args_requires_audio_file() {
        match parse_smoke_args(vec![]) {
            Err(ParseOutcome::Error(msg)) => {
                assert!(
                    msg.contains("缺少必需参数 --audio-file"),
                    "unexpected error: {msg}"
                );
            }
            _ => panic!("expected Error outcome"),
        }
    }

    #[test]
    fn parse_smoke_args_rejects_unknown_arg() {
        match parse_smoke_args(vec!["--bogus".to_string()]) {
            Err(ParseOutcome::Error(msg)) => {
                assert!(msg.contains("不支持的参数"), "unexpected error: {msg}");
            }
            _ => panic!("expected Error outcome"),
        }
    }

    #[test]
    fn parse_smoke_args_rejects_fcitx5_backend() {
        match parse_smoke_args(vec![
            "--audio-file".to_string(),
            "x.wav".to_string(),
            "--backend".to_string(),
            "fcitx5".to_string(),
        ]) {
            Err(ParseOutcome::Error(msg)) => {
                assert!(
                    msg.contains("Fcitx5 路径还没有接入原生绑定"),
                    "unexpected error: {msg}"
                );
            }
            _ => panic!("expected Error outcome"),
        }
    }
}
