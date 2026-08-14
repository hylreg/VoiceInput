mod backend;
mod host;
mod hotkey;
mod ibus;
mod live;
mod live_cli;
mod local;
mod recorder;
mod runtime;
mod settings;
mod smoke;
mod tray;

pub use backend::{parse_backend_kind, LinuxBackend, LinuxBackendKind, MockLinuxBackend};
pub use host::{LinuxHostConfig, LinuxInputMethodHost};
pub use hotkey::{LinuxHotkeySpec, LinuxHotkeyWatcher};
pub use ibus::{
    IbusBackend, IbusEngineEvent, IbusEngineSpec, MockIbusBridge,
};
pub use live::{LiveJobHandle, LiveJobState};
pub use live_cli::{parse_live_args, run_live_with_args, LinuxLiveArgs};
pub use local::{
    build_local_python_runtime_config, LinuxLocalVoiceInput, LinuxLocalVoiceInputConfig,
    LocalVoiceInputConfig, parse_audio_file_with_optional_backend_arg,
};
pub use recorder::{FileAudioRecorder, LinuxMicAudioRecorder};
pub use runtime::{run_live_app, LinuxLiveAppConfig};
pub use settings::{settings_path, LinuxAppSettings};
pub use smoke::run_smoke;
pub use tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
