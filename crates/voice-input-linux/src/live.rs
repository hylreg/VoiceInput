use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{
    AppConfig, AppController, AudioRecorder, InputMethodHost, MockHotkeyManager, Result,
};

pub fn preflight_python_asr(asr: &FunAsrConfig) -> Result<()> {
    let _ = PythonFunAsrRunner::connect(asr.clone())?;
    Ok(())
}

pub fn build_python_live_controller(
    app: AppConfig,
    asr: FunAsrConfig,
    recorder: Box<dyn AudioRecorder>,
    host: Box<dyn InputMethodHost>,
) -> Result<AppController> {
    let asr_runner = PythonFunAsrRunner::connect(asr.clone())?;
    let transcriber = LocalFunAsrTranscriber::new(asr, Box::new(asr_runner));
    Ok(AppController::new(
        app,
        Box::new(MockHotkeyManager),
        recorder,
        Box::new(transcriber),
        host,
    ))
}

pub fn run_controller_job(controller: &AppController) -> Result<String> {
    controller.process_once()
}

pub fn run_python_live_job(
    app: AppConfig,
    asr: FunAsrConfig,
    recorder: Box<dyn AudioRecorder>,
    host: Box<dyn InputMethodHost>,
) -> Result<String> {
    let controller = build_python_live_controller(app, asr, recorder, host)?;
    run_controller_job(&controller)
}

pub fn rollback_live_host(host: &dyn InputMethodHost) {
    let _ = host.cancel_composition();
    let _ = host.end_composition();
}

pub fn finish_live_host(host: &dyn InputMethodHost, text: &str) -> Result<()> {
    host.commit_text(text)?;
    host.end_composition()
}

pub fn log_live_job_result(result: Result<String>, success_prefix: &str, error_prefix: &str) {
    match result {
        Ok(text) => println!("{success_prefix}{text}"),
        Err(err) => eprintln!("{error_prefix}{err}"),
    }
}

pub fn print_live_ready<I, S>(platform: &str, hotkey: &str, instructions: &str, extra_lines: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    println!("VoiceInput {platform} 常驻应用已启动");
    println!("热键：{hotkey}");
    println!("说明：{instructions}");
    for line in extra_lines {
        println!("{}", line.as_ref());
    }
}

pub fn run_logged_queued_live_job<Run>(
    state: &Arc<QueuedLiveJobState>,
    success_prefix: &str,
    error_prefix: &str,
    run: Run,
) -> bool
where
    Run: FnOnce() -> Result<String>,
{
    let Some(_job) = QueuedLiveJobState::try_acquire_pending(state) else {
        return false;
    };

    log_live_job_result(run(), success_prefix, error_prefix);
    true
}

pub fn spawn_logged_live_job<Run>(
    name: &str,
    state: &Arc<LiveJobState>,
    success_prefix: &'static str,
    error_prefix: &'static str,
    run: Run,
) -> std::io::Result<bool>
where
    Run: FnOnce() -> Result<String> + Send + 'static,
{
    let Some(job) = LiveJobState::try_acquire(state) else {
        return Ok(false);
    };

    thread::Builder::new()
        .name(name.to_string())
        .spawn(move || {
            let _job = job;
            log_live_job_result(run(), success_prefix, error_prefix);
        })?;

    Ok(true)
}

#[derive(Default)]
pub struct LiveJobState {
    active: AtomicBool,
}

pub struct LiveJobHandle {
    state: Arc<LiveJobState>,
}

impl LiveJobState {
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn try_start(&self) -> bool {
        self.active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn finish(&self) {
        self.active.store(false, Ordering::SeqCst);
    }

    pub fn try_acquire(state: &Arc<Self>) -> Option<LiveJobHandle> {
        if state.try_start() {
            Some(LiveJobHandle {
                state: Arc::clone(state),
            })
        } else {
            None
        }
    }
}

#[derive(Default)]
pub struct QueuedLiveJobState {
    pending_start: AtomicBool,
    active: AtomicBool,
}

pub struct QueuedLiveJobHandle {
    state: Arc<QueuedLiveJobState>,
}

impl QueuedLiveJobState {
    pub fn request_start(&self) {
        self.pending_start.store(true, Ordering::SeqCst);
    }

    pub fn take_pending_start(&self) -> bool {
        self.pending_start.swap(false, Ordering::SeqCst)
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn try_start(&self) -> bool {
        self.active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    pub fn finish(&self) {
        self.active.store(false, Ordering::SeqCst);
    }

    pub fn try_acquire_pending(state: &Arc<Self>) -> Option<QueuedLiveJobHandle> {
        if !state.take_pending_start() {
            return None;
        }

        if state.try_start() {
            Some(QueuedLiveJobHandle {
                state: Arc::clone(state),
            })
        } else {
            None
        }
    }
}

impl Drop for LiveJobHandle {
    fn drop(&mut self) {
        self.state.finish();
    }
}

impl Drop for QueuedLiveJobHandle {
    fn drop(&mut self) {
        self.state.finish();
    }
}

pub struct LiveHostSession<'a> {
    host: &'a dyn InputMethodHost,
    completed: bool,
}

impl<'a> LiveHostSession<'a> {
    pub fn begin(host: &'a dyn InputMethodHost, initial_preedit: Option<&str>) -> Result<Self> {
        host.start_composition()?;
        if let Some(text) = initial_preedit {
            if let Err(err) = host.update_preedit(text) {
                rollback_live_host(host);
                return Err(err);
            }
        }

        Ok(Self {
            host,
            completed: false,
        })
    }

    pub fn update_preedit(&self, text: &str) -> Result<()> {
        self.host.update_preedit(text)
    }

    pub fn commit(mut self, text: &str) -> Result<()> {
        finish_live_host(self.host, text)?;
        self.completed = true;
        Ok(())
    }

    pub fn rollback(mut self) {
        rollback_live_host(self.host);
        self.completed = true;
    }
}

impl Drop for LiveHostSession<'_> {
    fn drop(&mut self) {
        if !self.completed {
            rollback_live_host(self.host);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{run_logged_queued_live_job, LiveHostSession, LiveJobState, QueuedLiveJobState};
    use std::sync::Arc;
    use voice_input_core::{InputMethodHost, MockInputMethodHost, Result, VoiceInputError};

    struct FailingPreeditHost;

    impl InputMethodHost for FailingPreeditHost {
        fn start_composition(&self) -> Result<()> {
            Ok(())
        }

        fn update_preedit(&self, _text: &str) -> Result<()> {
            Err(VoiceInputError::Injection("preedit failed".to_string()))
        }

        fn commit_text(&self, _text: &str) -> Result<()> {
            Ok(())
        }

        fn cancel_composition(&self) -> Result<()> {
            Ok(())
        }

        fn end_composition(&self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn live_host_session_rolls_back_on_drop() {
        let host = MockInputMethodHost::default();
        let session = LiveHostSession::begin(&host, Some("录音中")).expect("begin session");
        session.update_preedit("结果").expect("update preedit");
        drop(session);

        assert_eq!(
            host.events(),
            vec![
                "开始输入".to_string(),
                "更新预编辑：录音中".to_string(),
                "更新预编辑：结果".to_string(),
                "取消输入".to_string(),
                "结束输入".to_string(),
            ]
        );
    }

    #[test]
    fn live_host_session_rolls_back_if_initial_preedit_fails() {
        let host = FailingPreeditHost;
        let result = LiveHostSession::begin(&host, Some("录音中"));
        match result {
            Ok(_) => panic!("preedit should fail"),
            Err(err) => assert!(err.to_string().contains("preedit failed")),
        }
    }

    #[test]
    fn live_job_state_gates_single_active_job() {
        let state = LiveJobState::default();
        assert!(state.try_start());
        assert!(state.is_active());
        assert!(!state.try_start());
        state.finish();
        assert!(!state.is_active());
        assert!(state.try_start());
    }

    #[test]
    fn queued_live_job_state_tracks_pending_and_active() {
        let state = QueuedLiveJobState::default();
        assert!(!state.take_pending_start());
        state.request_start();
        assert!(state.take_pending_start());
        assert!(!state.take_pending_start());
        assert!(state.try_start());
        assert!(state.is_active());
        assert!(!state.try_start());
        state.finish();
        assert!(!state.is_active());
    }

    #[test]
    fn live_job_handle_releases_state_on_drop() {
        let state = Arc::new(LiveJobState::default());
        let handle = LiveJobState::try_acquire(&state).expect("acquire active job");
        assert!(state.is_active());
        assert!(LiveJobState::try_acquire(&state).is_none());
        drop(handle);
        assert!(!state.is_active());
    }

    #[test]
    fn queued_live_job_handle_consumes_pending_start_and_releases_on_drop() {
        let state = Arc::new(QueuedLiveJobState::default());
        assert!(QueuedLiveJobState::try_acquire_pending(&state).is_none());
        state.request_start();
        let handle =
            QueuedLiveJobState::try_acquire_pending(&state).expect("acquire queued active job");
        assert!(state.is_active());
        assert!(!state.take_pending_start());
        drop(handle);
        assert!(!state.is_active());
    }

    #[test]
    fn run_logged_queued_live_job_consumes_pending_request_and_resets_active_state() {
        let state = Arc::new(QueuedLiveJobState::default());
        state.request_start();

        let started = run_logged_queued_live_job(&state, "ok:", "err:", || Ok("done".to_string()));

        assert!(started);
        assert!(!state.is_active());
        assert!(!state.take_pending_start());
    }
}
