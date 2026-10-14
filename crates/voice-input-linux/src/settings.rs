use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LinuxAppSettings {
    pub double_press_window_ms: u64,
    pub silence_stop_timeout_ms: u64,
    /// 本机麦克风（双击 Ctrl）是否生效。
    pub mic_enabled: bool,
    /// 手机桥接是否生效。关掉时端口仍监听，POST /send 返回 409。
    pub bridge_enabled: bool,
    /// 桥接监听端口。IP 不落盘——网卡是 DHCP 动态的，每次用时现算。
    pub bridge_port: u16,
    /// 访问 token，32 位十六进制。空串表示尚未生成。
    pub bridge_token: String,
}

impl Default for LinuxAppSettings {
    fn default() -> Self {
        Self {
            double_press_window_ms: 300,
            silence_stop_timeout_ms: 1500,
            mic_enabled: true,
            bridge_enabled: true,
            bridge_port: 8765,
            bridge_token: String::new(),
        }
    }
}

impl LinuxAppSettings {
    pub fn load() -> Self {
        let path = settings_path();
        match fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|err| format!("创建配置目录失败 {}：{err}", parent.display()))?;
        }

        let content =
            toml::to_string_pretty(self).map_err(|err| format!("序列化配置失败：{err}"))?;
        fs::write(&path, content)
            .map_err(|err| format!("写入配置文件失败 {}：{err}", path.display()))
    }
}

pub fn settings_path() -> PathBuf {
    if let Some(config_home) = env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(config_home)
            .join("voice-input")
            .join("linux-app.toml");
    }

    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("voice-input")
            .join("linux-app.toml");
    }

    PathBuf::from("voice-input-linux-app.toml")
}

#[cfg(test)]
mod tests {
    use super::LinuxAppSettings;

    #[test]
    fn defaults_enable_mic_and_bridge() {
        let settings = LinuxAppSettings::default();

        assert!(settings.mic_enabled);
        assert!(settings.bridge_enabled);
    }

    #[test]
    fn default_bridge_port_is_8765() {
        assert_eq!(LinuxAppSettings::default().bridge_port, 8765);
    }

    #[test]
    fn old_config_without_new_fields_still_loads() {
        // 老版本写出的配置只有两个字段，必须能读进来并回落默认。
        let content = "double_press_window_ms = 300\nsilence_stop_timeout_ms = 1500\n";

        let settings: LinuxAppSettings = toml::from_str(content).expect("老配置应当能反序列化");

        assert_eq!(settings.double_press_window_ms, 300);
        assert!(settings.mic_enabled);
        assert!(settings.bridge_enabled);
        assert_eq!(settings.bridge_port, 8765);
    }

    #[test]
    fn bridge_token_defaults_to_empty() {
        // 空 token 表示「尚未生成」，由 BridgeHandle 在启动时填充后落盘。
        assert_eq!(LinuxAppSettings::default().bridge_token, "");
    }
}
