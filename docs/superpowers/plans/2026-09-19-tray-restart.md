# 托盘菜单新增「重启输入法」实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 VoiceInput 常驻托盘右键菜单里增加「重启输入法」项，点击后通过 `systemctl --user restart voice-input.service` 重启常驻服务，当前进程走正常退出路径。

**Architecture:** 新增单一职责模块 `restart.rs`，包含纯函数（cgroup 解析、命令构造）与 `RestartController`（检测 + 发起）。托盘层只做「发起 + 置标志」，主循环看到标志后走正常退出路径。helper 命令用 `systemd-run` 放进独立 transient unit，避免 restart 的 SIGTERM 把 helper 一起杀掉。

**Tech Stack:** Rust 2021、ksni 0.3（托盘）、systemd 用户服务、`/proc/self/cgroup` 检测

**设计文档：** [docs/superpowers/specs/2026-09-19-tray-restart-design.md](../specs/2026-09-19-tray-restart-design.md)

---

## 文件结构

| 文件 | 动作 | 职责 |
|---|---|---|
| `crates/voice-input-linux/src/restart.rs` | 新建 | cgroup 检测纯函数、`systemd-run` 命令构造、`RestartController` |
| `crates/voice-input-linux/src/lib.rs` | 修改 | 声明并导出 `restart` 模块 |
| `crates/voice-input-linux/src/tray.rs` | 修改 | 配置/托盘增加 `restart` 与 `restart_requested` 字段，菜单插入重启项，handle 增加 `is_restart_requested()` |
| `crates/voice-input-linux/src/runtime.rs` | 修改 | 构造 `RestartController::detect()` 与 `restart_requested`，主循环退出条件加上它 |

`restart.rs` 不依赖 `tray` 或 `runtime`，可独立测试。

---

## Task 1: cgroup 检测纯函数

**Files:**
- Create: `crates/voice-input-linux/src/restart.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Test: `crates/voice-input-linux/src/restart.rs`（模块内 `#[cfg(test)]`）

- [ ] **Step 1: 在 lib.rs 声明模块**

修改 `crates/voice-input-linux/src/lib.rs`，在 `mod recorder;` 之后按字母序插入 `mod restart;`：

```rust
mod host;
mod hotkey;
mod ibus;
mod live;
mod live_cli;
mod recorder;
mod restart;
mod runtime;
mod settings;
mod smoke;
mod tray;
```

同时在 `pub use recorder::{FileAudioRecorder, LinuxMicAudioRecorder};` 之后插入导出（本任务只导出已存在的前两项，后两项在 Task 2 / Task 3 补齐）：

```rust
pub use restart::{cgroup_matches_unit, SERVICE_UNIT};
```

- [ ] **Step 2: 写失败的测试**

创建 `crates/voice-input-linux/src/restart.rs`，只写常量、函数签名（`todo!()` 占位）与测试：

```rust
/// VoiceInput 常驻服务的 systemd 用户 unit 名。
pub const SERVICE_UNIT: &str = "voice-input.service";

/// 判断 cgroup 文件内容里，当前进程是否属于指定 unit。
///
/// 兼容两种格式：
/// - cgroup v1：`1:name=systemd:/user.slice/.../voice-input.service`
/// - cgroup v2：`0::/user.slice/.../voice-input.service`
pub fn cgroup_matches_unit(cgroup_content: &str, unit: &str) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const V2_MATCH: &str = "\
13:freezer:/
11:devices:/user.slice
1:name=systemd:/user.slice/user-1000.slice/user@1000.service/voice-input.service
0::/user.slice/user-1000.slice/user@1000.service/voice-input.service
";

    #[test]
    fn cgroup_v2_path_matches_unit() {
        let content = "0::/user.slice/user-1000.slice/user@1000.service/voice-input.service\n";
        assert!(cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn cgroup_v1_name_systemd_path_matches_unit() {
        let content =
            "1:name=systemd:/user.slice/user-1000.slice/user@1000.service/voice-input.service\n";
        assert!(cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn mixed_v1_v2_content_matches_unit() {
        assert!(cgroup_matches_unit(V2_MATCH, "voice-input.service"));
    }

    #[test]
    fn different_unit_does_not_match() {
        let content = "0::/user.slice/user-1000.slice/user@1000.service/app-org.chromium.Chromium-6815.scope\n";
        assert!(!cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn suffix_similar_unit_is_not_a_match() {
        let content = "0::/user.slice/user-1000.slice/user@1000.service/my-voice-input.service\n";
        assert!(!cgroup_matches_unit(content, "voice-input.service"));
    }

    #[test]
    fn empty_content_does_not_match() {
        assert!(!cgroup_matches_unit("", "voice-input.service"));
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p voice-input-linux restart:: --features ibus`
Expected: 6 个测试全部 FAIL，panic 信息为 `not yet implemented`

- [ ] **Step 4: 写最小实现**

把 `restart.rs` 里的 `todo!()` 替换为：

```rust
pub fn cgroup_matches_unit(cgroup_content: &str, unit: &str) -> bool {
    let suffix = format!("/{unit}");

    cgroup_content.lines().any(|line| {
        // v1 行形如 `1:name=systemd:/path`，v2 行形如 `0::/path`；
        // 两种情况取最后一个 ':' 之后的部分即路径。
        let path = match line.rfind(':') {
            Some(idx) => &line[idx + 1..],
            None => line,
        };

        path == suffix || path.ends_with(&suffix)
    })
}
```

前导 `/` 是必需的：没有它 `my-voice-input.service` 会被误判为命中。

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p voice-input-linux restart:: --features ibus`
Expected: 6 passed

- [ ] **Step 6: 提交**

```bash
git add crates/voice-input-linux/src/restart.rs crates/voice-input-linux/src/lib.rs
git commit -m "feat(voice-input-linux): 新增 cgroup unit 检测纯函数"
```

---

## Task 2: systemd-run 命令构造

**Files:**
- Modify: `crates/voice-input-linux/src/restart.rs`
- Test: `crates/voice-input-linux/src/restart.rs`

- [ ] **Step 1: 写失败的测试**

在 `restart.rs` 的 `mod tests` 末尾追加：

```rust
    #[test]
    fn restart_command_uses_systemd_run_transient_unit() {
        let args = restart_command("voice-input.service", "voice-input-restart-42-1700000000");

        assert_eq!(
            args,
            vec![
                "--user",
                "--collect",
                "--unit=voice-input-restart-42-1700000000",
                "systemctl",
                "--user",
                "restart",
                "voice-input.service",
            ]
        );
    }

    #[test]
    fn restart_command_targets_given_unit() {
        let args = restart_command("other.service", "helper");

        assert_eq!(args.last().map(String::as_str), Some("other.service"));
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux restart:: --features ibus`
Expected: 编译失败，`cannot find function restart_command in this scope`

- [ ] **Step 3: 写最小实现**

在 `cgroup_matches_unit` 之后插入：

```rust
/// 构造 `systemd-run` helper 的参数列表（不含 `systemd-run` 本身）。
///
/// helper 跑在独立 transient unit 里，因此不会被 restart 的 SIGTERM 波及。
/// `--collect` 保证 helper 结束后自动回收，不残留 failed unit。
pub fn restart_command(unit: &str, helper_unit: &str) -> Vec<String> {
    vec![
        "--user".to_string(),
        "--collect".to_string(),
        format!("--unit={helper_unit}"),
        "systemctl".to_string(),
        "--user".to_string(),
        "restart".to_string(),
        unit.to_string(),
    ]
}
```

同步把 `lib.rs` 的导出行改为：

```rust
pub use restart::{cgroup_matches_unit, restart_command, SERVICE_UNIT};
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux restart:: --features ibus`
Expected: 8 passed

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/restart.rs crates/voice-input-linux/src/lib.rs
git commit -m "feat(voice-input-linux): 新增 systemd-run 重启命令构造"
```

---

## Task 3: RestartController 检测与发起

**Files:**
- Modify: `crates/voice-input-linux/src/restart.rs`
- Test: `crates/voice-input-linux/src/restart.rs`

- [ ] **Step 1: 写失败的测试**

在 `restart.rs` 的 `mod tests` 末尾追加：

```rust
    #[test]
    fn controller_available_when_cgroup_matches() {
        let controller = RestartController::from_cgroup_content(V2_MATCH, "voice-input.service");

        assert!(controller.is_available());
    }

    #[test]
    fn controller_unavailable_when_cgroup_differs() {
        let controller =
            RestartController::from_cgroup_content("0::/user.slice/user@1000.service\n", "voice-input.service");

        assert!(!controller.is_available());
    }

    #[test]
    fn unavailable_label_explains_requirement() {
        let controller =
            RestartController::from_cgroup_content("0::/user.slice/user@1000.service\n", "voice-input.service");

        assert_eq!(controller.unavailable_label(), "（需 systemd 服务）");
    }

    #[test]
    fn request_fails_when_unavailable() {
        let controller =
            RestartController::from_cgroup_content("0::/user.slice/user@1000.service\n", "voice-input.service");

        let err = controller.request().expect_err("不可用时应当报错");
        assert!(
            err.contains("systemd"),
            "错误信息应当提到 systemd，实际：{err}"
        );
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test -p voice-input-linux restart:: --features ibus`
Expected: 编译失败，`cannot find type RestartController in this scope`

- [ ] **Step 3: 写最小实现**

在 `restart.rs` 文件顶部（`pub const SERVICE_UNIT` 之前）加入 use 声明：

```rust
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
```

在 `restart_command` 之后插入：

```rust
/// 重启控制器的可用性由「当前进程是否跑在目标 systemd unit 里」决定。
#[derive(Debug, Clone)]
pub struct RestartController {
    unit: Option<String>,
}

impl RestartController {
    /// 读取 `/proc/self/cgroup` 做检测。读不到时视为不可用。
    pub fn detect() -> Self {
        let unit = SERVICE_UNIT;
        match std::fs::read_to_string("/proc/self/cgroup") {
            Ok(content) => Self::from_cgroup_content(&content, unit),
            Err(_) => Self { unit: None },
        }
    }

    /// 从给定 cgroup 内容构造，便于单测。
    pub fn from_cgroup_content(content: &str, unit: &str) -> Self {
        if cgroup_matches_unit(content, unit) {
            Self {
                unit: Some(unit.to_string()),
            }
        } else {
            Self { unit: None }
        }
    }

    pub fn is_available(&self) -> bool {
        self.unit.is_some()
    }

    /// 不可用时拼在菜单标签后面的后缀。
    pub fn unavailable_label(&self) -> &'static str {
        "（需 systemd 服务）"
    }

    /// 发起重启。成功返回后调用方应当走正常退出路径。
    pub fn request(&self) -> Result<(), String> {
        let Some(unit) = self.unit.as_deref() else {
            return Err(format!(
                "当前进程不在 {SERVICE_UNIT} 的 systemd 服务中，无法重启输入法"
            ));
        };

        let helper_unit = format!(
            "voice-input-restart-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        );

        let status = Command::new("systemd-run")
            .args(restart_command(unit, &helper_unit))
            .status()
            .map_err(|e| format!("执行 systemd-run 失败：{e}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("systemd-run 返回非零退出码：{status}"))
        }
    }
}
```

helper unit 名带 pid 与秒级时间戳，避免连点两次时 transient unit 重名冲突。

同步把 `lib.rs` 的导出行改为：

```rust
pub use restart::{cgroup_matches_unit, restart_command, RestartController, SERVICE_UNIT};
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test -p voice-input-linux restart:: --features ibus`
Expected: 12 passed

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/restart.rs crates/voice-input-linux/src/lib.rs
git commit -m "feat(voice-input-linux): 新增 RestartController 检测与发起重启"
```

---

## Task 4: 托盘菜单接入

**Files:**
- Modify: `crates/voice-input-linux/src/tray.rs`
- Test: `crates/voice-input-linux/src/tray.rs`（模块内 `#[cfg(test)]`，本任务新增）

- [ ] **Step 1: 给 LinuxTrayConfig 与 LinuxTrayHandle 加字段**

在 `tray.rs` 顶部 use 区加入：

```rust
    use crate::restart::RestartController;
```

`LinuxTrayConfig` 结构体增加两个字段：

```rust
    #[derive(Debug, Clone)]
    pub struct LinuxTrayConfig {
        pub service_name: String,
        pub title: String,
        pub recorder: LinuxMicAudioRecorder,
        pub quit_requested: Arc<AtomicBool>,
        pub restart: RestartController,
        pub restart_requested: Arc<AtomicBool>,
    }
```

`impl LinuxTrayConfig` 的 `new` 增加两个参数并写入字段：

```rust
        pub fn new(
            service_name: impl Into<String>,
            title: impl Into<String>,
            recorder: LinuxMicAudioRecorder,
            quit_requested: Arc<AtomicBool>,
            restart: RestartController,
            restart_requested: Arc<AtomicBool>,
        ) -> Self {
            Self {
                service_name: service_name.into(),
                title: title.into(),
                recorder,
                quit_requested,
                restart,
                restart_requested,
            }
        }
```

`LinuxTrayHandle` 增加字段与访问器：

```rust
    pub struct LinuxTrayHandle {
        handle: ksni::blocking::Handle<LinuxTray>,
        quit_requested: Arc<AtomicBool>,
        restart_requested: Arc<AtomicBool>,
    }
```

`LinuxTrayHandle::spawn` 中，在 `let quit_requested = Arc::clone(&tray.quit_requested);` 之后加入：

```rust
            let restart_requested = Arc::clone(&tray.restart_requested);
```

`Ok(Self { ... })` 改为：

```rust
            Ok(Self {
                handle,
                quit_requested,
                restart_requested,
            })
```

`is_quit_requested` 之后加入：

```rust
        pub fn is_restart_requested(&self) -> bool {
            self.restart_requested.load(Ordering::SeqCst)
        }
```

- [ ] **Step 2: 给 LinuxTray 加字段**

结构体：

```rust
    struct LinuxTray {
        service_name: String,
        title: String,
        recording: bool,
        recorder: LinuxMicAudioRecorder,
        quit_requested: Arc<AtomicBool>,
        restart: RestartController,
        restart_requested: Arc<AtomicBool>,
    }
```

`LinuxTray::new` 对应补充：

```rust
                quit_requested: config.quit_requested,
                restart: config.restart,
                restart_requested: config.restart_requested,
            }
```

- [ ] **Step 3: 写失败的测试**

`RestartController` 不可用时的标签拼装逻辑需要可测。在 `tray.rs` 的 `impl LinuxTray` 里 `stop_label` 之后加入方法：

```rust
        fn restart_label(&self) -> String {
            if self.restart.is_available() {
                "重启输入法".to_string()
            } else {
                format!("重启输入法{}", self.restart.unavailable_label())
            }
        }

        fn restart_enabled(&self) -> bool {
            self.restart.is_available() && !self.recording
        }
```

在 `tray.rs` 文件末尾（`pub use` 行之前）加入测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::linux_tray::LinuxTray;
    use crate::recorder::LinuxMicAudioRecorder;
    use crate::restart::RestartController;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::Duration;

    fn tray_with(cgroup: &str, recording: bool) -> LinuxTray {
        let restart = RestartController::from_cgroup_content(cgroup, "voice-input.service");
        LinuxTray {
            service_name: "voice-input".to_string(),
            title: "VoiceInput".to_string(),
            recording,
            recorder: LinuxMicAudioRecorder::new(Duration::from_secs(30)),
            quit_requested: Arc::new(AtomicBool::new(false)),
            restart,
            restart_requested: Arc::new(AtomicBool::new(false)),
        }
    }

    const MATCHING: &str = "0::/user.slice/user-1000.slice/user@1000.service/voice-input.service\n";

    #[test]
    fn restart_label_is_plain_when_available() {
        let tray = tray_with(MATCHING, false);
        assert_eq!(tray.restart_label(), "重启输入法");
    }

    #[test]
    fn restart_label_explains_when_unavailable() {
        let tray = tray_with("0::/user.slice/user@1000.service\n", false);
        assert_eq!(tray.restart_label(), "重启输入法（需 systemd 服务）");
    }

    #[test]
    fn restart_disabled_when_unavailable() {
        let tray = tray_with("0::/user.slice/user@1000.service\n", false);
        assert!(!tray.restart_enabled());
    }

    #[test]
    fn restart_disabled_while_recording() {
        let tray = tray_with(MATCHING, true);
        assert!(!tray.restart_enabled());
    }

    #[test]
    fn restart_enabled_when_available_and_idle() {
        let tray = tray_with(MATCHING, false);
        assert!(tray.restart_enabled());
    }
}
```

注意：`restart.rs` 里的 `RestartController` 需要通过 `use crate::restart::RestartController;` 在 `linux_tray` 模块内可见，测试模块直接 `use crate::restart::RestartController;` 访问。

- [ ] **Step 4: 运行测试确认失败**

Run: `cargo test -p voice-input-linux tray:: --features ibus`
Expected: 编译失败，`no method named restart_label found`

- [ ] **Step 5: 在 menu() 里插入菜单项**

在 `tray.rs` 的 `menu()` 方法里，`items.push(MenuItem::Separator);` 与「退出」项之间插入：

```rust
            items.push(
                StandardItem {
                    label: self.restart_label(),
                    enabled: self.restart_enabled(),
                    visible: true,
                    icon_name: "view-refresh".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| match tray.restart.request() {
                        Ok(()) => tray.restart_requested.store(true, Ordering::SeqCst),
                        Err(err) => eprintln!("重启输入法失败：{err}"),
                    }),
                    ..Default::default()
                }
                .into(),
            );

            items.push(MenuItem::Separator);
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test -p voice-input-linux tray:: --features ibus`
Expected: 5 passed

- [ ] **Step 7: 提交**

```bash
git add crates/voice-input-linux/src/tray.rs
git commit -m "feat(voice-input-linux): 托盘菜单新增重启输入法项"
```

---

## Task 5: runtime 接入重启标志

**Files:**
- Modify: `crates/voice-input-linux/src/runtime.rs`

- [ ] **Step 1: 加入 use 与状态**

在 `runtime.rs` 顶部 use 区，把

```rust
use crate::tray::{spawn_linux_tray, LinuxTrayConfig, LinuxTrayHandle};
```

之后加入：

```rust
use crate::restart::RestartController;
```

在 `run_live_app` 中，`let quit_requested = Arc::new(AtomicBool::new(false));` 之后加入：

```rust
    let restart_requested = Arc::new(AtomicBool::new(false));
    let restart = RestartController::detect();
```

- [ ] **Step 2: 传给托盘配置**

把 `spawn_linux_tray` 调用改为：

```rust
        let tray = spawn_linux_tray(LinuxTrayConfig::new(
            config.service_name.clone(),
            "VoiceInput".to_string(),
            recorder.clone(),
            Arc::clone(&quit_requested),
            restart,
            Arc::clone(&restart_requested),
        ))?;
```

- [ ] **Step 3: 主循环退出条件加上重启标志**

把主循环开头的：

```rust
        if quit_requested.load(Ordering::SeqCst) {
            watcher.stop();
            break;
        }
```

改为：

```rust
        if quit_requested.load(Ordering::SeqCst) || restart_requested.load(Ordering::SeqCst) {
            watcher.stop();
            break;
        }
```

- [ ] **Step 4: 运行完整测试与编译检查**

Run: `cargo test -p voice-input-linux --features ibus`
Expected: 全部通过（原有 6 个单元 + 2 个集成 + 本次新增 17 个）

Run: `cargo build -p voice-input-linux --features ibus --release`
Expected: 编译成功

- [ ] **Step 5: 提交**

```bash
git add crates/voice-input-linux/src/runtime.rs
git commit -m "feat(voice-input-linux): 主循环响应重启请求并走正常退出路径"
```

---

## Task 6: 代码风格与真机验证

**Files:** 无新增

- [ ] **Step 1: 格式化与静态检查**

```bash
cargo fmt --all
cargo clippy -p voice-input-linux --features ibus -- -D warnings
```

Expected: fmt 无输出；clippy 无 warning。

**偏差记录（2026-09-19）：** 本机 `cargo-clippy` 组件缺失，`rustup component add clippy`
因镜像 404 失败，clippy 步骤跳过。`cargo fmt --all` 会波及 5 个与本次功能无关的文件
（`funasr.rs`、`hotkey.rs`、`ibus.rs`、`live_cli.rs`、`main.rs`，共 66 行既有格式漂移）。
为避免把无关改动混进本次提交，改为只对本次功能涉及的文件做 fmt 检查：

```bash
cd /home/lab/Projects/VoiceInput
rustfmt --check --edition 2021 \
  crates/voice-input-linux/src/restart.rs \
  crates/voice-input-linux/src/system_ime.rs \
  crates/voice-input-linux/src/tray.rs \
  crates/voice-input-linux/src/runtime.rs \
  crates/voice-input-linux/src/lib.rs
```

Expected: 无输出（说明这几个文件本来就已符合 rustfmt）。

- [ ] **Step 2: 部署新二进制并重启服务**

```bash
cd /home/lab/Projects/VoiceInput
cargo build -p voice-input-linux --features ibus --release
systemctl --user restart voice-input.service
sleep 15
systemctl --user status voice-input.service --no-pager | head -5
```

Expected: 服务 active (running)，MainPID 与重启前不同。

- [ ] **Step 3: 真机点一次菜单项**

在托盘图标上右键 → 点「重启输入法」，然后：

```bash
journalctl --user -u voice-input.service -f
```

Expected: 看到旧实例退出、新实例打印「正在预加载 ASR 模型...」→「ASR 模型预加载完成」。
再执行 `systemctl --user status voice-input.service --no-pager | head -5`，MainPID 应再次变化。

- [ ] **Step 4: 验证非 systemd 场景降级**

```bash
systemctl --user stop voice-input.service
cd /home/lab/Projects/VoiceInput
./target/release/voice-input-linux live --backend ibus
```

Expected: 托盘菜单里该项显示为置灰的「重启输入法（需 systemd 服务）」，点击无反应。
验证完 `Ctrl+C` 结束该进程，再 `systemctl --user start voice-input.service` 恢复。

- [ ] **Step 5: 真机验证重启系统输入法**

记录重启前的 ibus-daemon PID 与 VoiceInput MainPID：

```bash
pgrep -a ibus-daemon
systemctl --user show voice-input.service -p MainPID
```

在托盘图标上右键 → 点「重启系统输入法」，然后重新执行上面两条命令。

Expected: `ibus-daemon` 的 PID 变化；VoiceInput 的 MainPID **不变**。

**实测结果（2026-09-19，IBus 1.5.22）：** 期望中的「ibus-daemon PID 变化」未出现。
strace 显示 `ibus restart` 只是向 `/org/freedesktop/IBus` 发 `Exit(true)`——
地址文件 guid 变化、抽象 socket 换名，但 daemon PID 不变。
`--restart` 在 ibus-daemon 里指的是重启 panel/config 子进程，不是 daemon 自身。
这一项的实际效果是「重置 ibus 总线连接」。经确认保留官方命令，不改为 kill + 拉起。

实测确认的部分：菜单项显示正常且可点击；VoiceInput MainPID 保持不变（这是本项的
关键约束，已验证）。

---

## Task 7: 托盘菜单新增「重启系统输入法」（ibus）

**Files:**
- Create: `crates/voice-input-linux/src/system_ime.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Modify: `crates/voice-input-linux/src/tray.rs`
- Modify: `crates/voice-input-linux/src/runtime.rs`
- Test: `crates/voice-input-linux/src/system_ime.rs`（模块内 `#[cfg(test)]`）、`tray.rs` 既有测试模块

设计依据：spec 的「追加需求：重启系统输入法（ibus 框架）」一节。

- [ ] **Step 1: 在 lib.rs 声明模块**

按字母序把 `mod system_ime;` 插到 `mod smoke;` 与 `mod tray;` 之间：

```rust
mod restart;
mod runtime;
mod settings;
mod smoke;
mod system_ime;
mod tray;
```

在 `pub use smoke::{run_smoke, transcribe_file};` 之后加入导出：

```rust
pub use system_ime::{names_contain_ibus_daemon, SystemImeController, IBUS_DAEMON_COMM};
```

- [ ] **Step 2: 写失败的测试**

创建 `crates/voice-input-linux/src/system_ime.rs`，只写常量、签名（`todo!()` 占位）与测试：

```rust
/// ibus 主进程的 comm 名（/proc/<pid>/comm 截断到 15 字节，此名未超限）。
pub const IBUS_DAEMON_COMM: &str = "ibus-daemon";

/// 判断一组 comm 名里是否存在 ibus 主进程。
pub fn names_contain_ibus_daemon<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    todo!()
}

#[derive(Debug, Clone)]
pub struct SystemImeController {
    available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_with_daemon_matches() {
        assert!(names_contain_ibus_daemon(["ibus-daemon"]));
    }

    #[test]
    fn probe_with_daemon_among_others_matches() {
        assert!(names_contain_ibus_daemon([
            "gnome-shell",
            "ibus-daemon",
            "ibus-x11",
        ]));
    }

    #[test]
    fn probe_with_only_child_processes_does_not_match() {
        assert!(!names_contain_ibus_daemon(["ibus-dconf", "ibus-x11", "ibus-portal"]));
    }

    #[test]
    fn probe_with_empty_list_does_not_match() {
        assert!(!names_contain_ibus_daemon([] as [&str; 0]));
    }

    #[test]
    fn probe_does_not_match_prefix_comm() {
        assert!(!names_contain_ibus_daemon(["ibus-daemon-x"]));
    }
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p voice-input-linux system_ime:: --features ibus`
Expected: 5 个测试全部 FAIL，panic 信息为 `not yet implemented`

- [ ] **Step 4: 写最小实现**

把 `names_contain_ibus_daemon` 的 `todo!()` 替换为：

```rust
pub fn names_contain_ibus_daemon<'a>(names: impl IntoIterator<Item = &'a str>) -> bool {
    names.into_iter().any(|name| name == IBUS_DAEMON_COMM)
}
```

在文件顶部加入 use：

```rust
use std::process::Command;
```

在 `names_contain_ibus_daemon` 之后插入：

```rust
/// 系统输入法（ibus）控制器。可用性由「是否有 ibus-daemon 在跑」决定。
#[derive(Debug, Clone)]
pub struct SystemImeController {
    available: bool,
}

impl SystemImeController {
    /// 扫描 `/proc/*/comm` 做检测。
    ///
    /// 不用 `ibus address` 探测：daemon 卡死时该 D-Bus 调用会阻塞到超时（约 25 秒），
    /// 而探测发生在 `menu()` 里，会把托盘线程挂住。
    pub fn detect() -> Self {
        let names = std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let comm = entry.path().join("comm");
                std::fs::read_to_string(comm).ok()
            })
            .collect::<Vec<_>>();
        let refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        Self::from_probe(&refs)
    }

    /// 从给定 comm 名列表构造，便于单测。
    pub fn from_probe(names: &[&str]) -> Self {
        Self {
            available: names_contain_ibus_daemon(names.iter().copied()),
        }
    }

    pub fn is_available(&self) -> bool {
        self.available
    }

    /// 不可用时拼在菜单标签后面的后缀。
    pub fn unavailable_label(&self) -> &'static str {
        "（未检测到 ibus）"
    }

    /// 调用 `ibus restart` 重启系统输入法。不影响 VoiceInput 自身。
    pub fn request(&self) -> Result<(), String> {
        if !self.available {
            return Err("未检测到 ibus，无法重启系统输入法".to_string());
        }

        let status = Command::new("ibus")
            .arg("restart")
            .status()
            .map_err(|e| format!("执行 ibus restart 失败：{e}"))?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("ibus restart 返回非零退出码：{status}"))
        }
    }
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test -p voice-input-linux system_ime:: --features ibus`
Expected: 5 passed

- [ ] **Step 6: 提交**

```bash
git add crates/voice-input-linux/src/system_ime.rs crates/voice-input-linux/src/lib.rs
git commit -m "feat(voice-input-linux): 新增 SystemImeController 检测与重启 ibus"
```

---

## Task 8: 托盘菜单接入「重启系统输入法」

**Files:**
- Modify: `crates/voice-input-linux/src/tray.rs`
- Modify: `crates/voice-input-linux/src/runtime.rs`

- [ ] **Step 1: 给配置与托盘加字段**

`tray.rs` 的 `mod linux_tray` 内 use 区，在 `use crate::restart::RestartController;` 之后加入：

```rust
    use crate::system_ime::SystemImeController;
```

`LinuxTrayConfig` 增加字段（放在 `restart_requested` 之后）：

```rust
        pub ime: SystemImeController,
```

`impl LinuxTrayConfig` 的 `new` 增加第 7 个参数 `ime: SystemImeController` 并写入字段：

```rust
        pub fn new(
            service_name: impl Into<String>,
            title: impl Into<String>,
            recorder: LinuxMicAudioRecorder,
            quit_requested: Arc<AtomicBool>,
            restart: RestartController,
            restart_requested: Arc<AtomicBool>,
            ime: SystemImeController,
        ) -> Self {
            Self {
                service_name: service_name.into(),
                title: title.into(),
                recorder,
                quit_requested,
                restart,
                restart_requested,
                ime,
            }
        }
```

`LinuxTray` 结构体增加字段：

```rust
        ime: SystemImeController,
```

`LinuxTray::new` 对应补充：

```rust
                ime: config.ime,
```

- [ ] **Step 2: 写失败的测试**

在 `impl LinuxTray` 里 `restart_enabled` 之后加入方法：

```rust
        fn ime_label(&self) -> String {
            if self.ime.is_available() {
                "重启系统输入法".to_string()
            } else {
                format!("重启系统输入法{}", self.ime.unavailable_label())
            }
        }

        fn ime_enabled(&self) -> bool {
            self.ime.is_available() && !self.recording
        }
```

在 `tray.rs` 测试模块里，把 `tray_with` 改为接受 ibus 状态并补上 `ime` 字段：

```rust
        fn tray_with(cgroup: &str, recording: bool, ibus_running: bool) -> LinuxTray {
            let restart = RestartController::from_cgroup_content(cgroup, "voice-input.service");
            let ime = if ibus_running {
                SystemImeController::from_probe(&["ibus-daemon"])
            } else {
                SystemImeController::from_probe(&["ibus-x11"])
            };
            LinuxTray {
                service_name: "voice-input".to_string(),
                title: "VoiceInput".to_string(),
                recording,
                recorder: LinuxMicAudioRecorder::new(Duration::from_secs(30)),
                quit_requested: Arc::new(AtomicBool::new(false)),
                restart,
                restart_requested: Arc::new(AtomicBool::new(false)),
                ime,
            }
        }
```

既有 5 个测试的调用点同步补第三个参数（均为 `true`，ibus 状态与它们无关）：

```rust
        let tray = tray_with(MATCHING, false, true);
        let tray = tray_with("0::/user.slice/user@1000.service\n", false, true);
        let tray = tray_with(MATCHING, true, true);
```

再追加四个新测试：

```rust
        #[test]
        fn ime_label_is_plain_when_available() {
            let tray = tray_with(MATCHING, false, true);
            assert_eq!(tray.ime_label(), "重启系统输入法");
        }

        #[test]
        fn ime_label_explains_when_unavailable() {
            let tray = tray_with(MATCHING, false, false);
            assert_eq!(tray.ime_label(), "重启系统输入法（未检测到 ibus）");
        }

        #[test]
        fn ime_disabled_when_unavailable() {
            let tray = tray_with(MATCHING, false, false);
            assert!(!tray.ime_enabled());
        }

        #[test]
        fn ime_disabled_while_recording() {
            let tray = tray_with(MATCHING, true, true);
            assert!(!tray.ime_enabled());
        }
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p voice-input-linux tray:: --features ibus`
Expected: 编译失败，`no method named ime_label found`

- [ ] **Step 4: 在 menu() 里插入菜单项**

在重启项之后、第二个 `items.push(MenuItem::Separator);` 之前插入：

```rust
            items.push(
                StandardItem {
                    label: self.ime_label(),
                    enabled: self.ime_enabled(),
                    visible: true,
                    icon_name: "input-keyboard".to_string(),
                    activate: Box::new(|tray: &mut LinuxTray| {
                        if let Err(err) = tray.ime.request() {
                            eprintln!("重启系统输入法失败：{err}");
                        }
                    }),
                    ..Default::default()
                }
                .into(),
            );
```

关键：这个闭包**不**设置 `restart_requested`——重启系统输入法不应重启 VoiceInput 自身。

- [ ] **Step 5: runtime 传入控制器**

`runtime.rs` 顶部 use 区加入：

```rust
use crate::system_ime::SystemImeController;
```

在 `let restart = RestartController::detect();` 之后加入：

```rust
    let ime = SystemImeController::detect();
```

`spawn_linux_tray` 调用补上第 7 个参数：

```rust
        let tray = spawn_linux_tray(LinuxTrayConfig::new(
            config.service_name.clone(),
            "VoiceInput".to_string(),
            recorder.clone(),
            Arc::clone(&quit_requested),
            restart,
            Arc::clone(&restart_requested),
            ime,
        ))?;
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test -p voice-input-linux tray:: --features ibus`
Expected: 9 passed（原有 5 个 + 新增 4 个）

Run: `cargo test -p voice-input-linux --features ibus`
Expected: 全部通过

Run: `cargo build -p voice-input-linux --features ibus --release`
Expected: 编译成功

- [ ] **Step 7: 提交**

```bash
git add crates/voice-input-linux/src/tray.rs crates/voice-input-linux/src/runtime.rs
git commit -m "feat(voice-input-linux): 托盘菜单新增重启系统输入法项"
```

---

## 完成后

- 更新 `docs/superpowers/specs/2026-09-19-tray-restart-design.md` 无需改动（实现与设计一致）
- 若真机验证发现偏差，在 plan 文档末尾补记偏差说明后再收尾
