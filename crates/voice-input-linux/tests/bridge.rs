use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use voice_input_linux::{
    spawn_bridge, BridgeConfig, BridgeRequest, BridgeStatus, LinuxMicAudioRecorder,
};

/// 口令，同时也是 URL 路径的第一段。
const PASS: &str = "lanhexin";

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
        PASS.to_string(),
        LinuxMicAudioRecorder::new(Duration::from_secs(30)),
    ));
    let (sender, receiver) = mpsc::sync_channel(8);
    let handle =
        spawn_bridge(BridgeConfig { port: 0 }, status, sender).expect("桥接服务应当启动成功");

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

/// 用给定口令前缀发一次 `/send`。口令是路径第一段，不再有任何鉴权头。
fn send_request(addr: &str, pass: &str, body: &str) -> String {
    raw_request(
        addr,
        &format!(
            "POST /{pass}/send HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

#[test]
fn page_is_served_under_the_pass_phrase() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(
        addr.as_str(),
        &format!("GET /{PASS}/ HTTP/1.1\r\nHost: x\r\n\r\n"),
    );

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "实际：{response}"
    );
    // 只看响应头：页面脚本里本身就有 Content-Type: application/json 的字样，
    // 断言整个响应不含该串会误判。
    let (head, _body) = response.split_once("\r\n\r\n").expect("应当有头体分隔");
    assert!(
        head.contains("Content-Type: text/html") && !head.contains("application/json"),
        "页面不应该是 JSON：{response}"
    );
}

#[test]
fn page_is_served_without_trailing_slash() {
    // 用户手打地址时不会带尾斜杠。省掉它页面就成 404 了，体验上很莫名。
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(
        addr.as_str(),
        &format!("GET /{PASS} HTTP/1.1\r\nHost: x\r\n\r\n"),
    );

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "实际：{response}"
    );
}

#[test]
fn bare_root_is_unauthorized() {
    // 裸 `/` 的口令段是空串。它既不是合法口令，页面也没法从中知道该打哪儿去，
    // 所以跟口令错一样回 401。
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(addr.as_str(), "GET / HTTP/1.1\r\nHost: x\r\n\r\n");

    assert!(response.starts_with("HTTP/1.1 401 "), "实际：{response}");
}

#[test]
fn wrong_pass_phrase_is_unauthorized() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(addr.as_str(), "GET /nope/ HTTP/1.1\r\nHost: x\r\n\r\n");

    assert!(response.starts_with("HTTP/1.1 401 "), "实际：{response}");
}

#[test]
fn wrong_pass_phrase_does_not_leak_path_existence() {
    // 先鉴权后路由：口令不对时，路径对不对都是 401，攻击者没法靠
    // 「404 还是 401」把口令一段段试出来。
    let (_handle, addr, _rx) = test_bridge(true);

    let real_path = raw_request(addr.as_str(), "GET /nope/send HTTP/1.1\r\nHost: x\r\n\r\n");
    let fake_path = raw_request(
        addr.as_str(),
        "GET /nope/whatever HTTP/1.1\r\nHost: x\r\n\r\n",
    );

    assert!(real_path.starts_with("HTTP/1.1 401 "), "实际：{real_path}");
    assert!(fake_path.starts_with("HTTP/1.1 401 "), "实际：{fake_path}");
}

#[test]
fn send_with_valid_pass_phrase_is_queued_and_acked() {
    let (_handle, addr, rx) = test_bridge(true);

    // 主循环的替身：收下请求并回票成功。
    let worker = std::thread::spawn(move || {
        let request = rx.recv().expect("应当收到桥接请求");
        assert_eq!(request.text, "你好世界");
        request.reply.send(Ok(())).expect("回票应当成功");
    });

    let response = send_request(&addr, PASS, r#"{"text":"你好世界"}"#);

    worker.join().expect("worker 线程应当正常结束");
    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "实际：{response}"
    );
    assert!(response.ends_with(r#"{"ok":true}"#), "实际：{response}");
}

#[test]
fn send_with_wrong_pass_phrase_is_unauthorized() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = send_request(&addr, "wrongpass", r#"{"text":"x"}"#);

    assert!(response.starts_with("HTTP/1.1 401 "), "实际：{response}");
}

#[test]
fn send_when_disabled_is_conflict() {
    let (_handle, addr, _rx) = test_bridge(false);

    let response = send_request(&addr, PASS, r#"{"text":"x"}"#);

    assert!(response.starts_with("HTTP/1.1 409 "), "实际：{response}");
}

#[test]
fn send_too_long_is_payload_too_large() {
    let (_handle, addr, _rx) = test_bridge(true);
    let long = "字".repeat(501);
    let body = format!(r#"{{"text":"{long}"}}"#);

    let response = send_request(&addr, PASS, &body);

    assert!(response.starts_with("HTTP/1.1 413 "), "实际：{response}");
}

#[test]
fn send_blank_text_is_bad_request() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = send_request(&addr, PASS, r#"{"text":"   \n  "}"#);

    assert!(response.starts_with("HTTP/1.1 400 "), "实际：{response}");
}

#[test]
fn too_many_concurrent_requests_is_too_many_requests() {
    let (_handle, addr, _rx) = test_bridge(true);
    // 不排空队列，并发发 9 个请求。现在触发 429 的是「并发连接上限」（8 个）：
    // 第 9 个连接在 accept 时就被直接回 429。队列满那条分支（TrySendError::Full）
    // 在此上限下已不可达，仅是纵深防御——≤8 个并发连接、每连接一个请求
    // （Connection: close），队列的 8 格不可能被填满。
    // 每个被接受请求会阻塞等待回票直到 2s 超时，所以并发发出后统一收结果。
    let mut threads = Vec::new();
    for _ in 0..9 {
        let addr = addr.clone();
        threads.push(std::thread::spawn(move || {
            send_request(&addr, PASS, r#"{"text":"x"}"#)
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

    let response = send_request(&addr, PASS, r#"{"text":"x"}"#);

    assert!(response.starts_with("HTTP/1.1 503 "), "实际：{response}");
}

#[test]
fn status_reports_mic_and_bridge_state() {
    let (_handle, addr, _rx) = test_bridge(false);

    let response = raw_request(
        addr.as_str(),
        &format!("GET /{PASS}/api/status HTTP/1.1\r\nHost: x\r\n\r\n"),
    );

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "实际：{response}"
    );
    assert!(
        response.contains(r#""bridge_enabled":false"#),
        "实际：{response}"
    );
    assert!(
        response.contains(r#""mic_recording":false"#),
        "实际：{response}"
    );
}

#[test]
fn status_with_wrong_pass_phrase_is_unauthorized() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(
        addr.as_str(),
        "GET /nope/api/status HTTP/1.1\r\nHost: x\r\n\r\n",
    );

    assert!(response.starts_with("HTTP/1.1 401 "), "实际：{response}");
}

#[test]
fn unknown_path_under_right_pass_phrase_is_not_found() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(
        addr.as_str(),
        &format!("GET /{PASS}/nope HTTP/1.1\r\nHost: x\r\n\r\n"),
    );

    assert!(response.starts_with("HTTP/1.1 404 "), "实际：{response}");
}

#[test]
fn response_always_closes_connection() {
    let (_handle, addr, _rx) = test_bridge(true);

    let response = raw_request(
        addr.as_str(),
        &format!("GET /{PASS}/ HTTP/1.1\r\nHost: x\r\n\r\n"),
    );

    assert!(
        response.contains("Connection: close\r\n"),
        "实际：{response}"
    );
}

#[test]
fn dripping_header_is_cut_off_by_aggregate_deadline() {
    let (_handle, addr, _rx) = test_bridge(true);

    let mut stream = TcpStream::connect(addr.as_str()).expect("应当能连上桥接服务");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("设置读超时");
    // 每 200ms 滴一个字节，单次 read 都远在 5s socket 读超时之内。
    // 若只有 per-read 超时，这样发完 "GET / HTTP/1.1\r\nHost: x\r\nkey: "（26 字节）
    // 要 5s 以上；整条请求的 5s 截止时间必须先把连接掐掉。
    let drip = b"GET / HTTP/1.1\r\nHost: x\r\nkey: ";
    let started = std::time::Instant::now();
    let mut writer = stream.try_clone().expect("克隆流");
    let drip_thread = std::thread::spawn(move || {
        for byte in drip {
            if writer.write_all(&[*byte]).is_err() {
                // 服务端已经关掉连接，正常。
                return;
            }
            let _ = writer.flush();
            std::thread::sleep(Duration::from_millis(200));
        }
    });

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    let elapsed = started.elapsed();
    drip_thread.join().expect("发送线程应当正常结束");

    assert!(
        response.starts_with("HTTP/1.1 400 "),
        "应当收到读取超时错误响应，实际：{response}"
    );
    assert!(
        elapsed < Duration::from_secs(8),
        "整条请求的截止时间应当先于点滴发送完成触发，实际耗时 {elapsed:?}"
    );
}
