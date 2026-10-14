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
