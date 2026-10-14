//! 手机页面。单独放一个模块，让 `include_str!` 的路径短而稳定。

pub const PAGE: &str = include_str!("bridge_page.html");
