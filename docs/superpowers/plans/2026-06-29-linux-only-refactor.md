# 纯 Linux 语音输入法重构 — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove macOS and Windows support, delete `voice-input-runtime` and `voice-input-cli` crates, merge their logic into `voice-input-linux` as the single binary target.

**Architecture:** 4-crate workspace (`voice-input-core`, `voice-input-asr`, `voice-input-audio`, `voice-input-linux`). The linux crate absorbs `CompositionDriver`/`StatefulInputMethodHost` from runtime::host, `LocalVoiceInputRuntime`/config from runtime::local, all live session types from runtime::live, and CLI arg parsing from voice-input-cli. Single binary at `voice-input-linux/src/main.rs`.

**Tech Stack:** Rust 2021, ibus, cpal, ksni, arboard, device_query, xdotool

---

### Task 1: Inline runtime types into linux crate (host, local, live)

**Files:**
- Modify: `crates/voice-input-linux/src/host.rs`
- Modify: `crates/voice-input-linux/src/local.rs`
- Create: `crates/voice-input-linux/src/live.rs`

- [ ] **Step 1: Rewrite host.rs — inline CompositionDriver + StatefulInputMethodHost**

Replace the entire content of `crates/voice-input-linux/src/host.rs`:

```rust
use std::cell::RefCell;

use crate::backend::{backend_from_kind, LinuxBackend, LinuxBackendKind};
use voice_input_core::{CompositionState, InputMethodHost, Result};

// ── Inlined from voice-input-runtime::host ──

pub trait CompositionDriver {
    fn start_composition(&self) -> Result<()>;
    fn update_preedit(&self, text: &str) -> Result<()>;
    fn show_recording_indicator(&self) -> Result<()> {
        Ok(())
    }
    fn clear_recording_indicator(&self) -> Result<()> {
        Ok(())
    }
    fn commit_text(&self, text: &str) -> Result<()>;
    fn cancel_composition(&self) -> Result<()>;
    fn end_composition(&self) -> Result<()>;
}

pub struct StatefulInputMethodHost<D> {
    driver: D,
    state: RefCell<CompositionState>,
}

impl<D> StatefulInputMethodHost<D> {
    pub fn new(driver: D) -> Self {
        Self {
            driver,
            state: RefCell::new(CompositionState::default()),
        }
    }

    pub fn state(&self) -> CompositionState {
        self.state.borrow().clone()
    }

    pub fn driver(&self) -> &D {
        &self.driver
    }
}

impl<D> InputMethodHost for StatefulInputMethodHost<D>
where
    D: CompositionDriver,
{
    fn start_composition(&self) -> Result<()> {
        self.driver.start_composition()?;
        self.state.borrow_mut().start();
        Ok(())
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.driver.update_preedit(text)?;
        self.state.borrow_mut().update(text);
        Ok(())
    }

    fn show_recording_indicator(&self) -> Result<()> {
        self.driver.show_recording_indicator()
    }

    fn clear_recording_indicator(&self) -> Result<()> {
        self.driver.clear_recording_indicator()
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.driver.commit_text(text)?;
        self.state.borrow_mut().commit(text);
        Ok(())
    }

    fn cancel_composition(&self) -> Result<()> {
        self.driver.cancel_composition()?;
        self.state.borrow_mut().cancel();
        Ok(())
    }

    fn end_composition(&self) -> Result<()> {
        self.driver.end_composition()
    }
}

// ── Linux-specific host ──

#[derive(Debug, Clone)]
pub struct LinuxHostConfig {
    pub backend: LinuxBackendKind,
    pub service_name: String,
}

impl Default for LinuxHostConfig {
    fn default() -> Self {
        Self {
            backend: LinuxBackendKind::Fcitx5,
            service_name: "voice-input".to_string(),
        }
    }
}

pub struct LinuxInputMethodHost {
    config: LinuxHostConfig,
    inner: StatefulInputMethodHost<LinuxHostDriver>,
}

struct LinuxHostDriver {
    backend: Box<dyn LinuxBackend>,
}

impl LinuxInputMethodHost {
    pub fn new(config: LinuxHostConfig) -> Self {
        let backend_kind = config.backend;
        Self::new_with_backend(config, backend_from_kind(backend_kind))
    }

    pub fn new_with_backend(config: LinuxHostConfig, backend: Box<dyn LinuxBackend>) -> Self {
        Self {
            config,
            inner: StatefulInputMethodHost::new(LinuxHostDriver { backend }),
        }
    }

    pub fn backend_kind(&self) -> LinuxBackendKind {
        self.config.backend
    }
}

impl CompositionDriver for LinuxHostDriver {
    fn start_composition(&self) -> Result<()> {
        self.backend.start()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.backend.update_preedit(text)
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.backend.commit_text(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        self.backend.cancel()
    }

    fn end_composition(&self) -> Result<()> {
        self.backend.stop()
    }
}

impl InputMethodHost for LinuxInputMethodHost {
    fn start_composition(&self) -> Result<()> {
        self.inner.start_composition()
    }

    fn update_preedit(&self, text: &str) -> Result<()> {
        self.inner.update_preedit(text)
    }

    fn commit_text(&self, text: &str) -> Result<()> {
        self.inner.commit_text(text)
    }

    fn cancel_composition(&self) -> Result<()> {
        self.inner.cancel_composition()
    }

    fn end_composition(&self) -> Result<()> {
        self.inner.end_composition()
    }
}
```

- [ ] **Step 2: Rewrite local.rs — inline LocalVoiceInputConfig + LocalVoiceInputRuntime**

Replace the entire content of `crates/voice-input-linux/src/local.rs`:

```rust
use std::path::PathBuf;

use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager, InputMethodHost};

use crate::backend::{LinuxBackend, LinuxBackendKind};
use crate::host::{LinuxHostConfig, LinuxInputMethodHost};

// ── Inlined from voice-input-runtime::local ──

#[derive(Debug, Clone)]
pub struct LocalVoiceInputConfig {
    pub app: AppConfig,
    pub asr: FunAsrConfig,
}

impl Default for LocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            app: AppConfig::default(),
            asr: FunAsrConfig::from_env(),
        }
    }
}

pub fn build_local_python_runtime_config(
) -> voice_input_core::Result<(LocalVoiceInputConfig, Box<dyn FunAsrRunner>)> {
    let config = LocalVoiceInputConfig::default();
    let runner = PythonFunAsrRunner::connect(config.asr.clone())?;
    Ok((config, Box::new(runner)))
}

// ── Linux-specific local voice input ──

#[derive(Debug, Clone)]
pub struct LinuxLocalVoiceInputConfig {
    pub runtime: LocalVoiceInputConfig,
    pub host: LinuxHostConfig,
}

impl Default for LinuxLocalVoiceInputConfig {
    fn default() -> Self {
        Self {
            runtime: LocalVoiceInputConfig::default(),
            host: LinuxHostConfig::default(),
        }
    }
}

pub struct LinuxLocalVoiceInput {
    controller: AppController,
    backend_kind: LinuxBackendKind,
    service_name: String,
}

impl LinuxLocalVoiceInput {
    pub fn new(
        config: LinuxLocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        backend: Box<dyn LinuxBackend>,
    ) -> Self {
        let backend_kind = backend.kind();
        let service_name = config.host.service_name.clone();
        let transcriber = LocalFunAsrTranscriber::new(config.runtime.asr, runner);
        let host = LinuxInputMethodHost::new_with_backend(config.host, backend);
        let controller = AppController::new(
            config.runtime.app,
            hotkeys,
            recorder,
            Box::new(transcriber),
            Box::new(host),
        );

        Self {
            controller,
            backend_kind,
            service_name,
        }
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn backend_kind(&self) -> LinuxBackendKind {
        self.backend_kind
    }

    pub fn service_name(&self) -> &str {
        &self.service_name
    }
}
```

- [ ] **Step 3: Create live.rs — copy all live types from voice-input-runtime**

Create `crates/voice-input-linux/src/live.rs` with the complete content from `crates/voice-input-runtime/src/live.rs`, but adjust the crate paths:
- `use voice_input_asr::` stays as-is
- `use voice_input_core::` stays as-is
- `use crate::` instead of `use super::` for module-internal refs

The file is ~598 lines (the entire voice-input-runtime/src/live.rs with tests). Since the file content is the same as what was already read, use:

```bash
cp crates/voice-input-runtime/src/live.rs crates/voice-input-linux/src/live.rs
```

Then fix the crate-internal `use` paths — there are no internal use paths to fix since live.rs only uses external crate types and `use super::` in tests. The `use super::` in `#[cfg(test)]` refers to module-level types which are all defined in the same file.

- [ ] **Step 4: Commit**

```bash
git add crates/voice-input-linux/src/host.rs \
        crates/voice-input-linux/src/local.rs \
        crates/voice-input-linux/src/live.rs
git commit -m "refactor(voice-input-linux): inline runtime types (host, local, live)"
```

---

### Task 2: Update all linux crate consumers to use local modules

**Files:**
- Modify: `crates/voice-input-linux/src/runtime.rs`
- Modify: `crates/voice-input-linux/src/hotkey.rs`
- Modify: `crates/voice-input-linux/src/smoke.rs`
- Modify: `crates/voice-input-linux/src/lib.rs`
- Modify: `crates/voice-input-linux/src/main.rs`
- Delete: `crates/voice-input-linux/src/bin/voice-input-linux-app.rs`

- [ ] **Step 1: Update runtime.rs — replace voice-input-runtime imports**

In `crates/voice-input-linux/src/runtime.rs`, find the `#[cfg(target_os = "linux")]` block's inner imports (around line 24-27):

Remove:
```rust
    use voice_input_runtime::{
        print_live_ready, run_streaming_live_cycle, stream_preview_chunk, LiveJobHandle,
        LiveJobState,
    };
```

And add after the existing `use voice_input_asr::` block:
```rust
    use crate::live::{
        print_live_ready, run_streaming_live_cycle, stream_preview_chunk, LiveJobHandle,
        LiveJobState,
    };
```

- [ ] **Step 2: Update hotkey.rs — replace voice-input-runtime import**

In `crates/voice-input-linux/src/hotkey.rs`, line 16, change:
```rust
use voice_input_runtime::LiveJobState;
```
to:
```rust
use crate::live::LiveJobState;
```

- [ ] **Step 3: Update hotkey.rs not-linux stub — fix LiveJobState reference**

In the `#[cfg(not(target_os = "linux"))]` block of hotkey.rs (around lines 228-253), the `LinuxHotkeyWatcher::spawn` takes `_active: Arc<LiveJobState>`. After the import change, this still resolves correctly since `LiveJobState` is now in `crate::live`.

- [ ] **Step 4: Update smoke.rs — replace voice-input-runtime import**

In `crates/voice-input-linux/src/smoke.rs`, line 4, change:
```rust
use voice_input_runtime::build_local_python_runtime_config;
```
to:
```rust
use crate::local::build_local_python_runtime_config;
```

- [ ] **Step 5: Update main.rs — replace voice-input-runtime import**

In `crates/voice-input-linux/src/main.rs`, lines 4-6, change:
```rust
    let (audio_file, backend) =
        match voice_input_runtime::parse_audio_file_with_optional_backend_arg(
```
to:
```rust
    let (audio_file, backend) =
        match crate::parse_audio_file_with_optional_backend_arg(
```

- [ ] **Step 6: Move parse_audio_file functions into local.rs (or a new cli.rs placeholder)**

The functions `parse_audio_file_with_optional_backend_arg`, `parse_required_audio_file_arg` are currently in `voice-input-runtime::local`. Since main.rs needs them, add them at the end of `crates/voice-input-linux/src/local.rs`:

```rust
// ── CLI argument parsing (from voice-input-runtime::local) ──

pub fn parse_required_audio_file_arg(args: Vec<String>) -> Result<PathBuf, String> {
    let mut iter = args.into_iter();
    let _bin = iter.next();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--audio-file" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --audio-file 的值"))?;
                return Ok(PathBuf::from(value));
            }
            "--help" | "-h" => return Err(String::from("help")),
            other => return Err(format!("不支持的参数：{other}")),
        }
    }

    Err(String::from("缺少必需参数 --audio-file"))
}

pub fn parse_audio_file_with_optional_backend_arg<T, F>(
    args: Vec<String>,
    default_backend: T,
    parse_backend: F,
) -> Result<(PathBuf, T), String>
where
    F: Fn(&str) -> Result<T, String>,
{
    let mut audio_file = None;
    let mut backend = Some(default_backend);
    let mut iter = args.into_iter();
    let _bin = iter.next();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--audio-file" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --audio-file 的值"))?;
                audio_file = Some(PathBuf::from(value));
            }
            "--backend" => {
                let value = iter
                    .next()
                    .ok_or_else(|| String::from("缺少 --backend 的值"))?;
                backend = Some(parse_backend(&value)?);
            }
            "--help" | "-h" => return Err(String::from("help")),
            other => return Err(format!("不支持的参数：{other}")),
        }
    }

    let audio_file = audio_file.ok_or_else(|| String::from("缺少必需参数 --audio-file"))?;
    let backend = backend.expect("default backend should always be present");
    Ok((audio_file, backend))
}
```

Then update main.rs to use `crate::local::parse_audio_file_with_optional_backend_arg`.

- [ ] **Step 7: Update lib.rs — add live module, remove runtime re-exports**

In `crates/voice-input-linux/src/lib.rs`:
- Add `mod live;` to the module declarations
- Add re-exports for the live types: currently `runtime.rs` imports `LiveJobHandle, LiveJobState` which are now in `crate::live`. These are used internally only, so `pub(crate)` re-exports work. But current lib.rs re-exports everything as `pub`. Keep the same pattern for now.

Add to lib.rs (after `mod ibus;` and before `mod local;`):
```rust
mod live;
```

Add to the `pub use` block:
```rust
pub use live::{LiveJobHandle, LiveJobState, QueuedLiveJobState, QueuedLiveJobHandle};
```

- [ ] **Step 8: Update main.rs — use crate::local qualified path**

The import now resolves through `crate::local::parse_audio_file_with_optional_backend_arg`. Update main.rs to match:

```rust
use std::env;

fn main() {
    let (audio_file, backend) =
        match voice_input_linux::local::parse_audio_file_with_optional_backend_arg(
```

Wait — main.rs is inside the crate, so it uses `crate::` not `voice_input_linux::`. But the current main.rs just calls:
```rust
voice_input_linux::run_smoke(audio_file, backend)
```

Actually, since main.rs is `src/main.rs` in the voice-input-linux crate, it can use `crate::` paths. But the current convention uses `voice_input_linux::` which also works (the crate re-exports itself). Let me keep the existing pattern for minimal change.

The key change in main.rs is just the `voice_input_runtime::parse_audio_file_with_optional_backend_arg` → `voice_input_linux::local::parse_audio_file_with_optional_backend_arg`. But wait, `local` module functions need to be public. Let me make sure `parse_audio_file_with_optional_backend_arg` is `pub` in local.rs.

Since main.rs uses the crate's own public API via `voice_input_linux::`, the functions need to be re-exported from lib.rs. Add to lib.rs:

```rust
pub use local::{
    build_local_python_runtime_config, parse_audio_file_with_optional_backend_arg,
    parse_required_audio_file_arg, LinuxLocalVoiceInput, LinuxLocalVoiceInputConfig,
    LocalVoiceInputConfig,
};
```

Then main.rs becomes:

```rust
use std::env;

fn main() {
    let (audio_file, backend) =
        match voice_input_linux::parse_audio_file_with_optional_backend_arg(
            env::args().collect(),
            voice_input_linux::LinuxBackendKind::IBus,
            voice_input_linux::parse_backend_kind,
        ) {
            Ok(args) => args,
            Err(message) => {
                if message == "help" {
                    print_usage();
                    std::process::exit(0);
                }
                eprintln!("{message}");
                print_usage();
                std::process::exit(2);
            }
        };

    if let Err(message) = voice_input_linux::run_smoke(audio_file, backend) {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

fn print_usage() {
    eprintln!(
        "用法：cargo run -p voice-input-linux --features ibus -- --audio-file /path/to/audio.wav [--backend ibus|fcitx5]"
    );
}
```

- [ ] **Step 9: Delete bin/voice-input-linux-app.rs**

Remove the old app binary entry point. After Task 3, the unified main.rs handles both smoke and live.

```bash
rm crates/voice-input-linux/src/bin/voice-input-linux-app.rs
```

If the `bin/` directory is now empty:
```bash
rmdir crates/voice-input-linux/src/bin/
```

- [ ] **Step 10: Commit**

```bash
git add crates/voice-input-linux/src/
git commit -m "refactor(voice-input-linux): update all consumers to use local modules, remove runtime dep"
```

---

### Task 3: Rewrite main.rs as unified entry + update Cargo.toml

**Files:**
- Modify: `crates/voice-input-linux/src/main.rs` (full rewrite)
- Modify: `crates/voice-input-linux/Cargo.toml`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Rewrite main.rs as unified CLI entry**

Replace `crates/voice-input-linux/src/main.rs`:

```rust
use std::env;
use std::path::PathBuf;

use voice_input_linux::{
    parse_backend_kind, parse_live_args, run_live_with_args, run_smoke, LinuxBackendKind,
};

fn main() {
    std::process::exit(run());
}

fn run() -> i32 {
    let args: Vec<String> = env::args().collect();
    let command = match parse_command(args) {
        Ok(cmd) => cmd,
        Err(ParseOutcome::Help(msg)) => {
            eprintln!("{msg}");
            return 0;
        }
        Err(ParseOutcome::Error(msg)) => {
            eprintln!("{msg}");
            eprintln!("{}", usage());
            return 2;
        }
    };

    let result = match command {
        Command::Smoke {
            audio_file,
            backend,
        } => run_smoke(audio_file, backend),
        Command::Live(args) => run_live_with_args(args),
    };

    match result {
        Ok(()) => 0,
        Err(msg) => {
            eprintln!("{msg}");
            1
        }
    }
}

enum Command {
    Smoke {
        audio_file: PathBuf,
        backend: LinuxBackendKind,
    },
    Live(voice_input_linux::LinuxLiveArgs),
}

enum ParseOutcome {
    Help(String),
    Error(String),
}

fn parse_command(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut iter = args.into_iter();
    let _bin = iter.next();

    let Some(subcommand) = iter.next() else {
        return Err(ParseOutcome::Error("缺少子命令".to_string()));
    };

    if matches!(subcommand.as_str(), "--help" | "-h" | "help") {
        return Err(ParseOutcome::Help(usage()));
    }

    match subcommand.to_ascii_lowercase().as_str() {
        "smoke" => parse_smoke_args(iter.collect()),
        "live" => parse_live_subcommand(iter.collect()),
        other => Err(ParseOutcome::Error(format!("不支持的子命令：{other}"))),
    }
}

fn parse_smoke_args(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut forwarded = vec!["voice-input-linux-smoke".to_string()];
    forwarded.extend(args);
    let (audio_file, backend) =
        voice_input_linux::parse_audio_file_with_optional_backend_arg(
            forwarded,
            LinuxBackendKind::IBus,
            parse_backend_kind,
        )
        .map_err(|msg| {
            if msg == "help" {
                ParseOutcome::Help(usage())
            } else {
                ParseOutcome::Error(msg)
            }
        })?;
    Ok(Command::Smoke {
        audio_file,
        backend,
    })
}

fn parse_live_subcommand(args: Vec<String>) -> Result<Command, ParseOutcome> {
    let mut forwarded = vec!["voice-input-linux-live".to_string()];
    forwarded.extend(args);
    let live_args = parse_live_args(forwarded).map_err(|msg| {
        if msg == "help" {
            ParseOutcome::Help(usage())
        } else {
            ParseOutcome::Error(msg)
        }
    })?;
    Ok(Command::Live(live_args))
}

fn usage() -> String {
    concat!(
        "用法：cargo run -p voice-input-linux -- <smoke|live> [args]\n",
        "\n",
        "smoke: cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav [--backend ibus]\n",
        "live:  cargo run -p voice-input-linux --features ibus -- live [--backend ibus] [--activation-hotkey DoubleCtrl] [--double-ctrl-window-ms 300] [--silence-stop-ms 1500]\n",
    )
    .to_string()
}
```

- [ ] **Step 2: Update voice-input-linux/Cargo.toml — remove runtime dep**

In `crates/voice-input-linux/Cargo.toml`, remove the line:
```toml
voice-input-runtime = { path = "../voice-input-runtime" }
```

- [ ] **Step 3: Update workspace Cargo.toml**

Replace `Cargo.toml` (root):

```toml
[workspace]
members = [
    "crates/voice-input-core",
    "crates/voice-input-linux",
    "crates/voice-input-asr",
    "crates/voice-input-audio",
]
resolver = "2"
```

- [ ] **Step 4: Commit**

```bash
git add crates/voice-input-linux/src/main.rs \
        crates/voice-input-linux/Cargo.toml \
        Cargo.toml
git commit -m "refactor: unified CLI entry, remove runtime from deps, shrink workspace"
```

---

### Task 4: Update tests to remove voice-input-runtime references

**Files:**
- Modify: `crates/voice-input-linux/tests/session.rs`

- [ ] **Step 1: Update tests/session.rs**

In `crates/voice-input-linux/tests/session.rs`, line 9, change:
```rust
use voice_input_runtime::LocalVoiceInputConfig;
```
to:
```rust
use voice_input_linux::LocalVoiceInputConfig;
```

- [ ] **Step 2: Verify no other test files reference removed crates**

```bash
grep -rn "voice_input_runtime\|voice-input-runtime" crates/voice-input-linux/tests/
```

Expected: no output.

- [ ] **Step 3: Commit**

```bash
git add crates/voice-input-linux/tests/session.rs
git commit -m "test(voice-input-linux): update test imports to use local types"
```

---

### Task 5: Delete old crate directories

- [ ] **Step 1: Remove old crate directories**

```bash
rm -rf crates/voice-input-macos
rm -rf crates/voice-input-windows
rm -rf crates/voice-input-runtime
rm -rf crates/voice-input-cli
```

- [ ] **Step 2: Commit**

```bash
git add -A crates/
git commit -m "refactor: remove macos, windows, runtime, cli crates"
```

---

### Task 6: Update scripts/voiceinput.sh

**Files:**
- Modify: `scripts/voiceinput.sh`

- [ ] **Step 1: Remove macos and windows functions**

Delete the following functions from `scripts/voiceinput.sh`:
- `voiceinput_macos_smoke_impl` (lines 534-573)
- `voiceinput_windows_smoke_impl` (lines 640-693)
- `voiceinput_windows_install_impl` (lines 695-770)
- `voiceinput_macos_install_impl` (lines 772-854)
- `voiceinput_package_macos_impl` (lines 1054-1227)
- `voiceinput_dev_install_macos_impl` (lines 1229-1257)

- [ ] **Step 2: Update voiceinput_run_platform_smoke — remove macos/windows cases**

In `voiceinput_run_platform_smoke` (lines 176-199), replace:
```bash
voiceinput_run_platform_smoke() {
  local platform="$1"
  local audio_file="$2"
  local backend="${3:-ibus}"

  case "$platform" in
    macos)
      echo "正在运行 macOS smoke 验证"
      voiceinput_macos_smoke_impl --audio-file "$audio_file"
      ;;
    linux)
      echo "正在运行 Linux smoke"
      voiceinput_linux_smoke_impl --audio-file "$audio_file" --backend "$backend"
      ;;
    windows)
      echo "正在运行 Windows smoke 验证"
      voiceinput_windows_smoke_impl --audio-file "$audio_file"
      ;;
    *)
      echo "不支持的 smoke 平台：$platform" >&2
      exit 2
      ;;
  esac
}
```
with:
```bash
voiceinput_run_platform_smoke() {
  local platform="$1"
  local audio_file="$2"
  local backend="${3:-ibus}"

  case "$platform" in
    linux)
      echo "正在运行 Linux smoke"
      voiceinput_linux_smoke_impl --audio-file "$audio_file" --backend "$backend"
      ;;
    *)
      echo "不支持的 smoke 平台：$platform" >&2
      exit 2
      ;;
  esac
}
```

- [ ] **Step 3: Update voiceinput_run_platform_live — remove macos/windows cases**

In `voiceinput_run_platform_live` (lines 201-227), replace:
```bash
voiceinput_run_platform_live() {
  local platform="$1"
  local backend="${2:-ibus}"

  voiceinput_ensure_cargo
  voiceinput_ensure_uv

  case "$platform" in
    macos)
      echo "正在启动 macOS 常驻应用"
      voiceinput_run_cli_release live macos
      ;;
    linux)
      echo "正在启动 Linux 常驻托盘版"
      voiceinput_refresh_cargo_path
      voiceinput_run_cli_linux live linux --backend "$backend"
      ;;
    windows)
      echo "正在启动 Windows 常驻应用"
      voiceinput_run_cli_release live windows
      ;;
    *)
      echo "不支持的 live 平台：$platform" >&2
      exit 2
      ;;
  esac
}
```
with:
```bash
voiceinput_run_platform_live() {
  local platform="$1"
  local backend="${2:-ibus}"

  voiceinput_ensure_cargo
  voiceinput_ensure_uv

  case "$platform" in
    linux)
      echo "正在启动 Linux 常驻托盘版"
      voiceinput_refresh_cargo_path
      voiceinput_run_cli_linux live linux --backend "$backend"
      ;;
    *)
      echo "不支持的 live 平台：$platform" >&2
      exit 2
      ;;
  esac
}
```

- [ ] **Step 4: Update voiceinput_run_cli_linux — remove feature flag**

In `voiceinput_run_cli_linux` (lines 157-166), change `--features linux-ibus-smoke` to `--features ibus`:

```bash
voiceinput_run_cli_linux() {
  local cargo_bin
  cargo_bin="$(voiceinput_find_cargo_bin)"
  if [[ -z "$cargo_bin" ]]; then
    echo "未找到 cargo，可先执行 scripts/voiceinput.sh bootstrap" >&2
    exit 1
  fi

  uv run -- "$cargo_bin" run -p voice-input-linux --features ibus -- "$@"
}
```

(Note: this now uses `-p voice-input-linux` instead of `-p voice-input-cli`, and `--features ibus` instead of `--features linux-ibus-smoke`.)

- [ ] **Step 5: Update voiceinput_setup_linux_autostart — remove feature flag**

In `voiceinput_setup_linux_autostart` (around line 971), change:
```bash
cargo build -p voice-input-cli --features linux-ibus-smoke --release
```
to:
```bash
cargo build -p voice-input-linux --features ibus --release
```

And update the launcher script (around line 991) from:
```bash
exec "$REPO_ROOT/target/release/voice-input-cli" live linux --backend "$$BACKEND_PLACEHOLDER$$"
```
to:
```bash
exec "$REPO_ROOT/target/release/voice-input-linux" live --backend "$$BACKEND_PLACEHOLDER$$"
```

- [ ] **Step 6: Update voiceinput_linux_smoke_impl — fix feature flag**

In `voiceinput_linux_smoke_impl` (line 637), change:
```bash
voiceinput_run_cli_linux smoke linux --audio-file "$audio_file" --backend "$backend"
```
to:
```bash
voiceinput_run_cli_linux smoke --audio-file "$audio_file" --backend "$backend"
```

- [ ] **Step 7: Update voiceinput_bootstrap_impl — remove macos reference**

In `voiceinput_bootstrap_impl` (lines 524-527), replace:
```bash
  if [[ -n "$smoke_audio_file" ]]; then
    echo "正在运行 macOS smoke"
    uv run -- cargo run -p voice-input-macos --bin voice-input-macos -- --audio-file "$smoke_audio_file"
  fi
```
with:
```bash
  if [[ -n "$smoke_audio_file" ]]; then
    echo "正在运行 Linux smoke"
    uv run -- cargo run -p voice-input-linux --features ibus -- smoke --audio-file "$smoke_audio_file"
  fi
```

- [ ] **Step 8: Update usage() function — remove macos/windows entries**

In `usage()` (lines 1421-1449), remove the lines:
```
  macos install          ...
  macos package          ...
  macos smoke            ...
  macos dev-install      ...
  windows install        ...
  windows smoke          ...
```
And update any remaining references accordingly.

- [ ] **Step 9: Update top-level command dispatch — remove macos/windows**

In the command dispatch section (lines 1451-1515), remove:
- The platform detection logic for `macos` and `windows` (lines 1459-1468 handling)
- `macos-install)`, `macos-package)`, `macos-smoke)`, `macos-dev-install)` cases
- `windows-install)`, `windows-smoke)` cases

- [ ] **Step 10: Update voiceinput_run_cli — remove macos/windows from platform dispatch**

The `voiceinput_run_cli` function is used for smoke/live commands. After the removal of macos/windows commands, this function may have fewer callers. Keep it as-is since it's still used for `bootstrap` smoke.

- [ ] **Step 11: Commit**

```bash
git add scripts/voiceinput.sh
git commit -m "refactor(scripts): remove macos and windows support from voiceinput.sh"
```

---

### Task 7: Update README.md

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Rewrite README.md for Linux-only**

Replace `README.md`:

```markdown
# VoiceInput

Linux 语音输入法项目。

## 架构

- `voice-input-core`：纯业务状态机和 trait 定义
- `voice-input-asr`：ASR 配置、runner、transcriber
- `voice-input-audio`：文件录音、PCM/WAV 公共处理
- `voice-input-linux`：Linux 平台实现、CLI 入口、常驻托盘

## Linux 快速开始

1. Ubuntu 20.04 上先装 `build-essential`、`pkg-config`、`libdbus-1-dev`、`libibus-1.0-dev`、`python3`、`python3-venv`、`python3-pip`
2. 如果要让 Rust 侧录音后端也可用，再补 `libasound2-dev` 和 `portaudio19-dev`
3. 如果要用 Linux 全局热键监听，再补 `libx11-dev`
4. `scripts/voiceinput.sh bootstrap`
5. `cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus`
6. `scripts/voiceinput.sh linux install`
7. Linux 默认热键是双击 Ctrl
8. 如果要切模型，可以加 `--model qwen` 或 `--model qwen-0.6b`
9. `--backend` 只影响 Linux 宿主后端
10. 常驻版也可直接走 `cargo run -p voice-input-linux --features ibus -- live --backend ibus`

## 命令入口

- `cargo run -p voice-input-linux --features ibus -- <smoke|live> [args]`：统一 CLI 入口
- `scripts/voiceinput.sh ...`：环境准备、模型部署、安装/常驻入口

## Smoke 流程

```bash
cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus
```

## Live 流程

```bash
cargo run -p voice-input-linux --features ibus -- live --backend ibus
```

可额外传 `--double-ctrl-window-ms` 和 `--silence-stop-ms`。

## 脚本入口

- `scripts/voiceinput.sh`：统一入口
- `config/models.json`：模型 catalog 单一来源
- `config/voiceinput.env`：由 catalog 生成的仓库级默认配置
- `scripts/voiceinput_config.sh`：共享 helper

如果要切默认模型：

```bash
scripts/voiceinput.sh model <funasr|qwen|qwen-0.6b>
```

## Python 环境

1. `uv venv .venv`
2. `uv pip install -r scripts/requirements-asr-base.txt`
3. `uv pip install -r scripts/requirements-asr-runtime.txt`
4. `source .venv/bin/activate`
5. 或者直接使用 `uv run`
6. `scripts/voiceinput.sh bootstrap`
7. 如果要切模型，可以传入 `--model qwen` 或 `--model qwen-0.6b`
8. 如果同时想跑 smoke，可以传入 `--audio-file testdata/smoke.wav`

## 模型部署

1. `scripts/voiceinput.sh bootstrap`
2. `scripts/voiceinput.sh bootstrap --audio-file testdata/smoke.wav`
3. 一键部署会先安装依赖，再下载模型

## Linux 安装

1. `scripts/voiceinput.sh linux install`
2. 如果要安装时切模型，可以传入 `--model qwen` 或 `--model qwen-0.6b`
3. 默认会设置 systemd 开机自启，可使用 `--no-autostart` 跳过
4. `scripts/voiceinput.sh linux uninstall` 可移除常驻版及开机自启

日常调试链路：

1. 改代码
2. 运行 `scripts/voiceinput.sh linux install`
3. 直接在前台应用里验证输入效果
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: update README for Linux-only project"
```

---

### Task 8: Build and test

- [ ] **Step 1: Build check (no ibus feature)**

```bash
cd /home/lab/Projects/VoiceInput && cargo check -p voice-input-linux 2>&1
```

Expected: Compiles successfully (or errors that need fixing).

- [ ] **Step 2: Build check (with ibus feature)**

```bash
cd /home/lab/Projects/VoiceInput && cargo check -p voice-input-linux --features ibus 2>&1
```

Expected: Compiles successfully.

- [ ] **Step 3: Run tests**

```bash
cd /home/lab/Projects/VoiceInput && cargo test -p voice-input-linux --features ibus 2>&1
```

Expected: All tests pass.

- [ ] **Step 4: Run full workspace tests**

```bash
cd /home/lab/Projects/VoiceInput && cargo test --features ibus 2>&1
```

Expected: All workspace tests pass.

- [ ] **Step 5: Run smoke test**

```bash
cd /home/lab/Projects/VoiceInput && cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus 2>&1
```

Expected: Smoke test runs and prints recognition result.

- [ ] **Step 6: Fix any issues and commit**

```bash
git add -A
git commit -m "fix: build and test fixes for Linux-only refactor"
```

---

### Non-requirements / Out of scope

- No changes to `voice-input-core`, `voice-input-asr`, `voice-input-audio` public APIs
- No changes to FunASR/Python deployment
- No behavior changes to hotkey, recording, text injection, or IBus backend
- Config files (`config/models.json`, `config/voiceinput.env`) unchanged
- `testdata/smoke.wav` unchanged
