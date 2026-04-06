use std::path::PathBuf;

use voice_input_core::MockHotkeyManager;
use voice_input_runtime::build_local_python_runtime_config;

use crate::{
    ClipboardWindowsImeBridge, FileAudioRecorder, WindowsHostConfig, WindowsLocalVoiceInput,
    WindowsLocalVoiceInputConfig,
};

pub fn run_smoke(audio_path: PathBuf) -> Result<(), String> {
    let (runtime_config, asr_runner) =
        build_local_python_runtime_config().map_err(|err| format!("预加载 ASR 模型失败：{err}"))?;

    let pipeline = WindowsLocalVoiceInput::new(
        WindowsLocalVoiceInputConfig {
            runtime: runtime_config,
            host: WindowsHostConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(audio_path)),
        asr_runner,
        Box::new(ClipboardWindowsImeBridge),
    );

    let text = pipeline
        .run_once()
        .map_err(|err| format!("Windows 本地管线失败：{err}"))?;

    println!("识别结果：{text}");
    println!("Windows App ID：{}", pipeline.app_id());
    println!("提交方式：Windows Unicode 注入，失败时回退剪贴板粘贴");
    Ok(())
}
