use std::path::PathBuf;

use voice_input_core::MockHotkeyManager;

use crate::local::build_local_python_runtime_config;
use crate::{FileAudioRecorder, LinuxLocalVoiceInput};

pub fn run_smoke(audio_path: PathBuf) -> Result<(), String> {
    if cfg!(not(feature = "ibus")) {
        return Err("当前构建未启用 IBus 支持，请使用 --features ibus 重新构建".to_string());
    }

    let (runtime_config, asr_runner) =
        build_local_python_runtime_config().map_err(|err| format!("预加载 ASR 模型失败：{err}"))?;

    let pipeline = LinuxLocalVoiceInput::new(
        runtime_config,
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(audio_path)),
        asr_runner,
        "voice-input",
    );

    let text = pipeline
        .run_once()
        .map_err(|err| format!("Linux 本地管线失败：{err}"))?;

    println!("识别结果：{text}");
    println!("服务名：{}", pipeline.service_name());
    Ok(())
}
