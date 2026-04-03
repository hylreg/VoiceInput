#![allow(unexpected_cfgs)]

#[cfg(target_os = "linux")]
mod linux_runtime {
    use std::env;
    use std::process::Command;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use crate::backend::LinuxBackendKind;
    use crate::host::{LinuxHostConfig, LinuxInputMethodHost};
    use crate::hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
    use crate::ibus::backspace_in_active_window;
    use crate::recorder::LinuxMicAudioRecorder;
    use crate::tray::{spawn_linux_tray, LinuxTrayConfig};
    use voice_input_asr::{
        FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber, PythonFunAsrRunner,
        SocketFunAsrStreamingRunner,
    };
    use voice_input_core::{AppConfig, InputMethodHost, Result};

    #[derive(Debug, Clone)]
    pub struct LinuxLiveAppConfig {
        pub app: AppConfig,
        pub host: LinuxHostConfig,
        pub asr: FunAsrConfig,
        pub max_recording_duration: Duration,
        pub double_ctrl_window: Duration,
        pub silence_stop_timeout: Duration,
        pub show_status_item: bool,
    }

    impl Default for LinuxLiveAppConfig {
        fn default() -> Self {
            let mut app = AppConfig::default();
            app.activation_hotkey = "DoubleCtrlStrict".to_string();

            Self {
                app,
                host: LinuxHostConfig {
                    backend: LinuxBackendKind::IBus,
                    service_name: "voice-input".to_string(),
                },
                asr: FunAsrConfig::default(),
                max_recording_duration: Duration::from_secs(30),
                double_ctrl_window: Duration::from_millis(300),
                silence_stop_timeout: Duration::from_millis(1500),
                show_status_item: true,
            }
        }
    }

    fn describe_activation_hotkey(spec: &str, double_ctrl_window: Duration) -> String {
        if spec.eq_ignore_ascii_case("doublectrl")
            || spec.eq_ignore_ascii_case("double-ctrl")
            || spec.eq_ignore_ascii_case("double_ctrl")
            || spec.eq_ignore_ascii_case("doublectrlstrict")
            || spec.eq_ignore_ascii_case("double-ctrl-strict")
            || spec.eq_ignore_ascii_case("double_ctrl_strict")
        {
            format!("双击 Ctrl（严格，{}ms）", double_ctrl_window.as_millis())
        } else {
            spec.to_string()
        }
    }

    fn recording_indicator_text(preview: Option<&str>) -> String {
        match preview {
            Some(text) if !text.trim().is_empty() => format!("录音中 {}", text.trim()),
            _ => "录音中".to_string(),
        }
    }

    const RECORDING_MARKER: &str = "[REC]";

    fn type_recording_marker() -> Result<()> {
        let status = Command::new("xdotool")
            .args(["type", "--clearmodifiers", "--delay", "0", RECORDING_MARKER])
            .status()
            .map_err(|e| voice_input_core::VoiceInputError::Injection(format!(
                "调用 xdotool 失败：{e}"
            )))?;

        if !status.success() {
            return Err(voice_input_core::VoiceInputError::Injection(format!(
                "xdotool 输入失败，退出码：{status}"
            )));
        }

        Ok(())
    }

    pub fn run_live_app(config: LinuxLiveAppConfig) -> Result<()> {
        let recorder = LinuxMicAudioRecorder::new(config.max_recording_duration);
        let recorder_for_watcher = recorder.clone();
        let active = Arc::new(AtomicBool::new(false));
        let active_for_watcher = Arc::clone(&active);
        let quit_requested = Arc::new(AtomicBool::new(false));
        let activation_hotkey = config.app.activation_hotkey.clone();
        let hotkey = LinuxHotkeySpec::parse(&activation_hotkey)?;
        let watcher = LinuxHotkeyWatcher::spawn(
            hotkey,
            active_for_watcher,
            recorder_for_watcher,
            config.double_ctrl_window,
        )?;
        let host = LinuxInputMethodHost::new(config.host.clone());
        println!("正在预加载 FunASR 模型...");
        let asr_runner: Box<dyn FunAsrRunner> =
            if let Ok(socket_path) = env::var("VOICEINPUT_FUNASR_SOCKET") {
                println!("检测到外部 FunASR 调试服务：{socket_path}");
                Box::new(SocketFunAsrStreamingRunner::connect(
                    socket_path,
                    config.asr.clone(),
                )?)
            } else {
                Box::new(PythonFunAsrRunner::connect(config.asr.clone())?)
            };
        let transcriber = LocalFunAsrTranscriber::new(config.asr.clone(), asr_runner);
        println!("FunASR 模型预加载完成");
        let tray = if config.show_status_item {
            let tray = spawn_linux_tray(LinuxTrayConfig::new(
                config.host.service_name.clone(),
                "VoiceInput".to_string(),
                recorder.clone(),
                Arc::clone(&quit_requested),
            ))?;
            tray.set_recording(false);
            Some(tray)
        } else {
            None
        };

        println!("VoiceInput Linux 常驻应用已启动");
        println!(
            "热键：{}",
            describe_activation_hotkey(&activation_hotkey, config.double_ctrl_window)
        );
        println!("双击间隔：{}ms", config.double_ctrl_window.as_millis());
        println!(
            "静音自动停录：{}ms",
            config.silence_stop_timeout.as_millis()
        );
        println!("说明：双击一次开始录音，再双击一次停止并转写");
        if config.show_status_item {
            println!("状态提示：已启用");
        }

        loop {
            if quit_requested.load(Ordering::SeqCst) {
                watcher.stop();
                break;
            }

            let triggered = watcher.wait_for_trigger_timeout(Duration::from_millis(250))?;
            if !triggered {
                continue;
            }

            if active
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                continue;
            }

            if let Some(tray) = tray.as_ref() {
                tray.set_recording(true);
            }

            println!("正在录音...");
            let silence_stop_enabled = Arc::new(AtomicBool::new(false));
            let mut recording_indicator_inserted = false;
            if let Err(err) = host.start_composition() {
                eprintln!("Linux 常驻输入失败：{err}");
                active.store(false, Ordering::SeqCst);
                if let Some(tray) = tray.as_ref() {
                    tray.set_recording(false);
                }
                continue;
            }

            if let Err(err) = type_recording_marker() {
                eprintln!("Linux 常驻输入失败：录音状态图标插入失败：{err}");
            } else {
                recording_indicator_inserted = true;
            }

            if let Err(err) = host.update_preedit(&recording_indicator_text(None)) {
                eprintln!("Linux 常驻输入失败：录音状态图标更新失败：{err}");
            }

            let audio = match recorder.record_once_with_chunks(
                Duration::from_millis(100),
                config.silence_stop_timeout,
                Arc::clone(&silence_stop_enabled),
                |_, _, _| {},
            ) {
                Ok(audio) => audio,
                Err(err) => {
                    if recording_indicator_inserted {
                        let _ = backspace_in_active_window(RECORDING_MARKER.chars().count());
                    }
                    active.store(false, Ordering::SeqCst);
                    if let Some(tray) = tray.as_ref() {
                        tray.set_recording(false);
                    }
                    let _ = host.cancel_composition();
                    let _ = host.end_composition();
                    eprintln!("Linux 常驻输入失败：{err}");
                    continue;
                }
            };
            active.store(false, Ordering::SeqCst);

            if let Some(tray) = tray.as_ref() {
                tray.set_recording(false);
                if tray.is_quit_requested() {
                    watcher.stop();
                    break;
                }
            }

            let transcript = match transcriber.transcribe_allow_empty(&audio) {
                Ok(text) => text.trim().to_string(),
                Err(err) => {
                    if recording_indicator_inserted {
                        let _ = backspace_in_active_window(RECORDING_MARKER.chars().count());
                    }
                    let _ = host.cancel_composition();
                    let _ = host.end_composition();
                    eprintln!("Linux 常驻输入失败：转写错误：{err}");
                    continue;
                }
            };

            if transcript.trim().is_empty() {
                if recording_indicator_inserted {
                    let _ = backspace_in_active_window(RECORDING_MARKER.chars().count());
                }
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                eprintln!("Linux 常驻输入失败：转写结果为空");
                continue;
            }

            println!("识别结果：{transcript}");

            if let Err(err) = host.update_preedit(&recording_indicator_text(Some(&transcript))) {
                if recording_indicator_inserted {
                    let _ = backspace_in_active_window(RECORDING_MARKER.chars().count());
                }
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                eprintln!("Linux 常驻输入失败：预编辑更新失败：{err}");
                continue;
            }

            if recording_indicator_inserted {
                if let Err(err) = backspace_in_active_window(RECORDING_MARKER.chars().count()) {
                    let _ = host.cancel_composition();
                    let _ = host.end_composition();
                    eprintln!("Linux 常驻输入失败：清理录音图标失败：{err}");
                    continue;
                }
            }

            if let Err(err) = host.commit_text(&transcript) {
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                eprintln!("Linux 常驻输入失败：提交失败：{err}");
                continue;
            }

            if let Err(err) = host.end_composition() {
                eprintln!("Linux 常驻输入失败：结束输入失败：{err}");
            }
        }

        if let Some(tray) = tray.as_ref() {
            tray.shutdown();
        }

        Ok(())
    }
}

#[cfg(target_os = "linux")]
pub use linux_runtime::{run_live_app, LinuxLiveAppConfig};

#[cfg(not(target_os = "linux"))]
mod not_linux {
    use crate::host::LinuxHostConfig;
    use std::time::Duration;
    use voice_input_asr::FunAsrConfig;
    use voice_input_core::{AppConfig, Result, VoiceInputError};

    #[derive(Debug, Clone)]
    pub struct LinuxLiveAppConfig {
        pub app: AppConfig,
        pub host: LinuxHostConfig,
        pub asr: FunAsrConfig,
        pub max_recording_duration: Duration,
        pub double_ctrl_window: Duration,
        pub silence_stop_timeout: Duration,
        pub show_status_item: bool,
    }

    impl Default for LinuxLiveAppConfig {
        fn default() -> Self {
            Self {
                app: AppConfig::default(),
                host: LinuxHostConfig::default(),
                asr: FunAsrConfig::default(),
                max_recording_duration: Duration::from_secs(30),
                double_ctrl_window: Duration::from_millis(300),
                silence_stop_timeout: Duration::from_millis(1500),
                show_status_item: false,
            }
        }
    }

    pub fn run_live_app(_config: LinuxLiveAppConfig) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Linux 常驻应用只支持 Linux".to_string(),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub use not_linux::{run_live_app, LinuxLiveAppConfig};
