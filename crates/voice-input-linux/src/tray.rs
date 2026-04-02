#![allow(unexpected_cfgs)]

#[cfg(target_os = "linux")]
mod linux_tray {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use ksni::blocking::TrayMethods;
    use ksni::menu::StandardItem;
    use ksni::{Category, MenuItem, Status, ToolTip, Tray};

    use crate::recorder::LinuxMicAudioRecorder;
    use voice_input_core::{Result, VoiceInputError};

    #[derive(Debug, Clone)]
    pub struct LinuxTrayConfig {
        pub service_name: String,
        pub title: String,
        pub recorder: LinuxMicAudioRecorder,
        pub quit_requested: Arc<AtomicBool>,
    }

    impl LinuxTrayConfig {
        pub fn new(
            service_name: impl Into<String>,
            title: impl Into<String>,
            recorder: LinuxMicAudioRecorder,
            quit_requested: Arc<AtomicBool>,
        ) -> Self {
            Self {
                service_name: service_name.into(),
                title: title.into(),
                recorder,
                quit_requested,
            }
        }
    }

    pub struct LinuxTrayHandle {
        handle: ksni::blocking::Handle<LinuxTray>,
        quit_requested: Arc<AtomicBool>,
    }

    impl LinuxTrayHandle {
        pub fn spawn(config: LinuxTrayConfig) -> Result<Self> {
            let tray = LinuxTray::new(config);
            let quit_requested = Arc::clone(&tray.quit_requested);
            let handle = tray
                .disable_dbus_name(false)
                .assume_sni_available(true)
                .spawn()
                .map_err(|err| VoiceInputError::Injection(format!("启动 Linux 状态栏失败：{err}")))?;

            Ok(Self {
                handle,
                quit_requested,
            })
        }

        pub fn set_recording(&self, recording: bool) {
            let _ = self.handle.update(|tray| {
                tray.recording = recording;
            });
        }

        pub fn request_quit(&self) {
            self.quit_requested.store(true, Ordering::SeqCst);
            let _ = self.handle.update(|tray| {
                tray.quit_requested.store(true, Ordering::SeqCst);
            });
        }

        pub fn is_quit_requested(&self) -> bool {
            self.quit_requested.load(Ordering::SeqCst)
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
    }

    impl LinuxTray {
        fn new(config: LinuxTrayConfig) -> Self {
            Self {
                service_name: config.service_name,
                title: config.title,
                recording: false,
                recorder: config.recorder,
                quit_requested: config.quit_requested,
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
}

#[cfg(target_os = "linux")]
pub use linux_tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};

#[cfg(not(target_os = "linux"))]
mod not_linux {
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use crate::recorder::LinuxMicAudioRecorder;
    use voice_input_core::{Result, VoiceInputError};

    #[derive(Debug, Clone)]
    pub struct LinuxTrayConfig {
        pub service_name: String,
        pub title: String,
        pub recorder: LinuxMicAudioRecorder,
        pub quit_requested: Arc<AtomicBool>,
    }

    impl LinuxTrayConfig {
        pub fn new(
            service_name: impl Into<String>,
            title: impl Into<String>,
            recorder: LinuxMicAudioRecorder,
            quit_requested: Arc<AtomicBool>,
        ) -> Self {
            Self {
                service_name: service_name.into(),
                title: title.into(),
                recorder,
                quit_requested,
            }
        }
    }

    pub struct LinuxTrayHandle;

    impl LinuxTrayHandle {
        pub fn spawn(_config: LinuxTrayConfig) -> Result<Self> {
            Err(VoiceInputError::Injection(
                "Linux 状态栏只支持 Linux".to_string(),
            ))
        }

        pub fn set_recording(&self, _recording: bool) {}

        pub fn request_quit(&self) {}

        pub fn is_quit_requested(&self) -> bool {
            false
        }

        pub fn shutdown(&self) {}
    }

    pub fn spawn_linux_tray(_config: LinuxTrayConfig) -> Result<LinuxTrayHandle> {
        Err(VoiceInputError::Injection(
            "Linux 状态栏只支持 Linux".to_string(),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
pub use not_linux::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
