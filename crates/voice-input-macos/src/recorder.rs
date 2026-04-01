use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use voice_input_core::{AudioRecorder, Result, VoiceInputError};

#[derive(Debug, Clone)]
pub struct FileAudioRecorder {
    path: std::path::PathBuf,
}

impl FileAudioRecorder {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &std::path::PathBuf {
        &self.path
    }
}

impl AudioRecorder for FileAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        std::fs::read(&self.path).map_err(|e| {
            VoiceInputError::Audio(format!("读取音频文件失败 {}：{e}", self.path.display()))
        })
    }
}

#[cfg(target_os = "macos")]
#[derive(Clone)]
pub struct MicAudioRecorder {
    inner: Arc<MicAudioRecorderInner>,
}

#[cfg(target_os = "macos")]
struct MicAudioRecorderInner {
    recording: AtomicBool,
    stop_requested: AtomicBool,
    gate: (Mutex<()>, Condvar),
    max_duration: Duration,
}

#[cfg(target_os = "macos")]
impl Default for MicAudioRecorder {
    fn default() -> Self {
        Self::new(Duration::from_secs(12))
    }
}

#[cfg(target_os = "macos")]
impl MicAudioRecorder {
    pub fn new(max_duration: Duration) -> Self {
        Self {
            inner: Arc::new(MicAudioRecorderInner {
                recording: AtomicBool::new(false),
                stop_requested: AtomicBool::new(false),
                gate: (Mutex::new(()), Condvar::new()),
                max_duration,
            }),
        }
    }

    pub fn stop(&self) {
        self.inner.stop_requested.store(true, Ordering::SeqCst);
        self.inner.gate.1.notify_all();
    }

    pub fn is_recording(&self) -> bool {
        self.inner.recording.load(Ordering::SeqCst)
    }

    fn finish(&self) {
        self.inner.recording.store(false, Ordering::SeqCst);
        self.inner.stop_requested.store(false, Ordering::SeqCst);
        self.inner.gate.1.notify_all();
    }
}

#[cfg(target_os = "macos")]
impl AudioRecorder for MicAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

        if self
            .inner
            .recording
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(VoiceInputError::Audio("当前已经在录音".to_string()));
        }

        self.inner.stop_requested.store(false, Ordering::SeqCst);

        let host = cpal::default_host();
        let device = host.default_input_device().ok_or_else(|| {
            self.finish();
            VoiceInputError::Audio("未找到默认麦克风输入设备".to_string())
        })?;

        let supported_config = device.default_input_config().map_err(|e| {
            self.finish();
            VoiceInputError::Audio(format!("读取默认输入配置失败：{e}"))
        })?;

        let stream_config: cpal::StreamConfig = supported_config.clone().into();
        let sample_rate = stream_config.sample_rate.0;
        let channels = usize::from(stream_config.channels.max(1));
        let samples = Arc::new(Mutex::new(Vec::<i16>::new()));
        let samples_for_stream = samples.clone();

        let stream = match supported_config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        push_mono_i16_f32(data, channels, &samples_for_stream);
                    },
                    move |err| {
                        eprintln!("麦克风录音错误：{err}");
                    },
                    None,
                )
                .map_err(|e| VoiceInputError::Audio(format!("创建 F32 录音流失败：{e}")))?,
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        push_mono_i16_i16(data, channels, &samples_for_stream);
                    },
                    move |err| {
                        eprintln!("麦克风录音错误：{err}");
                    },
                    None,
                )
                .map_err(|e| VoiceInputError::Audio(format!("创建 I16 录音流失败：{e}")))?,
            cpal::SampleFormat::U16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| {
                        push_mono_i16_u16(data, channels, &samples_for_stream);
                    },
                    move |err| {
                        eprintln!("麦克风录音错误：{err}");
                    },
                    None,
                )
                .map_err(|e| VoiceInputError::Audio(format!("创建 U16 录音流失败：{e}")))?,
            other => {
                self.finish();
                return Err(VoiceInputError::Audio(format!(
                    "不支持的麦克风采样格式：{other:?}"
                )));
            }
        };

        if let Err(e) = stream.play() {
            self.finish();
            return Err(VoiceInputError::Audio(format!("启动麦克风录音失败：{e}")));
        }

        let start = Instant::now();
        while !self.inner.stop_requested.load(Ordering::SeqCst) {
            if start.elapsed() >= self.inner.max_duration {
                self.stop();
                break;
            }

            let guard = self.inner.gate.0.lock().map_err(|_| {
                self.finish();
                VoiceInputError::Audio("等待停止信号失败".to_string())
            })?;

            if self
                .inner
                .gate
                .1
                .wait_timeout(guard, Duration::from_millis(100))
                .is_err()
            {
                self.finish();
                return Err(VoiceInputError::Audio("等待录音停止信号失败".to_string()));
            }
        }

        drop(stream);

        let captured = samples
            .lock()
            .map_err(|_| {
                self.finish();
                VoiceInputError::Audio("读取录音缓存失败".to_string())
            })?
            .clone();

        self.finish();

        if captured.is_empty() {
            return Err(VoiceInputError::Audio("没有录到有效音频".to_string()));
        }

        Ok(write_pcm_wav(&captured, sample_rate))
    }
}

#[cfg(target_os = "macos")]
fn push_mono_i16_f32(data: &[f32], channels: usize, sink: &Arc<Mutex<Vec<i16>>>) {
    if channels == 0 {
        return;
    }

    if let Ok(mut buffer) = sink.lock() {
        for frame in data.chunks(channels) {
            let sum = frame.iter().copied().sum::<f32>();
            let mono = (sum / frame.len() as f32).clamp(-1.0, 1.0);
            buffer.push((mono * i16::MAX as f32) as i16);
        }
    }
}

#[cfg(target_os = "macos")]
fn push_mono_i16_i16(data: &[i16], channels: usize, sink: &Arc<Mutex<Vec<i16>>>) {
    if channels == 0 {
        return;
    }

    if let Ok(mut buffer) = sink.lock() {
        for frame in data.chunks(channels) {
            let sum = frame.iter().map(|sample| i32::from(*sample)).sum::<i32>();
            let mono = (sum / frame.len() as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            buffer.push(mono);
        }
    }
}

#[cfg(target_os = "macos")]
fn push_mono_i16_u16(data: &[u16], channels: usize, sink: &Arc<Mutex<Vec<i16>>>) {
    if channels == 0 {
        return;
    }

    if let Ok(mut buffer) = sink.lock() {
        for frame in data.chunks(channels) {
            let sum = frame.iter().map(|sample| i32::from(*sample)).sum::<i32>();
            let avg = sum / frame.len() as i32;
            let mono = (avg - 32768).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            buffer.push(mono);
        }
    }
}

#[cfg(target_os = "macos")]
fn write_pcm_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let mut output = Vec::with_capacity(44 + samples.len() * 2);
    let data_bytes = (samples.len() * 2) as u32;
    let byte_rate = sample_rate * 2;
    let block_align = 2u16;

    output.extend_from_slice(b"RIFF");
    output.extend_from_slice(&(36 + data_bytes).to_le_bytes());
    output.extend_from_slice(b"WAVE");
    output.extend_from_slice(b"fmt ");
    output.extend_from_slice(&16u32.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&sample_rate.to_le_bytes());
    output.extend_from_slice(&byte_rate.to_le_bytes());
    output.extend_from_slice(&block_align.to_le_bytes());
    output.extend_from_slice(&16u16.to_le_bytes());
    output.extend_from_slice(b"data");
    output.extend_from_slice(&data_bytes.to_le_bytes());

    for sample in samples {
        output.write_all(&sample.to_le_bytes()).expect("写入 wav");
    }

    output
}

#[cfg(not(target_os = "macos"))]
#[derive(Clone, Default)]
pub struct MicAudioRecorder;

#[cfg(not(target_os = "macos"))]
impl MicAudioRecorder {
    pub fn stop(&self) {}

    pub fn is_recording(&self) -> bool {
        false
    }
}

#[cfg(not(target_os = "macos"))]
impl AudioRecorder for MicAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        Err(VoiceInputError::Audio("麦克风录音只支持 macOS".to_string()))
    }
}
