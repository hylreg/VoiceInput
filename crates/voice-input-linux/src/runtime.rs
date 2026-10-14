use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

use crate::bridge::BridgeRequest;
use crate::bridge_http::char_count;
use crate::host::LinuxInputMethodHost;
use crate::hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
use crate::ibus::{backspace_in_active_window, insert_indicator_and_save_clipboard};
use crate::live::{print_live_ready, LiveJobState};
use crate::recorder::LinuxMicAudioRecorder;
use crate::restart::RestartController;
use crate::settings::LinuxAppSettings;
use crate::system_ime::SystemImeController;
use crate::tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
use fs2::FileExt;
use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_audio::{has_peak_above, write_pcm_wav};
use voice_input_core::{AppConfig, InputMethodHost, Result, VoiceInputError};

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
        app.activation_hotkey = "DoubleCtrl".to_string();

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

/// 填充词（如“嗯。”）不注入目标窗口，直接丢弃。
fn is_filler_transcript(text: &str) -> bool {
    matches!(text, "嗯" | "嗯。")
}

/// 排空桥接队列并把每条文本注入当前光标。返回成功注入的条数。
///
/// 注入前保存剪贴板、注入后还原——`insert_text_into_active_window` 对非 ASCII
/// 文本走剪贴板粘贴路径，不还原的话用户正在复制的内容会被手机文字覆盖。
/// 这里不调 `insert_indicator_and_save_clipboard`：那会在光标处插一个 ●，
/// 而用户在手机上，看不到也不需要。
fn drain_bridge_requests(rx: &Receiver<BridgeRequest>, host: &dyn InputMethodHost) -> usize {
    let mut injected = 0;

    while let Ok(request) = rx.try_recv() {
        let saved = crate::ibus::read_clipboard_text();
        let result = host.commit_text(&request.text);
        if let Some(previous) = crate::ibus::clipboard_text_to_restore(saved) {
            if let Err(err) = crate::ibus::restore_clipboard_text(&previous) {
                eprintln!("还原剪贴板失败：{err}");
            }
        }

        // 日志只记长度与结果，不记内容——手机语音可能包含密码或私事。
        match result {
            Ok(()) => {
                injected += 1;
                println!("手机输入：已注入 {} 字", char_count(&request.text));
                // 页面可能已经断开，回票失败不是错误。
                let _ = request.reply.try_send(Ok(()));
            }
            Err(err) => {
                eprintln!("手机输入注入失败：{err}");
                let _ = request.reply.try_send(Err(err.to_string()));
            }
        }
    }

    injected
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
            let transcript = transcriber.transcribe_allow_empty(&wav)?.trim().to_string();

            if transcript.trim().is_empty() {
                eprintln!("转写结果为空");
                let _ = host.cancel_composition();
                let _ = host.end_composition();
                return Ok(false);
            }

            if is_filler_transcript(&transcript) {
                eprintln!("识别结果为填充词，已丢弃：{transcript}");
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
    let restart_requested = Arc::new(AtomicBool::new(false));
    let restart = RestartController::detect();
    let ime = SystemImeController::detect();
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
            restart,
            Arc::clone(&restart_requested),
            ime,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            false,
            8765,
            String::new(),
            None,
            LinuxAppSettings::default(),
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
        if quit_requested.load(Ordering::SeqCst) || restart_requested.load(Ordering::SeqCst) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::mpsc;
    use voice_input_core::InputMethodHost;

    #[test]
    fn filler_transcript_is_discarded() {
        assert!(is_filler_transcript("嗯。"));
        assert!(is_filler_transcript("嗯"));
        assert!(!is_filler_transcript("嗯嗯"));
        assert!(!is_filler_transcript("好的"));
        assert!(!is_filler_transcript(""));
    }

    /// 记录收到的文本，用于断言注入内容。
    #[derive(Default)]
    struct RecordingHost {
        committed: RefCell<Vec<String>>,
        fail: bool,
    }

    impl InputMethodHost for RecordingHost {
        fn start_composition(&self) -> voice_input_core::Result<()> {
            Ok(())
        }

        fn commit_text(&self, text: &str) -> voice_input_core::Result<()> {
            if self.fail {
                return Err(voice_input_core::VoiceInputError::Injection(
                    "模拟注入失败".to_string(),
                ));
            }
            self.committed.borrow_mut().push(text.to_string());
            Ok(())
        }

        fn cancel_composition(&self) -> voice_input_core::Result<()> {
            Ok(())
        }

        fn end_composition(&self) -> voice_input_core::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn drains_all_queued_requests() {
        let host = RecordingHost::default();
        let (tx, rx) = mpsc::sync_channel(8);
        let mut replies = Vec::new();
        for text in ["第一条", "第二条", "第三条"] {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.try_send(BridgeRequest {
                text: text.to_string(),
                reply: reply_tx,
            })
            .expect("入队应当成功");
            replies.push(reply_rx);
        }

        let injected = drain_bridge_requests(&rx, &host);

        assert_eq!(injected, 3);
        assert_eq!(
            host.committed.borrow().clone(),
            vec!["第一条", "第二条", "第三条"]
        );
        for reply in replies {
            assert!(reply.try_recv().expect("每条都应当回票").is_ok());
        }
    }

    #[test]
    fn empty_queue_injects_nothing() {
        let host = RecordingHost::default();
        let (_tx, rx) = mpsc::sync_channel::<BridgeRequest>(8);

        assert_eq!(drain_bridge_requests(&rx, &host), 0);
        assert!(host.committed.borrow().is_empty());
    }

    #[test]
    fn injection_failure_is_reported_but_does_not_stop_the_drain() {
        let host = RecordingHost {
            fail: true,
            ..Default::default()
        };
        let (tx, rx) = mpsc::sync_channel(8);
        let mut replies = Vec::new();
        for text in ["甲", "乙"] {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.try_send(BridgeRequest {
                text: text.to_string(),
                reply: reply_tx,
            })
            .expect("入队应当成功");
            replies.push(reply_rx);
        }

        let injected = drain_bridge_requests(&rx, &host);

        assert_eq!(injected, 0, "失败的不计入成功数");
        for reply in replies {
            assert!(
                reply.try_recv().expect("失败也要回票").is_err(),
                "失败必须回票，否则页面只能干等到超时"
            );
        }
    }

    #[test]
    fn disconnected_reply_does_not_panic() {
        // 手机提前断开时回票会失败，主循环不能因此崩溃。
        let host = RecordingHost::default();
        let (tx, rx) = mpsc::sync_channel(8);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        drop(reply_rx);
        tx.try_send(BridgeRequest {
            text: "内容".to_string(),
            reply: reply_tx,
        })
        .expect("入队应当成功");

        assert_eq!(drain_bridge_requests(&rx, &host), 1);
    }
}
