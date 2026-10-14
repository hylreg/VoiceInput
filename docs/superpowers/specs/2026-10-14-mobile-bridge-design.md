# 手机外部输入桥接设计

日期：2026-10-14

## 目标

用手机当 VoiceInput 的输入源：手机浏览器打开本机局域网地址，用手机自带输入法的语音键说话，
确认文字后点「发送」，文字注入本机当前光标处。

不改动现有「双击 Ctrl → 本机麦克风录音 → 注入」链路，两条路径并存，各由一个托盘开关控制。

## 为什么不走手机麦克风

不采集手机麦克风的音频，也不做 HTTPS。语音识别发生在手机键盘里，我们只传文字。
这消掉了 `getUserMedia` 必须 HTTPS、自签证书在手机上难装的整块成本。

代价：转写质量取决于手机输入法，不经过本机 ASR。这是用户明确要的方向。

## 架构

沿用项目现有的「常驻线程 + 共享状态 + 主循环轮询」模式，不引入新依赖（纯 std）：

```
手机浏览器 ──HTTP(局域网)──> 服务线程（TcpListener, 0.0.0.0:8765）
                                  │  SyncSender<BridgeRequest>
                                  ▼
                          主循环（现有 250ms 轮询，try_recv 排空）
                                  │  host.commit_text(text)
                                  ▼
                          LinuxInputMethodHost（xdotool 注入）
```

注入统一由主循环执行，理由是 `LinuxInputMethodHost` 只在主循环使用，集中一处可避免
手机注入与本机录音同时调 `xdotool` 抢焦点。这与现有 `LiveJobState` 单任务闸门同一思路。

## 请求流程

`POST /send` 携带 `{ "text": "..." }`：

1. HTTP 线程校验 token 与开关状态，做文本归一化，然后送进有界队列
2. 主循环每轮 `try_recv` 排空队列，调 `commit_text` 注入
3. 结果通过请求内附的一次性 channel 回票（`mpsc::Sender<Result<(), String>>`）
4. HTTP 线程等回票，超时 2s 返回 503

注入延迟由主循环的 250ms 轮询周期决定，最坏 250ms，可接受。

`GET /api/status` **不走主循环**。它读的两个值都是现成的共享状态，
HTTP 线程直接读：`mic_recording` 取 `LinuxMicAudioRecorder::is_recording()`
（recorder.rs:47，`LinuxMicAudioRecorder` 内部是 `Arc`，可 clone 进服务线程），
`bridge_enabled` 取共享的 `Arc<AtomicBool>`。这样状态查询不会被 30s 的录音周期阻塞。

## 接口

| 方法 | 路径 | token | 说明 |
|---|---|---|---|
| `GET` | `/` | 否 | 返回内嵌页面（空表单，无敏感信息） |
| `POST` | `/send` | 是 | `{"text":"..."}` → 注入 |
| `GET` | `/api/status` | 是 | `{"mic_recording":bool,"bridge_enabled":bool}` |

token 通过 `X-Auth-Token` 请求头传递（页面与接口同源，不触发 CORS 预检）。

token 放在 URL **fragment**（`http://10.10.10.42:8765/#k=<token>`）。fragment 不进浏览器历史、
不发 Referer。本机所在网段 `10.10.10.0/24` 上还有其他设备（实测 `.38/.50/.35/.249`），
query string 会泄漏，fragment 不会。

`GET /` 公开是必须的——页面本身要靠读 fragment 才能拿到 token，两者互锁就死锁了。
页面静态内容不含 token，无泄露风险。

`/api/status` 与 `/send` 的关键差别：**开关关闭时 `/api/status` 仍返回 200**
并如实报告 `bridge_enabled: false`，而不是返回 409。否则页面无法区分「开关关了」和「token 错了」。
409 只用于 `/send`。

### 状态码

| 码 | 场景 |
|---|---|
| 200 | 注入成功 |
| 401 | token 缺失或错误 |
| 409 | 手机输入开关已关闭 |
| 413 | 文本超过 500 字符 |
| 429 | 队列满（本机处理不过来） |
| 503 | 主循环 2s 内未回票（通常是在录音） |

## 文本归一化

注入前：**换行折叠为空格 → 连续空白折叠为单个空格 → 首尾 trim**。

手机 `textarea` 允许换行，`xdotool type` 会把 `\n` 发成回车——在聊天框里等于直接发出消息。
归一化后为空则拒绝，不注入。

## 剪贴板

`insert_text_into_active_window`（crates/voice-input-linux/src/ibus.rs:83）对非 ASCII 文本走
剪贴板粘贴路径。手机路径必须**沿用** `insert_indicator_and_save_clipboard`
（crates/voice-input-linux/src/ibus.rs:48）的保存-还原，否则用户正在复制的内容会被手机文字覆盖。

不插 `●` 指示符——用户在手机上，看不到本机光标处的指示符，插了反而是噪声。

## 托盘菜单

```
状态：已就绪
停止当前录音
───
本机麦克风：已启用        ← 点击切换
手机输入：已启用          ← 点击切换
───
复制外部输入地址
───
重启输入法
重启系统输入法
───
退出
```

开关用**纯文字项**，不用 `CheckmarkItem`。ksni 0.3.5 的 `CheckmarkItem` 会自行维护并翻转
`toggle_state`，若同时依据自己的 bool 渲染，菜单显示与实际状态会不同步。文字项自行渲染，
不可能不同步。

### 开关语义

两个开关独立，默认都开。

- **本机麦克风**关：双击 Ctrl 触发被忽略，每次触发打一行日志
- **手机输入**关：端口继续监听，`POST /send` 返回 409，页面显示「手机输入已关闭」

端口继续监听是刻意的——避免关一次开关就要在手机上重连。

### 持久化时机

内存态切换，**不即时写盘**；在已有的正常退出路径上（托盘退出 / 重启输入法）统一落盘一次。

理由：点一次开关就 fsync 一次 settings，对目录被 inotify 监视的场景会引发连锁反应；
主循环反正每轮读内存变量，写盘对生效无帮助。

副作用：被 SIGKILL 时开关丢失、回落默认（两个都开）。默认是安全方向，可接受。

### 复制地址

点击时**现算**本机 IP，不缓存在 settings 里。

本机网卡是 DHCP 动态（`valid_lft` 会变），IP 会变。settings 只存端口与 token。

URL 组成：`http://<当前IP>:<端口>/#k=<token>`，写进剪贴板。

## 地址与 token

- **token**：首次启动从 `/dev/urandom` 生成 32 位十六进制，存 `~/.config/voice-input/linux-app.toml`
- **token 绝不进仓库**——它是机器级凭据，不属于 `config/voiceinput.env` 那类仓库级默认
- **IP 探测**：`UdpSocket::bind("0.0.0.0:0")` + `connect("192.0.2.1:80")` 读 `local_addr()`。
  UDP 不发包，纯 std，无新依赖
- **端口**：默认 8765（实测空闲），可配 `--bridge-port`，进 settings
- **绑定**：`0.0.0.0`

绑 `0.0.0.0` 顺带覆盖 `tailscale0`（`100.65.179.39`），多一条不依赖同 Wi-Fi 的通路。
**但这同时意味着 tailnet 内其他机器也够得着**（tailnet 里有一台 Mac `air`），
token 是唯一屏障，不可视为装饰。

## 页面

服务端 `include_str!` 嵌入的单个 HTML，无构建步骤、无外部资源。

- `textarea` + 大号「发送」按钮（拇指够得着），viewport 适配，深色配色
- `textarea` autofocus
- **只有按钮发送，回车不触发发送**——手机语音输入会插换行，回车当发送必然误触
- **发送后清空输入框但保持焦点**，手机键盘不收起，可以连着说下一句
- 发送前先拉 `GET /api/status`：本机录音中直接提示「本机正在录音，稍后重试」并保留输入框内容，
  不干等 2s 超时（录音周期最长 30s）
- 发送结果追加到页面下方历史列表（时间戳 + 文字）；**失败条目标红并留在列表里**可重发
- 历史存 `localStorage`，刷新不丢。服务端**不存历史**，页面即全部状态
- token 从 fragment 读出后存 `localStorage`，之后开裸地址也能用
- 收到 401 显示「token 已变更，请从托盘重新复制地址」，并给可粘贴新地址的输入框

## 降级与错误处理

- 端口被占用 / 绑定失败：**不影响主程序启动**，打警告，托盘「手机输入」项置灰
  （对齐现有 `SystemImeController::detect()` 的优雅降级写法）
- 请求体上限 500 字符。`xdotool type --delay 0` 仍逐字符走 X 协议，几百字明显卡顿；
  400-500 字符的手机端语音输入已属少见。超限返回 413 并回显实际长度
- socket 设 5s 读超时，避免半个连接占死线程
- 服务端最多 8 个并发连接；队列为 `sync_channel(8)`，满则 429
- 每连接一个线程，不做 keep-alive，用完即关
- **日志只记长度与结果，不记内容**——手机语音可能包含密码或私事，journal 是落盘的
- 注入目标不确定是固有风险（焦点可能在终端、编辑器或已最小化的窗口），无法彻底解决；
  页面必须明确显示成功/失败，不允许静默成功

## 代码落点

| 文件 | 内容 |
|---|---|
| `crates/voice-input-linux/src/bridge.rs` | 服务线程、config、handle、IP/token 生成 |
| `crates/voice-input-linux/src/bridge_http.rs` | **纯函数**：请求行/头/Content-Length/fragment 解析、响应组装、文本归一化 |
| `crates/voice-input-linux/src/bridge_page.html` | 内嵌页面 |
| `src/tray.rs` | 两个开关项 + 「复制地址」 |
| `src/settings.rs` | 两个开关 + token + 端口 |
| `src/runtime.rs` | 主循环排空队列；`host` 参数由具体类型改为 `&dyn InputMethodHost` 以便单测 |

## 测试策略

沿用项目现有风格（纯函数单测 + 少量集成）：

- `bridge_http`：请求行/头/Content-Length 解析、响应组装、归一化（换行、多空白、空串、超长）全部纯函数单测
- `bridge.rs`：测试内绑 0 端口跑真实 TCP 往返，覆盖 200 / 401 / 409 / 413 / 429
- 主循环排空逻辑：用测试内的假 host 断言注入文本与调用次数
- CLI：`--bridge-port` 解析单测

## 明确不做（YAGNI）

- 不做 HTTPS / 自签证书
- 不在手机上采集麦克风（改用手机键盘语音）
- 不做服务端历史
- 不做配对流程、不做多设备管理
- 不做文件/图片传输
- 不做手机端静音自动发送（手机键盘自带语音结束）
- 不做按 IP 限流——有 token + 有界队列已足够，这是单个可信用户的服务
- 不做二维码（见下节）

## 后续项：二维码

托盘画二维码供手机扫描，比手打 `http://10.10.10.42:8765/#k=<32位十六进制>` 舒适得多。

本次不做，原因：本机既无 `qrencode`，也无 Python `qrcode`；crates.io 可达但需**新增
`qrcode` + `image` 两个依赖**。本仓库刚完成一轮「删约 2000 行、砍 optional 依赖」的瘦身，
加两个依赖换一个 onboarding 便利不划算。

它是纯加法（一个 `url_to_png_bytes` 纯函数 + 一个托盘项），不影响本设计，用着烦再补。

## 部署

服务由 `~/.config/systemd/user/voice-input.service` 拉起（`graphical-session.target`），
`ExecStart=~/.local/bin/voice-input` → 读 `config/voiceinput.env` → 执行
`target/release/voice-input-linux live --backend ibus`。

`--bridge-port` 从 env 或 launcher 传即可，**不需要改 unit 文件**。
