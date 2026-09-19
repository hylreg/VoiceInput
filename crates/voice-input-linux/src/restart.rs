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
        // v1 行形如 `1:name=systemd:/path`，v2 行形如 `0::/path`；
        // 两种情况取最后一个 ':' 之后的部分即路径。
        let path = match line.rfind(':') {
            Some(idx) => &line[idx + 1..],
            None => line,
        };

        path == suffix || path.ends_with(&suffix)
    })
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
}
