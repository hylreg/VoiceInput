use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use voice_input_core::{Result, VoiceInputError};

#[cfg(target_os = "linux")]
use device_query::{DeviceQuery, DeviceState, Keycode};

#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxHotkeySpec {
    double_ctrl: bool,
    key: Keycode,
    control: bool,
    shift: bool,
    alt: bool,
    meta: bool,
}

#[cfg(target_os = "linux")]
impl LinuxHotkeySpec {
    pub fn parse(spec: &str) -> Result<Self> {
        let mut parsed = LinuxHotkeySpec {
            double_ctrl: false,
            key: Keycode::Space,
            control: false,
            shift: false,
            alt: false,
            meta: false,
        };

        for token in spec
            .split('+')
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            if token.eq_ignore_ascii_case("doublectrl")
                || token.eq_ignore_ascii_case("double-ctrl")
                || token.eq_ignore_ascii_case("double_ctrl")
                || token.eq_ignore_ascii_case("doublectrlstrict")
                || token.eq_ignore_ascii_case("double-ctrl-strict")
                || token.eq_ignore_ascii_case("double_ctrl_strict")
            {
                parsed.double_ctrl = true;
                continue;
            }

            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => parsed.control = true,
                "shift" => parsed.shift = true,
                "alt" | "option" => parsed.alt = true,
                "cmd" | "command" | "meta" => parsed.meta = true,
                "space" => parsed.key = Keycode::Space,
                "tab" => parsed.key = Keycode::Tab,
                "enter" | "return" => parsed.key = Keycode::Enter,
                "esc" | "escape" => parsed.key = Keycode::Escape,
                "delete" | "backspace" => parsed.key = Keycode::Delete,
                "f1" => parsed.key = Keycode::F1,
                "f2" => parsed.key = Keycode::F2,
                "f3" => parsed.key = Keycode::F3,
                "f4" => parsed.key = Keycode::F4,
                "f5" => parsed.key = Keycode::F5,
                "f6" => parsed.key = Keycode::F6,
                "f7" => parsed.key = Keycode::F7,
                "f8" => parsed.key = Keycode::F8,
                "f9" => parsed.key = Keycode::F9,
                "f10" => parsed.key = Keycode::F10,
                "f11" => parsed.key = Keycode::F11,
                "f12" => parsed.key = Keycode::F12,
                other if other.len() == 1 => {
                    parsed.key = keycode_from_token(other.chars().next().unwrap())?;
                }
                other => {
                    return Err(VoiceInputError::Hotkey(format!(
                        "不支持的热键片段：{other}"
                    )));
                }
            }
        }

        Ok(parsed)
    }

    pub fn matches(&self, keys: &[Keycode]) -> bool {
        if self.double_ctrl {
            return is_ctrl_only(keys);
        }

        if !keys.contains(&self.key) {
            return false;
        }

        if self.control && !has_any(keys, &[Keycode::LControl, Keycode::RControl]) {
            return false;
        }
        if self.shift && !has_any(keys, &[Keycode::LShift, Keycode::RShift]) {
            return false;
        }
        if self.alt
            && !has_any(
                keys,
                &[
                    Keycode::LAlt,
                    Keycode::RAlt,
                    Keycode::LOption,
                    Keycode::ROption,
                ],
            )
        {
            return false;
        }
        if self.meta
            && !has_any(
                keys,
                &[
                    Keycode::LMeta,
                    Keycode::RMeta,
                    Keycode::Command,
                    Keycode::RCommand,
                ],
            )
        {
            return false;
        }

        true
    }
}

#[cfg(target_os = "linux")]
fn keycode_from_token(token: char) -> Result<Keycode> {
    let key = match token.to_ascii_lowercase() {
        'a' => Keycode::A,
        'b' => Keycode::B,
        'c' => Keycode::C,
        'd' => Keycode::D,
        'e' => Keycode::E,
        'f' => Keycode::F,
        'g' => Keycode::G,
        'h' => Keycode::H,
        'i' => Keycode::I,
        'j' => Keycode::J,
        'k' => Keycode::K,
        'l' => Keycode::L,
        'm' => Keycode::M,
        'n' => Keycode::N,
        'o' => Keycode::O,
        'p' => Keycode::P,
        'q' => Keycode::Q,
        'r' => Keycode::R,
        's' => Keycode::S,
        't' => Keycode::T,
        'u' => Keycode::U,
        'v' => Keycode::V,
        'w' => Keycode::W,
        'x' => Keycode::X,
        'y' => Keycode::Y,
        'z' => Keycode::Z,
        '0' => Keycode::Key0,
        '1' => Keycode::Key1,
        '2' => Keycode::Key2,
        '3' => Keycode::Key3,
        '4' => Keycode::Key4,
        '5' => Keycode::Key5,
        '6' => Keycode::Key6,
        '7' => Keycode::Key7,
        '8' => Keycode::Key8,
        '9' => Keycode::Key9,
        other => {
            return Err(VoiceInputError::Hotkey(format!(
                "不支持的单字符热键：{other}"
            )));
        }
    };

    Ok(key)
}

#[cfg(target_os = "linux")]
fn has_any(keys: &[Keycode], candidates: &[Keycode]) -> bool {
    candidates.iter().any(|candidate| keys.contains(candidate))
}

#[cfg(target_os = "linux")]
fn is_ctrl_only(keys: &[Keycode]) -> bool {
    !keys.is_empty()
        && keys
            .iter()
            .all(|key| matches!(key, Keycode::LControl | Keycode::RControl))
}

#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxHotkeySpec;

#[cfg(not(target_os = "linux"))]
impl LinuxHotkeySpec {
    pub fn parse(_spec: &str) -> Result<Self> {
        Err(VoiceInputError::Hotkey(
            "Linux 热键监听只支持 Linux".to_string(),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub struct LinuxHotkeyWatcher;

#[cfg(not(target_os = "linux"))]
impl LinuxHotkeyWatcher {
    pub fn spawn(
        _spec: LinuxHotkeySpec,
        _active: Arc<AtomicBool>,
        _recorder: crate::recorder::LinuxMicAudioRecorder,
    ) -> Result<Self> {
        Err(VoiceInputError::Hotkey(
            "Linux 热键监听只支持 Linux".to_string(),
        ))
    }

    pub fn wait_for_trigger(&self) -> Result<()> {
        Err(VoiceInputError::Hotkey(
            "Linux 热键监听只支持 Linux".to_string(),
        ))
    }

    pub fn wait_for_trigger_timeout(&self, _timeout: Duration) -> Result<bool> {
        Err(VoiceInputError::Hotkey(
            "Linux 热键监听只支持 Linux".to_string(),
        ))
    }

    pub fn stop(&self) {}
}

#[cfg(target_os = "linux")]
pub struct LinuxHotkeyWatcher {
    receiver: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl LinuxHotkeyWatcher {
    pub fn spawn(
        spec: LinuxHotkeySpec,
        active: Arc<AtomicBool>,
        recorder: crate::recorder::LinuxMicAudioRecorder,
        double_ctrl_window: Duration,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);

        let handle = thread::spawn(move || {
            let device = DeviceState::new();
            let mut latched = false;
            let mut last_ctrl_press: Option<Instant> = None;
            let mut last_trigger_at: Option<Instant> = None;
            const TRIGGER_COOLDOWN: Duration = Duration::from_millis(800);

            while !stop_for_thread.load(Ordering::SeqCst) {
                let keys = device.get_keys();
                if spec.double_ctrl {
                    let ctrl_pressed = spec.matches(&keys);

                    if ctrl_pressed && !latched {
                        let now = Instant::now();
                        let recently_triggered = last_trigger_at
                            .map(|last| now.duration_since(last) <= TRIGGER_COOLDOWN)
                            .unwrap_or(false);
                        let triggered = last_ctrl_press
                            .map(|last| now.duration_since(last) <= double_ctrl_window)
                            .unwrap_or(false);

                        if triggered {
                            if !recently_triggered {
                                if active.load(Ordering::SeqCst) {
                                    eprintln!("检测到双击 Ctrl 停止热键，正在结束录音...");
                                    recorder.stop();
                                } else {
                                    eprintln!("检测到双击 Ctrl 开始热键，正在启动录音...");
                                    let _ = sender.send(());
                                }
                                last_trigger_at = Some(now);
                            }
                            last_ctrl_press = None;
                        } else {
                            last_ctrl_press = Some(now);
                        }

                        latched = true;
                    } else if !ctrl_pressed {
                        latched = false;
                    }
                } else {
                    let pressed = spec.matches(&keys);

                    if pressed && !latched {
                        let now = Instant::now();
                        let recently_triggered = last_trigger_at
                            .map(|last| now.duration_since(last) <= TRIGGER_COOLDOWN)
                            .unwrap_or(false);
                        if recently_triggered {
                            latched = true;
                            continue;
                        }

                        if active.load(Ordering::SeqCst) {
                            eprintln!("检测到停止热键，正在结束录音...");
                            recorder.stop();
                        } else {
                            eprintln!("检测到开始热键，正在启动录音...");
                            let _ = sender.send(());
                        }
                        last_trigger_at = Some(now);
                        latched = true;
                    } else if !pressed {
                        latched = false;
                    }
                }

                thread::sleep(Duration::from_millis(25));
            }
        });

        Ok(Self {
            receiver,
            stop,
            handle: Some(handle),
        })
    }

    pub fn wait_for_trigger(&self) -> Result<()> {
        self.receiver
            .recv()
            .map_err(|_| VoiceInputError::Hotkey("热键监听已停止".to_string()))
    }

    pub fn wait_for_trigger_timeout(&self, timeout: Duration) -> Result<bool> {
        match self.receiver.recv_timeout(timeout) {
            Ok(_) => Ok(true),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(false),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(VoiceInputError::Hotkey("热键监听已停止".to_string()))
            }
        }
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

#[cfg(target_os = "linux")]
impl Drop for LinuxHotkeyWatcher {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;

    #[test]
    fn parses_default_linux_hotkey() {
        let spec = LinuxHotkeySpec::parse("Ctrl+Shift+Space").expect("parse hotkey");
        assert!(spec.matches(&[Keycode::Space, Keycode::LControl, Keycode::LShift]));
        assert!(!spec.matches(&[Keycode::Space, Keycode::LControl]));
    }

    #[test]
    fn parses_mac_like_default_hotkey_for_linux_runtime() {
        let spec = LinuxHotkeySpec::parse("Ctrl+Shift+Space").expect("parse hotkey");
        assert!(spec.matches(&[Keycode::Space, Keycode::LControl, Keycode::LShift]));
    }

    #[test]
    fn parses_double_ctrl_hotkey() {
        let spec = LinuxHotkeySpec::parse("DoubleCtrl").expect("parse hotkey");
        assert!(spec.matches(&[Keycode::LControl]));
        assert!(spec.matches(&[Keycode::RControl]));
        assert!(spec.matches(&[Keycode::LControl, Keycode::RControl]));
        assert!(!spec.matches(&[Keycode::Space]));
        assert!(!spec.matches(&[Keycode::LControl, Keycode::Space]));
    }

    #[test]
    fn parses_double_ctrl_strict_hotkey() {
        let spec = LinuxHotkeySpec::parse("DoubleCtrlStrict").expect("parse hotkey");
        assert!(spec.matches(&[Keycode::LControl]));
        assert!(spec.matches(&[Keycode::RControl]));
        assert!(!spec.matches(&[Keycode::Space]));
    }
}
