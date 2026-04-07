#![allow(unexpected_cfgs)]

#[cfg(target_os = "linux")]
mod linux_runtime {
    use std::cell::{Cell, RefCell};
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
    use crate::tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
    use voice_input_asr::{
        FunAsrConfig, FunAsrRunner, FunAsrStreamingRunner, LocalFunAsrTranscriber,
        PythonFunAsrRunner, PythonFunAsrStreamingRunner, SocketFunAsrStreamingRunner,
    };
    use voice_input_core::{AppConfig, Result};
    use voice_input_runtime::{
        print_live_ready, run_streaming_live_cycle, stream_preview_chunk, LiveJobHandle,
        LiveJobState,
    };

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
            app.activation_hotkey = "Ctrl+Shift+Space".to_string();

            Self {
                app,
                host: LinuxHostConfig {
                    backend: LinuxBackendKind::IBus,
                    service_name: "voice-input".to_string(),
                },
                asr: FunAsrConfig::from_env(),
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

    const RECORDING_MARKER: &str = "●";

    fn type_recording_marker() -> Result<()> {
        let status = Command::new("xdotool")
            .args(["type", "--clearmodifiers", "--delay", "0", RECORDING_MARKER])
            .status()
            .map_err(|e| {
                voice_input_core::VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))
            })?;

        if !status.success() {
            return Err(voice_input_core::VoiceInputError::Injection(format!(
                "xdotool 输入失败，退出码：{status}"
            )));
        }

        Ok(())
    }

    fn build_linux_asr(
        config: &FunAsrConfig,
    ) -> Result<(
        Box<dyn FunAsrRunner>,
        Option<Box<dyn FunAsrStreamingRunner>>,
    )> {
        if let Ok(socket_path) = env::var("VOICEINPUT_FUNASR_SOCKET") {
            println!("检测到外部 ASR 调试服务：{socket_path}");
            let runner = SocketFunAsrStreamingRunner::connect(socket_path, config.clone())?;
            return Ok((Box::new(runner.clone()), Some(Box::new(runner))));
        }

        if config.is_qwen() {
            return Ok((Box::new(PythonFunAsrRunner::connect(config.clone())?), None));
        }

        let runner = PythonFunAsrRunner::connect(config.clone())?;
        let streaming_runner = PythonFunAsrStreamingRunner::connect(config.clone())?;
        Ok((Box::new(runner), Some(Box::new(streaming_runner))))
    }

    fn run_recording_cycle(
        recorder: &LinuxMicAudioRecorder,
        host: &LinuxInputMethodHost,
        transcriber: &LocalFunAsrTranscriber,
        preview_runner: Option<&dyn FunAsrStreamingRunner>,
        silence_stop_timeout: Duration,
        tray: Option<&LinuxTrayHandle>,
        watcher: &LinuxHotkeyWatcher,
        _job: LiveJobHandle,
    ) -> Result<bool> {
        if let Some(tray) = tray {
            tray.set_recording(true);
        }

        println!("正在录音...");
        let silence_stop_enabled = Arc::new(AtomicBool::new(true));
        let recording_indicator_inserted = Cell::new(false);
        let preview_error = RefCell::new(None::<String>);
        let result = run_streaming_live_cycle(
            host,
            transcriber,
            preview_runner,
            recording_indicator_text,
            |session, preview_runner| {
                if let Err(err) = type_recording_marker() {
                    eprintln!("Linux 常驻输入失败：录音状态图标插入失败：{err}");
                } else {
                    recording_indicator_inserted.set(true);
                }

                let audio = recorder.record_once_with_chunks(
                    Duration::from_millis(100),
                    silence_stop_timeout,
                    Arc::clone(&silence_stop_enabled),
                    |sample_rate, samples, is_final| {
                        let Some(preview_runner) = preview_runner else {
                            return;
                        };
                        if let Err(err) = stream_preview_chunk(
                            preview_runner,
                            session,
                            sample_rate,
                            &samples,
                            is_final,
                        ) {
                            *preview_error.borrow_mut() = Some(format!("流式预览失败：{err}"));
                        }
                    },
                );
                audio
            },
            || {
                if let Some(err) = preview_error.borrow_mut().take() {
                    return Err(voice_input_core::VoiceInputError::Transcription(err));
                }

                if let Some(tray) = tray {
                    tray.set_recording(false);
                    if tray.is_quit_requested() {
                        watcher.stop();
                    }
                }

                if recording_indicator_inserted.get() {
                    backspace_in_active_window(RECORDING_MARKER.chars().count())?;
                }
                Ok(())
            },
        );

        match result {
            Ok(transcript) => {
                if let Some(tray) = tray {
                    tray.set_recording(false);
                    if tray.is_quit_requested() {
                        watcher.stop();
                        return Ok(true);
                    }
                }
                println!("识别结果：{transcript}");
            }
            Err(err) => {
                if let Some(tray) = tray {
                    tray.set_recording(false);
                }
                eprintln!("Linux 常驻输入失败：{err}");
            }
        }

        Ok(false)
    }

    pub fn run_live_app(config: LinuxLiveAppConfig) -> Result<()> {
        let recorder = LinuxMicAudioRecorder::new(config.max_recording_duration);
        let recorder_for_watcher = recorder.clone();
        let active = Arc::new(LiveJobState::default());
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
        println!("正在预加载 ASR 模型...");
        let (asr_runner, preview_runner) = build_linux_asr(&config.asr)?;
        let transcriber = LocalFunAsrTranscriber::new(config.asr.clone(), asr_runner);
        println!("ASR 模型预加载完成");
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

        let hotkey_label =
            describe_activation_hotkey(&activation_hotkey, config.double_ctrl_window);
        let silence_label = format!(
            "静音自动停录：{}ms",
            config.silence_stop_timeout.as_millis()
        );
        let status_label = if config.show_status_item {
            Some("状态提示：已启用".to_string())
        } else {
            None
        };
        print_live_ready(
            "Linux",
            &hotkey_label,
            "双击一次开始录音，再双击一次停止并转写",
            [
                format!("双击间隔：{}ms", config.double_ctrl_window.as_millis()),
                silence_label,
            ]
            .into_iter()
            .chain(status_label.into_iter()),
        );

        loop {
            if quit_requested.load(Ordering::SeqCst) {
                watcher.stop();
                break;
            }

            let triggered = watcher.wait_for_trigger_timeout(Duration::from_millis(250))?;
            if !triggered {
                continue;
            }

            let Some(job) = LiveJobState::try_acquire(&active) else {
                continue;
            };

            if run_recording_cycle(
                &recorder,
                &host,
                &transcriber,
                preview_runner.as_deref(),
                config.silence_stop_timeout,
                tray.as_ref(),
                &watcher,
                job,
            )? {
                break;
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
