use std::time::Duration;

use crate::{
    run_live_app, settings_path, LinuxAppSettings, LinuxBackendKind, LinuxHostConfig,
    LinuxLiveAppConfig,
};

#[derive(Debug, Clone)]
pub struct LinuxLiveArgs {
    pub backend: LinuxBackendKind,
    pub activation_hotkey: Option<String>,
    pub double_press_window_ms: Option<u64>,
    pub silence_stop_ms: Option<u64>,
}

impl Default for LinuxLiveArgs {
    fn default() -> Self {
        Self {
            backend: LinuxBackendKind::IBus,
            activation_hotkey: None,
            double_press_window_ms: None,
            silence_stop_ms: None,
        }
    }
}

pub fn run_live_with_args(args: LinuxLiveArgs) -> Result<(), String> {
    let persisted_settings = LinuxAppSettings::load();
    let effective_window_ms = args
        .double_press_window_ms
        .unwrap_or(persisted_settings.double_press_window_ms);
    let effective_silence_stop_ms = args
        .silence_stop_ms
        .unwrap_or(persisted_settings.silence_stop_timeout_ms)
        .max(1500);

    println!("配置文件：{}", settings_path().display());
    println!(
        "已加载双击间隔：{}ms，生效值：{}ms",
        persisted_settings.double_press_window_ms, effective_window_ms
    );
    println!(
        "已加载静音停录：{}ms，生效值：{}ms",
        persisted_settings.silence_stop_timeout_ms, effective_silence_stop_ms
    );
    if effective_silence_stop_ms != persisted_settings.silence_stop_timeout_ms {
        println!("静音自动停录已提升到更保守的下限：1500ms");
    }

    let settings = LinuxAppSettings {
        double_press_window_ms: effective_window_ms,
        silence_stop_timeout_ms: effective_silence_stop_ms,
    };
    if let Err(err) = settings.save() {
        eprintln!("保存 Linux 配置失败：{err}");
    }

    if args.backend == LinuxBackendKind::Fcitx5 {
        return Err("Fcitx5 常驻路径还没有接入原生绑定，请先使用 --backend ibus".to_string());
    }

    #[cfg(not(feature = "ibus"))]
    if args.backend == LinuxBackendKind::IBus {
        return Err(
            "当前构建未启用 IBus 支持，请改用 `cargo run -p voice-input-linux --features ibus --bin voice-input-linux-app -- --backend ibus`"
                .to_string(),
        );
    }

    let mut config = LinuxLiveAppConfig {
        host: LinuxHostConfig {
            backend: args.backend,
            service_name: "voice-input".to_string(),
        },
        max_recording_duration: Duration::from_secs(30),
        double_press_window: Duration::from_millis(effective_window_ms),
        silence_stop_timeout: Duration::from_millis(effective_silence_stop_ms),
        ..Default::default()
    };

    if let Some(hotkey) = args.activation_hotkey.as_deref() {
        config.app.activation_hotkey = hotkey.to_string();
    }

    run_live_app(config).map_err(|err| format!("Linux 常驻应用启动失败：{err}"))
}

pub fn parse_live_args(args: Vec<String>) -> Result<LinuxLiveArgs, String> {
    let mut parsed = LinuxLiveArgs::default();
    let mut iter = args.into_iter();
    let _bin = iter.next();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--backend" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --backend 的值"))?;
                parsed.backend = parse_backend(&value)?;
            }
            "--activation-hotkey" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --activation-hotkey 的值"))?;
                parsed.activation_hotkey = Some(value);
            }
            "--double-press-window-ms" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --double-press-window-ms 的值"))?;
                parsed.double_press_window_ms = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| String::from("--double-press-window-ms 必须是整数毫秒"))?,
                );
            }
            "--silence-stop-ms" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --silence-stop-ms 的值"))?;
                parsed.silence_stop_ms = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| String::from("--silence-stop-ms 必须是整数毫秒"))?,
                );
            }
            "--help" | "-h" => return Err(String::from("help")),
            other => return Err(format!("不支持的参数：{other}")),
        }
    }

    Ok(parsed)
}

fn parse_backend(value: &str) -> Result<LinuxBackendKind, String> {
    match value.to_ascii_lowercase().as_str() {
        "ibus" => Ok(LinuxBackendKind::IBus),
        "fcitx5" | "fcitx" => Ok(LinuxBackendKind::Fcitx5),
        other => Err(format!("不支持的 Linux 后端：{other}")),
    }
}
