#![allow(unexpected_cfgs)]

#[cfg(target_os = "linux")]
mod linux_runtime {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use crate::backend::{LinuxBackendKind};
    use crate::hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
    use crate::host::{LinuxHostConfig, LinuxInputMethodHost};
    use crate::recorder::LinuxMicAudioRecorder;
    use crate::tray::{spawn_linux_tray, LinuxTrayConfig};
    use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, PythonFunAsrRunner};
    use voice_input_core::{AppConfig, AppController, MockHotkeyManager, Result};

    #[derive(Debug, Clone)]
    pub struct LinuxLiveAppConfig {
        pub app: AppConfig,
        pub host: LinuxHostConfig,
        pub asr: FunAsrConfig,
        pub max_recording_duration: Duration,
        pub show_status_item: bool,
    }

    impl Default for LinuxLiveAppConfig {
        fn default() -> Self {
            Self {
                app: AppConfig::default(),
                host: LinuxHostConfig {
                    backend: LinuxBackendKind::IBus,
                    service_name: "voice-input".to_string(),
                },
                asr: FunAsrConfig::default(),
                max_recording_duration: Duration::from_secs(12),
                show_status_item: true,
            }
        }
    }

    pub fn run_live_app(config: LinuxLiveAppConfig) -> Result<()> {
        let recorder = LinuxMicAudioRecorder::new(config.max_recording_duration);
        let recorder_for_watcher = recorder.clone();
        let active = Arc::new(AtomicBool::new(false));
        let active_for_watcher = Arc::clone(&active);
        let quit_requested = Arc::new(AtomicBool::new(false));
        let hotkey = LinuxHotkeySpec::parse(&config.app.activation_hotkey)?;
        let watcher = LinuxHotkeyWatcher::spawn(hotkey, active_for_watcher, recorder_for_watcher)?;
        let host = LinuxInputMethodHost::new(config.host.clone());
        println!("正在预加载 FunASR 模型...");
        let asr_runner = PythonFunAsrRunner::connect(config.asr.clone())?;
        println!("FunASR 模型预加载完成");
        let transcriber = LocalFunAsrTranscriber::new(config.asr, Box::new(asr_runner));
        let controller = AppController::new(
            config.app,
            Box::new(MockHotkeyManager),
            Box::new(recorder.clone()),
            Box::new(transcriber),
            Box::new(host),
        );
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
        println!("热键：{}", controller.config.activation_hotkey);
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
            let outcome = controller.process_once();
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
        pub show_status_item: bool,
    }

    impl Default for LinuxLiveAppConfig {
        fn default() -> Self {
            Self {
                app: AppConfig::default(),
                host: LinuxHostConfig::default(),
                asr: FunAsrConfig::default(),
                max_recording_duration: Duration::from_secs(12),
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
