mod linux_tray {
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use ksni::blocking::TrayMethods;
    use ksni::menu::StandardItem;
    use ksni::{Category, MenuItem, Status, ToolTip, Tray};

    use crate::bridge_addr::bridge_url;
    use crate::recorder::LinuxMicAudioRecorder;
    use crate::restart::RestartController;
    use crate::settings::LinuxAppSettings;
    use crate::system_ime::SystemImeController;
    use voice_input_core::{Result, VoiceInputError};

    #[derive(Clone)]
    pub struct LinuxTrayConfig {
        pub service_name: String,
        pub title: String,
        pub recorder: LinuxMicAudioRecorder,
        pub quit_requested: Arc<AtomicBool>,
        pub restart: RestartController,
        pub restart_requested: Arc<AtomicBool>,
        pub ime: SystemImeController,
        /// 本机麦克风开关的共享状态，托盘与主循环共读。
        pub mic_enabled: Arc<AtomicBool>,
        /// 手机桥接开关的共享状态。
        pub bridge_enabled: Arc<AtomicBool>,
        /// 端口是否绑定成功。false 时相关菜单项置灰。
        pub bridge_available: bool,
        pub bridge_port: u16,
        pub bridge_token: String,
        /// 用户显式指定的稳定主机名（如 tailnet 名）。空串表示没设，
        /// 此时地址回落到 `local_ip`。刻意不自动填 IP——IP 是 DHCP 动态的。
        pub bridge_host: String,
        /// 本机局域网 IP。`None` 表示探测失败。
        pub local_ip: Option<String>,
        /// 初始开关状态，用于预填托盘的本地副本。
        pub settings: LinuxAppSettings,
    }

    // 手写 Debug 而不 derive：`bridge_token` 是桥接监听器唯一的门禁，
    // 日志最终落到磁盘上的 journal，规范要求日志绝不记录敏感内容。
    // 与 settings.rs / bridge.rs 的取舍一致，只打印长度。
    impl std::fmt::Debug for LinuxTrayConfig {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("LinuxTrayConfig")
                .field("service_name", &self.service_name)
                .field("title", &self.title)
                .field("recorder", &self.recorder)
                .field("quit_requested", &self.quit_requested)
                .field("restart", &self.restart)
                .field("restart_requested", &self.restart_requested)
                .field("ime", &self.ime)
                .field("mic_enabled", &self.mic_enabled)
                .field("bridge_enabled", &self.bridge_enabled)
                .field("bridge_available", &self.bridge_available)
                .field("bridge_port", &self.bridge_port)
                .field(
                    "bridge_token",
                    &format_args!("<redacted; {} chars>", self.bridge_token.len()),
                )
                .field("bridge_host", &self.bridge_host)
                .field("local_ip", &self.local_ip)
                .field("settings", &self.settings)
                .finish()
        }
    }

    impl LinuxTrayConfig {
        #[allow(clippy::too_many_arguments)]
        pub fn new(
            service_name: impl Into<String>,
            title: impl Into<String>,
            recorder: LinuxMicAudioRecorder,
            quit_requested: Arc<AtomicBool>,
            restart: RestartController,
            restart_requested: Arc<AtomicBool>,
            ime: SystemImeController,
            mic_enabled: Arc<AtomicBool>,
            bridge_enabled: Arc<AtomicBool>,
            bridge_available: bool,
            bridge_port: u16,
            bridge_token: String,
            bridge_host: String,
            local_ip: Option<String>,
            settings: LinuxAppSettings,
        ) -> Self {
            Self {
                service_name: service_name.into(),
                title: title.into(),
                recorder,
                quit_requested,
                restart,
                restart_requested,
                ime,
                mic_enabled,
                bridge_enabled,
                bridge_available,
                bridge_port,
                bridge_token,
                bridge_host,
                local_ip,
                settings,
            }
        }
    }

    pub struct LinuxTrayHandle {
        handle: ksni::blocking::Handle<LinuxTray>,
        quit_requested: Arc<AtomicBool>,
        restart_requested: Arc<AtomicBool>,
        mic_enabled: Arc<AtomicBool>,
        bridge_enabled: Arc<AtomicBool>,
    }

    impl LinuxTrayHandle {
        pub fn spawn(config: LinuxTrayConfig) -> Result<Self> {
            let mic_enabled = Arc::clone(&config.mic_enabled);
            let bridge_enabled = Arc::clone(&config.bridge_enabled);
            let tray = LinuxTray::new(config);
            let quit_requested = Arc::clone(&tray.quit_requested);
            let restart_requested = Arc::clone(&tray.restart_requested);
            let handle = tray
                .disable_dbus_name(false)
                .assume_sni_available(true)
                .spawn()
                .map_err(|err| {
                    VoiceInputError::Injection(format!("启动 Linux 状态栏失败：{err}"))
                })?;

            Ok(Self {
                handle,
                quit_requested,
                restart_requested,
                mic_enabled,
                bridge_enabled,
            })
        }

        pub fn set_recording(&self, recording: bool) {
            let _ = self.handle.update(|tray| {
                tray.recording = recording;
            });
        }

        pub fn is_quit_requested(&self) -> bool {
            self.quit_requested.load(Ordering::SeqCst)
        }

        pub fn is_restart_requested(&self) -> bool {
            self.restart_requested.load(Ordering::SeqCst)
        }

        pub fn is_mic_enabled(&self) -> bool {
            self.mic_enabled.load(Ordering::SeqCst)
        }

        pub fn is_bridge_enabled(&self) -> bool {
            self.bridge_enabled.load(Ordering::SeqCst)
        }

        /// 把内存里的开关状态落盘。只在正常退出路径调用——
        /// 每次点击都写盘会在目录被 inotify 监视时引发连锁反应，
        /// 而主循环反正每轮读内存变量，即时写盘对生效没有帮助。
        pub fn persist_settings(&self) {
            // update 返回 None 表示状态栏服务已经退出（SNI 崩溃或连接已关）。
            // 此时闭包根本没执行，只靠内层的 save() 报错是看不到的——
            // 用户的开关会悄无声息地丢掉，所以这一层也必须报。
            if self
                .handle
                .update(|tray| {
                    let snapshot = tray.settings.borrow().clone();
                    if let Err(err) = snapshot.save() {
                        eprintln!("保存 Linux 配置失败：{err}");
                    }
                })
                .is_none()
            {
                eprintln!("保存 Linux 配置失败：状态栏服务已退出，开关未落盘");
            }
        }

        pub fn shutdown(&self) {
            self.handle.shutdown().wait();
        }
    }

    struct LinuxTray {
        service_name: String,
        title: String,
        recording: bool,
        recorder: LinuxMicAudioRecorder,
        quit_requested: Arc<AtomicBool>,
        restart: RestartController,
        restart_requested: Arc<AtomicBool>,
        ime: SystemImeController,
        // 开关用 Cell：ksni 在每次 clicked 之后都会重算菜单（service.rs 的
        // event() → update()），menu() 拿到的一定是最新值，不需要原子类型。
        mic_enabled: Cell<bool>,
        bridge_enabled: Cell<bool>,
        /// 与 Cell 同步的共享状态，服务线程与主循环读它。
        mic_enabled_shared: Arc<AtomicBool>,
        bridge_enabled_shared: Arc<AtomicBool>,
        bridge_available: bool,
        bridge_port: u16,
        bridge_token: String,
        bridge_host: String,
        local_ip: Option<String>,
        /// 与共享 AtomicBool 同步的 settings 副本，退出时整体落盘。
        settings: RefCell<LinuxAppSettings>,
    }

    impl LinuxTray {
        fn new(config: LinuxTrayConfig) -> Self {
            let mic_enabled_shared = Arc::clone(&config.mic_enabled);
            let bridge_enabled_shared = Arc::clone(&config.bridge_enabled);

            Self {
                service_name: config.service_name,
                title: config.title,
                recording: false,
                recorder: config.recorder,
                quit_requested: config.quit_requested,
                restart: config.restart,
                restart_requested: config.restart_requested,
                ime: config.ime,
                mic_enabled: Cell::new(config.settings.mic_enabled),
                bridge_enabled: Cell::new(config.settings.bridge_enabled),
                mic_enabled_shared,
                bridge_enabled_shared,
                bridge_available: config.bridge_available,
                bridge_port: config.bridge_port,
                bridge_token: config.bridge_token,
                bridge_host: config.bridge_host,
                local_ip: config.local_ip,
                settings: RefCell::new(config.settings),
            }
        }

        fn status_label(&self) -> String {
            if self.recording {
                format!("{} 正在录音", self.title)
            } else {
                format!("{} 已就绪", self.title)
            }
        }

        fn stop_label(&self) -> String {
            if self.recording {
                "停止当前录音".to_string()
            } else {
                "当前未在录音".to_string()
            }
        }

        fn restart_label(&self) -> String {
            if self.restart.is_available() {
                "重启输入法".to_string()
            } else {
                format!("重启输入法{}", self.restart.unavailable_label())
            }
        }

        fn restart_enabled(&self) -> bool {
            self.restart.is_available() && !self.recording
        }

        fn ime_label(&self) -> String {
            if self.ime.is_available() {
                "重启系统输入法".to_string()
            } else {
                format!("重启系统输入法{}", self.ime.unavailable_label())
            }
        }

        fn ime_enabled(&self) -> bool {
            self.ime.is_available() && !self.recording
        }

        fn mic_label(&self) -> String {
            format!(
                "本机麦克风：{}",
                if self.mic_enabled.get() {
                    "已启用"
                } else {
                    "已停用"
                }
            )
        }

        fn mic_toggle_enabled(&self) -> bool {
            !self.recording
        }

        fn toggle_mic(&mut self) {
            let next = !self.mic_enabled.get();
            self.mic_enabled.set(next);
            self.mic_enabled_shared.store(next, Ordering::SeqCst);
            self.settings.borrow_mut().mic_enabled = next;
        }

        fn bridge_label(&self) -> String {
            if !self.bridge_available {
                return "手机输入（端口被占用）".to_string();
            }
            format!(
                "手机输入：{}",
                if self.bridge_enabled.get() {
                    "已启用"
                } else {
                    "已停用"
                }
            )
        }

        fn bridge_toggle_enabled(&self) -> bool {
            self.bridge_available
        }

        fn toggle_bridge(&mut self) {
            let next = !self.bridge_enabled.get();
            self.bridge_enabled.set(next);
            self.bridge_enabled_shared.store(next, Ordering::SeqCst);
            self.settings.borrow_mut().bridge_enabled = next;
        }

        fn copy_address_label(&self) -> String {
            format!("复制外部输入地址（{}）", self.bridge_port)
        }

        /// 只在端口绑定失败时置灰——此时地址没有意义（规范「开关语义」）。
        /// IP 探测失败仍保持可点：回落到 127.0.0.1 是本机自测页面用不上的唯一手段。
        fn copy_address_enabled(&self) -> bool {
            self.bridge_available
        }

        /// 地址在点击时现算——网卡是 DHCP 动态的，IP 会变。
        ///
        /// 主机名三级回落：配置里显式写的稳定名字（tailnet 名等）→ 本机探测到的
        /// IP（会随 DHCP 变，但不用配就能用）→ `127.0.0.1`（手机上打不开，
        /// 只为本机浏览器验证页面本身，这是唯一的本机自测手段）。
        ///
        /// 返回 `String` 而非 `Option`：回落让「拼不出地址」不可能发生，
        /// 用 `Option` 只会让每个调用方都写一段永远走不到的分支。
        fn address_url(&self) -> String {
            let host = if self.bridge_host.trim().is_empty() {
                self.local_ip
                    .clone()
                    .unwrap_or_else(|| "127.0.0.1".to_string())
            } else {
                self.bridge_host.clone()
            };
            bridge_url(&host, self.bridge_port, &self.bridge_token)
        }
    }

    impl Tray for LinuxTray {
        const MENU_ON_ACTIVATE: bool = true;

        fn id(&self) -> String {
            format!("{}-status", self.service_name)
        }

        fn category(&self) -> Category {
            Category::ApplicationStatus
        }

        fn title(&self) -> String {
            self.title.clone()
        }

        fn status(&self) -> Status {
            if self.recording {
                Status::NeedsAttention
            } else {
                Status::Active
            }
        }

        fn icon_name(&self) -> String {
            if self.recording {
                "media-record".to_string()
            } else {
                "input-keyboard".to_string()
            }
        }

        fn tool_tip(&self) -> ToolTip {
            ToolTip {
                icon_name: self.icon_name(),
                icon_pixmap: Vec::new(),
                title: self.status_label(),
                description: if self.recording {
                    "当前正在录音，点击菜单里的停止项可以结束这次录音".to_string()
                } else {
                    "按全局热键开始录音".to_string()
                },
            }
        }

        fn menu(&self) -> Vec<MenuItem<Self>> {
            let mut items = Vec::new();

            items.push(
                StandardItem {
                    label: self.status_label(),
                    enabled: false,
                    visible: true,
                    icon_name: self.icon_name(),
                    ..Default::default()
                }
                .into(),
            );

            items.push(
                StandardItem {
                    label: self.stop_label(),
                    enabled: self.recording,
                    visible: true,
                    icon_name: "media-playback-stop".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        tray.recorder.stop();
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(MenuItem::Separator);

            items.push(
                StandardItem {
                    label: self.mic_label(),
                    enabled: self.mic_toggle_enabled(),
                    visible: true,
                    icon_name: "audio-input-microphone".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        tray.toggle_mic();
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(
                StandardItem {
                    label: self.bridge_label(),
                    enabled: self.bridge_toggle_enabled(),
                    visible: true,
                    icon_name: "phone".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        tray.toggle_bridge();
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(
                StandardItem {
                    label: self.copy_address_label(),
                    enabled: self.copy_address_enabled(),
                    visible: true,
                    icon_name: "edit-copy".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        // IP 探测失败时地址回落到 127.0.0.1：手机打不开，但本机浏览器
                        // 能验证页面是否正常。提示必须先于写剪贴板打印，才能在写失败时也看到。
                        if tray.local_ip.is_none() {
                            eprintln!(
                                "外部输入地址：未探测到局域网 IP，已回退到 127.0.0.1（仅供本机浏览器验证）"
                            );
                        }
                        match crate::ibus::restore_clipboard_text(&tray.address_url()) {
                            // 日志不记 URL 内容——里面含 token。
                            Ok(()) => println!("外部输入地址已复制到剪贴板"),
                            Err(err) => eprintln!("复制外部输入地址失败：{err}"),
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(MenuItem::Separator);

            items.push(
                StandardItem {
                    label: self.restart_label(),
                    enabled: self.restart_enabled(),
                    visible: true,
                    icon_name: "view-refresh".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| match tray.restart.request() {
                        Ok(()) => tray.restart_requested.store(true, Ordering::SeqCst),
                        Err(err) => eprintln!("重启输入法失败：{err}"),
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(
                StandardItem {
                    label: self.ime_label(),
                    enabled: self.ime_enabled(),
                    visible: true,
                    icon_name: "input-keyboard".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        if let Err(err) = tray.ime.request() {
                            eprintln!("重启系统输入法失败：{err}");
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(MenuItem::Separator);

            items.push(
                StandardItem {
                    label: "退出".to_string(),
                    enabled: true,
                    visible: true,
                    icon_name: "application-exit".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        tray.quit_requested.store(true, Ordering::SeqCst);
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items
        }

        fn watcher_offline(&self, _reason: ksni::OfflineReason) -> bool {
            true
        }
    }

    pub fn spawn_linux_tray(config: LinuxTrayConfig) -> Result<LinuxTrayHandle> {
        LinuxTrayHandle::spawn(config)
    }

    #[cfg(test)]
    mod tests {
        use super::LinuxTray;
        use crate::recorder::LinuxMicAudioRecorder;
        use crate::restart::RestartController;
        use crate::settings::LinuxAppSettings;
        use crate::system_ime::SystemImeController;
        use std::cell::{Cell, RefCell};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        use std::time::Duration;

        fn tray_with(cgroup: &str, recording: bool, ibus_running: bool) -> LinuxTray {
            tray_with_bridge(
                cgroup,
                recording,
                ibus_running,
                true,
                true,
                Some("10.0.0.5"),
            )
        }

        fn tray_with_bridge(
            cgroup: &str,
            recording: bool,
            ibus_running: bool,
            mic_enabled: bool,
            bridge_enabled: bool,
            local_ip: Option<&str>,
        ) -> LinuxTray {
            let restart = RestartController::from_cgroup_content(cgroup, "voice-input.service");
            let ime = if ibus_running {
                SystemImeController::from_probe(&["ibus-daemon"])
            } else {
                SystemImeController::from_probe(&["ibus-x11"])
            };
            LinuxTray {
                service_name: "voice-input".to_string(),
                title: "VoiceInput".to_string(),
                recording,
                recorder: LinuxMicAudioRecorder::new(Duration::from_secs(30)),
                quit_requested: Arc::new(AtomicBool::new(false)),
                restart,
                restart_requested: Arc::new(AtomicBool::new(false)),
                ime,
                mic_enabled: Cell::new(mic_enabled),
                bridge_enabled: Cell::new(bridge_enabled),
                mic_enabled_shared: Arc::new(AtomicBool::new(mic_enabled)),
                bridge_enabled_shared: Arc::new(AtomicBool::new(bridge_enabled)),
                // 端口可用性与开关状态是两回事：关掉开关不代表端口没绑上。
                // 这里固定为 true，需要「端口被占用」场景的测试自行改写该字段。
                bridge_available: true,
                bridge_port: 8765,
                bridge_token: "abc123".to_string(),
                bridge_host: String::new(),
                local_ip: local_ip.map(str::to_string),
                settings: RefCell::new(LinuxAppSettings::default()),
            }
        }

        const MATCHING: &str =
            "0::/user.slice/user-1000.slice/user@1000.service/voice-input.service\n";

        #[test]
        fn restart_label_is_plain_when_available() {
            let tray = tray_with(MATCHING, false, true);
            assert_eq!(tray.restart_label(), "重启输入法");
        }

        #[test]
        fn restart_label_explains_when_unavailable() {
            let tray = tray_with("0::/user.slice/user@1000.service\n", false, true);
            assert_eq!(tray.restart_label(), "重启输入法（需 systemd 服务）");
        }

        #[test]
        fn restart_disabled_when_unavailable() {
            let tray = tray_with("0::/user.slice/user@1000.service\n", false, true);
            assert!(!tray.restart_enabled());
        }

        #[test]
        fn restart_disabled_while_recording() {
            let tray = tray_with(MATCHING, true, true);
            assert!(!tray.restart_enabled());
        }

        #[test]
        fn restart_enabled_when_available_and_idle() {
            let tray = tray_with(MATCHING, false, true);
            assert!(tray.restart_enabled());
        }

        #[test]
        fn ime_label_is_plain_when_available() {
            let tray = tray_with(MATCHING, false, true);
            assert_eq!(tray.ime_label(), "重启系统输入法");
        }

        #[test]
        fn ime_label_explains_when_unavailable() {
            let tray = tray_with(MATCHING, false, false);
            assert_eq!(tray.ime_label(), "重启系统输入法（未检测到 ibus）");
        }

        #[test]
        fn ime_disabled_when_unavailable() {
            let tray = tray_with(MATCHING, false, false);
            assert!(!tray.ime_enabled());
        }

        #[test]
        fn ime_disabled_while_recording() {
            let tray = tray_with(MATCHING, true, true);
            assert!(!tray.ime_enabled());
        }

        #[test]
        fn ime_enabled_when_available_and_idle() {
            let tray = tray_with(MATCHING, false, true);
            assert!(tray.ime_enabled());
        }

        #[test]
        fn mic_label_shows_enabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.mic_label(), "本机麦克风：已启用");
        }

        #[test]
        fn mic_label_shows_disabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, false, true, Some("10.0.0.5"));

            assert_eq!(tray.mic_label(), "本机麦克风：已停用");
        }

        #[test]
        fn toggling_mic_flips_label() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_mic();

            assert_eq!(tray.mic_label(), "本机麦克风：已停用");
            assert!(!tray.mic_enabled.get());
        }

        #[test]
        fn toggling_mic_twice_returns_to_enabled() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_mic();
            tray.toggle_mic();

            assert!(tray.mic_enabled.get());
        }

        #[test]
        fn toggle_mic_updates_shared_state() {
            // 主循环读的是共享 Arc<AtomicBool>，不是托盘里的 Cell。
            // 只断言 Cell 的话，将来有人删掉 `.store(...)` 也不会有测试变红。
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_mic();

            assert!(!tray.mic_enabled.get(), "Cell 应当翻转");
            assert!(
                !tray.mic_enabled_shared.load(Ordering::SeqCst),
                "共享状态必须同步翻转，否则主循环读到的还是旧值"
            );
        }

        #[test]
        fn toggle_bridge_updates_shared_state() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_bridge();

            assert!(!tray.bridge_enabled.get(), "Cell 应当翻转");
            assert!(
                !tray.bridge_enabled_shared.load(Ordering::SeqCst),
                "共享状态必须同步翻转，否则桥接服务读到的还是旧值"
            );
        }

        #[test]
        fn bridge_label_shows_enabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.bridge_label(), "手机输入：已启用");
        }

        #[test]
        fn bridge_label_shows_disabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, true, false, Some("10.0.0.5"));

            assert_eq!(tray.bridge_label(), "手机输入：已停用");
        }

        #[test]
        fn toggling_bridge_flips_label() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_bridge();

            assert_eq!(tray.bridge_label(), "手机输入：已停用");
        }

        #[test]
        fn bridge_label_explains_when_port_unavailable() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_available = false;

            assert_eq!(tray.bridge_label(), "手机输入（端口被占用）");
        }

        #[test]
        fn bridge_toggle_is_disabled_when_port_unavailable() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_available = false;

            assert!(!tray.bridge_toggle_enabled());
        }

        #[test]
        fn copy_address_enabled_when_available() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert!(tray.copy_address_enabled());
        }

        #[test]
        fn copy_address_disabled_when_port_unavailable() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_available = false;

            assert!(!tray.copy_address_enabled());
        }

        #[test]
        fn copy_address_stays_enabled_when_ip_unknown() {
            // 规范：置灰只看端口绑定，IP 探测失败仍可点，回落到本机自测地址。
            let tray = tray_with_bridge(MATCHING, false, true, true, true, None);

            assert!(tray.copy_address_enabled());
        }

        #[test]
        fn address_label_shows_port() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.copy_address_label(), "复制外部输入地址（8765）");
        }

        #[test]
        fn address_url_uses_current_ip() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.address_url(), "http://10.0.0.5:8765/abc123/");
        }

        #[test]
        fn address_url_falls_back_to_loopback_when_ip_unknown() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, None);

            assert_eq!(tray.address_url(), "http://127.0.0.1:8765/abc123/");
        }

        #[test]
        fn address_url_prefers_configured_host() {
            // 配了稳定主机名（tailnet 名等）就用它——IP 是 DHCP 动态的，
            // 名字才是「设一次就不用再管」的那条路。
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_host = "leju.tail3e3e47.ts.net".to_string();

            assert_eq!(
                tray.address_url(),
                "http://leju.tail3e3e47.ts.net:8765/abc123/"
            );
        }

        #[test]
        fn address_url_ignores_blank_configured_host() {
            // 空串是「没设」的哨兵值，不能把地址拼成 http://:8765/xxx/。
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_host = "   ".to_string();

            assert_eq!(tray.address_url(), "http://10.0.0.5:8765/abc123/");
        }

        #[test]
        fn address_url_never_carries_the_old_fragment() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            let url = tray.address_url();
            assert!(!url.contains('#'), "实际：{url}");
            assert!(!url.contains("k="), "实际：{url}");
        }

        #[test]
        fn toggles_are_also_disabled_while_recording() {
            let tray = tray_with_bridge(MATCHING, true, true, true, true, Some("10.0.0.5"));

            assert!(!tray.mic_toggle_enabled());
        }
    }
}

pub use linux_tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
