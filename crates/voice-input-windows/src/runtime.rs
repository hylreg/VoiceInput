#[cfg(target_os = "windows")]
mod windows_runtime {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, VK_BACK, VK_ESCAPE, VK_F1, VK_F10,
        VK_F11, VK_F12, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_RETURN,
        VK_SPACE, VK_TAB,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, RegisterHotKey, TranslateMessage, UnregisterHotKey, MSG,
        WM_HOTKEY,
    };

    use crate::bridge::{ClipboardWindowsImeBridge, WindowsImeBridge};
    use crate::host::{WindowsHostConfig, WindowsInputMethodHost};
    use crate::recorder::WindowsMicAudioRecorder;
    use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, PythonFunAsrRunner};
    use voice_input_core::{AppConfig, AppController, MockHotkeyManager, Result, VoiceInputError};

    #[derive(Debug, Clone)]
    pub struct WindowsLiveAppConfig {
        pub app: AppConfig,
        pub host: WindowsHostConfig,
        pub asr: FunAsrConfig,
        pub max_recording_duration: Duration,
    }

    impl Default for WindowsLiveAppConfig {
        fn default() -> Self {
            Self {
                app: AppConfig::default(),
                host: WindowsHostConfig::default(),
                asr: FunAsrConfig::from_env(),
                max_recording_duration: Duration::from_secs(12),
            }
        }
    }

    #[derive(Default)]
    struct RuntimeState {
        job_active: AtomicBool,
    }

    #[derive(Clone, Copy)]
    struct HotkeySpec {
        modifiers: u32,
        virtual_key: u32,
    }

    impl HotkeySpec {
        fn parse(spec: &str) -> Result<Self> {
            let mut modifiers = MOD_NOREPEAT;
            let mut virtual_key = VK_SPACE as u32;

            for token in spec
                .split('+')
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                match token.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => modifiers |= MOD_CONTROL,
                    "shift" => modifiers |= MOD_SHIFT,
                    "alt" | "option" => modifiers |= MOD_ALT,
                    "win" | "meta" | "super" => modifiers |= MOD_WIN,
                    "space" => virtual_key = VK_SPACE as u32,
                    "tab" => virtual_key = VK_TAB as u32,
                    "enter" | "return" => virtual_key = VK_RETURN as u32,
                    "esc" | "escape" => virtual_key = VK_ESCAPE as u32,
                    "backspace" | "delete" => virtual_key = VK_BACK as u32,
                    "f1" => virtual_key = VK_F1 as u32,
                    "f2" => virtual_key = VK_F2 as u32,
                    "f3" => virtual_key = VK_F3 as u32,
                    "f4" => virtual_key = VK_F4 as u32,
                    "f5" => virtual_key = VK_F5 as u32,
                    "f6" => virtual_key = VK_F6 as u32,
                    "f7" => virtual_key = VK_F7 as u32,
                    "f8" => virtual_key = VK_F8 as u32,
                    "f9" => virtual_key = VK_F9 as u32,
                    "f10" => virtual_key = VK_F10 as u32,
                    "f11" => virtual_key = VK_F11 as u32,
                    "f12" => virtual_key = VK_F12 as u32,
                    other if other.len() == 1 => {
                        virtual_key = other.chars().next().unwrap().to_ascii_uppercase() as u32;
                    }
                    other => {
                        return Err(VoiceInputError::Hotkey(format!(
                            "不支持的 Windows 热键片段：{other}"
                        )));
                    }
                }
            }

            Ok(Self {
                modifiers,
                virtual_key,
            })
        }
    }

    struct HotkeyRegistration {
        id: i32,
    }

    impl Drop for HotkeyRegistration {
        fn drop(&mut self) {
            unsafe {
                UnregisterHotKey(std::ptr::null_mut(), self.id);
            }
        }
    }

    pub fn run_live_app(config: WindowsLiveAppConfig) -> Result<()> {
        let hotkey = HotkeySpec::parse(&config.app.activation_hotkey)?;
        let recorder = WindowsMicAudioRecorder::new(config.max_recording_duration);
        let state = Arc::new(RuntimeState::default());

        println!("正在预检 Windows ASR 环境...");
        let _ = PythonFunAsrRunner::connect(config.asr.clone())?;
        println!("ASR 环境预检完成");

        let registration = unsafe {
            if RegisterHotKey(
                std::ptr::null_mut(),
                1,
                hotkey.modifiers,
                hotkey.virtual_key,
            ) == 0
            {
                return Err(VoiceInputError::Hotkey(
                    "注册 Windows 全局热键失败，请检查是否被其他应用占用".to_string(),
                ));
            }
            HotkeyRegistration { id: 1 }
        };

        println!("VoiceInput Windows 常驻应用已启动");
        println!("热键：{}", config.app.activation_hotkey);
        println!("说明：按一次开始录音，再按一次停止并转写");
        println!("提交方式：Windows Unicode 注入，失败时回退剪贴板粘贴");

        let mut msg = MSG {
            hwnd: std::ptr::null_mut(),
            message: 0,
            wParam: 0,
            lParam: 0,
            time: 0,
            pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
        };

        loop {
            let result = unsafe { GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) };
            if result == -1 {
                return Err(VoiceInputError::Hotkey(
                    "Windows 消息循环读取失败".to_string(),
                ));
            }
            if result == 0 {
                break;
            }

            if msg.message == WM_HOTKEY && msg.wParam == registration.id as usize {
                if state.job_active.load(Ordering::SeqCst) {
                    if recorder.is_recording() {
                        println!("收到停止热键");
                        recorder.stop();
                    }
                } else if state
                    .job_active
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    println!("收到启动热键");
                    spawn_recording_job(config.clone(), recorder.clone(), Arc::clone(&state));
                }
            }

            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        drop(registration);
        Ok(())
    }

    fn spawn_recording_job(
        config: WindowsLiveAppConfig,
        recorder: WindowsMicAudioRecorder,
        state: Arc<RuntimeState>,
    ) {
        std::thread::Builder::new()
            .name("voiceinput-windows-recording".to_string())
            .spawn(move || {
                let bridge: Box<dyn WindowsImeBridge> = Box::new(ClipboardWindowsImeBridge);
                let host = WindowsInputMethodHost::new_with_bridge(config.host, bridge);
                let asr_runner = match PythonFunAsrRunner::connect(config.asr.clone()) {
                    Ok(runner) => runner,
                    Err(err) => {
                        eprintln!("Windows 常驻输入失败：预加载 ASR 模型失败：{err}");
                        state.job_active.store(false, Ordering::SeqCst);
                        return;
                    }
                };
                let transcriber = LocalFunAsrTranscriber::new(config.asr, Box::new(asr_runner));
                let controller = AppController::new(
                    config.app,
                    Box::new(MockHotkeyManager),
                    Box::new(recorder),
                    Box::new(transcriber),
                    Box::new(host),
                );

                println!("正在录音...");
                match controller.process_once() {
                    Ok(text) => println!("识别结果：{text}"),
                    Err(err) => eprintln!("Windows 常驻输入失败：{err}"),
                }
                state.job_active.store(false, Ordering::SeqCst);
            })
            .expect("spawn windows recording worker");
    }
}

#[cfg(target_os = "windows")]
pub use windows_runtime::{run_live_app, WindowsLiveAppConfig};

#[cfg(not(target_os = "windows"))]
mod not_windows {
    use std::time::Duration;

    use crate::host::WindowsHostConfig;
    use voice_input_asr::FunAsrConfig;
    use voice_input_core::{AppConfig, Result, VoiceInputError};

    #[derive(Debug, Clone)]
    pub struct WindowsLiveAppConfig {
        pub app: AppConfig,
        pub host: WindowsHostConfig,
        pub asr: FunAsrConfig,
        pub max_recording_duration: Duration,
    }

    impl Default for WindowsLiveAppConfig {
        fn default() -> Self {
            Self {
                app: AppConfig::default(),
                host: WindowsHostConfig::default(),
                asr: FunAsrConfig::from_env(),
                max_recording_duration: Duration::from_secs(12),
            }
        }
    }

    pub fn run_live_app(_config: WindowsLiveAppConfig) -> Result<()> {
        Err(VoiceInputError::Injection(
            "Windows 常驻模式只支持 Windows".to_string(),
        ))
    }
}

#[cfg(not(target_os = "windows"))]
pub use not_windows::{run_live_app, WindowsLiveAppConfig};
