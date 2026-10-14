# 手机外部输入桥接实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让手机浏览器通过局域网向本机当前光标注入文字，用手机输入法自带的语音识别，与现有「双击 Ctrl → 本机麦克风」链路并存，各由一个托盘开关控制。

**Architecture:** 新增 `bridge_http.rs`（请求解析、token 校验、文本归一化、路由、响应组装，全为纯函数）与 `bridge.rs`（IP 探测、token 生成、`TcpListener` 服务线程、`BridgeHandle`）。服务线程不做注入，只把 `BridgeRequest` 送进有界 `SyncSender`；主循环每轮排空队列并调 `host.commit_text`，从而与录音路径串行化，避免两者争抢 `xdotool`。

**Tech Stack:** Rust 2021、`std::net` / `std::thread` / `std::sync::mpsc`、ksni 0.3（托盘）、arboard 3.6（剪贴板）、serde_json

**设计文档：** [docs/superpowers/specs/2026-10-14-mobile-bridge-design.md](../specs/2026-10-14-mobile-bridge-design.md)

---

## 与设计文档的三处偏差

实施时以本节为准。三处都在文档语义框架内，只是把文档没写死的细节定下来：

1. **`voice-input-linux` 增加 `serde_json` 依赖。** 文档写「不引入新依赖（纯 std）」，但手写 JSON 解析要正确处理转义引号、反斜杠、`\uXXXX`，容易出隐蔽 bug。`serde_json` 已在根 `Cargo.lock` 里（`voice-input-asr` 在用），本机已有源码缓存，**不需要任何新的网络下载**。判为文档措辞过宽，按「不新增需要下载的依赖」执行。

2. **补充两个状态码：`400` 与 `404`。** 文档只列了 200/401/409/413/429/503。归一化后为空的文本需要 `400`（文档只说「拒绝，不注入」，未定码），未知路径需要 `404`。两者不改变文档语义。

3. **托盘开关的即时反馈靠「点击后重渲染」而非共享状态。** ksni 0.3.5 在菜单项 `clicked` 之后会调 `update()` 重算菜单（`service.rs` 的 `event()` → `self.update(&conn)`），而 `menu()` 每次都被重新调用、`flattened_menu` 每次都被重建。因此托盘持有的开关状态**用 `Cell<bool>` 即可，不必上原子类型**；开关值仍以 `Arc<AtomicBool>` 共享给服务线程与主循环。

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `crates/voice-input-linux/src/bridge_http.rs` | 新建 | 纯函数：请求头解析、token 校验、文本归一化、JSON 取字段、路由、响应组装 |
| `crates/voice-input-linux/src/bridge.rs` | 新建 | IP 探测与 URL 构造、token 生成、服务线程、`BridgeHandle` |
| `crates/voice-input-linux/src/bridge_page.html` | 新建 | 内嵌手机页面（`include_str!`） |
| `crates/voice-input-linux/src/settings.rs` | 修改 | 新增 `mic_enabled` / `bridge_enabled` / `bridge_port` / `bridge_token` |
| `crates/voice-input-linux/src/ibus.rs` | 修改 | 抽出 `read_clipboard_text` / `restore_clipboard_text` 工具 |
| `crates/voice-input-linux/src/tray.rs` | 修改 | 两个开关项 + 「复制外部输入地址」 |
| `crates/voice-input-linux/src/runtime.rs` | 修改 | 主循环排空桥接队列、接线、退出时落盘 settings |
| `crates/voice-input-linux/src/live_cli.rs` | 修改 | `--bridge-port` 参数 |
| `crates/voice-input-linux/src/lib.rs` | 修改 | 声明并导出新模块 |
| `crates/voice-input-linux/Cargo.toml` | 修改 | 加 `serde_json` |
| `README.md` | 修改 | 补手机输入一节 |

`bridge_http.rs` 不依赖 `bridge.rs`、`tray`、`runtime`，可完全独立测试。

---

## Task 1: settings 新增桥接字段

**Files:**
- Modify: `crates/voice-input-linux/src/settings.rs`
- Test: `crates/voice-input-linux/src/settings.rs`（模块内 `#[cfg(test)]`）

- [ ] **Step 1: 写失败的测试**

在 `crates/voice-input-linux/src/settings.rs` 末尾追加测试模块：

```rust
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux settings::tests`

Expected: 编译失败，报 `no field 'mic_enabled'` / `no field 'bridge_enabled'` / `no field 'bridge_port'` / `no field 'bridge_token'`。

- [ ] **Step 3: 实现字段**

把 `crates/voice-input-linux/src/settings.rs` 的结构体与默认值改成：

```rust
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
```

`#[serde(default)]` 已经标在结构体上，老配置缺字段会逐字段回落到 `Default`，无需额外处理。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux settings::tests`

Expected: `test result: ok. 4 passed`

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/settings.rs
git commit -m "feat(voice-input-linux): settings 新增桥接字段

mic_enabled / bridge_enabled 默认都开，bridge_port 默认 8765，
bridge_token 空串表示尚未生成。老配置靠 serde(default) 逐字段回落。"
```

---

## Task 2: 文本归一化纯函数

**Files:**
- Create: `crates/voice-input-linux/src/bridge_http.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Test: `crates/voice-input-linux/src/bridge_http.rs`（模块内 `#[cfg(test)]`）

- [ ] **Step 1: 在 lib.rs 声明模块**

修改 `crates/voice-input-linux/src/lib.rs`，在 `mod host;` 之前按字母序插入 `mod bridge_http;`：

```rust
mod bridge_http;
mod host;
mod hotkey;
mod ibus;
mod live;
mod live_cli;
mod recorder;
mod restart;
mod runtime;
mod settings;
mod smoke;
mod system_ime;
mod tray;
```

本任务暂时不导出任何公开项（导出在 Task 5 补齐）。

- [ ] **Step 2: 写失败的测试**

创建 `crates/voice-input-linux/src/bridge_http.rs`，只写签名（`todo!()`）与测试：

```rust
/// 注入前的文本归一化：换行折成空格 → 连续空白折叠为单个空格 → 首尾 trim。
///
/// 手机 textarea 允许换行，而 xdotool type 会把 `\n` 发成回车——
/// 在聊天框里等于直接发出消息，必须先折叠掉。
pub fn normalize_text(raw: &str) -> String {
    todo!()
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
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: 编译通过但测试 panic，报 `not yet implemented`（9 个测试全部失败）。

- [ ] **Step 4: 实现**

替换 `bridge_http.rs` 里的 `todo!()`：

```rust
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
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: `test result: ok. 9 passed`

- [ ] **Step 6: 提交**

```bash
git add crates/voice-input-linux/src/bridge_http.rs crates/voice-input-linux/src/lib.rs
git commit -m "feat(voice-input-linux): 新增桥接文本归一化纯函数

换行折成空格、连续空白折叠、首尾 trim。手机输入法的换行若直接透传，
xdotool 会发成回车，在聊天框里等于误发消息。"
```

---

## Task 3: HTTP 请求头解析纯函数

**Files:**
- Modify: `crates/voice-input-linux/src/bridge_http.rs`
- Test: `crates/voice-input-linux/src/bridge_http.rs`（同一 `#[cfg(test)]` 模块内追加）

- [ ] **Step 1: 写失败的测试**

在 `crates/voice-input-linux/src/bridge_http.rs` 的 `mod tests` 内追加（保留已有的归一化测试）：

```rust
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
        assert_eq!(parse_head(SEND_HEAD).unwrap().token.as_deref(), Some("abc123"));
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: 编译失败，报 `cannot find function 'parse_head'`。

- [ ] **Step 3: 实现**

在 `crates/voice-input-linux/src/bridge_http.rs` 的 `normalize_text` **之前**插入：

```rust
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
    let end = target.find(|c| c == '?' || c == '#').unwrap_or(target.len());
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: `test result: ok. 21 passed`

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/bridge_http.rs
git commit -m "feat(voice-input-linux): 新增 HTTP 请求头解析纯函数

解析请求行、X-Auth-Token（大小写不敏感）、Content-Length，
并剥掉 query 与 fragment。Content-Length 非法时直接报错，
不静默当 0——那会把请求体当请求头读，导致解析错位。"
```

---

## Task 4: token 校验与路由纯函数

**Files:**
- Modify: `crates/voice-input-linux/src/bridge_http.rs`
- Test: `crates/voice-input-linux/src/bridge_http.rs`

- [ ] **Step 1: 写失败的测试**

在 `bridge_http.rs` 的 `mod tests` 内追加：

```rust
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: 编译失败，报 `cannot find function 'tokens_match'` 与 `cannot find type 'Route'`。

- [ ] **Step 3: 实现**

在 `bridge_http.rs` 的 `parse_head` 之后插入：

```rust
/// 常量时间比较，避免逐字节提前返回泄漏 token 前缀。
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: `test result: ok. 30 passed`

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/bridge_http.rs
git commit -m "feat(voice-input-linux): 新增 token 校验与路由纯函数

token 用常量时间比较；路由按 (方法, 路径) 判定，
GET / 公开（页面要靠 fragment 里的 token 才能发请求，锁死会死循环）。"
```

---

## Task 5: JSON 字段、文案与响应组装

**Files:**
- Modify: `crates/voice-input-linux/src/bridge_http.rs`
- Modify: `crates/voice-input-linux/Cargo.toml`
- Test: `crates/voice-input-linux/src/bridge_http.rs`

- [ ] **Step 1: 加依赖**

在 `crates/voice-input-linux/Cargo.toml` 的 `[dependencies]` 里，按字母序在 `serde` 之后插入：

```toml
serde_json = "1"
```

`serde_json` 已在根 `Cargo.lock` 中（`voice-input-asr` 在用），本机已有源码缓存，不会触发新下载。

- [ ] **Step 2: 写失败的测试**

在 `bridge_http.rs` 的 `mod tests` 内追加：

```rust
    #[test]
    fn extracts_text_field_from_json_body() {
        assert_eq!(json_field_text(r#"{"text":"你好"}"#).as_deref(), Some("你好"));
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

        assert!(rendered.starts_with("HTTP/1.1 200 OK\r\n"), "实际：{rendered}");
        assert!(rendered.contains("Content-Length: 11\r\n"), "实际：{rendered}");
        assert!(rendered.ends_with(r#"{"ok":true}"#), "实际：{rendered}");
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
        assert_eq!(HttpResponse::error(401, "x").content_type, "application/json");
    }

    #[test]
    fn html_response_sets_html_content_type() {
        assert_eq!(HttpResponse::html("<p>x</p>".to_string()).content_type, "text/html");
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
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: 编译失败，报 `cannot find function 'json_field_text'`、`cannot find type 'HttpResponse'` 等。

- [ ] **Step 4: 实现**

在 `bridge_http.rs` 顶部（`use` 之后、`RequestHead` 之前）插入常量：

```rust
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
```

在 `route` 之后插入：

```rust
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
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p voice-input-linux bridge_http::tests`

Expected: `test result: ok. 44 passed`

- [ ] **Step 6: 提交**

```bash
git add crates/voice-input-linux/src/bridge_http.rs crates/voice-input-linux/Cargo.toml
git commit -m "feat(voice-input-linux): 新增 JSON 取字段、错误文案与响应组装

错误文案是单一来源，页面直接渲染 error 字段，不做状态码到文案的二次映射。
响应体用 serde_json 序列化，保证引号换行被正确转义；Content-Length 按字节算。"
```

---

## Task 6: 剪贴板保存与还原工具

**Files:**
- Modify: `crates/voice-input-linux/src/ibus.rs`
- Test: `crates/voice-input-linux/src/ibus.rs`

手机注入路径不能调 `insert_indicator_and_save_clipboard`——那会在光标处插一个 `●`，
而用户人在手机上，看不到也不需要这个指示符。但剪贴板保存-还原必须留下。
本任务先把这套逻辑抽成两个不碰光标的工具，Task 8 注入时调用。

- [ ] **Step 1: 写失败的测试**

在 `crates/voice-input-linux/src/ibus.rs` 末尾追加（文件中目前没有 `mod tests`）：

```rust
#[cfg(test)]
mod tests {
    use super::clipboard_text_to_restore;

    #[test]
    fn restores_when_previous_text_present() {
        assert_eq!(
            clipboard_text_to_restore(Some("原有内容".to_string())).as_deref(),
            Some("原有内容")
        );
    }

    #[test]
    fn skips_restore_when_clipboard_was_unreadable() {
        // 读不到原内容（剪贴板被别的进程占用或本来就没有）时不还原，
        // 不该往剪贴板里塞空串。
        assert_eq!(clipboard_text_to_restore(None), None);
    }

    #[test]
    fn restores_empty_string_when_clipboard_held_empty_string() {
        // Some("") 与 None 语义不同：前者是「剪贴板里确实是一段空文本」，
        // 需要还原，否则会留下我们注入过的内容。
        assert_eq!(
            clipboard_text_to_restore(Some(String::new())).as_deref(),
            Some("")
        );
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux ibus::tests`

Expected: 编译失败，报 `cannot find function 'clipboard_text_to_restore'`。

- [ ] **Step 3: 实现**

在 `crates/voice-input-linux/src/ibus.rs` 中 `insert_indicator_and_save_clipboard` 之后插入：

```rust
/// 读取当前剪贴板文本。剪贴板为空或格式不是文本时返回 `None`。
///
/// 剪贴板被别的进程占用时会返回 `None` 而不是报错——“读不到原内容”
/// 只意味着后面不还原，不应该让整次注入失败。
#[cfg(feature = "ibus")]
pub fn read_clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// 把文本写回剪贴板。失败时返回错误说明。
#[cfg(feature = "ibus")]
pub fn restore_clipboard_text(text: &str) -> Result<()> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))
}

/// 决定注入后要把什么写回剪贴板。
///
/// `Some(t)` 表示需要还原这段文本（含 `Some("")`——那代表剪贴板里确实
/// 是一段空文本，不还原就会留下我们注入过的内容）；`None` 表示读不到
/// 原内容，跳过还原。
pub fn clipboard_text_to_restore(previous: Option<String>) -> Option<String> {
    previous
}

#[cfg(not(feature = "ibus"))]
pub fn read_clipboard_text() -> Option<String> {
    None
}

#[cfg(not(feature = "ibus"))]
pub fn restore_clipboard_text(_text: &str) -> Result<()> {
    Ok(())
}
```

`clipboard_text_to_restore` 不加 `#[cfg]`——它是纯函数，两个构建下都要能编译，供单测使用。
函数体看着平凡，但它把「`Some("")` 要还原、`None` 不还原」这条语义固定在测试里，
避免日后有人图省事改成 `previous.filter(|s| !s.is_empty())`。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux ibus::tests`

Expected: `test result: ok. 3 passed`

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/ibus.rs
git commit -m "feat(voice-input-linux): 抽离剪贴板读写与还原判定工具

手机注入路径不能复用 insert_indicator_and_save_clipboard（那会插 ●），
但剪贴板保存-还原必须保留，否则用户正在复制的内容会被手机文字覆盖。"
```

---

## Task 7: 桥接服务线程

**Files:**
- Create: `crates/voice-input-linux/src/bridge.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Test: `crates/voice-input-linux/tests/bridge.rs`（集成测试，真实 TCP 往返）

- [ ] **Step 1: 写失败的测试**

创建 `crates/voice-input-linux/tests/bridge.rs`：

```rust
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use voice_input_linux::{
    spawn_bridge, BridgeConfig, BridgeRequest, BridgeStatus, LinuxMicAudioRecorder,
};

const TOKEN: &str = "deadbeefdeadbeefdeadbeefdeadbeef";

/// 起一个测试用桥接服务，绑 0 端口。返回服务句柄、连接地址、接收端。
fn test_bridge(
    enabled: bool,
) -> (
    voice_input_linux::BridgeHandle,
    String,
    mpsc::Receiver<BridgeRequest>,
) {
    let status = Arc::new(BridgeStatus::new(
        Arc::new(AtomicBool::new(enabled)),
        TOKEN.to_string(),
        LinuxMicAudioRecorder::new(Duration::from_secs(30)),
    ));
    let (sender, receiver) = mpsc::sync_channel(8);
    let handle = spawn_bridge(BridgeConfig { port: 0 }, status, sender)
        .expect("桥接服务应当启动成功");

    let addr = handle.local_addr().to_string();
    (handle, addr, receiver)
}

/// 发一个原始 HTTP 请求，读回全部响应文本。
fn raw_request(addr: &str, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).expect("应当能连上桥接服务");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("设置读超时");
    stream
        .write_all(request.as_bytes())
        .expect("写请求应当成功");
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    response
}

fn send_request(addr: &str, token: &str, body: &str) -> String {
    raw_request(
        addr,
        &format!(
            "POST /send HTTP/1.1\r\nHost: x\r\nX-Auth-Token: {token}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn page_is_public() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(addr.as_str(), "GET / HTTP/1.1\r\nHost: x\r\n\r\n");

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "实际：{response}");
    assert!(
        response.contains("application/json") == false,
        "页面不应该是 JSON：{response}"
    );
}

#[test]
fn send_with_valid_token_is_queued_and_acked() {
    let (_handle, addr, rx) = test_bridge(true);

    // 主循环的替身：收下请求并回票成功。
    let worker = std::thread::spawn(move || {
        let request = rx.recv().expect("应当收到桥接请求");
        assert_eq!(request.text, "你好世界");
        request.reply.send(Ok(())).expect("回票应当成功");
    });

    let response = send_request(&addr, TOKEN, r#"{"text":"你好世界"}"#);

    worker.join().expect("worker 线程应当正常结束");
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "实际：{response}");
    assert!(response.ends_with(r#"{"ok":true}"#), "实际：{response}");
}

#[test]
fn send_without_token_is_unauthorized() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = send_request(&addr, "wrong-token", r#"{"text":"x"}"#);

    assert!(response.starts_with("HTTP/1.1 401 "), "实际：{response}");
}

#[test]
fn send_when_disabled_is_conflict() {
    let (_handle, addr, _rx) = test_bridge(false);

    let response = send_request(&addr, TOKEN, r#"{"text":"x"}"#);

    assert!(response.starts_with("HTTP/1.1 409 "), "实际：{response}");
}

#[test]
fn send_too_long_is_payload_too_large() {
    let (_handle, addr, _rx) = test_bridge(true);
    let long = "字".repeat(501);
    let body = format!(r#"{{"text":"{long}"}}"#);

    let response = send_request(&addr, TOKEN, &body);

    assert!(response.starts_with("HTTP/1.1 413 "), "实际：{response}");
}

#[test]
fn send_blank_text_is_bad_request() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = send_request(&addr, TOKEN, r#"{"text":"   \n  "}"#);

    assert!(response.starts_with("HTTP/1.1 400 "), "实际：{response}");
}

#[test]
fn send_when_queue_full_is_too_many_requests() {
    let (_handle, addr, _rx) = test_bridge(true);
    // 不排空队列。队列容量 8，第 9 个请求必然满。
    // 每个请求会阻塞等待回票直到 2s 超时，所以并发发出后统一收结果。
    let mut threads = Vec::new();
    for _ in 0..9 {
        let addr = addr.clone();
        threads.push(std::thread::spawn(move || {
            send_request(&addr, TOKEN, r#"{"text":"x"}"#)
        }));
    }

    let responses: Vec<String> = threads
        .into_iter()
        .map(|t| t.join().expect("线程应当正常结束"))
        .collect();

    assert!(
        responses.iter().any(|r| r.starts_with("HTTP/1.1 429 ")),
        "9 个请求里应当至少有一个 429，实际：{responses:?}"
    );
}

#[test]
fn send_when_worker_never_acks_is_service_unavailable() {
    let (_handle, addr, _rx) = test_bridge(true);
    // 主循环替身缺失：请求进了队列但没人回票，2s 后应当超时。

    let response = send_request(&addr, TOKEN, r#"{"text":"x"}"#);

    assert!(response.starts_with("HTTP/1.1 503 "), "实际：{response}");
}

#[test]
fn status_reports_mic_and_bridge_state() {
    let (_handle, addr, _rx) = test_bridge(false);

    let response = raw_request(
        addr.as_str(),
        &format!("GET /api/status HTTP/1.1\r\nHost: x\r\nX-Auth-Token: {TOKEN}\r\n\r\n"),
    );

    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "实际：{response}");
    assert!(response.contains(r#""bridge_enabled":false"#), "实际：{response}");
    assert!(response.contains(r#""mic_recording":false"#), "实际：{response}");
}

#[test]
fn unknown_path_is_not_found() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(addr.as_str(), "GET /nope HTTP/1.1\r\nHost: x\r\n\r\n");

    assert!(response.starts_with("HTTP/1.1 404 "), "实际：{response}");
}

#[test]
fn response_always_closes_connection() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(addr.as_str(), "GET / HTTP/1.1\r\nHost: x\r\n\r\n");

    assert!(response.contains("Connection: close\r\n"), "实际：{response}");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux --test bridge`

Expected: 编译失败，`unresolved import voice_input_linux::spawn_bridge`。

- [ ] **Step 3: 实现**

创建 `crates/voice-input-linux/src/bridge.rs`：

```rust
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::bridge_http::{
    char_count, json_field_text, normalize_text, parse_head, route, tokens_match, HttpResponse,
    Route, MAX_BODY_BYTES, MAX_TEXT_CHARS, MSG_BRIDGE_DISABLED, MSG_BUSY, MSG_EMPTY,
    MSG_INJECT_FAILED, MSG_TIMEOUT, MSG_TOO_LONG, MSG_UNAUTHORIZED,
};
use crate::recorder::LinuxMicAudioRecorder;
use voice_input_core::{Result, VoiceInputError};

/// 单个连接的读写超时，避免半个连接占死线程。
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

/// 等待主循环回票的超时。主循环每 250ms 排空一次队列，2s 余量充足。
const ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// 队列容量。满了直接回 429，不无限堆积。
pub const QUEUE_CAPACITY: usize = 8;

/// 一次待注入的请求，由服务线程送给主循环。
pub struct BridgeRequest {
    pub text: String,
    /// 主循环处理后用它回票；错误串直接展示给用户。
    pub reply: SyncSender<std::result::Result<(), String>>,
}

/// 桥接运行时状态。`bridge_enabled` 由托盘切换；`mic_recording` 直接问录音器，
/// 不另设一份标志位——两份状态必然漂移。
#[derive(Debug)]
pub struct BridgeStatus {
    pub bridge_enabled: Arc<AtomicBool>,
    recorder: LinuxMicAudioRecorder,
    /// 访问 token。放在 `Mutex` 里是因为服务线程每个请求都要比对，
    /// 而托盘「复制地址」也需要读。
    token: std::sync::Mutex<String>,
}

impl BridgeStatus {
    pub fn new(
        bridge_enabled: Arc<AtomicBool>,
        token: String,
        recorder: LinuxMicAudioRecorder,
    ) -> Self {
        Self {
            bridge_enabled,
            recorder,
            token: std::sync::Mutex::new(token),
        }
    }

    /// 本机是否正在录音。录音中手机发来的请求要快速失败，
    /// 而不是干等 2s 超时（录音周期最长 30s）。
    pub fn mic_recording(&self) -> bool {
        self.recorder.is_recording()
    }

    /// 读当前 token，供服务线程比对与托盘拼地址。
    pub fn token(&self) -> String {
        self.token
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    /// 0 表示由系统分配（测试用）。
    pub port: u16,
}

pub struct BridgeHandle {
    addr: std::net::SocketAddr,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl BridgeHandle {
    pub fn local_addr(&self) -> std::net::SocketAddr {
        self.addr
    }
}

impl Drop for BridgeHandle {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        // 用一个连接唤醒阻塞在 accept 上的线程，再 join 回收。
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// 启动桥接服务线程。端口绑定失败时返回错误，由调用方决定是否降级。
pub fn spawn_bridge(
    config: BridgeConfig,
    status: Arc<BridgeStatus>,
    sender: SyncSender<BridgeRequest>,
) -> Result<BridgeHandle> {
    let listener = TcpListener::bind(("0.0.0.0", config.port)).map_err(|e| {
        VoiceInputError::Injection(format!("桥接端口 {} 绑定失败：{e}", config.port))
    })?;
    let addr = listener
        .local_addr()
        .map_err(|e| VoiceInputError::Injection(format!("读取桥接监听地址失败：{e}")))?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_for_thread = Arc::clone(&shutdown);

    let thread = thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown_for_thread.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else { continue };
            let status = Arc::clone(&status);
            let sender = sender.clone();
            // 唤醒用的空连接会立刻返回，无需处理。
            let _ = thread::Builder::new().spawn(move || {
                let _ = handle_connection(stream, &status, &sender);
            });
        }
    });

    Ok(BridgeHandle {
        addr,
        shutdown,
        thread: Some(thread),
    })
}

fn handle_connection(
    mut stream: TcpStream,
    status: &BridgeStatus,
    sender: &SyncSender<BridgeRequest>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(());
    }

    // 逐行读头，直到空行。
    let mut head = request_line;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let is_end = line == "\r\n" || line == "\n";
        head.push_str(&line);
        if is_end {
            break;
        }
    }

    let response = build_response(&head, &mut reader, status, sender);
    stream.write_all(response.render().as_bytes())?;
    stream.flush()
}

fn build_response(
    head: &str,
    reader: &mut impl BufRead,
    status: &BridgeStatus,
    sender: &SyncSender<BridgeRequest>,
) -> HttpResponse {
    let Ok(parsed) = parse_head(head) else {
        return HttpResponse::error(400, "请求头无法解析");
    };

    match route(&parsed.method, &parsed.path) {
        Route::NotFound => HttpResponse::error(404, "未知路径"),
        Route::Page => HttpResponse::html(crate::bridge_page::PAGE.to_string()),
        Route::Status => {
            let provided = parsed.token.as_deref().unwrap_or("");
            if !tokens_match(&status.token(), provided) {
                return HttpResponse::error(401, MSG_UNAUTHORIZED);
            }
            let body = format!(
                r#"{{"mic_recording":{},"bridge_enabled":{}}}"#,
                status.mic_recording(),
                status.bridge_enabled.load(Ordering::SeqCst),
            );
            HttpResponse {
                status: 200,
                content_type: "application/json",
                body,
            }
        }
        Route::Send => handle_send(parsed, reader, status, sender),
    }
}

fn handle_send(
    parsed: crate::bridge_http::RequestHead,
    reader: &mut impl BufRead,
    status: &BridgeStatus,
    sender: &SyncSender<BridgeRequest>,
) -> HttpResponse {
    let provided = parsed.token.as_deref().unwrap_or("");
    if !tokens_match(&status.token(), provided) {
        return HttpResponse::error(401, MSG_UNAUTHORIZED);
    }

    if !status.bridge_enabled.load(Ordering::SeqCst) {
        return HttpResponse::error(409, MSG_BRIDGE_DISABLED);
    }

    if parsed.content_length > MAX_BODY_BYTES {
        return HttpResponse::error(413, MSG_TOO_LONG);
    }

    let mut body = vec![0u8; parsed.content_length];
    if reader.read_exact(&mut body).is_err() {
        return HttpResponse::error(400, "请求体读取失败");
    }
    let Ok(body) = String::from_utf8(body) else {
        return HttpResponse::error(400, "请求体不是合法 UTF-8");
    };

    let Some(raw) = json_field_text(&body) else {
        return HttpResponse::error(400, MSG_EMPTY);
    };

    let text = normalize_text(&raw);
    if text.is_empty() {
        return HttpResponse::error(400, MSG_EMPTY);
    }
    if char_count(&text) > MAX_TEXT_CHARS {
        return HttpResponse::error(413, MSG_TOO_LONG);
    }

    // 日志只记长度与结果，不记内容——手机语音可能包含密码或私事。
    println!("手机输入：收到 {} 字，等待注入", char_count(&text));

    let (reply_tx, reply_rx) = std::sync::mpsc::sync_channel(1);
    let request = BridgeRequest {
        text,
        reply: reply_tx,
    };

    match sender.try_send(request) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return HttpResponse::error(429, MSG_BUSY),
        Err(TrySendError::Disconnected(_)) => {
            return HttpResponse::error(503, MSG_TIMEOUT);
        }
    }

    match reply_rx.recv_timeout(ACK_TIMEOUT) {
        Ok(Ok(())) => HttpResponse::ok_json(),
        Ok(Err(message)) => HttpResponse::error(500, &format!("{MSG_INJECT_FAILED}：{message}")),
        Err(_) => HttpResponse::error(503, MSG_TIMEOUT),
    }
}
```

- [ ] **Step 4: 创建内嵌页面**

创建 `crates/voice-input-linux/src/bridge_page.html`：

```html
<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>VoiceInput 手机输入</title>
<style>
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body {
    margin: 0; padding: 16px; min-height: 100vh;
    background: #17181c; color: #e8e8ea;
    font: 16px/1.5 -apple-system, "Noto Sans CJK SC", sans-serif;
    display: flex; flex-direction: column; gap: 12px;
  }
  h1 { font-size: 17px; margin: 0; font-weight: 600; }
  #notice { font-size: 13px; color: #9a9aa2; min-height: 1.2em; }
  #notice.error { color: #ff6b6b; }
  #notice.ok { color: #51cf66; }
  textarea {
    flex: 1 1 auto; min-height: 30vh; width: 100%;
    padding: 12px; border-radius: 10px; resize: vertical;
    background: #22232a; color: inherit; font: inherit;
    border: 1px solid #34353d;
  }
  textarea:focus { outline: 2px solid #4c8bf5; border-color: transparent; }
  button {
    width: 100%; padding: 18px; font-size: 18px; font-weight: 600;
    border: 0; border-radius: 10px; background: #4c8bf5; color: #fff;
  }
  button:disabled { background: #3a3b44; color: #8a8a92; }
  #history { list-style: none; margin: 0; padding: 0; font-size: 13px; }
  #history li {
    padding: 8px 10px; border-radius: 8px; margin-bottom: 6px;
    background: #22232a; word-break: break-word;
  }
  #history li.failed { background: #3a1f22; color: #ffb3b3; }
  #history time { color: #7a7a82; margin-right: 8px; }
</style>
</head>
<body>
<h1>VoiceInput 手机输入</h1>
<p id="notice"></p>
<textarea id="text" autofocus placeholder="用输入法的语音键说话，确认后点发送"></textarea>
<button id="send">发送</button>
<ul id="history"></ul>
<script>
(function () {
  var TEXT = document.getElementById('text');
  var SEND = document.getElementById('send');
  var NOTICE = document.getElementById('notice');
  var HISTORY = document.getElementById('history');
  var LS_HISTORY = 'voiceinput.history';
  var LS_TOKEN = 'voiceinput.token';

  function readFragmentToken() {
    var match = /(?:^|[#&])k=([0-9a-fA-F]+)/.exec(location.hash || '');
    return match ? match[1] : '';
  }

  // fragment 只发一次；存进 localStorage 之后开裸地址也能用。
  var fragmentToken = readFragmentToken();
  if (fragmentToken) { localStorage.setItem(LS_TOKEN, fragmentToken); }
  var TOKEN = fragmentToken || localStorage.getItem(LS_TOKEN) || '';

  function notice(text, kind) {
    NOTICE.textContent = text || '';
    NOTICE.className = kind || '';
  }

  function loadHistory() {
    try { return JSON.parse(localStorage.getItem(LS_HISTORY) || '[]'); }
    catch (e) { return []; }
  }
  function saveHistory(rows) {
    try { localStorage.setItem(LS_HISTORY, JSON.stringify(rows.slice(-50))); }
    catch (e) { /* 存不下就算了，页面本身可用 */ }
  }

  function render() {
    var rows = loadHistory();
    HISTORY.innerHTML = '';
    rows.slice().reverse().forEach(function (row) {
      var li = document.createElement('li');
      if (row.failed) { li.className = 'failed'; }
      var time = document.createElement('time');
      time.textContent = row.time;
      li.appendChild(time);
      li.appendChild(document.createTextNode(row.text));
      HISTORY.appendChild(li);
    });
  }

  function push(text, failed) {
    var rows = loadHistory();
    rows.push({
      text: text,
      failed: !!failed,
      time: new Date().toTimeString().slice(0, 8)
    });
    saveHistory(rows);
    render();
  }

  function authHeaders() {
    return { 'X-Auth-Token': TOKEN, 'Content-Type': 'application/json' };
  }

  function send() {
    var text = TEXT.value.trim();
    if (!text) { notice('文本为空', 'error'); return; }
    SEND.disabled = true;
    notice('检查本机状态…');

    fetch('/api/status', { headers: authHeaders() })
      .then(function (res) {
        if (res.status === 401) { throw new Error('token 已变更，请从托盘菜单重新复制地址'); }
        if (!res.ok) { throw new Error('状态查询失败（' + res.status + '）'); }
        return res.json();
      })
      .then(function (state) {
        if (!state.bridge_enabled) { throw new Error('手机输入已关闭，请在托盘菜单里开启'); }
        if (state.mic_recording) { throw new Error('本机正在录音，稍后重试'); }
        notice('发送中…');
        return fetch('/send', {
          method: 'POST',
          headers: authHeaders(),
          body: JSON.stringify({ text: text })
        });
      })
      .then(function (res) {
        return res.json().catch(function () { return {}; }).then(function (data) {
          if (res.ok) {
            push(text, false);
            TEXT.value = '';
            TEXT.focus();
            notice('已注入本机光标', 'ok');
            return;
          }
          throw new Error(data.error || ('发送失败（' + res.status + '）'));
        });
      })
      .catch(function (err) {
        push(text, true);
        notice(err.message, 'error');
      })
      .then(function () { SEND.disabled = false; });
  }

  SEND.addEventListener('click', send);
  // 回车不发送：手机语音输入会插入换行，回车当发送必然误触。
  TEXT.addEventListener('keydown', function (event) {
    if (event.key === 'Enter') { event.preventDefault(); }
  });

  if (!TOKEN) {
    notice('缺少 token，请用托盘菜单「复制外部输入地址」给出的完整链接打开', 'error');
    SEND.disabled = true;
  }
  render();
})();
</script>
</body>
</html>
```

创建 `crates/voice-input-linux/src/bridge_page.rs`：

```rust
//! 手机页面。单独放一个模块，让 `include_str!` 的路径短而稳定。

pub const PAGE: &str = include_str!("bridge_page.html");
```

- [ ] **Step 5: 在 lib.rs 接线**

修改 `crates/voice-input-linux/src/lib.rs`——模块声明：

```rust
mod bridge;
mod bridge_http;
mod bridge_page;
mod host;
mod hotkey;
mod ibus;
mod live;
mod live_cli;
mod recorder;
mod restart;
mod runtime;
mod settings;
mod smoke;
mod system_ime;
mod tray;
```

导出（在 `pub use host::LinuxInputMethodHost;` 之前插入）：

```rust
pub use bridge::{spawn_bridge, BridgeConfig, BridgeHandle, BridgeRequest, BridgeStatus};
pub use bridge_http::{
    char_count, json_field_text, normalize_text, parse_head, route, tokens_match, HttpResponse,
    RequestHead, Route, MAX_TEXT_CHARS,
};
pub use bridge_page::PAGE;
```

`LinuxMicAudioRecorder` 已经在 `lib.rs` 里导出过了（本任务需要它构造 `BridgeStatus`），
无需改动。

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test -p voice-input-linux --test bridge`

Expected: `test result: ok. 11 passed`

- [ ] **Step 7: 提交**

```bash
git add crates/voice-input-linux/src/bridge.rs crates/voice-input-linux/src/bridge_page.rs \
        crates/voice-input-linux/src/bridge_page.html crates/voice-input-linux/src/lib.rs \
        crates/voice-input-linux/tests/bridge.rs
git commit -m "feat(voice-input-linux): 新增手机桥接服务线程与内嵌页面

TcpListener 绑 0.0.0.0，POST /send 把请求送进容量 8 的有界队列并由主循环回票；
/api/status 直读共享状态不排队，避免被 30s 录音周期阻塞。
页面用手机键盘语音识别，只有按钮发送（回车不发送，避免语音换行误触）。"
```

---

## Task 8: IP 探测、token 生成与地址构造

**Files:**
- Create: `crates/voice-input-linux/src/bridge_addr.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Test: `crates/voice-input-linux/src/bridge_addr.rs`

- [ ] **Step 1: 写失败的测试**

创建 `crates/voice-input-linux/src/bridge_addr.rs`：

```rust
/// 探测本机在局域网里的 IPv4 地址。
///
/// 用一个不发包的 UDP `connect` 让内核按路由表挑出出口网卡的地址。
/// 选 `192.0.2.1` 是因为它是 RFC 5737 保留的文档地址，永远不会真的可达，
/// 所以这个调用只做路由查询、不产生任何流量。
pub fn detect_local_ipv4() -> Option<std::net::Ipv4Addr> {
    todo!()
}

/// 生成 16 字节随机数的十六进制串（32 个字符），作为访问 token。
pub fn generate_token() -> String {
    todo!()
}

/// 拼接手机端要打开的地址。token 放 fragment，不进浏览器历史也不发 Referer。
pub fn bridge_url(ip: &str, port: u16, token: &str) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_puts_token_in_fragment() {
        assert_eq!(
            bridge_url("10.0.0.5", 8765, "abc123"),
            "http://10.0.0.5:8765/#k=abc123"
        );
    }

    #[test]
    fn url_does_not_put_token_in_query() {
        // query 会进浏览器历史、会随 Referer 外发，必须避免。
        let url = bridge_url("10.0.0.5", 8765, "secret");

        assert!(!url.contains("?k="), "实际：{url}");
        assert!(url.contains("#k=secret"), "实际：{url}");
    }

    #[test]
    fn url_works_with_hostname() {
        assert_eq!(
            bridge_url("localhost", 8080, "t"),
            "http://localhost:8080/#k=t"
        );
    }

    #[test]
    fn generated_token_is_32_hex_chars() {
        let token = generate_token();

        assert_eq!(token.len(), 32, "实际：{token}");
        assert!(
            token.chars().all(|c| c.is_ascii_hexdigit()),
            "应当全是十六进制字符，实际：{token}"
        );
    }

    #[test]
    fn generated_tokens_differ() {
        // 不是严格的随机性检验，只挡住「返回常量」这种退化实现。
        assert_ne!(generate_token(), generate_token());
    }

    #[test]
    fn generated_token_is_lowercase_hex() {
        let token = generate_token();

        assert_eq!(token, token.to_ascii_lowercase(), "实际：{token}");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux bridge_addr::tests`

Expected: 编译失败，报 `file not found for module 'bridge_addr'`（模块尚未声明）。

先在 `crates/voice-input-linux/src/lib.rs` 里补上声明再跑：

```rust
mod bridge;
mod bridge_addr;
mod bridge_http;
mod bridge_page;
```

此时预期：`not yet implemented`（6 个测试全部失败）。

- [ ] **Step 3: 实现**

替换 `bridge_addr.rs` 里的三个 `todo!()`，并在文件顶部加 `use`：

```rust
use std::io::Read;
use std::net::{Ipv4Addr, UdpSocket};

/// 探测本机在局域网里的 IPv4 地址。
///
/// 用一个不发包的 UDP `connect` 让内核按路由表挑出出口网卡的地址。
/// 选 `192.0.2.1` 是因为它是 RFC 5737 保留的文档地址，永远不会真的可达，
/// 所以这个调用只做路由查询、不产生任何流量。
pub fn detect_local_ipv4() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:80").ok()?;

    match socket.local_addr().ok()? {
        std::net::SocketAddr::V4(addr) => Some(*addr.ip()),
        std::net::SocketAddr::V6(_) => None,
    }
}

/// 生成 16 字节随机数的十六进制串（32 个字符），作为访问 token。
pub fn generate_token() -> String {
    let mut bytes = [0u8; 16];

    // 读不到 urandom 时退化为「pid + 时间戳」这种弱随机。
    // 局域网场景下这已经足够拦住顺手乱试的人；真正的失败模式
    // 是「生成不出来导致启动失败」，那更糟。
    let filled = std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok();

    if !filled {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seed = (u128::from(std::process::id()) << 96) ^ nanos;
        bytes.copy_from_slice(&seed.to_le_bytes());
    }

    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 拼接手机端要打开的地址。token 放 fragment，不进浏览器历史也不发 Referer。
pub fn bridge_url(ip: &str, port: u16, token: &str) -> String {
    format!("http://{ip}:{port}/#k={token}")
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux bridge_addr::tests`

Expected: `test result: ok. 6 passed`

- [ ] **Step 5: 导出**

在 `crates/voice-input-linux/src/lib.rs` 的 `pub use bridge::...;` 之前插入：

```rust
pub use bridge_addr::{bridge_url, detect_local_ipv4, generate_token};
```

- [ ] **Step 6: 提交**

```bash
git add crates/voice-input-linux/src/bridge_addr.rs crates/voice-input-linux/src/lib.rs
git commit -m "feat(voice-input-linux): 新增 IP 探测、token 生成与地址构造

IP 用不发包的 UDP connect 查路由表（选 RFC 5737 文档地址 192.0.2.1，
永不可达所以无流量）。token 从 /dev/urandom 取 16 字节转十六进制，
放 URL fragment 而非 query——query 会进浏览器历史并随 Referer 外发。"
```

---

## Task 9: 托盘两个开关与「复制地址」

**Files:**
- Modify: `crates/voice-input-linux/src/tray.rs`
- Test: `crates/voice-input-linux/src/tray.rs`

- [ ] **Step 1: 写失败的测试**

在 `crates/voice-input-linux/src/tray.rs` 的 `mod tests` 里，先扩展构造函数。把现有的
`fn tray_with(cgroup: &str, recording: bool, ibus_running: bool) -> LinuxTray` 整体替换为：

```rust
        fn tray_with(cgroup: &str, recording: bool, ibus_running: bool) -> LinuxTray {
            tray_with_bridge(cgroup, recording, ibus_running, true, true, Some("10.0.0.5"))
        }

        fn tray_with_bridge(
            cgroup: &str,
            recording: bool,
            ibus_running: bool,
            mic_enabled: bool,
            bridge_enabled: bool,
            local_ip: Option<&str>,
        ) -> LinuxTray {
            let restart = RestartController::from_cgroup_content(cgroup, "voice-input.service");
            let ime = if ibus_running {
                SystemImeController::from_probe(&["ibus-daemon"])
            } else {
                SystemImeController::from_probe(&["ibus-x11"])
            };
            LinuxTray {
                service_name: "voice-input".to_string(),
                title: "VoiceInput".to_string(),
                recording,
                recorder: LinuxMicAudioRecorder::new(Duration::from_secs(30)),
                quit_requested: Arc::new(AtomicBool::new(false)),
                restart,
                restart_requested: Arc::new(AtomicBool::new(false)),
                ime,
                mic_enabled: Cell::new(mic_enabled),
                bridge_enabled: Cell::new(bridge_enabled),
                bridge_available: bridge_enabled,
                bridge_port: 8765,
                bridge_token: "abc123".to_string(),
                local_ip: local_ip.map(str::to_string),
                settings: RefCell::new(LinuxAppSettings::default()),
            }
        }
```

并在 `mod tests` 顶部的 `use` 里补：

```rust
        use crate::settings::LinuxAppSettings;
        use std::cell::{Cell, RefCell};
```

然后追加测试：

```rust
        #[test]
        fn mic_label_shows_enabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.mic_label(), "本机麦克风：已启用");
        }

        #[test]
        fn mic_label_shows_disabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, false, true, Some("10.0.0.5"));

            assert_eq!(tray.mic_label(), "本机麦克风：已停用");
        }

        #[test]
        fn toggling_mic_flips_label() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_mic();

            assert_eq!(tray.mic_label(), "本机麦克风：已停用");
            assert!(!tray.mic_enabled.get());
        }

        #[test]
        fn toggling_mic_twice_returns_to_enabled() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_mic();
            tray.toggle_mic();

            assert!(tray.mic_enabled.get());
        }

        #[test]
        fn bridge_label_shows_enabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.bridge_label(), "手机输入：已启用");
        }

        #[test]
        fn bridge_label_shows_disabled_state() {
            let tray = tray_with_bridge(MATCHING, false, true, true, false, Some("10.0.0.5"));

            assert_eq!(tray.bridge_label(), "手机输入：已停用");
        }

        #[test]
        fn toggling_bridge_flips_label() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            tray.toggle_bridge();

            assert_eq!(tray.bridge_label(), "手机输入：已停用");
        }

        #[test]
        fn bridge_label_explains_when_port_unavailable() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_available = false;

            assert_eq!(tray.bridge_label(), "手机输入（端口被占用）");
        }

        #[test]
        fn bridge_toggle_is_disabled_when_port_unavailable() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_available = false;

            assert!(!tray.bridge_toggle_enabled());
        }

        #[test]
        fn copy_address_enabled_when_available() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert!(tray.copy_address_enabled());
        }

        #[test]
        fn copy_address_disabled_when_port_unavailable() {
            let mut tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));
            tray.bridge_available = false;

            assert!(!tray.copy_address_enabled());
        }

        #[test]
        fn copy_address_disabled_when_ip_unknown() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, None);

            assert!(!tray.copy_address_enabled());
        }

        #[test]
        fn address_label_shows_port() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.copy_address_label(), "复制外部输入地址（8765）");
        }

        #[test]
        fn address_url_uses_current_ip() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, Some("10.0.0.5"));

            assert_eq!(tray.address_url().as_deref(), Some("http://10.0.0.5:8765/#k=abc123"));
        }

        #[test]
        fn address_url_falls_back_to_loopback_when_ip_unknown() {
            let tray = tray_with_bridge(MATCHING, false, true, true, true, None);

            assert_eq!(tray.address_url().as_deref(), Some("http://127.0.0.1:8765/#k=abc123"));
        }

        #[test]
        fn toggles_are_also_disabled_while_recording() {
            let tray = tray_with_bridge(MATCHING, true, true, true, true, Some("10.0.0.5"));

            assert!(!tray.mic_toggle_enabled());
        }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux tray::tests`

Expected: 编译失败，报 `no field 'mic_enabled'` / `cannot find function 'mic_label'` 等。

- [ ] **Step 3: 实现**

在 `crates/voice-input-linux/src/tray.rs` 的 `mod linux_tray` 顶部补 `use`：

```rust
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use ksni::blocking::TrayMethods;
    use ksni::menu::StandardItem;
    use ksni::{Category, MenuItem, Status, ToolTip, Tray};

    use crate::bridge_addr::bridge_url;
    use crate::recorder::LinuxMicAudioRecorder;
    use crate::restart::RestartController;
    use crate::settings::LinuxAppSettings;
    use crate::system_ime::SystemImeController;
    use voice_input_core::{Result, VoiceInputError};
```

`LinuxTrayConfig` 增加字段（结构体与 `new` 都要加）：

```rust
    #[derive(Debug, Clone)]
    pub struct LinuxTrayConfig {
        pub service_name: String,
        pub title: String,
        pub recorder: LinuxMicAudioRecorder,
        pub quit_requested: Arc<AtomicBool>,
        pub restart: RestartController,
        pub restart_requested: Arc<AtomicBool>,
        pub ime: SystemImeController,
        /// 本机麦克风开关的共享状态，托盘与主循环共读。
        pub mic_enabled: Arc<AtomicBool>,
        /// 手机桥接开关的共享状态。
        pub bridge_enabled: Arc<AtomicBool>,
        /// 端口是否绑定成功。false 时相关菜单项置灰。
        pub bridge_available: bool,
        pub bridge_port: u16,
        pub bridge_token: String,
        /// 本机局域网 IP。`None` 表示探测失败。
        pub local_ip: Option<String>,
        /// 初始开关状态，用于预填托盘的本地副本。
        pub settings: LinuxAppSettings,
    }

    impl LinuxTrayConfig {
        #[allow(clippy::too_many_arguments)]
        pub fn new(
            service_name: impl Into<String>,
            title: impl Into<String>,
            recorder: LinuxMicAudioRecorder,
            quit_requested: Arc<AtomicBool>,
            restart: RestartController,
            restart_requested: Arc<AtomicBool>,
            ime: SystemImeController,
            mic_enabled: Arc<AtomicBool>,
            bridge_enabled: Arc<AtomicBool>,
            bridge_available: bool,
            bridge_port: u16,
            bridge_token: String,
            local_ip: Option<String>,
            settings: LinuxAppSettings,
        ) -> Self {
            Self {
                service_name: service_name.into(),
                title: title.into(),
                recorder,
                quit_requested,
                restart,
                restart_requested,
                ime,
                mic_enabled,
                bridge_enabled,
                bridge_available,
                bridge_port,
                bridge_token,
                local_ip,
                settings,
            }
        }
    }
```

`LinuxTrayHandle` 增加两个 getter 与一个「落盘」方法：

```rust
        pub fn is_mic_enabled(&self) -> bool {
            self.mic_enabled.load(Ordering::SeqCst)
        }

        pub fn is_bridge_enabled(&self) -> bool {
            self.bridge_enabled.load(Ordering::SeqCst)
        }
```

`LinuxTrayHandle::spawn` 里要拿到共享状态：

```rust
        pub fn spawn(config: LinuxTrayConfig) -> Result<Self> {
            let mic_enabled = Arc::clone(&config.mic_enabled);
            let bridge_enabled = Arc::clone(&config.bridge_enabled);
            let tray = LinuxTray::new(config);
            let quit_requested = Arc::clone(&tray.quit_requested);
            let restart_requested = Arc::clone(&tray.restart_requested);
            let handle = tray
                .disable_dbus_name(false)
                .assume_sni_available(true)
                .spawn()
                .map_err(|err| {
                    VoiceInputError::Injection(format!("启动 Linux 状态栏失败：{err}"))
                })?;

            Ok(Self {
                handle,
                quit_requested,
                restart_requested,
                mic_enabled,
                bridge_enabled,
            })
        }
```

`LinuxTrayHandle` 结构体加两个字段：

```rust
    pub struct LinuxTrayHandle {
        handle: ksni::blocking::Handle<LinuxTray>,
        quit_requested: Arc<AtomicBool>,
        restart_requested: Arc<AtomicBool>,
        mic_enabled: Arc<AtomicBool>,
        bridge_enabled: Arc<AtomicBool>,
    }
```

`LinuxTray` 结构体加字段，`new` 里填充：

```rust
    struct LinuxTray {
        service_name: String,
        title: String,
        recording: bool,
        recorder: LinuxMicAudioRecorder,
        quit_requested: Arc<AtomicBool>,
        restart: RestartController,
        restart_requested: Arc<AtomicBool>,
        ime: SystemImeController,
        // 开关用 Cell：ksni 在每次 clicked 之后都会重算菜单（service.rs 的
        // event() → update()），menu() 拿到的一定是最新值，不需要原子类型。
        mic_enabled: Cell<bool>,
        bridge_enabled: Cell<bool>,
        bridge_available: bool,
        bridge_port: u16,
        bridge_token: String,
        local_ip: Option<String>,
        /// 与共享 AtomicBool 同步的 settings 副本，退出时整体落盘。
        settings: RefCell<LinuxAppSettings>,
    }
```

`LinuxTray::new`：

```rust
        fn new(config: LinuxTrayConfig) -> Self {
            Self {
                service_name: config.service_name,
                title: config.title,
                recording: false,
                recorder: config.recorder,
                quit_requested: config.quit_requested,
                restart: config.restart,
                restart_requested: config.restart_requested,
                ime: config.ime,
                mic_enabled: Cell::new(config.settings.mic_enabled),
                bridge_enabled: Cell::new(config.settings.bridge_enabled),
                bridge_available: config.bridge_available,
                bridge_port: config.bridge_port,
                bridge_token: config.bridge_token,
                local_ip: config.local_ip,
                settings: RefCell::new(config.settings),
            }
        }
```

在 `impl LinuxTray` 里补方法（接在 `ime_enabled` 之后）：

```rust
        fn mic_label(&self) -> String {
            format!(
                "本机麦克风：{}",
                if self.mic_enabled.get() { "已启用" } else { "已停用" }
            )
        }

        fn mic_toggle_enabled(&self) -> bool {
            !self.recording
        }

        fn toggle_mic(&mut self) {
            let next = !self.mic_enabled.get();
            self.mic_enabled.set(next);
            self.mic_enabled_shared.store(next, Ordering::SeqCst);
            self.settings.borrow_mut().mic_enabled = next;
        }

        fn bridge_label(&self) -> String {
            if !self.bridge_available {
                return "手机输入（端口被占用）".to_string();
            }
            format!(
                "手机输入：{}",
                if self.bridge_enabled.get() { "已启用" } else { "已停用" }
            )
        }

        fn bridge_toggle_enabled(&self) -> bool {
            self.bridge_available
        }

        fn toggle_bridge(&mut self) {
            let next = !self.bridge_enabled.get();
            self.bridge_enabled.set(next);
            self.bridge_enabled_shared.store(next, Ordering::SeqCst);
            self.settings.borrow_mut().bridge_enabled = next;
        }

        fn copy_address_label(&self) -> String {
            format!("复制外部输入地址（{}）", self.bridge_port)
        }

        fn copy_address_enabled(&self) -> bool {
            self.bridge_available && self.local_ip.is_some()
        }

        /// 地址在点击时现算——网卡是 DHCP 动态的，IP 会变。
        /// IP 探测失败时回落 127.0.0.1，方便在本机浏览器里验证页面本身。
        fn address_url(&self) -> Option<String> {
            let ip = self
                .local_ip
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string());
            Some(bridge_url(&ip, self.bridge_port, &self.bridge_token))
        }
```

`LinuxTray` 结构体除上面列的字段外，再加两个共享状态的持有者。
`new` 里**先把 `config` 的两个 `Arc` clone 出来**再构建其余字段：

```rust
        fn new(config: LinuxTrayConfig) -> Self {
            let mic_enabled_shared = Arc::clone(&config.mic_enabled);
            let bridge_enabled_shared = Arc::clone(&config.bridge_enabled);

            Self {
                service_name: config.service_name,
                title: config.title,
                recording: false,
                recorder: config.recorder,
                quit_requested: config.quit_requested,
                restart: config.restart,
                restart_requested: config.restart_requested,
                ime: config.ime,
                mic_enabled: Cell::new(config.settings.mic_enabled),
                bridge_enabled: Cell::new(config.settings.bridge_enabled),
                mic_enabled_shared,
                bridge_enabled_shared,
                bridge_available: config.bridge_available,
                bridge_port: config.bridge_port,
                bridge_token: config.bridge_token,
                local_ip: config.local_ip,
                settings: RefCell::new(config.settings),
            }
        }
```

结构体里对应的两个字段：

```rust
        /// 与 Cell 同步的共享状态，服务线程与主循环读它。
        mic_enabled_shared: Arc<AtomicBool>,
        bridge_enabled_shared: Arc<AtomicBool>,
```
```

在菜单里，`Menu: Separator`（第一个分隔符，位于「停止当前录音」之后）之后插入四项：

```rust
            items.push(
                StandardItem {
                    label: self.mic_label(),
                    enabled: self.mic_toggle_enabled(),
                    visible: true,
                    icon_name: "audio-input-microphone".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        tray.toggle_mic();
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(
                StandardItem {
                    label: self.bridge_label(),
                    enabled: self.bridge_toggle_enabled(),
                    visible: true,
                    icon_name: "phone".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        tray.toggle_bridge();
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(
                StandardItem {
                    label: self.copy_address_label(),
                    enabled: self.copy_address_enabled(),
                    visible: true,
                    icon_name: "edit-copy".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        let Some(url) = tray.address_url() else {
                            eprintln!("复制外部输入地址失败：没有可用地址");
                            return;
                        };
                        match crate::ibus::restore_clipboard_text(&url) {
                            // 日志不记 URL 内容——里面含 token。
                            Ok(()) => println!("外部输入地址已复制到剪贴板"),
                            Err(err) => eprintln!("复制外部输入地址失败：{err}"),
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(MenuItem::Separator);
```

菜单顺序最终为：状态 / 停止当前录音 / ─── / 本机麦克风 / 手机输入 / 复制外部输入地址 / ─── / 重启输入法 / 重启系统输入法 / ─── / 退出。

`LinuxTrayHandle` 增加落盘方法：

```rust
        /// 把内存里的开关状态落盘。只在正常退出路径调用——
        /// 每次点击都写盘会在目录被 inotify 监视时引发连锁反应，
        /// 而主循环反正每轮读内存变量，即时写盘对生效没有帮助。
        pub fn persist_settings(&self) {
            let _ = self.handle.update(|tray| {
                let snapshot = tray.settings.borrow().clone();
                if let Err(err) = snapshot.save() {
                    eprintln!("保存 Linux 配置失败：{err}");
                }
            });
        }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux tray::tests`

Expected: `test result: ok. 20 passed`（原有 4 个重启/ime 测试 + 新增 16 个）

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/tray.rs
git commit -m "feat(voice-input-linux): 托盘新增两个输入开关与复制地址

开关用纯文字项而非 CheckmarkItem——后者自行翻转 toggle_state，
与我们自己的 bool 会不同步。开关值同时写 Cell（托盘自己渲染）
与 Arc<AtomicBool>（服务线程/主循环读）。
复制地址在点击时现算 IP，网卡是 DHCP 动态的。"
```

---

## Task 10: 主循环排空队列并注入

**Files:**
- Modify: `crates/voice-input-linux/src/runtime.rs`
- Test: `crates/voice-input-linux/src/runtime.rs`

本任务先只做「排空 + 注入」这块可单测的逻辑。服务线程的接线放在 Task 11——
那样 Task 10 的测试不需要起真实 socket，跑得快。

- [ ] **Step 1: 写失败的测试**

在 `crates/voice-input-linux/src/runtime.rs` 的 `mod tests` 里，把现有内容替换为：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::mpsc;
    use voice_input_core::InputMethodHost;

    #[test]
    fn filler_transcript_is_discarded() {
        assert!(is_filler_transcript("嗯。"));
        assert!(is_filler_transcript("嗯"));
        assert!(!is_filler_transcript("嗯嗯"));
        assert!(!is_filler_transcript("好的"));
        assert!(!is_filler_transcript(""));
    }

    /// 记录收到的文本，用于断言注入内容。
    #[derive(Default)]
    struct RecordingHost {
        committed: RefCell<Vec<String>>,
        fail: bool,
    }

    impl InputMethodHost for RecordingHost {
        fn start_composition(&self) -> voice_input_core::Result<()> {
            Ok(())
        }

        fn commit_text(&self, text: &str) -> voice_input_core::Result<()> {
            if self.fail {
                return Err(voice_input_core::VoiceInputError::Injection(
                    "模拟注入失败".to_string(),
                ));
            }
            self.committed.borrow_mut().push(text.to_string());
            Ok(())
        }

        fn cancel_composition(&self) -> voice_input_core::Result<()> {
            Ok(())
        }

        fn end_composition(&self) -> voice_input_core::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn drains_all_queued_requests() {
        let host = RecordingHost::default();
        let (tx, rx) = mpsc::sync_channel(8);
        let mut replies = Vec::new();
        for text in ["第一条", "第二条", "第三条"] {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.try_send(BridgeRequest {
                text: text.to_string(),
                reply: reply_tx,
            })
            .expect("入队应当成功");
            replies.push(reply_rx);
        }

        let injected = drain_bridge_requests(&rx, &host);

        assert_eq!(injected, 3);
        assert_eq!(
            host.committed.borrow().clone(),
            vec!["第一条", "第二条", "第三条"]
        );
        for reply in replies {
            assert!(reply.try_recv().expect("每条都应当回票").is_ok());
        }
    }

    #[test]
    fn empty_queue_injects_nothing() {
        let host = RecordingHost::default();
        let (_tx, rx) = mpsc::sync_channel::<BridgeRequest>(8);

        assert_eq!(drain_bridge_requests(&rx, &host), 0);
        assert!(host.committed.borrow().is_empty());
    }

    #[test]
    fn injection_failure_is_reported_but_does_not_stop_the_drain() {
        let host = RecordingHost {
            fail: true,
            ..Default::default()
        };
        let (tx, rx) = mpsc::sync_channel(8);
        let mut replies = Vec::new();
        for text in ["甲", "乙"] {
            let (reply_tx, reply_rx) = mpsc::sync_channel(1);
            tx.try_send(BridgeRequest {
                text: text.to_string(),
                reply: reply_tx,
            })
            .expect("入队应当成功");
            replies.push(reply_rx);
        }

        let injected = drain_bridge_requests(&rx, &host);

        assert_eq!(injected, 0, "失败的不计入成功数");
        for reply in replies {
            assert!(
                reply.try_recv().expect("失败也要回票").is_err(),
                "失败必须回票，否则页面只能干等到超时"
            );
        }
    }

    #[test]
    fn disconnected_reply_does_not_panic() {
        // 手机提前断开时回票会失败，主循环不能因此崩溃。
        let host = RecordingHost::default();
        let (tx, rx) = mpsc::sync_channel(8);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        drop(reply_rx);
        tx.try_send(BridgeRequest {
            text: "内容".to_string(),
            reply: reply_tx,
        })
        .expect("入队应当成功");

        assert_eq!(drain_bridge_requests(&rx, &host), 1);
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux runtime::tests`

Expected: 编译失败，报 `cannot find function 'drain_bridge_requests'` 与 `cannot find type 'BridgeRequest'`。

- [ ] **Step 3: 实现**

在 `crates/voice-input-linux/src/runtime.rs` 顶部补 `use`：

```rust
use std::sync::mpsc::Receiver;

use crate::bridge::BridgeRequest;
use crate::bridge_http::char_count;
```

在 `is_filler_transcript` 之后插入：

```rust
/// 排空桥接队列并把每条文本注入当前光标。返回成功注入的条数。
///
/// 注入前保存剪贴板、注入后还原——`insert_text_into_active_window` 对非 ASCII
/// 文本走剪贴板粘贴路径，不还原的话用户正在复制的内容会被手机文字覆盖。
/// 这里不调 `insert_indicator_and_save_clipboard`：那会在光标处插一个 ●，
/// 而用户在手机上，看不到也不需要。
fn drain_bridge_requests(rx: &Receiver<BridgeRequest>, host: &dyn InputMethodHost) -> usize {
    let mut injected = 0;

    while let Ok(request) = rx.try_recv() {
        let saved = crate::ibus::read_clipboard_text();
        let result = host.commit_text(&request.text);
        if let Some(previous) = crate::ibus::clipboard_text_to_restore(saved) {
            if let Err(err) = crate::ibus::restore_clipboard_text(&previous) {
                eprintln!("还原剪贴板失败：{err}");
            }
        }

        // 日志只记长度与结果，不记内容——手机语音可能包含密码或私事。
        match result {
            Ok(()) => {
                injected += 1;
                println!("手机输入：已注入 {} 字", char_count(&request.text));
                // 页面可能已经断开，回票失败不是错误。
                let _ = request.reply.try_send(Ok(()));
            }
            Err(err) => {
                eprintln!("手机输入注入失败：{err}");
                let _ = request.reply.try_send(Err(err.to_string()));
            }
        }
    }

    injected
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux runtime::tests`

Expected: `test result: ok. 5 passed`

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/runtime.rs
git commit -m "feat(voice-input-linux): 主循环排空桥接队列并注入

注入前后保存-还原剪贴板（非 ASCII 文本走剪贴板粘贴路径）。
失败也回票，否则页面只能干等到 2s 超时，用户看不到原因。
日志只记长度与结果，不记内容。"
```

---

## Task 11: 服务线程接线与优雅降级

**Files:**
- Modify: `crates/voice-input-linux/src/runtime.rs`
- Modify: `crates/voice-input-linux/src/main.rs`（usage 文案）
- Test: `crates/voice-input-linux/src/runtime.rs`（`bridge_setup` 纯函数单测）

- [ ] **Step 1: 写失败的测试**

在 `crates/voice-input-linux/src/runtime.rs` 的 `mod tests` 末尾追加：

```rust
    #[test]
    fn bridge_setup_generates_token_when_missing() {
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 8765,
            stored_token: String::new(),
            saved_settings: LinuxAppSettings::default(),
        };

        let setup = prepare_bridge(config);

        assert_eq!(setup.token.len(), 32, "应当生成 32 位十六进制 token");
        assert!(
            setup.settings.bridge_token == setup.token,
            "生成的 token 必须写回 settings，否则重启后手机上存的地址就失效了"
        );
    }

    #[test]
    fn bridge_setup_reuses_stored_token() {
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 8765,
            stored_token: "storedtoken".to_string(),
            saved_settings: LinuxAppSettings::default(),
        };

        let setup = prepare_bridge(config);

        assert_eq!(setup.token, "storedtoken");
    }

    #[test]
    fn bridge_setup_carries_port_and_switches() {
        let saved = LinuxAppSettings {
            mic_enabled: false,
            bridge_enabled: true,
            bridge_port: 9999,
            ..Default::default()
        };
        let config = BridgeSetupConfig {
            enabled_in_settings: true,
            port: 9999,
            stored_token: "t".to_string(),
            saved_settings: saved,
        };

        let setup = prepare_bridge(config);

        assert_eq!(setup.settings.bridge_port, 9999);
        assert!(!setup.settings.mic_enabled);
        assert!(setup.settings.bridge_enabled);
    }

    #[test]
    fn bridge_setup_keeps_switches_from_settings() {
        let saved = LinuxAppSettings {
            mic_enabled: false,
            bridge_enabled: false,
            ..Default::default()
        };
        let config = BridgeSetupConfig {
            enabled_in_settings: false,
            port: 8765,
            stored_token: "t".to_string(),
            saved_settings: saved,
        };

        let setup = prepare_bridge(config);

        assert!(!setup.settings.bridge_enabled);
        assert!(!setup.settings.mic_enabled);
    }
```

在 `mod tests` 的 `use` 里补：

```rust
    use crate::settings::LinuxAppSettings;
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux runtime::tests`

Expected: 编译失败，报 `cannot find type 'BridgeSetupConfig'`。

- [ ] **Step 3: 实现 setup 纯函数**

在 `crates/voice-input-linux/src/runtime.rs` 顶部补 `use`：

```rust
use std::sync::Arc;

use crate::bridge::{spawn_bridge, BridgeConfig, BridgeStatus, QUEUE_CAPACITY};
use crate::bridge_addr::{detect_local_ipv4, generate_token};
use crate::settings::LinuxAppSettings;
```

在 `drain_bridge_requests` 之后插入：

```rust
/// 桥接启动所需的外部输入。抽成结构体是为了能在单测里构造。
#[derive(Debug, Clone)]
pub struct BridgeSetupConfig {
    pub enabled_in_settings: bool,
    pub port: u16,
    pub stored_token: String,
    pub saved_settings: LinuxAppSettings,
}

/// 桥接的启动参数与初始状态。
#[derive(Debug, Clone)]
pub struct BridgeSetup {
    pub token: String,
    pub settings: LinuxAppSettings,
}

/// 决定 token 与生效 settings。
///
/// token 缺失时生成一个新的并写回 settings——这一点必须落盘，
/// 否则每次重启都换 token，手机上存的地址全部失效。
pub fn prepare_bridge(config: BridgeSetupConfig) -> BridgeSetup {
    let token = if config.stored_token.trim().is_empty() {
        generate_token()
    } else {
        config.stored_token
    };

    let mut settings = config.saved_settings;
    settings.bridge_token = token.clone();
    settings.bridge_port = config.port;
    // `enabled_in_settings` 参数保留为显式兜底：settings 里的开关是权威值，
    // 但调用方若显式传入 false（例如 --no-bridge），以调用方为准。
    if !config.enabled_in_settings {
        settings.bridge_enabled = false;
    }

    BridgeSetup { token, settings }
}
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux runtime::tests`

Expected: `test result: ok. 9 passed`

- [ ] **Step 5: 接线到 `run_live_app`**

在 `crates/voice-input-linux/src/runtime.rs` 的 `LinuxLiveAppConfig` 增加一个字段：

```rust
#[derive(Debug, Clone)]
pub struct LinuxLiveAppConfig {
    pub app: AppConfig,
    pub asr: FunAsrConfig,
    pub max_recording_duration: Duration,
    pub double_press_window: Duration,
    pub silence_stop_timeout: Duration,
    pub show_status_item: bool,
    pub service_name: String,
    /// 桥接监听端口。
    pub bridge_port: u16,
}
```

`Default` 里补：

```rust
            bridge_port: 8765,
```

把 `run_recording_cycle` 的 `host` 参数类型由 `&LinuxInputMethodHost` 改为 `&dyn InputMethodHost`，
并在 `run_live_app` 里按下面这段改造（保留原有的 recorder / watcher / tray / ASR 初始化，
只在其中插入桥接部分）：

```rust
pub fn run_live_app(config: LinuxLiveAppConfig) -> Result<()> {
    let Some(_instance_guard) = SingleInstanceGuard::acquire()? else {
        return Err(VoiceInputError::Injection(
            "检测到已有 Linux 常驻实例正在运行，请先退出旧实例后再启动".to_string(),
        ));
    };

    let recorder = LinuxMicAudioRecorder::new(config.max_recording_duration);
    let recorder_for_watcher = recorder.clone();
    let active = Arc::new(LiveJobState::default());
    let active_for_watcher = Arc::clone(&active);
    let quit_requested = Arc::new(AtomicBool::new(false));
    let restart_requested = Arc::new(AtomicBool::new(false));
    let restart = RestartController::detect();
    let ime = SystemImeController::detect();
    let activation_hotkey = config.app.activation_hotkey.clone();
    let hotkey = LinuxHotkeySpec::parse(&activation_hotkey)?;
    let watcher = LinuxHotkeyWatcher::spawn(
        hotkey,
        active_for_watcher,
        recorder_for_watcher,
        config.double_press_window,
    )?;
    let host = LinuxInputMethodHost::new();
    println!("正在预加载 ASR 模型...");
    let asr_runner = build_linux_asr(&config.asr)?;
    let transcriber = LocalFunAsrTranscriber::new(config.asr.clone(), asr_runner);
    println!("ASR 模型预加载完成");

    // ---- 桥接 ----
    let saved_settings = LinuxAppSettings::load();
    let setup = prepare_bridge(BridgeSetupConfig {
        enabled_in_settings: saved_settings.bridge_enabled,
        port: config.bridge_port,
        stored_token: saved_settings.bridge_token.clone(),
        saved_settings,
    });
    let mic_enabled = Arc::new(AtomicBool::new(setup.settings.mic_enabled));
    let bridge_enabled = Arc::new(AtomicBool::new(setup.settings.bridge_enabled));

    let bridge_status = Arc::new(BridgeStatus::new(
        Arc::clone(&bridge_enabled),
        setup.token.clone(),
        recorder.clone(),
    ));
    let (bridge_tx, bridge_rx) = std::sync::mpsc::sync_channel(QUEUE_CAPACITY);
    let bridge = match spawn_bridge(
        BridgeConfig {
            port: config.bridge_port,
        },
        Arc::clone(&bridge_status),
        bridge_tx,
    ) {
        Ok(handle) => Some(handle),
        Err(err) => {
            // 端口绑定失败不影响主程序启动，退化为「无手机输入」。
            eprintln!("{err}");
            eprintln!("手机输入已禁用；本机麦克风不受影响");
            None
        }
    };

    let local_ip = detect_local_ipv4().map(|ip| ip.to_string());
    if let Some(ref handle) = bridge {
        let display_ip = local_ip
            .clone()
            .unwrap_or_else(|| "127.0.0.1".to_string());
        println!("手机输入已就绪，监听端口 {}", handle.local_addr().port());
        println!(
            "手机地址（托盘菜单可复制）：{}",
            crate::bridge_addr::bridge_url(&display_ip, handle.local_addr().port(), &setup.token)
        );
    }

    let tray = if config.show_status_item {
        let tray = spawn_linux_tray(LinuxTrayConfig::new(
            config.service_name.clone(),
            "VoiceInput".to_string(),
            recorder.clone(),
            Arc::clone(&quit_requested),
            restart,
            Arc::clone(&restart_requested),
            ime,
            Arc::clone(&mic_enabled),
            Arc::clone(&bridge_enabled),
            bridge.is_some(),
            config.bridge_port,
            setup.token.clone(),
            local_ip,
            setup.settings.clone(),
        ))?;
        tray.set_recording(false);
        Some(tray)
    } else {
        None
    };

    let hotkey_label = hotkey.describe(&activation_hotkey, config.double_press_window);
    let silence_label = format!(
        "静音自动停录：{}ms",
        config.silence_stop_timeout.as_millis()
    );
    let status_label = if config.show_status_item {
        Some("状态提示：已启用".to_string())
    } else {
        None
    };
    print_live_ready(
        "Linux",
        &hotkey_label,
        "双击一次开始录音，再双击一次停止并转写；手机发来的文字直接注入当前光标",
        [
            format!("双击间隔：{}ms", config.double_press_window.as_millis()),
            silence_label,
        ]
        .into_iter()
        .chain(status_label.into_iter()),
    );

    loop {
        if quit_requested.load(Ordering::SeqCst) || restart_requested.load(Ordering::SeqCst) {
            watcher.stop();
            break;
        }

        // 手机发来的文字优先处理：它们已经在等回票，拖久了 HTTP 会超时。
        drain_bridge_requests(&bridge_rx, &host);

        let triggered = watcher.wait_for_trigger_timeout(Duration::from_millis(250))?;
        if !triggered {
            continue;
        }

        // 本机麦克风开关关闭时忽略热键，但要留一行日志，
        // 否则用户会以为热键坏了。
        if !mic_enabled.load(Ordering::SeqCst) {
            println!("本机麦克风已停用，忽略热键触发");
            continue;
        }

        let Some(_job) = LiveJobState::try_acquire(&active) else {
            continue;
        };

        if run_recording_cycle(
            &recorder,
            &host,
            &transcriber,
            config.silence_stop_timeout,
            tray.as_ref(),
            &watcher,
        )? {
            break;
        }
    }

    // 正常退出路径：把内存里的开关状态落盘。
    if let Some(tray) = tray.as_ref() {
        tray.persist_settings();
        tray.shutdown();
    }

    Ok(())
}
```

注意 `bridge` 变量在 `loop` 里没有被读到，但它的 `Drop` 正是「退出时停掉服务线程」的实现——
所以**不能写成 `let _ = bridge;`**（那样会在启动后立刻 drop）。保持 `let bridge = ...` 绑定即可，
若 clippy 报 `unused_variables`，改为 `let _bridge = ...`（下划线前缀的名字不会提前 drop）。

- [ ] **Step 6: 运行全部测试**

Run: `cargo test -p voice-input-linux`

Expected: 全部通过（`unit` + `tests/bridge.rs` + `tests/session.rs` 三层）。

- [ ] **Step 7: 提交**

```bash
git add crates/voice-input-linux/src/runtime.rs
git commit -m "feat(voice-input-linux): 接线桥接服务并支持优雅降级

端口占用时不禁用主程序，退化为「无手机输入」并在托盘置灰相关项。
token 首次生成后写回 settings，避免每次重启换 token 让手机地址失效。
主循环每轮优先排空桥接队列——它们已经在等回票，拖久了会超时。
退出前把内存里的开关状态落盘。"
```

---

## Task 12: `--bridge-port` CLI 参数

**Files:**
- Modify: `crates/voice-input-linux/src/live_cli.rs`
- Test: `crates/voice-input-linux/src/live_cli.rs`

Task 1 给 `LinuxAppSettings` 加了字段后，`live_cli.rs` 里那句结构体字面量
（`LinuxAppSettings { double_press_window_ms, silence_stop_timeout_ms }`）**已经编译不过**。
本任务连同新参数一起修掉。

- [ ] **Step 1: 写失败的测试**

在 `crates/voice-input-linux/src/live_cli.rs` 的 `mod tests` 末尾追加：

```rust
    #[test]
    fn parse_live_args_reads_bridge_port() {
        let args = parse_live_args(vec![
            "voice-input-linux-live".to_string(),
            "--bridge-port".to_string(),
            "9000".to_string(),
        ])
        .expect("--bridge-port 应当解析成功");

        assert_eq!(args.bridge_port, Some(9000));
    }

    #[test]
    fn parse_live_args_defaults_bridge_port_to_none() {
        let args = parse_live_args(vec!["voice-input-linux-live".to_string()])
            .expect("空参数应当解析成功");

        assert_eq!(args.bridge_port, None);
    }

    #[test]
    fn parse_live_args_rejects_non_numeric_bridge_port() {
        let err = parse_live_args(vec![
            "voice-input-linux-live".to_string(),
            "--bridge-port".to_string(),
            "abc".to_string(),
        ])
        .expect_err("非数字端口应当被拒绝");

        assert!(
            err.contains("--bridge-port 必须是端口号"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_live_args_rejects_port_above_u16() {
        let err = parse_live_args(vec![
            "voice-input-linux-live".to_string(),
            "--bridge-port".to_string(),
            "70000".to_string(),
        ])
        .expect_err("超出 u16 的端口应当被拒绝");

        assert!(
            err.contains("--bridge-port 必须是端口号"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_live_args_rejects_port_zero() {
        // 0 会被系统随机分配，用户以为指定了端口但实际不是，宁可直接报错。
        let err = parse_live_args(vec![
            "voice-input-linux-live".to_string(),
            "--bridge-port".to_string(),
            "0".to_string(),
        ])
        .expect_err("0 端口应当被拒绝");

        assert!(
            err.contains("--bridge-port 不能是 0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_live_args_requires_bridge_port_value() {
        let err = parse_live_args(vec![
            "voice-input-linux-live".to_string(),
            "--bridge-port".to_string(),
        ])
        .expect_err("缺少值应当被拒绝");

        assert!(
            err.contains("缺少 --bridge-port 的值"),
            "unexpected error: {err}"
        );
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux live_cli::tests`

Expected: 编译失败——先报 `LinuxAppSettings` 字面量缺字段，改完那处后报
`no field 'bridge_port' on type 'LinuxLiveArgs'`。

- [ ] **Step 3: 实现**

`LinuxLiveArgs` 增加字段与默认值：

```rust
#[derive(Debug, Clone)]
pub struct LinuxLiveArgs {
    pub activation_hotkey: Option<String>,
    pub double_press_window_ms: Option<u64>,
    pub silence_stop_ms: Option<u64>,
    pub bridge_port: Option<u16>,
}

impl Default for LinuxLiveArgs {
    fn default() -> Self {
        Self {
            activation_hotkey: None,
            double_press_window_ms: None,
            silence_stop_ms: None,
            bridge_port: None,
        }
    }
}
```

`parse_live_args` 的 `match` 里，在 `--silence-stop-ms` 分支后插入：

```rust
            "--bridge-port" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --bridge-port 的值"))?;
                let port = value
                    .parse::<u16>()
                    .map_err(|_| String::from("--bridge-port 必须是端口号（1-65535）"))?;
                if port == 0 {
                    return Err(String::from("--bridge-port 不能是 0"));
                }
                parsed.bridge_port = Some(port);
            }
```

`run_live_with_args` 里把 settings 的构造改成从持久化值派生，只覆盖 CLI 显式给出的项：

```rust
pub fn run_live_with_args(args: LinuxLiveArgs) -> Result<(), String> {
    let persisted_settings = LinuxAppSettings::load();
    let effective_window_ms = args
        .double_press_window_ms
        .unwrap_or(persisted_settings.double_press_window_ms);
    let effective_silence_stop_ms = args
        .silence_stop_ms
        .unwrap_or(persisted_settings.silence_stop_timeout_ms)
        .max(1500);

    println!("配置文件：{}", settings_path().display());
    println!(
        "已加载双击间隔：{}ms，生效值：{}ms",
        persisted_settings.double_press_window_ms, effective_window_ms
    );
    println!(
        "已加载静音停录：{}ms，生效值：{}ms",
        persisted_settings.silence_stop_timeout_ms, effective_silence_stop_ms
    );
    if effective_silence_stop_ms != persisted_settings.silence_stop_timeout_ms {
        println!("静音自动停录已提升到更保守的下限：1500ms");
    }

    let effective_bridge_port = args.bridge_port.unwrap_or(persisted_settings.bridge_port);
    println!("手机输入端口：{effective_bridge_port}");

    // 从持久化值整体派生，只覆盖本次显式指定的项。
    // 不要用结构体字面量——那会每次新增字段就编译失败，也会丢掉 token 与开关。
    let mut settings = persisted_settings;
    settings.double_press_window_ms = effective_window_ms;
    settings.silence_stop_timeout_ms = effective_silence_stop_ms;
    settings.bridge_port = effective_bridge_port;
    if let Err(err) = settings.save() {
        eprintln!("保存 Linux 配置失败：{err}");
    }

    if cfg!(not(feature = "ibus")) {
        return Err("当前构建未启用 IBus 支持，请使用 --features ibus 重新构建".to_string());
    }

    let mut config = LinuxLiveAppConfig {
        max_recording_duration: Duration::from_secs(30),
        double_press_window: Duration::from_millis(effective_window_ms),
        silence_stop_timeout: Duration::from_millis(effective_silence_stop_ms),
        bridge_port: effective_bridge_port,
        ..Default::default()
    };

    if let Some(hotkey) = args.activation_hotkey.as_deref() {
        config.app.activation_hotkey = hotkey.to_string();
    }

    run_live_app(config).map_err(|err| format!("Linux 常驻应用启动失败：{err}"))
}
```

注意这里 `settings.save()` 发生在 `run_live_app` 之前——它写的是**本次生效的配置**
（含 token 与开关的持久化值），而运行期的开关切换改的是内存副本，由 `persist_settings()`
在退出时写回。两者写同一文件，顺序无碍。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux live_cli::tests`

Expected: `test result: ok. 12 passed`（原有 6 个 + 新增 6 个）

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/live_cli.rs
git commit -m "feat(voice-input-linux): 新增 --bridge-port 参数

顺手修掉 settings 结构体字面量——Task 1 加字段后它编译不过；
改为从持久化值整体派生再覆盖显式项，避免每次加字段都炸，
也不会丢掉已有的 token 与开关状态。"
```

---

## Task 13: 更新 README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 在「模型」一节之前插入手机输入一节**

在 `README.md` 里，「## 模型」这一行之前插入：

```markdown
## 手机外部输入

手机和本机连在同一局域网时，可以用手机输入法说话，把文字注入本机光标。
手机端不采集麦克风、不需要 HTTPS——识别发生在手机键盘里，只传文字。

1. 右键托盘图标，点「复制外部输入地址」
2. 把地址发到手机（微信/邮件均可），在手机上打开
3. 用输入法的语音键说话，确认文字后点「发送」

页面上的地址形如 `http://10.10.10.42:8765/#k=<32 位 token>`。
token 放 URL fragment（`#` 之后），不会进浏览器历史也不会随 Referer 外发。

托盘菜单里有两个独立开关：

| 菜单项 | 关掉后 |
|---|---|
| 本机麦克风：已启用 | 双击 Ctrl 被忽略（打一行日志），手机输入不受影响 |
| 手机输入：已启用 | 端口仍监听，手机发送时返回「手机输入已关闭」 |

开关状态在退出时落盘（被 `kill -9` 时丢失，回落到两个都开的默认值）。

端口默认 8765，可用 `--bridge-port` 指定：

```bash
cargo run -p voice-input-linux --features ibus -- live --backend ibus --bridge-port 9000
```

端口被占用时，**主程序照常启动**，只是托盘里手机输入相关项置灰。

服务绑 `0.0.0.0`，因此经由 tailscale 的地址同样可达（不限于同一 Wi-Fi）。
token 是唯一屏障，不要外传。
```

- [ ] **Step 2: 更新 live 命令示例**

在 `README.md` 的「命令入口」一节，把 live 那段改为：

```bash
# live：常驻托盘，热键触发录音；同时开放手机外部输入
cargo run -p voice-input-linux --features ibus -- live --backend ibus \
  [--activation-hotkey DoubleCtrl] [--double-press-window-ms 300] [--silence-stop-ms 1500] \
  [--bridge-port 8765]
```

- [ ] **Step 3: 提交**

```bash
git add README.md
git commit -m "docs: 补充手机外部输入说明

含打开方式、两个托盘开关的语义、退出时落盘的行为、
端口占用时的降级，以及「token 是唯一屏障」的提醒。"
```

---

## Task 14: 真机验证

**Files:** 无（只做验证与记录）

前 13 个任务的测试都在进程内跑，有两个环节只有真机上才暴露得出来：
`xdotool` 实际注入多长中文会卡，以及手机浏览器与 fragment token 的配合。

- [ ] **Step 1: 确认基线通过**

Run: `cargo test -p voice-input-linux && cargo build --release -p voice-input-linux --features ibus`

Expected: 测试全绿，release 构建成功。

- [ ] **Step 2: 格式化（只跑本次相关文件）**

Run: `cargo fmt -- crates/voice-input-linux/src/bridge.rs crates/voice-input-linux/src/bridge_http.rs crates/voice-input-linux/src/bridge_addr.rs crates/voice-input-linux/src/tray.rs crates/voice-input-linux/src/runtime.rs crates/voice-input-linux/src/settings.rs crates/voice-input-linux/src/ibus.rs crates/voice-input-linux/src/live_cli.rs`

Expected: 无输出（或已自动格式化）。跑完 `cargo test -p voice-input-linux` 复查仍全绿。

clippy 本机装不上（镜像 404），本任务跳过并记偏差。

- [ ] **Step 3: 本机自测页面与 401**

先开一个前台实例（不装服务），在终端里读它打印的地址：

```bash
target/release/voice-input-linux live --backend ibus
```

用打印出的 `http://127.0.0.1:8765/#k=<token>` 在本机浏览器打开，确认：

- 页面渲染正常，输入框自动聚焦
- 提示文字为空（没有「缺少 token」）
- 把地址栏的 `#k=...` 删掉再刷新 —— 页面应显示「缺少 token」并禁用发送按钮

再手动验证 401：

```bash
curl -i -H "X-Auth-Token: wrong" -H "Content-Type: application/json" \
  -d '{"text":"x"}' http://127.0.0.1:8765/send
```

Expected: `HTTP/1.1 401`。日志里**不应**出现请求内容。

- [ ] **Step 4: 本机 curl 打通注入链路**

先把光标放到任意可输入处（编辑器或终端），然后：

```bash
curl -i -H "X-Auth-Token: <实际 token>" -H "Content-Type: application/json" \
  -d '{"text":"来自 curl 的测试\n第二行"}' http://127.0.0.1:8765/send
```

Expected: 返回 `HTTP/1.1 200 OK`，且**换行被折成空格**，
光标处出现 `来自 curl 的测试 第二行`。

再用 `status` 确认只读状态不影响注入：

```bash
curl -i -H "X-Auth-Token: <实际 token>" http://127.0.0.1:8765/api/status
```

Expected: `200`，body 为 `{"mic_recording":false,"bridge_enabled":true}`。

- [ ] **Step 5: 手机实测**

1. 托盘菜单点「复制外部输入地址」，把地址发到手机并打开
2. 用手机输入法的语音键说一句中文，点「发送」
3. 确认本机光标处出现该文字，页面历史列表里出现该条
4. 发送一条很长的文本（>250 字），观察注入耗时是否可接受
5. 切到另一个窗口再发一条，确认注入到新焦点窗口

记录到设计文档的执行记录里：实际注入耗时、`MAX_TEXT_CHARS` 是否偏大或偏小、
手机上 fragment token 是否被正常读取（部分浏览器对 fragment 的处理有差异）。

- [ ] **Step 6: 验证开关与降级**

1. 托盘点「手机输入：已启用」→ 变「已停用」；手机上再点发送，应显示「手机输入已关闭」
2. 再点回「已启用」，手机发送恢复正常
3. 托盘点「本机麦克风：已启用」→ 变「已停用」；双击 Ctrl 应有「本机麦克风已停用，忽略热键触发」日志且不录音
4. 顺序终止实例（托盘退出或 `systemctl --user restart voice-input.service`），确认 settings 里两个开关的值已被写回

```bash
cat ~/.config/voice-input/linux-app.toml
```

- [ ] **Step 7: 验证端口占用降级**

先占住 8765：

```bash
python3 -c "import socket,time; s=socket.socket(); s.bind(('0.0.0.0',8765)); s.listen(); time.sleep(60)" &
```

再启动一个实例，确认：主程序正常启动、日志里有绑定失败提示、
托盘里「手机输入」与「复制外部输入地址」两项置灰。测完 `kill %1`。

- [ ] **Step 8: 记录结果**

把实测结论（含与预期的偏差）追加到
`docs/superpowers/specs/2026-10-14-mobile-bridge-design.md` 末尾的「实测记录」一节，提交。

---

## 自查记录

写完计划后对照设计文档逐节核对，发现并修正了以下问题：

1. **`BridgeStatus` 里 `mic_recording` 原本是独立的 `AtomicBool`** —— 两处状态（录音器的真实状态与这个标志位）必然漂移，例如录音中途被托盘「停止当前录音」结束。改为持 `LinuxMicAudioRecorder` 并调 `is_recording()`，全流程单一事实来源。

2. **`BridgeConfig` 原本同时带 `port` 与 `token`** —— 但 token 要在运行时被托盘读取，且 `BridgeStatus` 已持有它。留两份必然不一致，改成只在 `BridgeStatus` 里存一份，`BridgeConfig` 只留 `port`。

3. **`spawn_bridge` 的测试构造与实现签名对不上** —— 计划初稿里测试用 `BridgeConfig { port, token }`、实现已改成只读 `status`，会编译失败。已统一。

4. **Task 9 里 `LinuxTray` 少了两个 `Arc` 字段** —— `toggle_mic` 要写共享状态，但结构体只声明了 `Cell` 版本；且 `Arc::clone` 必须在 `config` 被部分移动之前执行，否则借用已失效。已补字段并把 clone 提到 `new` 的开头。

5. **Task 12 忘了 `live_cli.rs` 的结构体字面量会编译失败** —— Task 1 加字段就破坏了它。已把这条写进 Task 12 并改为从持久化值派生。

6. **计划初稿漏了 `/api/status` 的鉴权测试** —— 设计文档明确 status 需要 token，已补。

7. **`bridge` 变量在 `run_live_app` 里没被读** —— 它的 `Drop` 就是「退出时停线程」，写成 `let _ = bridge;` 会立刻 drop。已在 Task 11 Step 5 写清这一点。

8. **测试函数名不能用中文标识符混排** —— Task 4 初稿写了 `tokens_differ_on_不同长度`，虽合法但工具链行为不一致，已改为 `tokens_differ_on_length_mismatch`。

**设计文档覆盖检查：** 目标 / 为什么不走手机麦克风 / 架构 / 请求流程 / 接口（含状态码与响应体）/ 文本归一化 / 剪贴板 / 托盘菜单（含开关语义、持久化时机、复制地址）/ 地址与 token / 页面 / 降级与错误处理 / 代码落点 / 测试策略 / YAGNI / 部署 —— 逐条对应到 Task 1–14，无遗漏。二维码后续项按 YAGNI 不做。






