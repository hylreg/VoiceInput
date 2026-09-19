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

    #[test]
    fn unit_name_with_colon_in_path_still_matches() {
        let content = "0::/user.slice/user-1000.slice/voice:input.service\n";
        assert!(cgroup_matches_unit(content, "voice:input.service"));
    }
}
