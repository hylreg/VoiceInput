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
