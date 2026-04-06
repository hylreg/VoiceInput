use std::path::PathBuf;

use voice_input_asr::{FunAsrConfig, FunAsrRunner, LocalFunAsrTranscriber, PythonFunAsrRunner};
use voice_input_core::{AppConfig, AppController, AudioRecorder, HotkeyManager, InputMethodHost};

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

pub trait LocalRuntimeMetadata {
    fn label(&self) -> &str;
}

pub struct LocalVoiceInputRuntime<M> {
    controller: AppController,
    metadata: M,
}

impl<M> LocalVoiceInputRuntime<M> {
    pub fn new(
        config: LocalVoiceInputConfig,
        hotkeys: Box<dyn HotkeyManager>,
        recorder: Box<dyn AudioRecorder>,
        runner: Box<dyn FunAsrRunner>,
        host: Box<dyn InputMethodHost>,
        metadata: M,
    ) -> Self {
        let transcriber = LocalFunAsrTranscriber::new(config.asr, runner);
        let controller =
            AppController::new(config.app, hotkeys, recorder, Box::new(transcriber), host);

        Self {
            controller,
            metadata,
        }
    }

    pub fn controller(&self) -> &AppController {
        &self.controller
    }

    pub fn run_once(&self) -> voice_input_core::Result<String> {
        self.controller.run_demo()
    }

    pub fn metadata(&self) -> &M {
        &self.metadata
    }
}
