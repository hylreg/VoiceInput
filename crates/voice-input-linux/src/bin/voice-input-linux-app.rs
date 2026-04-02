use std::env;
use std::time::Duration;

use voice_input_linux::{
    run_live_app, settings_path, LinuxAppSettings, LinuxBackendKind, LinuxHostConfig,
    LinuxLiveAppConfig,
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

    let persisted_settings = LinuxAppSettings::load();
    let effective_window_ms = args
        .double_ctrl_window_ms
        .unwrap_or(persisted_settings.double_ctrl_window_ms);

    println!("配置文件：{}", settings_path().display());
    println!(
        "已加载双击间隔：{}ms，生效值：{}ms",
        persisted_settings.double_ctrl_window_ms, effective_window_ms
    );

    let config = LinuxLiveAppConfig {
        host: LinuxHostConfig {
            backend: args.backend,
            service_name: "voice-input".to_string(),
        },
        max_recording_duration: Duration::from_secs(12),
        double_ctrl_window: Duration::from_millis(effective_window_ms),
        ..Default::default()
    };

    let settings = LinuxAppSettings {
        double_ctrl_window_ms: effective_window_ms,
    };

    if let Err(err) = settings.save() {
        eprintln!("保存 Linux 配置失败：{err}");
    }

    if args.backend == LinuxBackendKind::Fcitx5 {
        eprintln!("Fcitx5 常驻路径还没有接入原生绑定，请先使用 --backend ibus");
        std::process::exit(2);
    }

    #[cfg(not(feature = "ibus"))]
    if args.backend == LinuxBackendKind::IBus {
        eprintln!(
            "当前构建未启用 IBus 支持，请改用 `cargo run -p voice-input-linux --features ibus --bin voice-input-linux-app -- --backend ibus`"
        );
        std::process::exit(2);
    }

    if let Err(err) = run_live_app(config) {
        eprintln!("Linux 常驻应用启动失败：{err}");
        std::process::exit(1);
    }
}

#[derive(Debug)]
struct Args {
    backend: LinuxBackendKind,
    double_ctrl_window_ms: Option<u64>,
}

impl Args {
    fn parse(args: Vec<String>) -> Result<Self, String> {
        let mut backend = LinuxBackendKind::IBus;
        let mut double_ctrl_window_ms = None;
        let mut iter = args.into_iter();
        let _bin = iter.next();

        while let Some(arg) = iter.next() {
            match arg.as_str() {
                "--backend" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| String::from("缺少 --backend 的值"))?;
                    backend = parse_backend(&value)?;
                }
                "--double-ctrl-window-ms" => {
                    let value = iter
                        .next()
                        .ok_or_else(|| String::from("缺少 --double-ctrl-window-ms 的值"))?;
                    double_ctrl_window_ms = Some(value
                        .parse::<u64>()
                        .map_err(|_| String::from("--double-ctrl-window-ms 必须是整数毫秒"))?);
                }
                "--help" | "-h" => return Err(String::from("help")),
                other => return Err(format!("不支持的参数：{other}")),
            }
        }

        Ok(Self {
            backend,
            double_ctrl_window_ms,
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

fn print_usage() {
    eprintln!(
        "用法：cargo run -p voice-input-linux --bin voice-input-linux-app -- --backend ibus [--double-ctrl-window-ms 200]"
    );
}
