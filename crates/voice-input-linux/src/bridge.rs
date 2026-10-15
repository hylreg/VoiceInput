use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::bridge_http::{
    char_count, json_field_text, normalize_text, parse_head, route, split_prefix, tokens_match,
    HttpResponse, Route, MAX_BODY_BYTES, MAX_TEXT_CHARS, MSG_BRIDGE_DISABLED, MSG_BUSY, MSG_EMPTY,
    MSG_INJECT_FAILED, MSG_TIMEOUT, MSG_TOO_LONG, MSG_UNAUTHORIZED,
};
use crate::recorder::LinuxMicAudioRecorder;
use voice_input_core::{Result, VoiceInputError};

/// 单个连接的读写超时，避免半个连接占死线程。
const SOCKET_TIMEOUT: Duration = Duration::from_secs(5);

/// 读取整条请求头的总时限。每次 `read_line` 都会重置 socket 读超时，
/// 所以只看 `SOCKET_TIMEOUT` 的话，对端每 <5s 滴一个字节就能把 worker
/// 永久占住；而 `MAX_CONNECTIONS` 的判定排在鉴权之前，8 条这样的连接
/// 就能让所有正当请求一直吃 429。监听绑在 0.0.0.0（tailnet 可达），
/// 所以再加一道整条请求的截止时间。
const HEAD_READ_DEADLINE: Duration = Duration::from_secs(5);

/// 等待主循环回票的超时。主循环每 250ms 排空一次队列，2s 余量充足。
const ACK_TIMEOUT: Duration = Duration::from_secs(2);

/// 队列容量。满了直接回 429，不无限堆积。
pub const QUEUE_CAPACITY: usize = 8;

/// 并发连接上限。监听绑在 0.0.0.0，局域网/tailnet 都能连，任何人都能建连接；
/// 超过这个数的连接直接 429，不排队也不占线程，避免半开连接把线程数堆到失控。
const MAX_CONNECTIONS: usize = 8;

/// accept 连续报错时的退避时长。EMFILE/ENFILE（fd 耗尽）或网卡掉线时
/// accept 会持续失败，不退避就是空转把一整个核打满。
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(50);

/// 一次待注入的请求，由服务线程送给主循环。
pub struct BridgeRequest {
    pub text: String,
    /// 这条请求最迟可被注入的时刻。主循环在录音里可能一卡就是 30s，
    /// 期间入队的请求，其 HTTP 客户端早已收到 503 并让用户重发——
    /// 过期后仍注入就会与用户重发的那条重复。
    pub deadline: Instant,
    /// 主循环处理后用它回票；错误串直接展示给用户。
    pub reply: SyncSender<std::result::Result<(), String>>,
}

/// 桥接运行时状态。`bridge_enabled` 由托盘切换；`mic_recording` 直接问录音器，
/// 不另设一份标志位——两份状态必然漂移。
pub struct BridgeStatus {
    pub bridge_enabled: Arc<AtomicBool>,
    recorder: LinuxMicAudioRecorder,
    /// 访问 token。放在 `Mutex` 里是因为服务线程每个请求都要比对，
    /// 而托盘「复制地址」也需要读。
    token: std::sync::Mutex<String>,
}

// 手写 Debug 而不 derive：token 是桥接监听器（0.0.0.0，局域网/tailnet 都能连）
// 唯一的门禁，而日志最终会落到磁盘上的 journal，规范要求日志绝不记录敏感内容，
// 所以这里只打印长度，绝不回显 token 明文。与 settings.rs 的 LinuxAppSettings 同一取舍。
impl std::fmt::Debug for BridgeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BridgeStatus")
            .field(
                "bridge_enabled",
                &self.bridge_enabled.load(Ordering::SeqCst),
            )
            .field("mic_recording", &self.mic_recording())
            .field(
                "bridge_token",
                &format_args!(
                    "<redacted; {} chars>",
                    self.token.lock().map(|guard| guard.len()).unwrap_or(0)
                ),
            )
            .field("recorder", &self.recorder)
            .finish()
    }
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
        // 这里连 0.0.0.0:<port> 能否唤醒 accept 是平台相关的赌注（Linux 把它当 loopback 处理）；
        // 万一该连接失败而 accept 线程仍阻塞，下面的 join 会卡住守护进程退出。
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
    let active = Arc::new(AtomicUsize::new(0));

    let thread = thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown_for_thread.load(Ordering::SeqCst) {
                break;
            }
            let Ok(stream) = stream else {
                // accept 持续报错（EMFILE/ENFILE fd 耗尽，或网卡掉线）时不能空转，
                // 否则一整个核会被打满。退避后再试。
                thread::sleep(ACCEPT_ERROR_BACKOFF);
                continue;
            };
            // fetch_add 先占位再回退，避免「先读后加」的竞态导致超限。
            let claimed = active.fetch_add(1, Ordering::SeqCst);
            if claimed >= MAX_CONNECTIONS {
                active.fetch_sub(1, Ordering::SeqCst);
                // 超限时直接回 429，既不排队也不占用线程。
                // 用 429 而非 503：429 就是「请求太多，稍后重试」，正是超容量的语义，
                // 也与队列满时返回的状态一致（同一件事对用户是同一个说法）。
                let _ = write_over_limit_response(stream);
                continue;
            }
            let status = Arc::clone(&status);
            let sender = sender.clone();
            let active_for_thread = Arc::clone(&active);
            // 唤醒用的空连接会立刻返回，无需处理。
            match thread::Builder::new().spawn(move || {
                let _ = handle_connection(stream, &status, &sender);
                active_for_thread.fetch_sub(1, Ordering::SeqCst);
            }) {
                Ok(_) => {}
                Err(err) => {
                    // 线程创建失败（负载高时 EAGAIN）时槽位必须归还，否则会永久少一个连接额度。
                    // 这里只记错误，不记请求内容——规范禁止记录注入内容，而且此刻还没读到任何请求。
                    active.fetch_sub(1, Ordering::SeqCst);
                    eprintln!("桥接：处理线程创建失败，已丢弃该连接：{err}");
                }
            }
        }
    });

    Ok(BridgeHandle {
        addr,
        shutdown,
        thread: Some(thread),
    })
}

/// 连接数超限时回一条 429。给流设个写超时，避免对端不读导致写阻塞把 accept 线程拖住。
fn write_over_limit_response(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    stream.write_all(HttpResponse::error(429, MSG_BUSY).render().as_bytes())?;
    stream.flush()
}

fn handle_connection(
    mut stream: TcpStream,
    status: &BridgeStatus,
    sender: &SyncSender<BridgeRequest>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;

    let mut reader = BufReader::new(stream.try_clone()?);
    let head = match read_head(&mut reader, Instant::now() + HEAD_READ_DEADLINE)? {
        HeadRead::TimedOut => return write_head_timeout_response(stream),
        HeadRead::Complete(head) => head,
    };

    // 连上就断（唤醒 accept 用的空连接）或对端关闭：没什么可回的。
    if head.is_empty() {
        return Ok(());
    }

    let response = build_response(&head, &mut reader, status, sender);
    stream.write_all(response.render().as_bytes())?;
    stream.flush()
}

/// 请求头读取结果。
enum HeadRead {
    /// 读到了完整的头（含终止空行；对端提前关闭时是已读到的部分）。
    Complete(String),
    /// 到截止时间还没读完。
    TimedOut,
}

/// 读一条请求的头（到空行为止），整条读取受 `deadline` 约束。
///
/// 这里刻意不用 `read_line`：它内部会反复重试底层 read，而每次 read 都会
/// 拿到一个全新的 socket 读超时窗口，对端每 <5s 滴一个字节就能把它永久挂住。
/// 改成 `fill_buf`/`consume` 每次只取一个字节，外层每取一字节就能查一次
/// 截止时间，整条请求因此有了真正的上界。
fn read_head(reader: &mut impl BufRead, deadline: Instant) -> std::io::Result<HeadRead> {
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        if Instant::now() >= deadline {
            return Ok(HeadRead::TimedOut);
        }

        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            // 对端关闭。已读到的部分照常交出去走解析失败路径。
            return Ok(HeadRead::Complete(
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
        bytes.push(chunk[0]);
        reader.consume(1);

        if ends_with_blank_line(&bytes) {
            return Ok(HeadRead::Complete(
                String::from_utf8_lossy(&bytes).into_owned(),
            ));
        }
    }
}

/// 判断累积的头字节是否已经以「空行」结束。
fn ends_with_blank_line(bytes: &[u8]) -> bool {
    bytes == b"\r\n" || bytes == b"\n" || bytes.ends_with(b"\r\n\r\n") || bytes.ends_with(b"\n\n")
}

/// 请求头没能在 `HEAD_READ_DEADLINE` 内读完时的回应。
///
/// 复用与畸形请求同一条路径的 400：`bridge_http::reason_phrase` 没有 408 的
/// 原因短语，凭空加一个状态码只会让页面拿到「Unknown」。
fn write_head_timeout_response(mut stream: TcpStream) -> std::io::Result<()> {
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    stream.write_all(
        HttpResponse::error(400, "请求头读取超时")
            .render()
            .as_bytes(),
    )?;
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

    // 口令是路径第一段，所以鉴权在这里做一次，下面的路由只看到剩下的部分。
    // 先鉴权后路由是刻意的：这样「口令不对」与「路径不存在」不会互相暴露——
    // 否则攻击者能靠「404 还是 401」把口令的正确前缀一段段试出来。
    // 代价是裸 `/` 也回 401（它的口令段是空串），页面拿不到任何提示，
    // 但裸 `/` 本来就不是给人看的地址。
    //
    // 大小写不敏感：盘上的口令统一存小写（`normalize_pass_phrase` 归一化过），
    // 而手机地址栏、微信、各种输入法都可能把它首字母大写。逐字比对会让
    // 「首字母大写了」这种地址直接 401 —— 用户只看到一句「口令已变更」，
    // 完全不知道该把哪个字母改回小写。降维成小写比对不会削弱什么：
    // 口令的字母表本来就只有 `[a-z0-9_-]`，少几个大小写变体，熵的差别可以忽略。
    let (prefix, rest) = split_prefix(&parsed.path);
    if !tokens_match(&status.token(), &prefix.to_ascii_lowercase()) {
        return HttpResponse::error(401, MSG_UNAUTHORIZED);
    }

    match route(&parsed.method, rest) {
        Route::NotFound => HttpResponse::error(404, "未知路径"),
        Route::Page => HttpResponse::html(crate::bridge_page::PAGE.to_string()),
        Route::Status => {
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
        // 与下面的 recv_timeout 同一个时限：超过这个点客户端已经收到 503，
        // 主循环再注入就会和用户重发的那条重复。
        deadline: Instant::now() + ACK_TIMEOUT,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个每次只按脚本吐字节的读取端，模拟点滴式发送。
    struct DripReader {
        chunk: Vec<u8>,
        consumed: bool,
    }

    impl std::io::Read for DripReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            // 每次只给一个字节，逼调用方反复 fill_buf。
            if self.consumed || buf.is_empty() {
                return Ok(0);
            }
            buf[0] = self.chunk.remove(0);
            self.consumed = self.chunk.is_empty();
            Ok(1)
        }
    }

    #[test]
    fn reads_head_until_blank_line() {
        let mut reader =
            std::io::BufReader::new(&b"GET / HTTP/1.1\r\nHost: x\r\n\r\nbody-ignored"[..]);

        let head = match read_head(&mut reader, Instant::now() + Duration::from_secs(5)) {
            Ok(HeadRead::Complete(head)) => head,
            _ => panic!("应当在截止时间前读完"),
        };

        assert_eq!(head, "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
        // 空行之后的请求体没有被吞掉，后续 read_exact 仍能读到。
        let mut body = String::new();
        use std::io::Read as _;
        reader.read_to_string(&mut body).unwrap();
        assert_eq!(body, "body-ignored");
    }

    #[test]
    fn read_head_times_out_when_deadline_already_passed() {
        // 截止时间已过时必须立刻返回 TimedOut，不能再去读。
        let mut reader = std::io::BufReader::new(&b"GET / HTTP/1.1\r\n"[..]);

        let result = read_head(&mut reader, Instant::now() - Duration::from_secs(1));

        assert!(matches!(result, Ok(HeadRead::TimedOut)));
    }

    #[test]
    fn read_head_reports_eof_partial_head() {
        // 对端发一半就关闭：把读到的部分交出去，走解析失败路径。
        let mut reader = std::io::BufReader::new(&b"GET / HT"[..]);

        let head = match read_head(&mut reader, Instant::now() + Duration::from_secs(5)) {
            Ok(HeadRead::Complete(head)) => head,
            _ => panic!("对端关闭应当收尾，而不是超时"),
        };

        assert_eq!(head, "GET / HT");
    }

    #[test]
    fn read_head_handles_bare_newline_terminator() {
        let mut reader = std::io::BufReader::new(&b"GET / HTTP/1.1\n\n"[..]);

        let head = match read_head(&mut reader, Instant::now() + Duration::from_secs(5)) {
            Ok(HeadRead::Complete(head)) => head,
            _ => panic!("裸 LF 空行也应当终止读取"),
        };

        assert_eq!(head, "GET / HTTP/1.1\n\n");
    }

    #[test]
    fn read_head_accepts_drip_reader() {
        // 每次只给一个字节的读取端，逻辑上应当正常工作（真实场景里
        // 由 deadline 兜住无限点滴；这里给足时间读完整条）。
        let drip = b"POST /send HTTP/1.1\r\n\r\n".to_vec();
        let mut reader = BufReader::new(DripReader {
            chunk: drip,
            consumed: false,
        });

        let head = match read_head(&mut reader, Instant::now() + Duration::from_secs(5)) {
            Ok(HeadRead::Complete(head)) => head,
            _ => panic!("应当读完"),
        };

        assert_eq!(head, "POST /send HTTP/1.1\r\n\r\n");
    }
}
