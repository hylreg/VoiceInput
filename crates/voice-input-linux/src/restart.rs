use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// VoiceInput 常驻服务的 systemd 用户 unit 名。
pub const SERVICE_UNIT: &str = "voice-input.service";

/// 判断 cgroup 文件内容里，当前进程是否属于指定 unit。
///
/// 兼容两种格式：
/// - cgroup v1：`1:name=systemd:/user.slice/.../voice-input.service`
/// - cgroup v2：`0::/user.slice/.../voice-input.service`
pub fn cgroup_matches_unit(cgroup_content: &str, unit: &str) -> bool {
    let suffix = format!("/{unit}");

    cgroup_content.lines().any(|line| {
        // 每行规范格式为 `hierarchy-ID:controller-list:cgroup-path`，
        // 前两个字段不含 ':'；按字段切 3 段取第三段，
        // 路径本身含 ':' 时也能完整保留。
        let path = match line.splitn(3, ':').nth(2) {
            Some(path) => path,
            None => line,
        };

        path.ends_with(&suffix)
    })
}

/// 构造 `systemd-run` helper 的参数列表（不含 `systemd-run` 本身）。
///
/// helper 跑在独立 transient unit 里，因此不会被 restart 的 SIGTERM 波及。
/// `--collect` 保证 helper 结束后自动回收，不残留 failed unit。
pub fn restart_command(unit: &str, helper_unit: &str) -> Vec<String> {
    vec![
        "--user".to_string(),
        "--collect".to_string(),
        format!("--unit={helper_unit}"),
        "systemctl".to_string(),
        "--user".to_string(),
        "restart".to_string(),
        unit.to_string(),
    ]
}

/// 构造 transient helper unit 名。带 pid 与毫秒时间戳，
/// 避免同一进程连续两次触发时重名。
fn helper_unit_name(pid: u32, ts_millis: u128) -> String {
    format!("voice-input-restart-{pid}-{ts_millis}")
}

/// 重启控制器的可用性由「当前进程是否跑在目标 systemd unit 里」决定。
#[derive(Debug, Clone)]
pub struct RestartController {
    unit: Option<String>,
}

impl RestartController {
    /// 读取 `/proc/self/cgroup` 做检测。读不到时视为不可用。
    pub fn detect() -> Self {
        let unit = SERVICE_UNIT;
        match std::fs::read_to_string("/proc/self/cgroup") {
            Ok(content) => Self::from_cgroup_content(&content, unit),
            Err(_) => Self { unit: None },
        }
    }

    /// 从给定 cgroup 内容构造，便于单测。
    pub fn from_cgroup_content(content: &str, unit: &str) -> Self {
        if cgroup_matches_unit(content, unit) {
            Self {
                unit: Some(unit.to_string()),
            }
        } else {
            Self { unit: None }
        }
    }

    pub fn is_available(&self) -> bool {
        self.unit.is_some()
    }

    /// 不可用时拼在菜单标签后面的后缀。
    pub fn unavailable_label(&self) -> &'static str {
        "（需 systemd 服务）"
    }

    /// 发起重启。成功返回后调用方应当走正常退出路径。
    pub fn request(&self) -> Result<(), String> {
        let Some(unit) = self.unit.as_deref() else {
            return Err(format!(
                "当前进程不在 {SERVICE_UNIT} 的 systemd 服务中，无法重启输入法"
            ));
        };

        let helper_unit = helper_unit_name(
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
        );

        let status = Command::new("systemd-run")
            .args(restart_command(unit, &helper_unit))
            .status()
            .map_err(|e| format!("执行 systemd-run 失败：{e}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("systemd-run 返回非零退出码：{status}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2_MATCH: &str = "\
13:freezer:/
11:devices:/user.slice
1:name=systemd:/user.slice/user-1000.slice/user@1000.service/voice-input.service
0::/user.slice/user-1000.slice/user@1000.service/voice-input.service
";

    /// 构造一个「不可用」状态的控制器，供多个测试复用。
    fn unavailable_controller() -> RestartController {
        RestartController::from_cgroup_content(
            "0::/user.slice/user@1000.service\n",
            "voice-input.service",
        )
    }

    #[test]
    fn cgroup_v2_path_matches_unit() {
        let content = "0::/user.slice/user-1000.slice/user@1000.service/voice-input.service\n";
        assert!(cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn cgroup_v1_name_systemd_path_matches_unit() {
        let content =
            "1:name=systemd:/user.slice/user-1000.slice/user@1000.service/voice-input.service\n";
        assert!(cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn mixed_v1_v2_content_matches_unit() {
        assert!(cgroup_matches_unit(V2_MATCH, "voice-input.service"));
    }

    #[test]
    fn different_unit_does_not_match() {
        let content = "0::/user.slice/user-1000.slice/user@1000.service/app-org.chromium.Chromium-6815.scope\n";
        assert!(!cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn suffix_similar_unit_is_not_a_match() {
        let content = "0::/user.slice/user-1000.slice/user@1000.service/my-voice-input.service\n";
        assert!(!cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn empty_content_does_not_match() {
        assert!(!cgroup_matches_unit("", "voice-input.service"));
    }

    #[test]
    fn unit_name_with_colon_in_path_still_matches() {
        let content = "0::/user.slice/user-1000.slice/voice:input.service\n";
        assert!(cgroup_matches_unit(content, "voice:input.service"));
    }

    #[test]
    fn restart_command_uses_systemd_run_transient_unit() {
        let args = restart_command("voice-input.service", "voice-input-restart-42-1700000000");

        assert_eq!(
            args,
            vec![
                "--user",
                "--collect",
                "--unit=voice-input-restart-42-1700000000",
                "systemctl",
                "--user",
                "restart",
                "voice-input.service",
            ]
        );
    }

    #[test]
    fn restart_command_targets_given_unit() {
        let args = restart_command("other.service", "helper");

        assert_eq!(args.last().map(String::as_str), Some("other.service"));
    }

    #[test]
    fn controller_available_when_cgroup_matches() {
        let controller = RestartController::from_cgroup_content(V2_MATCH, "voice-input.service");

        assert!(controller.is_available());
    }

    #[test]
    fn controller_unavailable_when_cgroup_differs() {
        let controller = unavailable_controller();

        assert!(!controller.is_available());
    }

    #[test]
    fn unavailable_label_explains_requirement() {
        let controller = unavailable_controller();

        assert_eq!(controller.unavailable_label(), "（需 systemd 服务）");
    }

    #[test]
    fn request_fails_when_unavailable() {
        let controller = unavailable_controller();

        let err = controller.request().expect_err("不可用时应当报错");
        assert!(
            err.contains("systemd"),
            "错误信息应当提到 systemd，实际：{err}"
        );
    }

    #[test]
    fn helper_unit_name_is_unique_per_millisecond() {
        assert_eq!(
            helper_unit_name(42, 1700000000000),
            "voice-input-restart-42-1700000000000"
        );
        assert_ne!(
            helper_unit_name(42, 1700000000000),
            helper_unit_name(42, 1700000000001)
        );
    }
}
