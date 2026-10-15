//! 手机页面。单独放一个模块，让 `include_str!` 的路径短而稳定。

pub const PAGE: &str = include_str!("bridge_page.html");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_uses_the_server_auth_failure_wording() {
        // 401 的文案有两处：静态 HTML 里的粘贴框标题，和 JS 里用来比对 401 的常量。
        // 两处都必须是服务端 `MSG_UNAUTHORIZED` 的原文——用户看到的提示、
        // 以及页面判断「这是口令问题」的依据，不能各说各话。
        let expected = crate::bridge_http::MSG_UNAUTHORIZED;
        let occurrences = PAGE.matches(expected).count();

        assert_eq!(
            occurrences, 2,
            "页面里应当恰好有两处「{expected}」（静态提示 + JS 常量），实际 {occurrences} 处"
        );
    }
}
