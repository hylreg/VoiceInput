use std::process::Command;

/// ibus 主进程的 comm 名（`/proc/<pid>/comm` 截断到 15 字节，此名未超限）。
pub const IBUS_DAEMON_COMM: &str = "ibus-daemon";

/// 判断一组 comm 名里是否存在 ibus 主进程。
pub fn names_contain_ibus_daemon<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    names.into_iter().any(|name| name == IBUS_DAEMON_COMM)
}

/// 系统输入法（ibus）控制器。可用性由「是否有 ibus-daemon 在跑」决定。
#[derive(Debug, Clone)]
pub struct SystemImeController {
    available: bool,
}

impl SystemImeController {
    /// 扫描 `/proc/*/comm` 做检测。
    ///
    /// 不用 `ibus address` 探测：daemon 卡死时该 D-Bus 调用会阻塞到超时（约 25 秒），
    /// 而探测发生在 `menu()` 里，会把托盘线程挂住。
    pub fn detect() -> Self {
        let names = std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| std::fs::read_to_string(entry.path().join("comm")).ok())
            .collect::<Vec<_>>();
        let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        Self::from_probe(&refs)
    }

    /// 从给定 comm 名列表构造，便于单测。
    pub fn from_probe(names: &[&str]) -> Self {
        Self {
            available: names_contain_ibus_daemon(names.iter().copied()),
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    /// 不可用时拼在菜单标签后面的后缀。
    pub fn unavailable_label(&self) -> &'static str {
        "（未检测到 ibus）"
    }

    /// 调用 `ibus restart` 重启系统输入法。不影响 VoiceInput 自身。
    pub fn request(&self) -> Result<(), String> {
        if !self.available {
            return Err("未检测到 ibus，无法重启系统输入法".to_string());
        }

        let status = Command::new("ibus")
            .arg("restart")
            .status()
            .map_err(|e| format!("执行 ibus restart 失败：{e}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("ibus restart 返回非零退出码：{status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_with_daemon_matches() {
        assert!(names_contain_ibus_daemon(["ibus-daemon"]));
    }

    #[test]
    fn probe_with_daemon_among_others_matches() {
        assert!(names_contain_ibus_daemon([
            "gnome-shell",
            "ibus-daemon",
            "ibus-x11",
        ]));
    }

    #[test]
    fn probe_with_only_child_processes_does_not_match() {
        assert!(!names_contain_ibus_daemon([
            "ibus-dconf",
            "ibus-x11",
            "ibus-portal",
        ]));
    }

    #[test]
    fn probe_with_empty_list_does_not_match() {
        assert!(!names_contain_ibus_daemon([] as [&str; 0]));
    }

    #[test]
    fn probe_does_not_match_prefix_comm() {
        assert!(!names_contain_ibus_daemon(["ibus-daemon-x"]));
    }

    #[test]
    fn controller_available_when_daemon_present() {
        let controller = SystemImeController::from_probe(&["ibus-daemon"]);

        assert!(controller.is_available());
    }

    #[test]
    fn controller_unavailable_when_daemon_absent() {
        let controller = SystemImeController::from_probe(&["ibus-x11"]);

        assert!(!controller.is_available());
    }

    #[test]
    fn unavailable_label_explains_requirement() {
        let controller = SystemImeController::from_probe(&[]);

        assert_eq!(controller.unavailable_label(), "（未检测到 ibus）");
    }

    #[test]
    fn request_fails_when_unavailable() {
        let controller = SystemImeController::from_probe(&[]);

        let err = controller.request().expect_err("不可用时应当报错");
        assert!(err.contains("ibus"), "错误信息应当提到 ibus，实际：{err}");
    }
}
