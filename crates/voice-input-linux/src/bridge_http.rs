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
}
