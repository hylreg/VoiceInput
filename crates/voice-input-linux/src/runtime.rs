#![allow(unexpected_cfgs)]

#[cfg(target_os = "linux")]
mod linux_runtime {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, RecvTimeoutError};
    use std::sync::Arc;
    use std::io::{self, Write};
    use std::thread;
    use std::time::Duration;

    use crate::backend::{LinuxBackendKind};
    use crate::hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
    use crate::host::{LinuxHostConfig, LinuxInputMethodHost};
    use crate::recorder::LinuxMicAudioRecorder;
    use crate::tray::{spawn_linux_tray, LinuxTrayConfig};
    use voice_input_asr::{FunAsrConfig, PythonFunAsrStreamingRunner};
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
                max_recording_duration: Duration::from_secs(12),
                double_ctrl_window: Duration::from_millis(200),
                silence_stop_timeout: Duration::from_millis(900),
                show_status_item: true,
            }
        }
    }

    enum RecordingEvent {
        Chunk {
            sample_rate: u32,
            samples: Vec<i16>,
            is_final: bool,
        },
        Finished(voice_input_core::Result<Vec<u8>>),
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

    fn run_streaming_session(
        recorder: LinuxMicAudioRecorder,
        host: &LinuxInputMethodHost,
        asr: &PythonFunAsrStreamingRunner,
        silence_stop_timeout: Duration,
    ) -> Result<String> {
        const STREAM_CHUNK_INTERVAL: Duration = Duration::from_millis(200);

        let (event_tx, event_rx) = mpsc::channel::<RecordingEvent>();
        let recorder_for_thread = recorder.clone();

        thread::spawn(move || {
            let result = recorder_for_thread.record_once_with_chunks(
                STREAM_CHUNK_INTERVAL,
                silence_stop_timeout,
                |sample_rate, samples, is_final| {
                    let _ = event_tx.send(RecordingEvent::Chunk {
                        sample_rate,
                        samples,
                        is_final,
                    });
                },
            );
            let _ = event_tx.send(RecordingEvent::Finished(result));
        });

        let mut last_preview = String::new();
        let mut final_text = None;

        loop {
            match event_rx.recv_timeout(Duration::from_millis(120)) {
                Ok(RecordingEvent::Chunk {
                    sample_rate,
                    samples,
                    is_final,
                }) => match asr.stream_chunk(&samples, sample_rate, is_final) {
                    Ok(text) => {
                        let preview = text.trim().to_string();
                        if !preview.is_empty() && preview != last_preview {
                            render_preview(&preview);
                            host.update_preedit(&preview)?;
                            last_preview = preview.clone();
                        }

                        if is_final {
                            final_text = Some(preview);
                        }
                    }
                    Err(err) => {
                        eprintln!("流式预览失败：{err}");
                    }
                },
                Ok(RecordingEvent::Finished(result)) => {
                    result?;
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(voice_input_core::VoiceInputError::Audio(
                        "流式录音线程已断开".to_string(),
                    ));
                }
            }
        }

        let final_text = final_text.unwrap_or_else(|| last_preview.clone()).trim().to_string();
        if final_text.is_empty() {
            return Err(voice_input_core::VoiceInputError::Transcription(
                "FunASR 没有返回识别文本，请检查麦克风输入、录音时长或环境噪声".to_string(),
            ));
        }

        if final_text != last_preview {
            render_preview(&final_text);
            host.update_preedit(&final_text)?;
        }
        println!();
        host.commit_text(&final_text)?;
        host.end_composition()?;

        Ok(final_text)
    }

    fn render_preview(text: &str) {
        print!("\r\x1b[2K预览：{text}");
        let _ = io::stdout().flush();
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
        println!("正在预加载 FunASR 流式模型...");
        let streaming_asr = PythonFunAsrStreamingRunner::connect(config.asr.clone())?;
        println!("FunASR 流式模型预加载完成");
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
        println!("静音自动停录：{}ms", config.silence_stop_timeout.as_millis());
        println!("流式 chunk：200ms，16kHz 送入 FunASR");
        println!("说明：按一次开始录音，再按一次停止并转写");
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
            if let Err(err) = host.start_composition() {
                eprintln!("Linux 常驻输入失败：{err}");
                active.store(false, Ordering::SeqCst);
                if let Some(tray) = tray.as_ref() {
                    tray.set_recording(false);
                }
                continue;
            }

            let outcome = run_streaming_session(
                recorder.clone(),
                &host,
                &streaming_asr,
                config.silence_stop_timeout,
            );
            active.store(false, Ordering::SeqCst);

            if let Some(tray) = tray.as_ref() {
                tray.set_recording(false);
                if tray.is_quit_requested() {
                    watcher.stop();
                    break;
                }
            }

            match outcome {
                Ok(text) => {
                    println!("识别结果：{text}");
                }
                Err(err) => {
                    recorder.stop();
                    let _ = host.cancel_composition();
                    let _ = host.end_composition();
                    eprintln!("Linux 常驻输入失败：{err}");
                }
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
    use voice_input_asr::FunAsrConfig;
    use voice_input_core::{AppConfig, Result, VoiceInputError};
    use std::time::Duration;

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
                max_recording_duration: Duration::from_secs(12),
                double_ctrl_window: Duration::from_millis(200),
                silence_stop_timeout: Duration::from_millis(900),
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
