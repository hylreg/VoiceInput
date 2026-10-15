/// 单次注入的文本上限（字符数，不是字节数）。
///
/// `xdotool type --delay 0` 仍逐字符走 X 协议，几百字就会明显卡顿；
/// 而 400-500 字符的手机端一次语音输入已属少见。
pub const MAX_TEXT_CHARS: usize = 500;

/// 请求体字节上限。纯防御，避免畸形请求撑爆内存。
pub const MAX_BODY_BYTES: usize = 8192;

// 面向用户的错误文案，单一来源——页面直接渲染服务端返回的 error 字段，
// 不做状态码到文案的二次映射，两边不会不一致。
pub const MSG_UNAUTHORIZED: &str = "token 无效，请从托盘菜单重新复制地址";
pub const MSG_BRIDGE_DISABLED: &str = "手机输入已关闭，请在托盘菜单里开启";
pub const MSG_TOO_LONG: &str = "文本过长，请拆分后分次发送";
pub const MSG_EMPTY: &str = "文本为空";
pub const MSG_BUSY: &str = "本机处理不过来，请稍后重试";
pub const MSG_TIMEOUT: &str = "本机正在忙，请稍后重试";
pub const MSG_INJECT_FAILED: &str = "注入失败";

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

/// 取请求体 JSON 里的 `text` 字段。任何形式的不合法输入都返回 `None`。
pub fn json_field_text(body: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get("text")?.as_str().map(str::to_string)
}

/// 字符数（不是字节数），用于上限判定与日志。
pub fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// 状态码对应的原因短语。
pub fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// 一个完整的 HTTP 响应。`render` 产出可直接写进 socket 的字节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl HttpResponse {
    pub fn html(body: String) -> Self {
        Self {
            status: 200,
            content_type: "text/html",
            body,
        }
    }

    pub fn ok_json() -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: r#"{"ok":true}"#.to_string(),
        }
    }

    /// 错误响应。用 `serde_json` 序列化，保证引号、反斜杠、换行被正确转义。
    pub fn error(status: u16, message: &str) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: serde_json::json!({ "error": message }).to_string(),
        }
    }

    pub fn render(&self) -> String {
        format!(
            "HTTP/1.1 {} {}\r\n\
             Content-Type: {}; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-store\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            self.status,
            reason_phrase(self.status),
            self.content_type,
            self.body.as_bytes().len(),
            self.body
        )
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

    #[test]
    fn extracts_text_field_from_json_body() {
        assert_eq!(
            json_field_text(r#"{"text":"你好"}"#).as_deref(),
            Some("你好")
        );
    }

    #[test]
    fn json_body_with_escapes_is_decoded() {
        assert_eq!(
            json_field_text(r#"{"text":"a\"b\nc"}"#).as_deref(),
            Some("a\"b\nc")
        );
    }

    #[test]
    fn json_body_without_text_field_is_none() {
        assert_eq!(json_field_text(r#"{"other":"x"}"#), None);
    }

    #[test]
    fn json_body_with_non_string_text_is_none() {
        assert_eq!(json_field_text(r#"{"text":42}"#), None);
    }

    #[test]
    fn malformed_json_is_none() {
        assert_eq!(json_field_text("{not json"), None);
    }

    #[test]
    fn counts_chars_not_bytes() {
        // 「你好」是 2 个字符、6 个字节。上限按字符算，否则中文会被误判超长。
        assert_eq!(char_count("你好"), 2);
    }

    #[test]
    fn render_includes_status_line_and_content_length() {
        let response = HttpResponse::ok_json();

        let rendered = response.render();

        assert!(
            rendered.starts_with("HTTP/1.1 200 OK\r\n"),
            "实际：{rendered}"
        );
        assert!(
            rendered.contains("Content-Length: 11\r\n"),
            "实际：{rendered}"
        );
        assert!(rendered.ends_with(r#"{"ok":true}"#), "实际：{rendered}");
    }

    #[test]
    fn render_forbids_caching() {
        // 手机浏览器缓存住旧页面时，会继续用旧的自动发送逻辑，
        // 而且我们在服务端改了行为它也不会知道。
        let rendered = HttpResponse::html("<p>x</p>".to_string()).render();

        assert!(
            rendered.contains("Cache-Control: no-store\r\n"),
            "实际：{rendered}"
        );
    }

    #[test]
    fn render_content_length_counts_bytes_not_chars() {
        let response = HttpResponse::error(400, "文本为空");

        let rendered = response.render();

        // body 是 {"error":"文本为空"}，UTF-8 下中文每字 3 字节。
        let expected_len = response.body.as_bytes().len().to_string();
        assert!(
            rendered.contains(&format!("Content-Length: {expected_len}\r\n")),
            "实际：{rendered}"
        );
    }

    #[test]
    fn error_response_escapes_special_chars() {
        let response = HttpResponse::error(500, "含\"引号\"和\n换行");

        // 反斜杠必须被转义，否则响应体不是合法 JSON。
        assert!(!response.body.contains('\n'), "实际：{}", response.body);
        assert!(response.body.contains(r#"\""#), "实际：{}", response.body);
    }

    #[test]
    fn error_response_sets_json_content_type() {
        assert_eq!(
            HttpResponse::error(401, "x").content_type,
            "application/json"
        );
    }

    #[test]
    fn html_response_sets_html_content_type() {
        assert_eq!(
            HttpResponse::html("<p>x</p>".to_string()).content_type,
            "text/html"
        );
    }

    #[test]
    fn reason_phrase_covers_used_statuses() {
        assert_eq!(reason_phrase(200), "OK");
        assert_eq!(reason_phrase(400), "Bad Request");
        assert_eq!(reason_phrase(401), "Unauthorized");
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(409), "Conflict");
        assert_eq!(reason_phrase(413), "Payload Too Large");
        assert_eq!(reason_phrase(429), "Too Many Requests");
        assert_eq!(reason_phrase(500), "Internal Server Error");
        assert_eq!(reason_phrase(503), "Service Unavailable");
    }

    #[test]
    fn message_constants_are_non_empty_chinese() {
        for message in [
            MSG_UNAUTHORIZED,
            MSG_BRIDGE_DISABLED,
            MSG_TOO_LONG,
            MSG_EMPTY,
            MSG_BUSY,
            MSG_TIMEOUT,
            MSG_INJECT_FAILED,
        ] {
            assert!(!message.is_empty());
            assert!(!message.is_ascii(), "文案应当是中文：{message}");
        }
    }
}
