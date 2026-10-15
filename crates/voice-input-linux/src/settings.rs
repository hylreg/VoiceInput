use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LinuxAppSettings {
    pub double_press_window_ms: u64,
    pub silence_stop_timeout_ms: u64,
    /// 本机麦克风（双击 Ctrl）是否生效。
    pub mic_enabled: bool,
    /// 手机桥接是否生效。关掉时端口仍监听，POST /send 返回 409。
    pub bridge_enabled: bool,
    /// 桥接监听端口。
    pub bridge_port: u16,
    /// 手机端地址里的口令，同时也是路径第一段。8-32 位 ASCII 字母数字与 `-_`。
    /// 空串表示尚未生成。
    pub bridge_token: String,
    /// 手机端地址用的稳定主机名（如 tailnet 名）。空串表示没设，届时回落到
    /// 本机探测到的 IP。**刻意不自动填 IP**——IP 是 DHCP 动态的，写进去就是错的。
    pub bridge_host: String,
}

// 手写 Debug 而不 derive：bridge_token 是桥接监听器（0.0.0.0:8765，局域网/tailnet
// 都能连）唯一的门禁，而日志最终会落到磁盘上的 journal，规范要求日志绝不记录敏感内容，
// 所以这里只打印长度，绝不回显口令明文。
impl std::fmt::Debug for LinuxAppSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinuxAppSettings")
            .field("double_press_window_ms", &self.double_press_window_ms)
            .field("silence_stop_timeout_ms", &self.silence_stop_timeout_ms)
            .field("mic_enabled", &self.mic_enabled)
            .field("bridge_enabled", &self.bridge_enabled)
            .field("bridge_port", &self.bridge_port)
            .field(
                "bridge_token",
                &format_args!("<redacted; {} chars>", self.bridge_token.len()),
            )
            // 主机名不是秘密，直接打出来——排查「手机打不开」时要看的就是它。
            .field("bridge_host", &self.bridge_host)
            .finish()
    }
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
            bridge_host: String::new(),
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

        // 配置文件里带着 bridge_token，而桥接监听器绑在 0.0.0.0:8765，局域网和
        // tailnet 都能连，口令是唯一门禁。本机 umask 通常是 0002，直接 fs::write
        // 会落成 0664，任何本地用户都能读到口令，所以这里强制 0600。
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            // mode(0o600) 只在「创建」时生效；对已存在的 0664 老文件它不会收紧权限，
            // 所以打开后必须再显式 set_permissions(0o600)，否则老装机仍是全局可读。
            // 收紧必须排在 write_all 之前：老文件在写入期间仍是旧权限，先写后收紧会
            // 留下一个 token 已落盘却全局可读的窗口。
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .map_err(|err| format!("写入配置文件失败 {}：{err}", path.display()))?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|err| format!("写入配置文件失败 {}：{err}", path.display()))?;
            file.write_all(content.as_bytes())
                .map_err(|err| format!("写入配置文件失败 {}：{err}", path.display()))?;
            // 必须 fsync：新生成的口令就落在这份文件里，直接 drop 句柄只把它
            // 交给内核回写窗口，此时崩溃/断电会丢口令，下次启动再生成一个新的，
            // 手机上所有书签地址就全失效了。
            file.sync_all()
                .map_err(|err| format!("写入配置文件失败 {}：{err}", path.display()))?;
            Ok(())
        }

        #[cfg(not(unix))]
        {
            // 非 unix 分支用 fs::write 一把梭，没法拿到文件句柄，也就无从 fsync。
            // 该平台本来也没有 umask/权限那套顾虑，保持现状。
            fs::write(&path, content)
                .map_err(|err| format!("写入配置文件失败 {}：{err}", path.display()))
        }
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
        // 空串表示「尚未生成」，由 prepare_bridge 在启动时填充后落盘。
        assert_eq!(LinuxAppSettings::default().bridge_token, "");
    }

    #[test]
    fn bridge_token_round_trips_through_toml() {
        // 老配置没有 bridge_token 键，必须回落空串（兼容旧装机）。
        let without_token = "double_press_window_ms = 300\n";
        let settings: LinuxAppSettings =
            toml::from_str(without_token).expect("缺 bridge_token 的老配置应当能反序列化");
        assert_eq!(settings.bridge_token, "");

        // 带口令的配置必须原样读回，否则桥接会拿错凭证。
        let token = "lanhexin";
        let with_token = format!("bridge_token = \"{token}\"\n");
        let settings: LinuxAppSettings =
            toml::from_str(&with_token).expect("带 bridge_token 的配置应当能反序列化");
        assert_eq!(settings.bridge_token, token);
    }

    #[test]
    fn bridge_host_defaults_to_empty() {
        // 空串是「没设，用探测到的 IP」的哨兵。默认绝不能填 IP——
        // IP 是 DHCP 动态的，写进配置第二天就是错的。
        assert_eq!(LinuxAppSettings::default().bridge_host, "");
    }

    #[test]
    fn bridge_host_round_trips_through_toml() {
        let without_host = "bridge_port = 8765\n";
        let settings: LinuxAppSettings =
            toml::from_str(without_host).expect("缺 bridge_host 的老配置应当能反序列化");
        assert_eq!(settings.bridge_host, "");

        let with_host = "bridge_host = \"leju.tail3e3e47.ts.net\"\n";
        let settings: LinuxAppSettings =
            toml::from_str(with_host).expect("带 bridge_host 的配置应当能反序列化");
        assert_eq!(settings.bridge_host, "leju.tail3e3e47.ts.net");
    }

    #[test]
    fn debug_redacts_pass_phrase_but_shows_host() {
        // 口令是门禁，日志落盘，绝不回显；主机名不是秘密，排查「手机打不开」要看它。
        let settings = LinuxAppSettings {
            bridge_token: "lanhexin".to_string(),
            bridge_host: "leju.tail3e3e47.ts.net".to_string(),
            ..LinuxAppSettings::default()
        };

        let rendered = format!("{settings:?}");
        assert!(!rendered.contains("lanhexin"), "实际：{rendered}");
        assert!(rendered.contains("<redacted; 8 chars>"), "实际：{rendered}");
        assert!(
            rendered.contains("leju.tail3e3e47.ts.net"),
            "实际：{rendered}"
        );
    }
}
