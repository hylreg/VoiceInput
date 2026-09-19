# 托盘菜单新增「重启输入法」设计

日期：2026-09-19

## 目标

在 VoiceInput 常驻托盘的右键菜单里增加一个「重启输入法」项，一键重启 VoiceInput 常驻服务，
用于热键失灵、ASR worker 卡死、双击 Ctrl 没反应等场合。不涉及系统输入法（ibus-daemon）。

## 现状

- 托盘菜单在 `crates/voice-input-linux/src/tray.rs`，现有三项：状态（置灰）、停止当前录音、退出
- 当前实例由 `~/.config/systemd/user/voice-input.service` 拉起，`ExecStart=~/.local/bin/voice-input`
  → `target/release/voice-input-linux live --backend ibus`
- 服务单元已有 `Restart=on-failure`、`RestartSec=5`
- 进程内已有单实例锁 `SingleInstanceGuard`（`$TMPDIR/voiceinput-linux.lock`），保证同时只有一个常驻实例

## 菜单变更

菜单顺序：状态 / 停止当前录音 / ─── / **重启输入法** / 退出。

标签沿用用户原话「重启输入法」。注意它重启的是 VoiceInput 自己，不是系统输入法；
托盘 tooltip 与菜单项文案都不额外解释这一点，避免菜单过长。

## 行为

点击「重启输入法」：

1. 发起 `systemctl --user restart voice-input.service`
2. 设置 `restart_requested` 标志，主循环下一轮（≤250ms）走正常退出路径：
   `watcher.stop()` → `tray.shutdown()` → `run_live_app` 返回 `Ok(())` → 进程 exit 0
3. systemd 完成 stop 后拉起新实例

「正常退出路径」意味着释放 D-Bus 名、释放单实例锁、关闭托盘，而不是被 SIGTERM 打断。

## 关键约束：不能把自己杀在半路

直接在进程里调 `systemctl --user restart voice-input.service` 会自杀：restart 对 unit cgroup
发 SIGTERM，而 `Command::spawn` 出的子进程同样落在这个 cgroup 里，可能在真正发出 restart 请求前就被杀。

解决办法：用 `systemd-run` 把 helper 放进独立的 transient unit。

```
systemd-run --user --collect --unit=voice-input-restart-<pid>-<secs> \
  systemctl --user restart voice-input.service
```

`systemd-run` 把命令入队到 systemd 后立即返回，父进程可以同步 `status()` 拿到成败，
不会把自己挂住，也不会被 restart 的 SIGTERM 波及。

`--collect` 保证 helper unit 结束后被自动回收，不残留 failed unit。
helper unit 名带 pid 与秒级时间戳，避免连点两次时名字冲突。

## 非 systemd 启动时的降级

从 `/proc/self/cgroup` 检测当前进程是否真的跑在 `voice-input.service` 这个 unit 里。

- 检测到：菜单项可用，标签「重启输入法」
- 检测不到（例如直接 `cargo run -p voice-input-linux -- live`）：菜单项置灰，
  标签「重启输入法（需 systemd 服务）」，点击无效果

置灰风格与现有的状态项一致，不弹窗、不报错。

录音进行中时该项也置灰——重启会丢掉正在录的这段。

## 代码落点

### 新增 `crates/voice-input-linux/src/restart.rs`

单一职责，独立可测，不依赖托盘或运行时：

```rust
pub const SERVICE_UNIT: &str = "voice-input.service";

/// 判断 cgroup 文件内容里，当前进程是否属于指定 unit。
/// 同时兼容 cgroup v1（`1:name=systemd:/path/to/unit`）与 v2（`/path/to/unit`）。
pub fn cgroup_matches_unit(cgroup_content: &str, unit: &str) -> bool;

/// 构造 systemd-run helper 的完整参数列表（纯函数，便于断言）。
pub fn restart_command(unit: &str, helper_unit: &str) -> Vec<String>;

pub struct RestartController {
    unit: Option<String>,
}

impl RestartController {
    /// 读取 /proc/self/cgroup 做检测
    pub fn detect() -> Self;
    /// 从给定内容构造，便于单测
    pub fn from_cgroup_content(content: &str, unit: &str) -> Self;
    pub fn is_available(&self) -> bool;
    /// 不可用时的置灰标签后缀
    pub fn unavailable_label(&self) -> &'static str;
    pub fn request(&self) -> Result<(), String>;
}
```

匹配规则：取每行最后一个 `:` 之后的部分作为路径（v2 行没有 `:`，整行即路径），
再判断路径是否等于 `unit` 或以 `/unit` 结尾。前导 `/` 是必需的，
否则 `my-voice-input.service` 会被误匹配。

### 修改 `crates/voice-input-linux/src/tray.rs`

- `LinuxTrayConfig` 增加 `restart: RestartController` 字段
- `LinuxTray` 增加对应字段与 `restart_requested: Arc<AtomicBool>`
- `LinuxTrayHandle` 增加 `is_restart_requested()`，与现有 `is_quit_requested()` 对称
- `menu()` 在分隔符之后、退出之前插入重启项：

```rust
StandardItem {
    label: if self.restart.is_available() {
        "重启输入法".to_string()
    } else {
        format!("重启输入法{}", self.restart.unavailable_label())
    },
    enabled: self.restart.is_available() && !self.recording,
    icon_name: "view-refresh".to_string(),
    activate: Box::new(|tray: &mut LinuxTray| match tray.restart.request() {
        Ok(()) => tray.restart_requested.store(true, Ordering::SeqCst),
        Err(err) => eprintln!("重启输入法失败：{err}"),
    }),
    ..Default::default()
}
```

托盘层只做「发起 + 置标志」，不写业务逻辑。

### 修改 `crates/voice-input-linux/src/runtime.rs`

- 构造 `RestartController::detect()`，连同新的 `restart_requested` 一起传给 `LinuxTrayConfig`
- 主循环退出条件加上 `restart_requested`

### 修改 `crates/voice-input-linux/src/lib.rs`

导出 `restart` 模块的公开项。

## 测试策略

`cargo test -p voice-input-linux` 覆盖 `restart.rs` 的纯函数：

- cgroup v2 格式命中
- cgroup v1 `1:name=systemd:` 格式命中
- 不同 unit 不命中
- `my-voice-input.service` 不被误判为命中
- 空内容不命中
- `restart_command` 参数列表与顺序正确
- `from_cgroup_content` 命中时 `is_available()` 为真、未命中时为假

真机验证：点一次菜单项，`journalctl --user -u voice-input.service -f` 应看到旧实例退出、
新实例完成 ASR 预加载；`systemctl --user status voice-input.service` 的 MainPID 应变化。

## 不做的事

- 不重启 ibus-daemon 或任何系统输入法进程
- 不支持手动 `cargo run` 启动场景的重启（该场景下菜单项直接置灰）
- 不改 systemd unit 文件、不改安装脚本、不改 CLI 参数
