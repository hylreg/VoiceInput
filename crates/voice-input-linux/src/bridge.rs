use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// 并发连接上限。监听绑在 0.0.0.0，局域网/tailnet 都能连，任何人都能建连接；
/// 超过这个数的连接直接 429，不排队也不占线程，避免半开连接把线程数堆到失控。
const MAX_CONNECTIONS: usize = 8;

/// accept 连续报错时的退避时长。EMFILE/ENFILE（fd 耗尽）或网卡掉线时
/// accept 会持续失败，不退避就是空转把一整个核打满。
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(50);

/// 一次待注入的请求，由服务线程送给主循环。
pub struct BridgeRequest {
    pub text: String,
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
