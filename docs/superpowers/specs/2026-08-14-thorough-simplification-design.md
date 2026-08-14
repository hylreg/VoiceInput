# VoiceInput 彻底精简设计

日期：2026-08-14

## 目标

在外部行为（CLI 参数、环境变量、默认值、systemd 安装/托盘流程）不变的前提下，删除死代码、坍缩冗余抽象层、合并双流水线。4 个 crate 布局保持不变。

预期效果：删除约 2000 行（当前约 6486 行 → 约 4500 行），三层 IME 抽象变一层，模型配置单一来源。

## 各 crate 变更

### voice-input-core：大幅瘦身

- 删除 `CompositionState`、`DictationEvent`、`TextInjector`、`MockTextInjector`、`InsertionMode`、`TranscriptionMode`（全部零引用）
- `AppConfig` 只保留 `activation_hotkey` 字段
- 删除 `AppController` + `controller.rs` 整个文件（双流水线合并后 smoke 不再走控制器）
- 删除 `HotkeyManager` trait + `MockHotkeyManager`
- 保留 `AudioRecorder`、`Transcriber`、`InputMethodHost` trait、`VoiceInputError`、`MockTranscriber`
- 删除 `Transcript` 类型（`partials` 无消费者，live 从不更新预编辑）：`Transcriber::transcribe` 直接返回 `Result<String>`
- 删除 `MockAudioRecorder`、`MockInputMethodHost`（控制器删除后无消费者）
- `InputMethodHost` trait 删除 `update_preedit`、`show_recording_indicator`、`clear_recording_indicator`（零消费者），只剩 start/commit/cancel/end 四个方法
- 新增 `RecordedAudio { samples: Vec<i16>, sample_rate: u32 }`，`AudioRecorder::record_once` 改返回它（统一 PCM 形态，消除 WAV 头扫描）

### voice-input-audio：统一音频形态

- recorder 直接返回 PCM，VAD 全部在 PCM 上做
- runtime.rs 的 `wav_has_voice_activity`（峰值 > 800）移入本 crate，**保持峰值语义不变**（不与 RMS 的 `has_voice_activity` 混淆）
- `file.rs`：`FileAudioRecorder` 改用 hound 解码 WAV 为 `RecordedAudio`（smoke 输入统一为 PCM + rate）

### voice-input-asr：单一 catalog + 单一 worker 协议

- 删除一次性 Python 脚本 `PYTHON_SCRIPT`、`PYTHON_QWEN_SCRIPT` 及对应一次性分支、手写 JSON 转义的 `serde_json_like_array`（改用 `serde_json::to_string`）
- 模型 catalog 单一来源：`include_str!("../../../config/models.json")` 编译期嵌入；删除 Rust 内置 JSON 字面量、运行时向上搜索 `config_catalog_path`、三个 default 构造函数里重复的 fallback 字面量
- 删除 `LocalFunAsrTranscriber::config()` 死 accessor
- 空转写错误消息去 FunASR 化措辞（qwen 后端也适用）

### voice-input-linux：坍缩三层 IME 抽象 + 双流水线合并

- 删除 `backend.rs` 整个文件：`LinuxBackend` trait、`Fcitx5Backend` 桩、`MockLinuxBackend`、`LinuxBackendKind` enum。`--backend` CLI 参数保留（只接受 `ibus`，其他值报错后丢弃）
- 删除 `IbusEngineBridge` trait、`IbusClientBridge`、`UnwiredIbusBridge`、`MockIbusBridge`、`IbusEngineEvent`、`IbusEngineSpec`（engine_name/object_path 从未被读取）
- `host.rs` 重写：`LinuxInputMethodHost` 直接实现 core 的 `InputMethodHost`——`commit_text` 直接调 xdotool 注入，其余方法 no-op（即现在 IbusClientBridge 的真实行为）
- `ibus.rs` 精简为注入工具模块：只留 `insert_text_into_active_window`、`insert_indicator_and_save_clipboard`、`backspace_in_active_window`、debug 宏
- `ClipboardRestoreGuard` 删除 `saved` 字段和空 `Drop` impl（`saved` 从未被读取，剪贴板已在函数内同步还原）
- 删除 `ibus` optional crate 依赖（代码从未 `use ibus::`）；feature 名保留（`--features ibus` 在脚本/README 中大量出现，只当作 cfg 开关）
- 双流水线合并：`local.rs` 的 `LinuxLocalVoiceInput`/`LocalVoiceInputConfig`/`LinuxLocalVoiceInputConfig`/`build_local_python_runtime_config` 全删；`smoke.rs` 直接写流程（读 WAV → 解码 PCM → 转录 → host.commit_text），保持原行为（smoke 仍会注入活动窗口）
- `runtime.rs`：`wav_has_voice_activity` 移除，录音周期改为 PCM VAD → `write_pcm_wav` → 转录；未使用的 `_job` 参数删除
- `hotkey.rs`：`LinuxHotkeySpec` 改为 `HotkeyKind::DoublePress(Modifier) | Combo { key, mods }` enum；DoubleCtrl/DoubleAlt 两段几乎相同的监听循环统一为双击修饰键单一实现；删除冗余别名解析分支
- `LinuxHostConfig` 扁平化：删掉只剩 `service_name` 的 struct，改为直接传 `String`
- 删除死代码：`parse_required_audio_file_arg`、`print_live_usage`、`LinuxTrayHandle::request_quit`、`capture_active_window`；`live_cli.rs` 里重复的 `parse_backend` 合并

### scripts 与文档

- 删除 `scripts/funasr_stream_server.py` + voiceinput.sh 的 `linux dev`/`linux dev-streaming` 子命令（设置的环境变量 Rust 端从不读取）
- `bootstrap --audio-file` 分支复用 `voiceinput_linux_smoke_impl`，删掉内联重复
- README：修正「Linux 默认热键是双击 Ctrl」→ 双击 Alt；`--double-ctrl-window-ms` → `--double-press-window-ms`；合并重复的 Smoke/Live/Python 环境章节
- main.rs usage 字符串同步修正

## 测试策略

- 删除：`core/tests/controller.rs`（控制器没了）、`session.rs` 中测转发层的测试（转发层没了）
- 保留适配：hotkey 解析测试、live 状态测试、funasr worker 协议测试、asr 配置测试（catalog 嵌入后断言不变，`Transcript` 断言改为 `String`）
- smoke 拆出可测的 `transcribe_file` 纯函数（host 提交部分不做单测）

## 验证

- `cargo build` + `cargo test` + `cargo clippy` 全绿
- 若 `.venv`/models 就绪，跑真实 smoke：`cargo run -p voice-input-linux --features ibus -- smoke --audio-file testdata/smoke.wav --backend ibus`

## 明确不做的

- 不合并 crate（保持 4 crate 布局）
- `uninstall` 不增加清理编译产物的 `--purge` 选项（行为增强，非精简）
- 不改变任何默认值、CLI 参数、环境变量语义
