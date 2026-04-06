use std::path::PathBuf;

use voice_input_core::{MockHotkeyManager, VoiceInputError};
use voice_input_runtime::build_local_python_runtime_config;

use crate::{
    FileAudioRecorder, LinuxBackend, LinuxBackendKind, LinuxHostConfig, LinuxLocalVoiceInput,
    LinuxLocalVoiceInputConfig,
};

pub fn run_smoke(audio_path: PathBuf, backend_kind: LinuxBackendKind) -> Result<(), String> {
    let backend = build_backend(backend_kind).map_err(|err| err.to_string())?;
    let (runtime_config, asr_runner) =
        build_local_python_runtime_config().map_err(|err| format!("预加载 ASR 模型失败：{err}"))?;

    let pipeline = LinuxLocalVoiceInput::new(
        LinuxLocalVoiceInputConfig {
            runtime: runtime_config,
            host: LinuxHostConfig {
                backend: backend_kind,
                service_name: "voice-input".to_string(),
            },
        },
        Box::new(MockHotkeyManager),
        Box::new(FileAudioRecorder::new(audio_path)),
        asr_runner,
        backend,
    );

    let text = pipeline
        .run_once()
        .map_err(|err| format!("Linux 本地管线失败：{err}"))?;

    println!("识别结果：{text}");
    println!("Linux 后端：{:?}", pipeline.backend_kind());
    println!("服务名：{}", pipeline.service_name());
    Ok(())
}

fn build_backend(kind: LinuxBackendKind) -> Result<Box<dyn LinuxBackend>, VoiceInputError> {
    match kind {
        LinuxBackendKind::IBus => build_ibus_backend(),
        LinuxBackendKind::Fcitx5 => Err(VoiceInputError::Injection(
            "Fcitx5 路径还没有接入原生绑定，请先使用 --backend ibus".to_string(),
        )),
    }
}

#[cfg(feature = "ibus")]
fn build_ibus_backend() -> Result<Box<dyn LinuxBackend>, VoiceInputError> {
    Ok(Box::new(crate::IbusBackend::new(
        crate::IbusEngineSpec::default(),
    )))
}

#[cfg(not(feature = "ibus"))]
fn build_ibus_backend() -> Result<Box<dyn LinuxBackend>, VoiceInputError> {
    Err(VoiceInputError::Injection(
        "当前构建未启用 IBus 支持，请使用 `cargo run -p voice-input-cli --features linux-ibus-smoke -- smoke linux --audio-file ... --backend ibus`"
            .to_string(),
    ))
}
