use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::live::LiveJobState;
use device_query::{DeviceQuery, DeviceState, Keycode};
use voice_input_core::{Result, VoiceInputError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModifierKey {
    Ctrl,
    Alt,
}

impl ModifierKey {
    fn label(self) -> &'static str {
        match self {
            Self::Ctrl => "Ctrl",
            Self::Alt => "Alt",
        }
    }

    fn is_held(self, keys: &[Keycode]) -> bool {
        match self {
            Self::Ctrl => has_any(keys, &[Keycode::LControl, Keycode::RControl]),
            Self::Alt => has_any(keys, &[Keycode::LAlt, Keycode::RAlt]),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyKind {
    DoublePress(ModifierKey),
    Combo {
        key: Keycode,
        control: bool,
        shift: bool,
        alt: bool,
        meta: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxHotkeySpec {
    kind: HotkeyKind,
}

impl LinuxHotkeySpec {
    pub fn parse(spec: &str) -> Result<Self> {
        let mut double_press: Option<ModifierKey> = None;
        let mut combo = HotkeyKind::Combo {
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
            match token.to_ascii_lowercase().as_str() {
                "longctrl" | "long-ctrl" | "long_ctrl" | "doublectrl" | "double-ctrl"
                | "double_ctrl" | "doublectrlstrict" | "double-ctrl-strict"
                | "double_ctrl_strict" => {
                    double_press = Some(ModifierKey::Ctrl);
                }
                "doublealt" | "double-alt" | "double_alt" => {
                    double_press = Some(ModifierKey::Alt);
                }
                "ctrl" | "control" => set_combo_modifier(&mut combo, ModifierFlag::Control),
                "shift" => set_combo_modifier(&mut combo, ModifierFlag::Shift),
                "alt" | "option" => set_combo_modifier(&mut combo, ModifierFlag::Alt),
                "cmd" | "command" | "meta" => set_combo_modifier(&mut combo, ModifierFlag::Meta),
                "space" => set_combo_key(&mut combo, Keycode::Space),
                "tab" => set_combo_key(&mut combo, Keycode::Tab),
                "enter" | "return" => set_combo_key(&mut combo, Keycode::Enter),
                "esc" | "escape" => set_combo_key(&mut combo, Keycode::Escape),
                "delete" | "backspace" => set_combo_key(&mut combo, Keycode::Delete),
                "f1" => set_combo_key(&mut combo, Keycode::F1),
                "f2" => set_combo_key(&mut combo, Keycode::F2),
                "f3" => set_combo_key(&mut combo, Keycode::F3),
                "f4" => set_combo_key(&mut combo, Keycode::F4),
                "f5" => set_combo_key(&mut combo, Keycode::F5),
                "f6" => set_combo_key(&mut combo, Keycode::F6),
                "f7" => set_combo_key(&mut combo, Keycode::F7),
                "f8" => set_combo_key(&mut combo, Keycode::F8),
                "f9" => set_combo_key(&mut combo, Keycode::F9),
                "f10" => set_combo_key(&mut combo, Keycode::F10),
                "f11" => set_combo_key(&mut combo, Keycode::F11),
                "f12" => set_combo_key(&mut combo, Keycode::F12),
                other if other.len() == 1 => set_combo_key(
                    &mut combo,
                    keycode_from_token(other.chars().next().unwrap())?,
                ),
                other => {
                    return Err(VoiceInputError::Hotkey(format!(
                        "不支持的热键片段：{other}"
                    )));
                }
            }
        }

        Ok(Self {
            kind: double_press.map(HotkeyKind::DoublePress).unwrap_or(combo),
        })
    }

    pub(crate) fn kind(&self) -> HotkeyKind {
        self.kind
    }

    /// 判断当前按下的按键集合是否满足热键。
    ///
    /// DoublePress 分支使用严格的 is_ctrl_only / is_alt_only 语义（仅修饰键
    /// 按下才算命中），主要供解析校验与测试使用；监听循环中的 DoublePress
    /// 检测改用 `ModifierKey::is_held`（has_any，容忍组合键），以避免 Ctrl+C
    /// 等组合键释放非修饰键时产生虚假上升沿。两者差异是有意设计。
    pub fn matches(&self, keys: &[Keycode]) -> bool {
        match self.kind {
            HotkeyKind::DoublePress(ModifierKey::Ctrl) => is_ctrl_only(keys),
            HotkeyKind::DoublePress(ModifierKey::Alt) => is_alt_only(keys),
            HotkeyKind::Combo {
                key,
                control,
                shift,
                alt,
                meta,
            } => {
                if !keys.contains(&key) {
                    return false;
                }

                if control && !has_any(keys, &[Keycode::LControl, Keycode::RControl]) {
                    return false;
                }
                if shift && !has_any(keys, &[Keycode::LShift, Keycode::RShift]) {
                    return false;
                }
                if alt
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
                if meta
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
    }
}

#[derive(Debug, Clone, Copy)]
enum ModifierFlag {
    Control,
    Shift,
    Alt,
    Meta,
}

fn set_combo_modifier(combo: &mut HotkeyKind, flag: ModifierFlag) {
    if let HotkeyKind::Combo {
        control,
        shift,
        alt,
        meta,
        ..
    } = combo
    {
        match flag {
            ModifierFlag::Control => *control = true,
            ModifierFlag::Shift => *shift = true,
            ModifierFlag::Alt => *alt = true,
            ModifierFlag::Meta => *meta = true,
        }
    }
}

fn set_combo_key(combo: &mut HotkeyKind, key: Keycode) {
    if let HotkeyKind::Combo { key: combo_key, .. } = combo {
        *combo_key = key;
    }
}

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

fn has_any(keys: &[Keycode], candidates: &[Keycode]) -> bool {
    candidates.iter().any(|candidate| keys.contains(candidate))
}

fn is_ctrl_only(keys: &[Keycode]) -> bool {
    !keys.is_empty()
        && keys
            .iter()
            .all(|key| matches!(key, Keycode::LControl | Keycode::RControl))
}

fn is_alt_only(keys: &[Keycode]) -> bool {
    !keys.is_empty()
        && keys
            .iter()
            .all(|key| matches!(key, Keycode::LAlt | Keycode::RAlt))
}

pub struct LinuxHotkeyWatcher {
    receiver: mpsc::Receiver<()>,
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl LinuxHotkeyWatcher {
    pub fn spawn(
        spec: LinuxHotkeySpec,
        active: Arc<LiveJobState>,
        recorder: crate::recorder::LinuxMicAudioRecorder,
        double_press_window: Duration,
    ) -> Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);

        let handle = thread::spawn(move || {
            let device = DeviceState::new();
            let mut last_trigger_at: Option<Instant> = None;
            let mut last_release: Option<Instant> = None;
            let mut was_held = false;
            let mut latched = false;
            const TRIGGER_COOLDOWN: Duration = Duration::from_millis(800);

            while !stop_for_thread.load(Ordering::SeqCst) {
                let keys = device.get_keys();

                match spec.kind() {
                    HotkeyKind::DoublePress(modifier) => {
                        // 使用 has_any 而不是 is_*_only 检测修饰键状态。
                        // is_*_only 会在组合键（如 Ctrl+C）释放 C 时产生虚假的
                        // 上升沿（因为 Ctrl 再次变成"单独按下"），导致误触发。
                        let held = modifier.is_held(&keys);
                        let now = Instant::now();
                        let in_cooldown = last_trigger_at
                            .map(|last| now.duration_since(last) <= TRIGGER_COOLDOWN)
                            .unwrap_or(false);

                        if held && !was_held {
                            // 修饰键刚按下
                            if !in_cooldown {
                                if let Some(release_time) = last_release {
                                    if now.duration_since(release_time) <= double_press_window {
                                        // 两次按下间隔在窗口内 → 触发
                                        let label = modifier.label();
                                        if active.is_active() {
                                            if recorder.is_recording() {
                                                eprintln!(
                                                    "检测到双击 {label} 停止热键，正在结束录音..."
                                                );
                                                recorder.stop();
                                            }
                                        } else {
                                            eprintln!(
                                                "检测到双击 {label} 开始热键，正在启动录音..."
                                            );
                                            let _ = sender.send(());
                                        }
                                        last_trigger_at = Some(now);
                                        last_release = None;
                                    }
                                }
                            }
                        } else if !held && was_held {
                            // 修饰键刚释放
                            last_release = Some(now);
                        }

                        was_held = held;
                    }
                    HotkeyKind::Combo { .. } => {
                        // 组合热键（Ctrl+Shift+Space 等）
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

                            if active.is_active() {
                                if recorder.is_recording() {
                                    eprintln!("检测到停止热键，正在结束录音...");
                                    recorder.stop();
                                }
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

impl Drop for LinuxHotkeyWatcher {
    fn drop(&mut self) {
        self.stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combo_hotkey() {
        let spec = LinuxHotkeySpec::parse("Ctrl+Shift+Space").expect("parse hotkey");
        assert!(spec.matches(&[Keycode::Space, Keycode::LControl, Keycode::LShift]));
        assert!(!spec.matches(&[Keycode::Space, Keycode::LControl]));
    }

    #[test]
    fn parses_double_ctrl_hotkey() {
        let spec = LinuxHotkeySpec::parse("DoubleCtrl").expect("parse hotkey");
        assert_eq!(spec.kind(), HotkeyKind::DoublePress(ModifierKey::Ctrl));
        assert!(spec.matches(&[Keycode::LControl]));
        assert!(spec.matches(&[Keycode::RControl]));
        assert!(spec.matches(&[Keycode::LControl, Keycode::RControl]));
        assert!(!spec.matches(&[Keycode::Space]));
        assert!(!spec.matches(&[Keycode::LControl, Keycode::Space]));
    }

    #[test]
    fn parses_double_alt_hotkey() {
        let spec = LinuxHotkeySpec::parse("DoubleAlt").expect("parse hotkey");
        assert_eq!(spec.kind(), HotkeyKind::DoublePress(ModifierKey::Alt));
        assert!(spec.matches(&[Keycode::LAlt]));
        assert!(spec.matches(&[Keycode::RAlt]));
        assert!(!spec.matches(&[Keycode::Space]));
        assert!(!spec.matches(&[Keycode::LAlt, Keycode::Space]));
    }

    #[test]
    fn double_press_alias_variants_parse_to_same_kind() {
        for alias in [
            "DoubleCtrl",
            "double-ctrl",
            "double_ctrl",
            "DoubleCtrlStrict",
            "double-ctrl-strict",
            "double_ctrl_strict",
            "LongCtrl",
            "long-ctrl",
            "long_ctrl",
        ] {
            let spec = LinuxHotkeySpec::parse(alias).expect("parse alias");
            assert_eq!(
                spec.kind(),
                HotkeyKind::DoublePress(ModifierKey::Ctrl),
                "alias: {alias}"
            );
        }
        for alias in ["DoubleAlt", "double-alt", "double_alt"] {
            let spec = LinuxHotkeySpec::parse(alias).expect("parse alias");
            assert_eq!(
                spec.kind(),
                HotkeyKind::DoublePress(ModifierKey::Alt),
                "alias: {alias}"
            );
        }
    }

    #[test]
    fn double_press_token_wins_over_combo_and_last_wins() {
        // 双击 token 覆盖组合 token（与旧代码 matches() 先查 double_* 的优先级一致）
        let spec = LinuxHotkeySpec::parse("Ctrl+DoubleAlt").expect("parse");
        assert_eq!(spec.kind(), HotkeyKind::DoublePress(ModifierKey::Alt));
        // 退化输入：两个双击 token 时后者覆盖前者
        let spec = LinuxHotkeySpec::parse("DoubleAlt+DoubleCtrl").expect("parse");
        assert_eq!(spec.kind(), HotkeyKind::DoublePress(ModifierKey::Ctrl));
    }
}
