/// 已解析的请求头。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestHead {
    pub method: String,
    /// 已剥掉 query 与 fragment 的路径。
    pub path: String,
    /// `X-Auth-Token` 头的值，缺失时为 `None`。
    pub token: Option<String>,
    /// `Content-Length`，缺失时为 0。
    pub content_length: usize,
}

/// 剥掉 target 里的 `?query` 与 `#fragment`。
///
/// 服务器本来收不到 fragment（浏览器不会发送），这里一并剥掉是为了
/// 让纯函数对任何输入都给出确定结果，不依赖调用方保证。
fn strip_target(target: &str) -> &str {
    let end = target
        .find(|c| c == '?' || c == '#')
        .unwrap_or(target.len());
    &target[..end]
}

/// 按大小写不敏感的名字查一个头，返回值已 trim。
fn find_header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.trim().eq_ignore_ascii_case(name) {
            Some(value.trim())
        } else {
            None
        }
    })
}

/// 解析请求头（到空行为止的部分）。
pub fn parse_head(head: &str) -> Result<RequestHead, String> {
    let mut lines = head.lines();
    let request_line = lines.next().ok_or_else(|| "请求行为空".to_string())?;

    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or_else(|| "请求行缺少方法".to_string())?;
    let target = parts.next().ok_or_else(|| "请求行缺少路径".to_string())?;

    if method.is_empty() || target.is_empty() {
        return Err("请求行字段为空".to_string());
    }

    let content_length = match find_header(head, "content-length") {
        None => 0,
        Some(raw) => raw
            .parse::<usize>()
            .map_err(|_| format!("Content-Length 非法：{raw}"))?,
    };

    Ok(RequestHead {
        method: method.to_string(),
        path: strip_target(target).to_string(),
        token: find_header(head, "x-auth-token").map(str::to_string),
        content_length,
    })
}

/// 内容按常量时间比较，避免逐字节提前返回泄漏 token 前缀。
///
/// 长度不同则立即返回，所以严格说不是全常量时间——但 token 长度本就公开
/// （地址格式 `#k=<32 位十六进制>` 里写着），泄漏长度无价值。
pub fn tokens_match(expected: &str, provided: &str) -> bool {
    let expected = expected.as_bytes();
    let provided = provided.as_bytes();

    if expected.len() != provided.len() {
        return false;
    }

    let mut diff = 0u8;
    for (a, b) in expected.iter().zip(provided.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

/// 路由判定结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// 手机页面本身，公开访问。
    Page,
    /// 注入文字，需要 token。
    Send,
    /// 状态查询，需要 token。
    Status,
    NotFound,
}

/// 按方法 + 路径判定路由。方法不匹配视为 NotFound（本服务不实现 405）。
pub fn route(method: &str, path: &str) -> Route {
    match (method, path) {
        ("GET", "/") => Route::Page,
        ("POST", "/send") => Route::Send,
        ("GET", "/api/status") => Route::Status,
        _ => Route::NotFound,
    }
}

/// 注入前的文本归一化：换行折成空格 → 连续空白折叠为单个空格 → 首尾 trim。
///
/// 手机 textarea 允许换行，而 xdotool type 会把 `\n` 发成回车——
/// 在聊天框里等于直接发出消息，必须先折叠掉。
pub fn normalize_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut pending_space = false;

    for ch in raw.chars() {
        // `is_whitespace` 覆盖 `\n` `\r` `\t` 与全角空格 U+3000。
        if ch.is_whitespace() {
            // 前导空白直接丢弃：只有已经写入过内容时才置待写标记。
            if !out.is_empty() {
                pending_space = true;
            }
            continue;
        }

        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }

    // 尾随空白不回写，等价于 trim_end。
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newlines_become_single_space() {
        assert_eq!(normalize_text("第一行\n第二行"), "第一行 第二行");
    }

    #[test]
    fn crlf_becomes_single_space() {
        assert_eq!(normalize_text("第一行\r\n第二行"), "第一行 第二行");
    }

    #[test]
    fn consecutive_whitespace_collapses() {
        assert_eq!(normalize_text("甲   \n\t  乙"), "甲 乙");
    }

    #[test]
    fn leading_and_trailing_whitespace_trimmed() {
        assert_eq!(normalize_text("  你好  \n"), "你好");
    }

    #[test]
    fn plain_text_is_unchanged() {
        assert_eq!(normalize_text("你好世界"), "你好世界");
    }

    #[test]
    fn ascii_internal_single_space_is_preserved() {
        assert_eq!(normalize_text("hello world"), "hello world");
    }

    #[test]
    fn full_width_space_is_treated_as_whitespace() {
        // 手机输入法常插入全角空格，必须一并折叠，否则会注入多余空白。
        assert_eq!(normalize_text("甲\u{3000}\u{3000}乙"), "甲 乙");
    }

    #[test]
    fn whitespace_only_normalizes_to_empty() {
        assert_eq!(normalize_text("   \n\t  "), "");
    }

    #[test]
    fn empty_input_normalizes_to_empty() {
        assert_eq!(normalize_text(""), "");
    }

    const SEND_HEAD: &str = "POST /send HTTP/1.1\r\nHost: 10.10.10.42:8765\r\nX-Auth-Token: abc123\r\nContent-Length: 27\r\n\r\n";

    #[test]
    fn parses_method_and_path() {
        let head = parse_head(SEND_HEAD).expect("请求头应当解析成功");

        assert_eq!(head.method, "POST");
        assert_eq!(head.path, "/send");
    }

    #[test]
    fn parses_content_length() {
        assert_eq!(parse_head(SEND_HEAD).unwrap().content_length, 27);
    }

    #[test]
    fn missing_content_length_is_zero() {
        let head = parse_head("GET / HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();

        assert_eq!(head.content_length, 0);
    }

    #[test]
    fn non_numeric_content_length_is_rejected() {
        let raw = "POST /send HTTP/1.1\r\nContent-Length: abc\r\n\r\n";

        assert!(parse_head(raw).is_err());
    }

    #[test]
    fn negative_content_length_is_rejected() {
        let raw = "POST /send HTTP/1.1\r\nContent-Length: -5\r\n\r\n";

        assert!(parse_head(raw).is_err());
    }

    #[test]
    fn parses_auth_token_header() {
        assert_eq!(
            parse_head(SEND_HEAD).unwrap().token.as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn auth_token_header_is_case_insensitive() {
        let raw = "POST /send HTTP/1.1\r\nx-auth-token: def456\r\n\r\n";

        assert_eq!(parse_head(raw).unwrap().token.as_deref(), Some("def456"));
    }

    #[test]
    fn missing_auth_token_is_none() {
        let raw = "GET / HTTP/1.1\r\nHost: x\r\n\r\n";

        assert_eq!(parse_head(raw).unwrap().token, None);
    }

    #[test]
    fn query_string_is_stripped_from_path() {
        let raw = "GET /api/status?t=1 HTTP/1.1\r\n\r\n";

        assert_eq!(parse_head(raw).unwrap().path, "/api/status");
    }

    #[test]
    fn empty_head_is_rejected() {
        assert!(parse_head("").is_err());
    }

    #[test]
    fn request_line_only_head_is_accepted() {
        let head = parse_head("GET / HTTP/1.1\r\n\r\n").expect("只有请求行也应当接受");

        assert_eq!(head.path, "/");
        assert_eq!(head.token, None);
        assert_eq!(head.content_length, 0);
    }

    #[test]
    fn truncated_request_line_is_rejected() {
        assert!(parse_head("GET\r\n\r\n").is_err());
    }

    #[test]
    fn tokens_match_when_equal() {
        assert!(tokens_match("abc123", "abc123"));
    }

    #[test]
    fn tokens_differ_on_wrong_value() {
        assert!(!tokens_match("abc123", "abc124"));
    }

    #[test]
    fn tokens_differ_on_length_mismatch() {
        assert!(!tokens_match("abc123", "abc12"));
        assert!(!tokens_match("abc12", "abc123"));
    }

    #[test]
    fn tokens_differ_when_provided_is_empty() {
        assert!(!tokens_match("abc123", ""));
    }

    #[test]
    fn routes_page_at_root() {
        assert_eq!(route("GET", "/"), Route::Page);
    }

    #[test]
    fn routes_send_at_send() {
        assert_eq!(route("POST", "/send"), Route::Send);
    }

    #[test]
    fn routes_status_at_api_status() {
        assert_eq!(route("GET", "/api/status"), Route::Status);
    }

    #[test]
    fn wrong_method_is_not_found() {
        assert_eq!(route("GET", "/send"), Route::NotFound);
        assert_eq!(route("POST", "/"), Route::NotFound);
        assert_eq!(route("POST", "/api/status"), Route::NotFound);
    }

    #[test]
    fn unknown_path_is_not_found() {
        assert_eq!(route("GET", "/nope"), Route::NotFound);
        assert_eq!(route("GET", "/api/"), Route::NotFound);
    }
}
