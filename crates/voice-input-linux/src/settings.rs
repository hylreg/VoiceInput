use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LinuxAppSettings {
    pub double_ctrl_window_ms: u64,
    pub silence_stop_timeout_ms: u64,
}

impl Default for LinuxAppSettings {
    fn default() -> Self {
        Self {
            double_ctrl_window_ms: 200,
            silence_stop_timeout_ms: 900,
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
            fs::create_dir_all(parent).map_err(|err| {
                format!("创建配置目录失败 {}：{err}", parent.display())
            })?;
        }

        let content = toml::to_string_pretty(self)
            .map_err(|err| format!("序列化配置失败：{err}"))?;
        fs::write(&path, content).map_err(|err| {
            format!("写入配置文件失败 {}：{err}", path.display())
        })
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
