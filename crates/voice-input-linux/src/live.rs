use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

impl Drop for LiveJobHandle {
    fn drop(&mut self) {
        self.state.finish();
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

#[cfg(test)]
mod tests {
    use super::LiveJobState;
    use std::sync::Arc;

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
    fn live_job_handle_releases_state_on_drop() {
        let state = Arc::new(LiveJobState::default());
        let handle = LiveJobState::try_acquire(&state).expect("acquire active job");
        assert!(state.is_active());
        assert!(LiveJobState::try_acquire(&state).is_none());
        drop(handle);
        assert!(!state.is_active());
    }
}
