mod linux_tray {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use ksni::blocking::TrayMethods;
    use ksni::menu::StandardItem;
    use ksni::{Category, MenuItem, Status, ToolTip, Tray};

    use crate::recorder::LinuxMicAudioRecorder;
    use crate::restart::RestartController;
    use voice_input_core::{Result, VoiceInputError};

    #[derive(Debug, Clone)]
    pub struct LinuxTrayConfig {
        pub service_name: String,
        pub title: String,
        pub recorder: LinuxMicAudioRecorder,
        pub quit_requested: Arc<AtomicBool>,
        pub restart: RestartController,
        pub restart_requested: Arc<AtomicBool>,
    }

    impl LinuxTrayConfig {
        pub fn new(
            service_name: impl Into<String>,
            title: impl Into<String>,
            recorder: LinuxMicAudioRecorder,
            quit_requested: Arc<AtomicBool>,
            restart: RestartController,
            restart_requested: Arc<AtomicBool>,
        ) -> Self {
            Self {
                service_name: service_name.into(),
                title: title.into(),
                recorder,
                quit_requested,
                restart,
                restart_requested,
            }
        }
    }

    pub struct LinuxTrayHandle {
        handle: ksni::blocking::Handle<LinuxTray>,
        quit_requested: Arc<AtomicBool>,
        restart_requested: Arc<AtomicBool>,
    }

    impl LinuxTrayHandle {
        pub fn spawn(config: LinuxTrayConfig) -> Result<Self> {
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
    }

    impl LinuxTray {
        fn new(config: LinuxTrayConfig) -> Self {
            Self {
                service_name: config.service_name,
                title: config.title,
                recording: false,
                recorder: config.recorder,
                quit_requested: config.quit_requested,
                restart: config.restart,
                restart_requested: config.restart_requested,
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
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        use std::time::Duration;

        fn tray_with(cgroup: &str, recording: bool) -> LinuxTray {
            let restart = RestartController::from_cgroup_content(cgroup, "voice-input.service");
            LinuxTray {
                service_name: "voice-input".to_string(),
                title: "VoiceInput".to_string(),
                recording,
                recorder: LinuxMicAudioRecorder::new(Duration::from_secs(30)),
                quit_requested: Arc::new(AtomicBool::new(false)),
                restart,
                restart_requested: Arc::new(AtomicBool::new(false)),
            }
        }

        const MATCHING: &str =
            "0::/user.slice/user-1000.slice/user@1000.service/voice-input.service\n";

        #[test]
        fn restart_label_is_plain_when_available() {
            let tray = tray_with(MATCHING, false);
            assert_eq!(tray.restart_label(), "重启输入法");
        }

        #[test]
        fn restart_label_explains_when_unavailable() {
            let tray = tray_with("0::/user.slice/user@1000.service\n", false);
            assert_eq!(tray.restart_label(), "重启输入法（需 systemd 服务）");
        }

        #[test]
        fn restart_disabled_when_unavailable() {
            let tray = tray_with("0::/user.slice/user@1000.service\n", false);
            assert!(!tray.restart_enabled());
        }

        #[test]
        fn restart_disabled_while_recording() {
            let tray = tray_with(MATCHING, true);
            assert!(!tray.restart_enabled());
        }

        #[test]
        fn restart_enabled_when_available_and_idle() {
            let tray = tray_with(MATCHING, false);
            assert!(tray.restart_enabled());
        }
    }
}

pub use linux_tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
