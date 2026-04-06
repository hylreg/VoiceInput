use std::path::PathBuf;

use voice_input_core::MockHotkeyManager;
use voice_input_runtime::build_local_python_runtime_config;

use crate::{
    FileAudioRecorder, MacHostConfig, MacLocalVoiceInput, MacLocalVoiceInputConfig,
    MockMacImeBridge,
};

pub fn run_smoke(audio_path: PathBuf) -> Result<(), String> {
    let bridge = MockMacImeBridge::default();
    let bridge_for_output = bridge.clone();
    let (runtime_config, asr_runner) =
        build_local_python_runtime_config().map_err(|err| format!("预加载 ASR 模型失败：{err}"))?;

    let pipeline = MacLocalVoiceInput::new(
        MacLocalVoiceInputConfig {
            runtime: runtime_config,
            host: MacHostConfig::default(),
        },
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(audio_path)),
        asr_runner,
        Box::new(bridge),
    );

    let text = pipeline
        .run_once()
        .map_err(|err| format!("macOS 本地管线失败：{err}"))?;

    println!("识别结果：{text}");
    println!("应用标识：{}", pipeline.host_bundle_id());
    let events = bridge_for_output
        .events()
        .into_iter()
        .map(|event| event.to_string())
        .collect::<Vec<_>>()
        .join("，");
    println!("输入法事件：{events}");
    Ok(())
}
