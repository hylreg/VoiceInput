use std::path::{Path, PathBuf};

use voice_input_asr::{FunAsrConfig, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_audio::write_pcm_wav;
use voice_input_core::{AudioRecorder, InputMethodHost, Transcriber};

use crate::host::LinuxInputMethodHost;
use crate::recorder::FileAudioRecorder;

/// 读取 WAV 文件并转写，返回识别文本。逻辑与提交解耦，便于测试。
pub fn transcribe_file(
    audio_path: &Path,
    transcriber: &LocalFunAsrTranscriber,
) -> voice_input_core::Result<String> {
    let audio = FileAudioRecorder::new(audio_path).record_once()?;
    let wav = write_pcm_wav(&audio.samples, audio.sample_rate)?;
    transcriber.transcribe(&wav)
}

pub fn run_smoke(audio_path: PathBuf) -> Result<(), String> {
    if cfg!(not(feature = "ibus")) {
        return Err("当前构建未启用 IBus 支持，请使用 --features ibus 重新构建".to_string());
    }

    let asr = FunAsrConfig::from_env();
    let runner = PythonFunAsrRunner::connect(asr.clone())
        .map_err(|err| format!("预加载 ASR 模型失败：{err}"))?;
    let transcriber = LocalFunAsrTranscriber::new(asr, Box::new(runner));

    let text = transcribe_file(&audio_path, &transcriber)
        .map_err(|err| format!("Linux smoke 管线失败：{err}"))?;

    let host = LinuxInputMethodHost::new();
    host.start_composition()
        .map_err(|err| format!("开始输入失败：{err}"))?;
    host.commit_text(&text)
        .map_err(|err| format!("提交文本失败：{err}"))?;
    host.end_composition()
        .map_err(|err| format!("结束输入失败：{err}"))?;

    println!("识别结果：{text}");
    Ok(())
}
