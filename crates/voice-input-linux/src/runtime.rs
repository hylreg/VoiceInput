use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::host::LinuxInputMethodHost;
use crate::hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
use crate::recorder::LinuxMicAudioRecorder;
use crate::tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
use fs2::FileExt;
use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{AppConfig, InputMethodHost, Result, VoiceInputError};
use voice_input_audio::{has_peak_above, write_pcm_wav};
use crate::live::{print_live_ready, LiveJobState};
use crate::ibus::{backspace_in_active_window, insert_indicator_and_save_clipboard};

#[derive(Debug, Clone)]
pub struct LinuxLiveAppConfig {
    pub app: AppConfig,
    pub asr: FunAsrConfig,
    pub max_recording_duration: Duration,
    pub double_press_window: Duration,
    pub silence_stop_timeout: Duration,
    pub show_status_item: bool,
    pub service_name: String,
}

impl Default for LinuxLiveAppConfig {
    fn default() -> Self {
        let mut app = AppConfig::default();
        app.activation_hotkey = "DoubleAlt".to_string();

        Self {
            app,
            asr: FunAsrConfig::from_env(),
            max_recording_duration: Duration::from_secs(30),
            double_press_window: Duration::from_millis(300),
            silence_stop_timeout: Duration::from_millis(1500),
            show_status_item: true,
            service_name: "voice-input".to_string(),
        }
    }
}

struct SingleInstanceGuard {
    _lock_file: File,
}

impl SingleInstanceGuard {
    fn acquire() -> Result<Option<Self>> {
        let lock_path = std::env::temp_dir().join("voiceinput-linux.lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|e| {
                VoiceInputError::Injection(format!(
                    "创建 Linux 单实例锁失败 {}：{e}",
                    lock_path.display()
                ))
            })?;

        match lock_file.try_lock_exclusive() {
            Ok(()) => {
                let mut lock_file_for_pid = lock_file;
                lock_file_for_pid.set_len(0).map_err(|e| {
                    VoiceInputError::Injection(format!("清空 Linux 单实例锁失败：{e}"))
                })?;
                lock_file_for_pid
                    .write_all(format!("pid={}\n", std::process::id()).as_bytes())
                    .map_err(|e| {
                        VoiceInputError::Injection(format!("写入 Linux 单实例锁失败：{e}"))
                    })?;
                Ok(Some(Self {
                    _lock_file: lock_file_for_pid,
                }))
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(err) => Err(VoiceInputError::Injection(format!(
                "获取 Linux 单实例锁失败 {}：{err}",
                lock_path.display()
            ))),
        }
    }
}

fn build_linux_asr(config: &FunAsrConfig) -> Result<Box<dyn FunAsrRunner>> {
    let runner = PythonFunAsrRunner::connect(config.clone())?;
    Ok(Box::new(runner))
}

fn run_recording_cycle(
    recorder: &LinuxMicAudioRecorder,
    host: &LinuxInputMethodHost,
    transcriber: &LocalFunAsrTranscriber,
    silence_stop_timeout: Duration,
    tray: Option<&LinuxTrayHandle>,
    watcher: &LinuxHotkeyWatcher,
) -> Result<bool> {
    if let Some(tray) = tray {
        tray.set_recording(true);
    }

    // 在光标处显示录音提示符，标记输入法 composition 开始
    // IBus 协议 preedit 不可用（VoiceInput 不是注册引擎），走 xdotool 直接输入
    host.start_composition()?;
    let _restore_clipboard = insert_indicator_and_save_clipboard()?;

    println!("正在录音...");
    let silence_stop_enabled = Arc::new(AtomicBool::new(true));
    let audio = recorder.record_once_with_chunks(
        Duration::from_millis(100),
        silence_stop_timeout,
        Arc::clone(&silence_stop_enabled),
        |_, _, _| {},
    );

    // 移除录音提示符
    let _ = backspace_in_active_window(1, None);

    if let Some(tray) = tray {
        tray.set_recording(false);
        if tray.is_quit_requested() {
            let _ = host.cancel_composition();
            let _ = host.end_composition();
            watcher.stop();
            return Ok(true);
        }
    }

    match audio {
        Ok(audio) => {
            // 峰值 > 800（约满量程 2.4%）认为有有效语音，
            // 对抗 AGC 底噪，避免对噪声幻觉转写。
            if !has_peak_above(&audio.samples, 800) {
                eprintln!("录音中未检测到有效语音，跳过转写");
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                return Ok(false);
            }

            let wav = write_pcm_wav(&audio.samples, audio.sample_rate)?;
            let transcript = transcriber
                .transcribe_allow_empty(&wav)?
                .trim()
                .to_string();

            if transcript.trim().is_empty() {
                eprintln!("转写结果为空");
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                return Ok(false);
            }

            println!("识别结果：{transcript}");

            if let Err(err) = host.commit_text(&transcript) {
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                return Err(err);
            }
            host.end_composition()?;
        }
        Err(err) => {
            if let Some(tray) = tray {
                tray.set_recording(false);
            }
            let _ = host.cancel_composition();
            let _ = host.end_composition();
            eprintln!("Linux 常驻输入失败：{err}");
        }
    }

    Ok(false)
}

pub fn run_live_app(config: LinuxLiveAppConfig) -> Result<()> {
    let Some(_instance_guard) = SingleInstanceGuard::acquire()? else {
        return Err(VoiceInputError::Injection(
            "检测到已有 Linux 常驻实例正在运行，请先退出旧实例后再启动".to_string(),
        ));
    };

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
        config.double_press_window,
    )?;
    let host = LinuxInputMethodHost::new();
    println!("正在预加载 ASR 模型...");
    let asr_runner = build_linux_asr(&config.asr)?;
    let transcriber = LocalFunAsrTranscriber::new(config.asr.clone(), asr_runner);
    println!("ASR 模型预加载完成");
    let tray = if config.show_status_item {
        let tray = spawn_linux_tray(LinuxTrayConfig::new(
            config.service_name.clone(),
            "VoiceInput".to_string(),
            recorder.clone(),
            Arc::clone(&quit_requested),
        ))?;
        tray.set_recording(false);
        Some(tray)
    } else {
        None
    };

    let hotkey_label = hotkey.describe(&activation_hotkey, config.double_press_window);
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
            format!("双击间隔：{}ms", config.double_press_window.as_millis()),
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

        let Some(_job) = LiveJobState::try_acquire(&active) else {
            continue;
        };

        if run_recording_cycle(
            &recorder,
            &host,
            &transcriber,
            config.silence_stop_timeout,
            tray.as_ref(),
            &watcher,
        )? {
            break;
        }
    }

    if let Some(tray) = tray.as_ref() {
        tray.shutdown();
    }

    Ok(())
}
