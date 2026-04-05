#![allow(unexpected_cfgs)]

#[cfg(target_os = "macos")]
mod mac_runtime {
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::raw::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use cocoa::appkit::{
        NSApp, NSApplication, NSApplicationActivationPolicyAccessory, NSMenu, NSMenuItem,
        NSStatusBar, NSStatusItem, NSVariableStatusItemLength,
    };
    use cocoa::base::{id, nil, YES};
    use cocoa::foundation::{NSAutoreleasePool, NSString};
    use core_foundation::date::CFDate;
    use core_foundation::runloop::{
        kCFRunLoopCommonModes, CFRunLoop, CFRunLoopTimer, CFRunLoopTimerContext, CFRunLoopTimerRef,
    };
    use core_graphics::event::{
        CGEventFlags, CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
        CGEventType, EventField, KeyCode,
    };
    use objc::runtime::Class;
    use objc::{msg_send, sel, sel_impl};
    use fs2::FileExt;

    use crate::bridge::{ClipboardMacImeBridge, MacImeBridge};
    use crate::host::{MacHostConfig, MacInputMethodHost};
    use crate::recorder::MicAudioRecorder;
    use voice_input_asr::{FunAsrConfig, PythonFunAsrRunner};
    use voice_input_core::{AppConfig, AppController, MockHotkeyManager, Result, VoiceInputError};

    #[derive(Debug, Clone)]
    pub struct MacLiveAppConfig {
        pub app: AppConfig,
        pub host: MacHostConfig,
        pub asr: FunAsrConfig,
        pub max_recording_duration: Duration,
        pub show_status_item: bool,
    }

    impl Default for MacLiveAppConfig {
        fn default() -> Self {
            Self {
                app: AppConfig::default(),
                host: MacHostConfig::default(),
                asr: FunAsrConfig::from_env(),
                max_recording_duration: Duration::from_secs(12),
                show_status_item: true,
            }
        }
    }

    #[derive(Default)]
    struct RuntimeState {
        pending_start: AtomicBool,
        job_active: AtomicBool,
    }

    struct MainLoopContext {
        controller: AppController,
        state: Arc<RuntimeState>,
    }

    struct SingleInstanceGuard {
        _lock_file: File,
    }

    impl SingleInstanceGuard {
        fn acquire() -> Result<Option<Self>> {
            let lock_path = std::env::temp_dir().join("voiceinput-macos.lock");
            let lock_file = OpenOptions::new()
                .create(true)
                .read(true)
                .write(true)
                .open(&lock_path)
                .map_err(|e| VoiceInputError::Injection(format!(
                    "创建 macOS 单实例锁失败 {}：{e}",
                    lock_path.display()
                )))?;

            match lock_file.try_lock_exclusive() {
                Ok(()) => {
                    let mut lock_file_for_pid = lock_file;
                    lock_file_for_pid
                        .set_len(0)
                        .map_err(|e| VoiceInputError::Injection(format!(
                            "清空 macOS 单实例锁失败：{e}"
                        )))?;
                    lock_file_for_pid
                        .write_all(format!("pid={}\n", std::process::id()).as_bytes())
                        .map_err(|e| VoiceInputError::Injection(format!(
                            "写入 macOS 单实例锁失败：{e}"
                        )))?;
                    Ok(Some(Self {
                        _lock_file: lock_file_for_pid,
                    }))
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
                Err(err) => Err(VoiceInputError::Injection(format!(
                    "获取 macOS 单实例锁失败 {}：{err}",
                    lock_path.display()
                ))),
            }
        }
    }

    unsafe fn configure_status_item_button(button: id) {
        if button == nil {
            return;
        }

        let title = NSString::alloc(nil).init_str("");
        let tooltip = NSString::alloc(nil).init_str("VoiceInput 正在后台运行");
        let symbol_name = NSString::alloc(nil).init_str("waveform.circle.fill");
        let accessibility_description = NSString::alloc(nil).init_str("VoiceInput");
        let _: () = msg_send![button, setTitle: title];
        let _: () = msg_send![button, setToolTip: tooltip];

        let nsimage_class = Class::get("NSImage").expect("NSImage class");
        let image: id = msg_send![
            nsimage_class,
            imageWithSystemSymbolName: symbol_name
            accessibilityDescription: accessibility_description
        ];

        if image != nil {
            let _: () = msg_send![image, setTemplate: YES];
            let _: () = msg_send![button, setImage: image];
        }
    }

    unsafe fn configure_application_icon() {
        let symbol_name = NSString::alloc(nil).init_str("waveform.circle.fill");
        let accessibility_description = NSString::alloc(nil).init_str("VoiceInput");
        let nsimage_class = Class::get("NSImage").expect("NSImage class");
        let image: id = msg_send![
            nsimage_class,
            imageWithSystemSymbolName: symbol_name
            accessibilityDescription: accessibility_description
        ];

        if image != nil {
            let _: () = msg_send![image, setTemplate: YES];
            let app = NSApp();
            let _: () = msg_send![app, setApplicationIconImage: image];
        }
    }

    #[derive(Clone, Copy)]
    struct HotkeySpec {
        key_code: u16,
        control: bool,
        shift: bool,
        option: bool,
        command: bool,
    }

    impl HotkeySpec {
        fn parse(spec: &str) -> Result<Self> {
            let mut parsed = HotkeySpec {
                key_code: KeyCode::SPACE,
                control: false,
                shift: false,
                option: false,
                command: false,
            };

            for token in spec
                .split('+')
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
            {
                match token.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => parsed.control = true,
                    "shift" => parsed.shift = true,
                    "alt" | "option" => parsed.option = true,
                    "cmd" | "command" | "meta" => parsed.command = true,
                    "space" => parsed.key_code = KeyCode::SPACE,
                    "tab" => parsed.key_code = KeyCode::TAB,
                    "enter" | "return" => parsed.key_code = KeyCode::RETURN,
                    "esc" | "escape" => parsed.key_code = KeyCode::ESCAPE,
                    "delete" | "backspace" => parsed.key_code = KeyCode::DELETE,
                    "f1" => parsed.key_code = KeyCode::F1,
                    "f2" => parsed.key_code = KeyCode::F2,
                    "f3" => parsed.key_code = KeyCode::F3,
                    "f4" => parsed.key_code = KeyCode::F4,
                    "f5" => parsed.key_code = KeyCode::F5,
                    "f6" => parsed.key_code = KeyCode::F6,
                    "f7" => parsed.key_code = KeyCode::F7,
                    "f8" => parsed.key_code = KeyCode::F8,
                    "f9" => parsed.key_code = KeyCode::F9,
                    "f10" => parsed.key_code = KeyCode::F10,
                    "f11" => parsed.key_code = KeyCode::F11,
                    "f12" => parsed.key_code = KeyCode::F12,
                    other if other.len() == 1 => {
                        parsed.key_code = letter_key_code(other.chars().next().unwrap());
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

        fn matches(&self, key_code: u16, flags: CGEventFlags) -> bool {
            if key_code != self.key_code {
                return false;
            }

            if self.control && !flags.contains(CGEventFlags::CGEventFlagControl) {
                return false;
            }
            if self.shift && !flags.contains(CGEventFlags::CGEventFlagShift) {
                return false;
            }
            if self.option && !flags.contains(CGEventFlags::CGEventFlagAlternate) {
                return false;
            }
            if self.command && !flags.contains(CGEventFlags::CGEventFlagCommand) {
                return false;
            }

            true
        }
    }

    fn letter_key_code(letter: char) -> u16 {
        match letter.to_ascii_lowercase() {
            'a' => 0x00,
            's' => 0x01,
            'd' => 0x02,
            'f' => 0x03,
            'h' => 0x04,
            'g' => 0x05,
            'z' => 0x06,
            'x' => 0x07,
            'c' => 0x08,
            'v' => 0x09,
            'b' => 0x0B,
            'q' => 0x0C,
            'w' => 0x0D,
            'e' => 0x0E,
            'r' => 0x0F,
            'y' => 0x10,
            't' => 0x11,
            '1' => 0x12,
            '2' => 0x13,
            '3' => 0x14,
            '4' => 0x15,
            '6' => 0x16,
            '5' => 0x17,
            '9' => 0x19,
            '7' => 0x1A,
            '8' => 0x1C,
            '0' => 0x1D,
            'o' => 0x1F,
            'u' => 0x20,
            'i' => 0x22,
            'p' => 0x23,
            'l' => 0x25,
            'j' => 0x26,
            'k' => 0x28,
            'n' => 0x2D,
            'm' => 0x2E,
            _ => KeyCode::SPACE,
        }
    }

    pub fn run_live_app(config: MacLiveAppConfig) -> Result<()> {
        unsafe {
            let _instance_guard = match SingleInstanceGuard::acquire()? {
                Some(guard) => guard,
                None => {
                    println!("检测到已有 VoiceInput macOS 实例正在运行，跳过重复启动");
                    return Ok(());
                }
            };

            let pool = NSAutoreleasePool::new(nil);
            let app = NSApp();
            app.setActivationPolicy_(NSApplicationActivationPolicyAccessory);
            configure_application_icon();

            if config.show_status_item {
                let status_bar = NSStatusBar::systemStatusBar(nil);
                let status_item = status_bar.statusItemWithLength_(NSVariableStatusItemLength);
                let button: id = status_item.button();
                configure_status_item_button(button);

                let menu = NSMenu::new(nil).autorelease();
                let quit_title = NSString::alloc(nil).init_str("退出");
                let quit_key = NSString::alloc(nil).init_str("q");
                let quit_item = NSMenuItem::alloc(nil)
                    .initWithTitle_action_keyEquivalent_(quit_title, sel!(terminate:), quit_key)
                    .autorelease();
                menu.addItem_(quit_item);
                status_item.setMenu_(menu);
            }

            let recorder = MicAudioRecorder::new(config.max_recording_duration);
            let bridge: Box<dyn MacImeBridge> = Box::new(ClipboardMacImeBridge::default());
            let host = MacInputMethodHost::new_with_bridge(config.host.clone(), bridge);
            println!("正在预加载 ASR 模型...");
            let asr_runner = PythonFunAsrRunner::connect(config.asr.clone())?;
            println!("ASR 模型预加载完成");
            println!("VoiceInput 常驻应用已启动");
            println!("热键：{}", config.app.activation_hotkey);
            println!("说明：按一次开始录音，按一次停止并转写");
            let transcriber =
                voice_input_asr::LocalFunAsrTranscriber::new(config.asr, Box::new(asr_runner));
            let controller = AppController::new(
                config.app,
                Box::new(MockHotkeyManager),
                Box::new(recorder.clone()),
                Box::new(transcriber),
                Box::new(host),
            );

            let hotkey = HotkeySpec::parse(&controller.config.activation_hotkey)?;
            let state = Arc::new(RuntimeState::default());

            spawn_hotkey_listener(hotkey, state.clone(), recorder.clone())?;

            let mut main_context = Box::new(MainLoopContext { controller, state });
            let main_context_ptr = &mut *main_context as *mut MainLoopContext as *mut c_void;

            let now = CFDate::now().abs_time();
            let mut timer_context = CFRunLoopTimerContext {
                version: 0,
                info: main_context_ptr,
                retain: None,
                release: None,
                copyDescription: None,
            };

            let timer = CFRunLoopTimer::new(now + 0.05, 0.05, 0, 0, pump_timer, &mut timer_context);
            CFRunLoop::get_current().add_timer(&timer, kCFRunLoopCommonModes);

            let _: () = msg_send![app, activateIgnoringOtherApps: YES];
            app.run();
            let _: () = msg_send![pool, drain];
            drop(main_context);
        }

        Ok(())
    }

    fn spawn_hotkey_listener(
        hotkey: HotkeySpec,
        state: Arc<RuntimeState>,
        recorder: MicAudioRecorder,
    ) -> Result<()> {
        let handle = thread::Builder::new()
            .name("voiceinput-hotkey".to_string())
            .spawn(move || {
                let tap = match CGEventTap::new(
                    CGEventTapLocation::HID,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::Default,
                    vec![CGEventType::KeyDown],
                    move |_proxy, event_type, event| {
                        if !matches!(event_type, CGEventType::KeyDown) {
                            return None;
                        }

                        if event.get_integer_value_field(EventField::KEYBOARD_EVENT_AUTOREPEAT) != 0
                        {
                            return None;
                        }

                        let key_code = event
                            .get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE)
                            as u16;
                        let flags = event.get_flags();

                        if !hotkey.matches(key_code, flags) {
                            return None;
                        }

                        if state.job_active.load(Ordering::SeqCst) {
                            if recorder.is_recording() {
                                println!("收到停止热键");
                                recorder.stop();
                            }
                        } else {
                            println!("收到启动热键");
                            state.pending_start.store(true, Ordering::SeqCst);
                        }

                        None
                    },
                ) {
                    Ok(tap) => tap,
                    Err(_) => {
                        eprintln!("创建全局热键监听失败，请检查辅助功能权限");
                        return;
                    }
                };

                let run_loop = CFRunLoop::get_current();
                let loop_source = match tap.mach_port.create_runloop_source(0) {
                    Ok(source) => source,
                    Err(_) => {
                        eprintln!("创建热键监听运行循环源失败");
                        return;
                    }
                };

                run_loop.add_source(&loop_source, unsafe { kCFRunLoopCommonModes });
                tap.enable();
                CFRunLoop::run_current();
            })
            .map_err(|e| VoiceInputError::Hotkey(format!("启动热键监听线程失败：{e}")))?;

        drop(handle);
        Ok(())
    }

    extern "C" fn pump_timer(_timer: CFRunLoopTimerRef, raw_info: *mut c_void) {
        if raw_info.is_null() {
            return;
        }

        let context: &mut MainLoopContext = unsafe { &mut *(raw_info as *mut MainLoopContext) };
        if !context.state.pending_start.swap(false, Ordering::SeqCst) {
            return;
        }

        if context.state.job_active.swap(true, Ordering::SeqCst) {
            return;
        }

        println!("开始录音并等待停止热键");
        let result = context.controller.process_once();
        match result {
            Ok(text) => {
                println!("识别完成：{text}");
            }
            Err(err) => {
                eprintln!("实时语音输入失败：{err}");
            }
        }

        context.state.job_active.store(false, Ordering::SeqCst);
    }
}

#[cfg(target_os = "macos")]
pub use mac_runtime::{run_live_app, MacLiveAppConfig};

#[cfg(not(target_os = "macos"))]
use voice_input_core::Result;

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, Default)]
pub struct MacLiveAppConfig;

#[cfg(not(target_os = "macos"))]
pub fn run_live_app(_config: MacLiveAppConfig) -> Result<()> {
    Err(voice_input_core::VoiceInputError::Injection(
        "macOS 常驻应用只支持在 macOS 上运行".to_string(),
    ))
}
