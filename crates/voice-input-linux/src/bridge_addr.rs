use std::io::Read;
use std::net::{Ipv4Addr, UdpSocket};

/// 口令允许的最短长度。太短的一串没有意义，挡不住顺手乱试的人。
pub const MIN_PASS_PHRASE_LEN: usize = 8;
/// 口令允许的最长长度。地址要能一眼看完、手打得出来。
pub const MAX_PASS_PHRASE_LEN: usize = 32;

/// 拼口令用的音节表。
///
/// 刻意全是单个 ASCII 音节：拼出来的口令只含 `[a-z]`，不需要百分号编码，
/// 也不会撞上 `/`（路径分隔符）或 `#`/`?`（会被 `strip_target` 截掉）。
const SYLLABLES: [&str; 24] = [
    "lan", "he", "xin", "yun", "fei", "guang", "shan", "shui", "tian", "kong", "feng", "yu",
    "ming", "yue", "song", "bai", "chun", "xia", "qiu", "dong", "nan", "bei", "zhong", "yuan",
];

/// 一个口令最少拼几个音节。三个音节读起来像个词，好记。
const MIN_SYLLABLES: usize = 3;

/// 取 8 个随机字节。
fn random_bytes() -> [u8; 8] {
    let mut bytes = [0u8; 8];
    let filled = std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .is_ok();
    if !filled {
        // 读不到 urandom 时退化为「pid + 时间戳」，仍然能拼出一个词。
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seed = (u128::from(std::process::id()) << 96) ^ nanos;
        bytes.copy_from_slice(&seed.to_le_bytes()[..8]);
    }
    bytes
}

/// 按给定字节拼口令，音节一个一个往上加。
///
/// 至少 [`MIN_SYLLABLES`] 个音节，且至少要够 [`MIN_PASS_PHRASE_LEN`] 个字：
/// 音节表里有 `he`、`yu` 这种两个字的，三个凑一起可能只有 7 个字（如 `lanheyu`），
/// 那就不满足我们自己定的口令下限了。这种值会被 [`normalize_pass_phrase`] 判成非法，
/// 下次启动 `prepare_bridge` 又得换一个，手机上刚存的地址白改。
fn phrase_from_bytes(bytes: &[u8]) -> String {
    let mut phrase = String::new();

    for (index, byte) in bytes.iter().enumerate() {
        if index >= MIN_SYLLABLES && phrase.len() >= MIN_PASS_PHRASE_LEN {
            break;
        }
        phrase.push_str(SYLLABLES[usize::from(*byte) % SYLLABLES.len()]);
    }

    phrase
}

/// 从音节表里随机取几个音节拼成一个词，如 `lanhexin`。
///
/// 这只是「好记的默认值」，不是加密强度的秘密——一共只有约 1.7 万种组合（≈14 比特）。
/// 真正的屏障是「必须先进 tailnet」加上托盘里的「手机输入」开关。
/// 需要更强的口令就用 [`normalize_pass_phrase`] 接受的字符集自己在配置里写一串随机的。
pub fn generate_pass_phrase() -> String {
    phrase_from_bytes(&random_bytes())
}

/// 校验并归一化用户给的口令。不合法返回 `None`。
///
/// 字符集卡死在 ASCII 的 `[A-Za-z0-9_-]`：路径里出现非 ASCII 就得百分号编码，
/// 出现 `/` 会直接把路由切断。卡死字符集就绕开了整类问题，也让「地址能不能手打」
/// 这件事有确定答案。
pub fn normalize_pass_phrase(raw: &str) -> Option<String> {
    let trimmed = raw.trim();

    if trimmed.len() < MIN_PASS_PHRASE_LEN || trimmed.len() > MAX_PASS_PHRASE_LEN {
        return None;
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }

    Some(trimmed.to_ascii_lowercase())
}

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

/// 拼接手机端要打开的地址。口令放路径第一段。
///
/// 末尾那个斜杠不能省：页面用它当相对地址的基准（`/yuyin` 与 `/yuyin/` 解析出来的
/// 基准不同），生成时就带上，页面侧不用再处理这个歧义。
pub fn bridge_url(host: &str, port: u16, pass_phrase: &str) -> String {
    format!("http://{host}:{port}/{pass_phrase}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_puts_pass_phrase_in_path() {
        assert_eq!(
            bridge_url("10.0.0.5", 8765, "lanhexin"),
            "http://10.0.0.5:8765/lanhexin/"
        );
    }

    #[test]
    fn url_keeps_trailing_slash() {
        // 少了它，页面解析出来的相对地址基准会退到根目录，接口就全 401 了。
        assert!(bridge_url("10.0.0.5", 8765, "lanhexin").ends_with('/'));
    }

    #[test]
    fn url_leaves_old_fragment_form_behind() {
        let url = bridge_url("10.0.0.5", 8765, "lanhexin");

        assert!(!url.contains("#"), "实际：{url}");
        assert!(!url.contains("k="), "实际：{url}");
    }

    #[test]
    fn url_works_with_hostname() {
        assert_eq!(
            bridge_url("leju.tail3e3e47.ts.net", 8080, "yuyin"),
            "http://leju.tail3e3e47.ts.net:8080/yuyin/"
        );
    }

    #[test]
    fn generated_pass_phrase_is_ascii_lowercase_token() {
        let pass = generate_pass_phrase();

        assert!(
            normalize_pass_phrase(&pass).is_some(),
            "生成的口令必须自己也认为合法，实际：{pass}"
        );
        assert_eq!(pass, pass.to_ascii_lowercase(), "实际：{pass}");
    }

    #[test]
    fn every_generated_phrase_passes_our_own_validation() {
        // 单抽一次的用例挡不住这个问题：音节表里的 `he`、`yu` 只有两个字，
        // 三个凑起来（如 `lanheyu`）会短于 8 字的下限，而生成器原本只取 3 个音节。
        // 那种口令自己都不合法，下次启动就会被换掉，手机上存的地址白改。
        // 这里把整个音节组合空间跑一遍，而不是碰运气抽一次。
        for a in 0..SYLLABLES.len() {
            for b in 0..SYLLABLES.len() {
                for c in 0..SYLLABLES.len() {
                    // 给第 4 个字节留出位置：前三个不够长时它会被用来补足。
                    let phrase = phrase_from_bytes(&[a as u8, b as u8, c as u8, 7]);

                    assert!(
                        normalize_pass_phrase(&phrase).is_some(),
                        "生成器拼出了自己都不认的口令：{phrase}"
                    );
                    assert!(
                        phrase.len() >= MIN_PASS_PHRASE_LEN,
                        "口令短于下限，实际：{phrase}"
                    );
                }
            }
        }
    }

    #[test]
    fn generator_stops_at_three_syllables_when_long_enough() {
        // 够长就到此为止，别把地址撑成一大串——它是要手打的。
        // `zhong`(5) + `yuan`(4) + `lan`(3) = 12 个字，第 4 个字节不该被用上。
        let phrase = phrase_from_bytes(&[22, 23, 0, 23]);

        assert_eq!(phrase, "zhongyuanlan");
    }

    #[test]
    fn generated_pass_phrases_differ() {
        // 不是严格的随机性检验，只挡住「返回常量」这种退化实现。
        assert_ne!(generate_pass_phrase(), generate_pass_phrase());
    }

    #[test]
    fn normalization_lowercases_and_trims() {
        assert_eq!(
            normalize_pass_phrase("  LanHeXin  ").as_deref(),
            Some("lanhexin")
        );
    }

    #[test]
    fn normalization_accepts_digits_dash_underscore() {
        assert_eq!(
            normalize_pass_phrase("ab-12_CD").as_deref(),
            Some("ab-12_cd")
        );
    }

    #[test]
    fn normalization_rejects_short_phrase() {
        assert_eq!(normalize_pass_phrase("abc"), None);
    }

    #[test]
    fn normalization_rejects_overlong_phrase() {
        let long = "a".repeat(MAX_PASS_PHRASE_LEN + 1);

        assert_eq!(normalize_pass_phrase(&long), None);
    }

    #[test]
    fn normalization_accepts_boundary_lengths() {
        let min = "a".repeat(MIN_PASS_PHRASE_LEN);
        let max = "a".repeat(MAX_PASS_PHRASE_LEN);

        assert!(normalize_pass_phrase(&min).is_some());
        assert!(normalize_pass_phrase(&max).is_some());
    }

    #[test]
    fn normalization_rejects_slash() {
        // 斜杠会把路径切断，路由就找不到前缀了。
        assert_eq!(normalize_pass_phrase("lan/hexin"), None);
    }

    #[test]
    fn normalization_rejects_non_ascii() {
        // 非 ASCII 进路径要百分号编码，浏览器发过来的和盘上存的就对不上。
        assert_eq!(normalize_pass_phrase("兰核心拼音"), None);
    }

    #[test]
    fn normalization_rejects_spaces_inside() {
        assert_eq!(normalize_pass_phrase("lan hexin"), None);
    }

    #[test]
    fn normalization_rejects_empty() {
        assert_eq!(normalize_pass_phrase(""), None);
        assert_eq!(normalize_pass_phrase("   "), None);
    }

    #[test]
    fn normalization_accepts_legacy_hex_token() {
        // 旧版盘上存的是 32 位十六进制。它是合法字符集，升级后必须能直接用，
        // 否则每台机器升一次级就换一次口令，手机上存的地址全废。
        let legacy = "3f9a2c8e1b7d4a6f0e5c9b3d8a1f7e2c";

        assert_eq!(normalize_pass_phrase(legacy).as_deref(), Some(legacy));
    }
}
