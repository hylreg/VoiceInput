#[cfg(feature = "ibus")]
use std::process::Command;
#[cfg(feature = "ibus")]
use std::thread;
#[cfg(feature = "ibus")]
use std::time::{Duration, Instant};

use voice_input_core::Result;
#[cfg(not(feature = "ibus"))]
use voice_input_core::VoiceInputError;
#[cfg(feature = "ibus")]
use voice_input_core::VoiceInputError;

#[cfg(feature = "ibus")]
macro_rules! debug_xdotool {
    ($label:expr, $cmd:expr, $args:expr) => {{
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = crate::ibus::debug_timestamp();
            eprintln!(
                "[VOICEINPUT_DEBUG {now}] {} → args={:?}",
                $label,
                $args,
            );
        }
        let result = $cmd;
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = crate::ibus::debug_timestamp();
            match &result {
                Ok(status) => eprintln!("[VOICEINPUT_DEBUG {now}] {} ← exit={}", $label, status.success()),
                Err(e) => eprintln!("[VOICEINPUT_DEBUG {now}] {} ← err={}", $label, e),
            }
        }
        result
    }};
}

#[cfg(feature = "ibus")]
pub fn debug_timestamp() -> String {
    let elapsed = DEBUG_START.elapsed();
    format!(
        "{}.{:03}s",
        elapsed.as_secs(),
        elapsed.subsec_millis()
    )
}

#[cfg(feature = "ibus")]
static DEBUG_START: std::sync::LazyLock<Instant> =
    std::sync::LazyLock::new(Instant::now);

/// 在光标处插入录音指示符 ●，同时保存当前剪贴板。
/// 返回的 guard 持有 Clipboard 对象，确保还原后剪贴板内容
/// 存活到整个录音周期结束，不被剪贴板管理器丢弃。
#[cfg(feature = "ibus")]
pub fn insert_indicator_and_save_clipboard() -> Result<ClipboardRestoreGuard> {
    let saved = arboard::Clipboard::new()
        .ok()
        .and_then(|mut c| c.get_text().ok());

    insert_text_into_active_window("●", None)?;

    // 立即还原用户剪贴板，避免 ● 残留在剪贴板中
    let clipboard = if let Some(ref text) = saved {
        let mut c = arboard::Clipboard::new()
            .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
        c.set_text(text.clone())
            .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))?;
        Some(c)
    } else {
        None
    };

    Ok(ClipboardRestoreGuard {
        _clipboard: clipboard,
    })
}

/// 读取当前剪贴板文本。剪贴板为空或格式不是文本时返回 `None`。
///
/// 剪贴板被别的进程占用时会返回 `None` 而不是报错——“读不到原内容”
/// 只意味着后面不还原，不应该让整次注入失败。
#[cfg(feature = "ibus")]
pub fn read_clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// 把文本写回剪贴板。失败时返回错误说明。
///
/// 调用顺序必须是「先 `read_clipboard_text` → 再注入 → 最后还原」。
/// 注入后（即 arboard 自己持有选区期间）再调 `read_clipboard_text` 读到的是
/// 我们自己刚写进去的内容，`Some("")` / `None` 的语义就失效了。
///
/// 与 `insert_indicator_and_save_clipboard` 不同，这里**不**持有
/// `ClipboardRestoreGuard`：那个 guard 的作用是让连接活满整个 30s 录音周期。
/// 手机注入是瞬时操作，且紧接着的 `insert_text_into_active_window` 会重新
/// 建立连接，进程本身又是常驻的，所以内容不会丢。调用方不要「还原完就闲着」——
/// 那点余量由剪贴板管理器兜底。
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

/// 持有剪贴板还原逻辑的守卫。
/// `_clipboard` 字段在整个录音周期内保持 Clipboard 连接存活，
/// 确保剪贴板管理器在足够长的时间内能同步内容。
pub struct ClipboardRestoreGuard {
    #[allow(dead_code)]
    _clipboard: Option<arboard::Clipboard>,
}

#[cfg(feature = "ibus")]
pub fn insert_text_into_active_window(text: &str, window_id: Option<&str>) -> Result<()> {
    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        let preview: String = text.chars().take(20).collect();
        eprintln!("[VOICEINPUT_DEBUG {now}] insert_text len={} is_ascii={} preview=\"{preview}\" win={window_id:?}", text.len(), text.chars().all(|c| c.is_ascii()));
    }

    // xdotool type 无法正确处理中文等多字节字符，对纯 ASCII 文本才
    // 优先使用打字方式（避免覆盖剪贴板），对非 ASCII 文本直接走剪贴板粘贴。
    let is_ascii = text.chars().all(|c| c.is_ascii());
    if is_ascii && type_text_in_active_window(text, window_id).is_ok() {
        return Ok(());
    }

    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| VoiceInputError::Injection(format!("打开系统剪贴板失败：{e}")))?;
    clipboard
        .set_text(text.to_string())
        .map_err(|e| VoiceInputError::Injection(format!("写入系统剪贴板失败：{e}")))?;

    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] insert_text clipboard set, sleeping 40ms");
    }

    thread::sleep(Duration::from_millis(40));

    for shortcut in [
        ["key", "--clearmodifiers", "Shift+Insert"],
        ["key", "--clearmodifiers", "ctrl+v"],
    ] {
        let status = debug_xdotool!(
            "insert_text paste",
            Command::new("xdotool")
                .args(shortcut)
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            shortcut
        )?;

        if status.success() {
            return Ok(());
        }
    }

    // 粘贴失败时回退到先聚焦窗口再试
    if let Some(id) = window_id {
        if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
            let now = debug_timestamp();
            eprintln!("[VOICEINPUT_DEBUG {now}] insert_text paste failed, trying windowfocus {id}");
        }
        let _ = debug_xdotool!(
            "insert_text windowfocus (retry)",
            Command::new("xdotool")
                .args(["windowfocus", "--sync", id])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["windowfocus", "--sync", id]
        );
        thread::sleep(Duration::from_millis(40));

        for shortcut in [
            ["key", "--clearmodifiers", "Shift+Insert"],
            ["key", "--clearmodifiers", "ctrl+v"],
        ] {
            let status = debug_xdotool!(
                "insert_text paste (retry)",
                Command::new("xdotool")
                    .args(shortcut)
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                shortcut
            )?;

            if status.success() {
                return Ok(());
            }
        }
    }

    Err(VoiceInputError::Injection(
        "xdotool 粘贴失败：Shift+Insert 和 ctrl+v 都未成功".to_string(),
    ))
}

#[cfg(feature = "ibus")]
pub fn type_text_in_active_window(text: &str, window_id: Option<&str>) -> Result<()> {
    // 调用方已确保窗口是活动窗口，不要在此处调用 focus_window——
    // xdotool windowfocus --sync 会抢占焦点，导致目标应用光标闪烁或消失。

    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] xdotool type text_len={} win={window_id:?}", text.len());
    }

    let status = debug_xdotool!(
        "xdotool type",
        Command::new("xdotool")
            .args(["type", "--clearmodifiers", "--delay", "0", text])
            .status()
            .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
        &["type", "--clearmodifiers", "--delay", "0", text]
    )?;

    if !status.success() {
        // 如果直接打字失败（例如 Wayland），回退到用 windowfocus 聚焦后再试
        if let Some(id) = window_id {
            let _ = debug_xdotool!(
                "xdotool type windowfocus (retry)",
                Command::new("xdotool")
                    .args(["windowfocus", "--sync", id])
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                &["windowfocus", "--sync", id]
            );
            thread::sleep(Duration::from_millis(40));
        }

        let status = debug_xdotool!(
            "xdotool type (retry)",
            Command::new("xdotool")
                .args(["type", "--clearmodifiers", "--delay", "0", text])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["type", "--clearmodifiers", "--delay", "0", text]
        )?;

        if !status.success() {
            return Err(VoiceInputError::Injection(format!(
                "xdotool 输入失败，退出码：{status}"
            )));
        }
    }

    Ok(())
}

#[cfg(feature = "ibus")]
pub fn backspace_in_active_window(count: usize, window_id: Option<&str>) -> Result<()> {
    // 不预先调用 focus_window——调用方已确保窗口是活动窗口。
    if std::env::var("VOICEINPUT_DEBUG").map_or(false, |v| v == "1") {
        let now = debug_timestamp();
        eprintln!("[VOICEINPUT_DEBUG {now}] backspace count={count} win={window_id:?}");
    }

    for _i in 0..count {
        let status = debug_xdotool!(
            "backspace",
            Command::new("xdotool")
                .args(["key", "--clearmodifiers", "BackSpace"])
                .status()
                .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
            &["key", "--clearmodifiers", "BackSpace"]
        )?;

        if !status.success() {
            // 退格失败（如 Wayland），先聚焦再重试一次
            if let Some(id) = window_id {
                let _ = debug_xdotool!(
                    "backspace windowfocus (retry)",
                    Command::new("xdotool")
                        .args(["windowfocus", "--sync", id])
                        .status()
                        .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                    &["windowfocus", "--sync", id]
                );
                thread::sleep(Duration::from_millis(40));
            }

            let status = debug_xdotool!(
                "backspace (retry)",
                Command::new("xdotool")
                    .args(["key", "--clearmodifiers", "BackSpace"])
                    .status()
                    .map_err(|e| VoiceInputError::Injection(format!("调用 xdotool 失败：{e}"))),
                &["key", "--clearmodifiers", "BackSpace"]
            )?;

            if !status.success() {
                return Err(VoiceInputError::Injection(format!(
                    "xdotool 退格失败，退出码：{status}"
                )));
            }
        }
    }

    Ok(())
}

#[cfg(not(feature = "ibus"))]
pub fn insert_indicator_and_save_clipboard() -> Result<ClipboardRestoreGuard> {
    Ok(ClipboardRestoreGuard { _clipboard: None })
}

#[cfg(not(feature = "ibus"))]
pub fn insert_text_into_active_window(_text: &str, _window_id: Option<&str>) -> Result<()> {
    Err(VoiceInputError::Injection(
        "当前构建未启用 IBus 支持，请使用 --features ibus 重新构建".to_string(),
    ))
}

#[cfg(not(feature = "ibus"))]
#[allow(dead_code)]
pub fn type_text_in_active_window(_text: &str, _window_id: Option<&str>) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "ibus"))]
pub fn backspace_in_active_window(_count: usize, _window_id: Option<&str>) -> Result<()> {
    Ok(())
}

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
