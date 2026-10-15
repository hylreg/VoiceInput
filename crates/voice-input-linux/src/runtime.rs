use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::bridge::{spawn_bridge, BridgeConfig, BridgeRequest, BridgeStatus, QUEUE_CAPACITY};
use crate::bridge_addr::{
    detect_local_ipv4, generate_pass_phrase, normalize_pass_phrase, MAX_PASS_PHRASE_LEN,
    MIN_PASS_PHRASE_LEN,
};
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
    pub bridge_port: u16,
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
            bridge_port: 8765,
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
///
/// 过期的请求（`deadline` 已过）不注入：主循环在录音里可能一卡就是 30s，
/// 期间入队的请求其 HTTP 客户端早已收到 503 并让用户重发，过期后仍注入
/// 就会与用户重发的那条重复。过期的照常回 `Err(MSG_TIMEOUT)` 保持协议统一。
fn drain_bridge_requests(rx: &Receiver<BridgeRequest>, host: &dyn InputMethodHost) -> usize {
    let mut injected = 0;

    while let Ok(request) = rx.try_recv() {
        if Instant::now() >= request.deadline {
            // 日志只记长度与结果，不记内容——手机语音可能包含密码或私事。
            println!("手机输入：丢弃过期请求（{} 字）", char_count(&request.text));
            let _ = request
                .reply
                .try_send(Err(crate::bridge_http::MSG_TIMEOUT.to_string()));
            continue;
        }

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

/// 桥接启动所需的外部输入。抽成结构体是为了能在单测里构造。
#[derive(Debug, Clone)]
pub struct BridgeSetupConfig {
    pub enabled_in_settings: bool,
    pub port: u16,
    pub stored_token: String,
    pub saved_settings: LinuxAppSettings,
}

/// 桥接的启动参数与初始状态。
#[derive(Debug, Clone)]
pub struct BridgeSetup {
    pub token: String,
    /// token 是本次新生成的（配置里原本没有）。调用方据此决定要不要落盘。
    pub token_is_new: bool,
    pub settings: LinuxAppSettings,
}

/// 决定口令与生效 settings。
///
/// 口令缺失或非法时生成一个新的并写回 settings。**`run_live_app` 必须在
/// `token_is_new` 为真时把它落盘**——落盘不在本函数里，因为本函数是纯函数、
/// 要能单测；少了那一步，每次重启都会换一个新口令，手机上存的地址全部失效。
pub fn prepare_bridge(config: BridgeSetupConfig) -> BridgeSetup {
    // 盘上的值可能非法：用户手改过配置，或者存的是别的什么。旧版的 32 位十六进制
    // 属于合法字符集（`[0-9a-f]`），归一化后原样沿用，升级不会换口令。
    let stored = normalize_pass_phrase(&config.stored_token);
    let token_is_new = stored.is_none();
    if token_is_new && !config.stored_token.trim().is_empty() {
        // 只在「写了东西但不合法」时提示。空串是正常的首次启动。
        eprintln!(
            "配置里的手机输入口令不合法（只允许 {MIN_PASS_PHRASE_LEN}-{MAX_PASS_PHRASE_LEN} \
             位 ASCII 字母数字与 -_），已重新生成"
        );
    }
    let token = stored.unwrap_or_else(generate_pass_phrase);

    let mut settings = config.saved_settings;
    settings.bridge_token = token.clone();
    settings.bridge_port = config.port;
    // `enabled_in_settings` 参数保留为显式兜底：settings 里的开关是权威值，
    // 但调用方若显式传入 false（例如 --no-bridge），以调用方为准。
    if !config.enabled_in_settings {
        settings.bridge_enabled = false;
    }

    BridgeSetup {
        token,
        token_is_new,
        settings,
    }
}

fn run_recording_cycle(
    recorder: &LinuxMicAudioRecorder,
    host: &dyn InputMethodHost,
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
    // ---- 桥接 ----
    let saved_settings = LinuxAppSettings::load();
    let setup = prepare_bridge(BridgeSetupConfig {
        enabled_in_settings: saved_settings.bridge_enabled,
        port: config.bridge_port,
        stored_token: saved_settings.bridge_token.clone(),
        saved_settings,
    });
    // 新生成的 token 必须立刻落盘。`live_cli` 那次 save() 发生在 `prepare_bridge`
    // 之前，写下的还是空串；少了这一处，每次重启都会换一个新 token，手机上存的
    // 地址和书签全部失效——这正是 `prepare_bridge` 约定要落盘的那份状态。
    // 只在真的生成了新 token 时写盘，避免每次启动都无谓地 fsync 一次配置目录。
    if setup.token_is_new {
        if let Err(err) = setup.settings.save() {
            // 不升级为启动失败：token 本次仍然可用，只是下次重启会再换一个。
            eprintln!("保存 Linux 配置失败：{err}");
        }
    }
    let mic_enabled = Arc::new(AtomicBool::new(setup.settings.mic_enabled));
    let bridge_enabled = Arc::new(AtomicBool::new(setup.settings.bridge_enabled));

    let bridge_status = Arc::new(BridgeStatus::new(
        Arc::clone(&bridge_enabled),
        setup.token.clone(),
        recorder.clone(),
    ));
    let (bridge_tx, bridge_rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
    let bridge = match spawn_bridge(
        BridgeConfig {
            port: config.bridge_port,
        },
        Arc::clone(&bridge_status),
        bridge_tx,
    ) {
        Ok(handle) => Some(handle),
        Err(err) => {
            // 端口绑定失败不影响主程序启动，退化为「无手机输入」。
            eprintln!("{err}");
            eprintln!("手机输入已禁用；本机麦克风不受影响");
            None
        }
    };

    let local_ip = detect_local_ipv4().map(|ip| ip.to_string());
    if let Some(ref handle) = bridge {
        // 只打端口，不打地址——地址里带口令，而 journal 是落盘的。
        // 需要地址时从托盘菜单「复制外部输入地址」取。
        println!("手机输入已就绪，监听端口 {}", handle.local_addr().port());
    }

    let tray = if config.show_status_item {
        let tray = spawn_linux_tray(LinuxTrayConfig::new(
            config.service_name.clone(),
            "VoiceInput".to_string(),
            recorder.clone(),
            Arc::clone(&quit_requested),
            restart,
            Arc::clone(&restart_requested),
            ime,
            Arc::clone(&mic_enabled),
            Arc::clone(&bridge_enabled),
            bridge.is_some(),
            config.bridge_port,
            setup.token.clone(),
            setup.settings.bridge_host.clone(),
            local_ip,
            setup.settings.clone(),
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
        "双击一次开始录音，再双击一次停止并转写；手机发来的文字直接注入当前光标",
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

        // 手机发来的文字优先处理：它们已经在等回票，拖久了 HTTP 会超时。
        drain_bridge_requests(&bridge_rx, &host);

        let triggered = watcher.wait_for_trigger_timeout(Duration::from_millis(250))?;
        if !triggered {
            continue;
        }

        // 本机麦克风开关关闭时忽略热键，但要留一行日志，
        // 否则用户会以为热键坏了。
        if !mic_enabled.load(Ordering::SeqCst) {
            println!("本机麦克风已停用，忽略热键触发");
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

    // 正常退出路径：把内存里的开关状态落盘。
    if let Some(tray) = tray.as_ref() {
        tray.persist_settings();
        tray.shutdown();
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::LinuxAppSettings;
    use std::cell::RefCell;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
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
                deadline: Instant::now() + Duration::from_secs(60),
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
                deadline: Instant::now() + Duration::from_secs(60),
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
            deadline: Instant::now() + Duration::from_secs(60),
            reply: reply_tx,
        })
        .expect("入队应当成功");

        assert_eq!(drain_bridge_requests(&rx, &host), 1);
    }

    #[test]
    fn expired_request_is_not_injected() {
        // 主循环可能卡在 30s 录音里，期间入队的请求客户端早已收到 503、
        // 用户已经重发；过期后仍注入就是一条重复。
        let host = RecordingHost::default();
        let (tx, rx) = mpsc::sync_channel(8);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        tx.try_send(BridgeRequest {
            text: "过期的".to_string(),
            deadline: Instant::now() - Duration::from_secs(1),
            reply: reply_tx,
        })
        .expect("入队应当成功");

        let injected = drain_bridge_requests(&rx, &host);

        assert_eq!(injected, 0, "过期请求不计入注入数");
        assert!(
            host.committed.borrow().is_empty(),
            "过期请求绝不能注入，实际：{:?}",
            host.committed.borrow()
        );
        assert!(
            reply_rx.try_recv().expect("过期也要回票").is_err(),
            "过期请求回票必须是 Err，页面才不会把它当成成功"
        );
    }

    #[test]
    fn mixed_batch_injects_only_live_requests() {
        let host = RecordingHost::default();
        let (tx, rx) = mpsc::sync_channel(8);
        let (expired_tx, expired_rx) = mpsc::sync_channel(1);
        let (live_tx, live_rx) = mpsc::sync_channel(1);
        tx.try_send(BridgeRequest {
            text: "过期的".to_string(),
            deadline: Instant::now() - Duration::from_secs(1),
            reply: expired_tx,
        })
        .expect("入队应当成功");
        tx.try_send(BridgeRequest {
            text: "有效的".to_string(),
            deadline: Instant::now() + Duration::from_secs(60),
            reply: live_tx,
        })
        .expect("入队应当成功");

        let injected = drain_bridge_requests(&rx, &host);

        assert_eq!(injected, 1, "只应注入未过期的那条");
        assert_eq!(host.committed.borrow().clone(), vec!["有效的"]);
        assert!(expired_rx.try_recv().expect("过期要回票").is_err());
        assert!(live_rx.try_recv().expect("有效要回票").is_ok());
    }

    #[test]
    fn bridge_setup_generates_pass_phrase_when_missing() {
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 8765,
            stored_token: String::new(),
            saved_settings: LinuxAppSettings::default(),
        };

        let setup = prepare_bridge(config);

        assert!(
            normalize_pass_phrase(&setup.token).is_some(),
            "生成的口令必须自己能认，实际：{}",
            setup.token
        );
        assert!(
            setup.settings.bridge_token == setup.token,
            "生成的口令必须写回 settings，否则重启后手机上存的地址就失效了"
        );
        assert!(setup.token_is_new, "没有旧口令时必须标记为新的");
    }

    #[test]
    fn bridge_setup_reuses_stored_pass_phrase() {
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 8765,
            stored_token: "lanhexin".to_string(),
            saved_settings: LinuxAppSettings::default(),
        };

        let setup = prepare_bridge(config);

        assert_eq!(setup.token, "lanhexin");
        assert!(!setup.token_is_new, "沿用旧口令时不该重复写盘");
    }

    #[test]
    fn bridge_setup_normalizes_stored_pass_phrase() {
        // 用户在配置里大写写了，地址是拼口令生成的，两处必须一致。
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 8765,
            stored_token: "  LanHeXin  ".to_string(),
            saved_settings: LinuxAppSettings::default(),
        };

        let setup = prepare_bridge(config);

        assert_eq!(setup.token, "lanhexin");
    }

    #[test]
    fn bridge_setup_replaces_illegal_stored_pass_phrase() {
        // 手改配置写成带斜杠的，会把路由切断；这种值不能沿用。
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 8765,
            stored_token: "lan/hexin".to_string(),
            saved_settings: LinuxAppSettings::default(),
        };

        let setup = prepare_bridge(config);

        assert_ne!(setup.token, "lan/hexin");
        assert!(normalize_pass_phrase(&setup.token).is_some());
        assert!(setup.token_is_new, "换成新口令后必须落盘");
    }

    #[test]
    fn bridge_setup_reuses_legacy_hex_token() {
        // 旧版盘上存的是 32 位十六进制，属于合法字符集。升级必须原样沿用，
        // 否则每台机器升一次级就换一次口令，手机上所有书签全废。
        let legacy = "3f9a2c8e1b7d4a6f0e5c9b3d8a1f7e2c";
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 8765,
            stored_token: legacy.to_string(),
            saved_settings: LinuxAppSettings::default(),
        };

        let setup = prepare_bridge(config);

        assert_eq!(setup.token, legacy);
        assert!(!setup.token_is_new);
    }

    #[test]
    fn bridge_setup_carries_port_and_switches() {
        let saved = LinuxAppSettings {
            mic_enabled: false,
            bridge_enabled: true,
            bridge_port: 9999,
            ..Default::default()
        };
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 9999,
            stored_token: "t".to_string(),
            saved_settings: saved,
        };

        let setup = prepare_bridge(config);

        assert_eq!(setup.settings.bridge_port, 9999);
        assert!(!setup.settings.mic_enabled);
        assert!(setup.settings.bridge_enabled);
    }

    #[test]
    fn bridge_setup_keeps_switches_from_settings() {
        let saved = LinuxAppSettings {
            mic_enabled: false,
            bridge_enabled: false,
            ..Default::default()
        };
        let config = BridgeSetupConfig {
            enabled_in_settings: false,
            port: 8765,
            stored_token: "t".to_string(),
            saved_settings: saved,
        };

        let setup = prepare_bridge(config);

        assert!(!setup.settings.bridge_enabled);
        assert!(!setup.settings.mic_enabled);
    }
}
