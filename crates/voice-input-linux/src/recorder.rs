use std::time::Duration;

use std::sync::atomic::AtomicBool;

#[cfg(target_os = "linux")]
use std::sync::atomic::Ordering;
#[cfg(not(target_os = "linux"))]
use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::{Arc, Condvar, Mutex};
#[cfg(target_os = "linux")]
use std::time::Instant;

pub use voice_input_audio::FileAudioRecorder;
use voice_input_core::{AudioRecorder, Result, VoiceInputError};

#[cfg(target_os = "linux")]
use voice_input_audio::{
    has_voice_activity, push_mono_i16_f32, push_mono_i16_i16, push_mono_i16_u16, write_pcm_wav,
};

#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
pub struct LinuxMicAudioRecorder {
    inner: Arc<LinuxMicAudioRecorderInner>,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct LinuxMicAudioRecorderInner {
    recording: AtomicBool,
    stop_requested: AtomicBool,
    gate: (Mutex<()>, Condvar),
    max_duration: Duration,
}

#[cfg(target_os = "linux")]
impl Default for LinuxMicAudioRecorder {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

#[cfg(target_os = "linux")]
impl LinuxMicAudioRecorder {
    pub fn new(max_duration: Duration) -> Self {
        Self {
            inner: Arc::new(LinuxMicAudioRecorderInner {
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

    pub fn record_once_with_chunks<F>(
        &self,
        chunk_interval: Duration,
        silence_stop_timeout: Duration,
        silence_stop_enabled: Arc<AtomicBool>,
        mut on_snapshot: F,
    ) -> Result<Vec<u8>>
    where
        F: FnMut(u32, Vec<i16>, bool),
    {
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
        let mut last_chunk_len = 0usize;
        let mut last_chunk_at = Instant::now();
        let mut last_voice_at = Instant::now();
        let mut saw_voice_activity = false;

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

            if !chunk_interval.is_zero() && last_chunk_at.elapsed() >= chunk_interval {
                if let Ok(current) = samples.lock() {
                    if current.len() > last_chunk_len {
                        let new_samples = &current[last_chunk_len..];
                        if has_voice_activity(new_samples) {
                            saw_voice_activity = true;
                            last_voice_at = Instant::now();
                        }

                        last_chunk_len = current.len();
                        on_snapshot(sample_rate, new_samples.to_vec(), false);
                    }
                }
                last_chunk_at = Instant::now();
            }

            if !silence_stop_timeout.is_zero()
                && silence_stop_enabled.load(Ordering::SeqCst)
                && saw_voice_activity
                && last_voice_at.elapsed() >= silence_stop_timeout
            {
                eprintln!(
                    "检测到持续静音，自动结束录音（{}ms）...",
                    silence_stop_timeout.as_millis()
                );
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

        if captured.len() > last_chunk_len {
            on_snapshot(sample_rate, captured[last_chunk_len..].to_vec(), true);
        } else {
            on_snapshot(sample_rate, Vec::new(), true);
        }

        if captured.is_empty() {
            return Err(VoiceInputError::Audio("没有录到有效音频".to_string()));
        }

        let duration_secs = captured.len() as f32 / sample_rate as f32;
        println!(
            "录音完成：{} 个采样，约 {:.2} 秒",
            captured.len(),
            duration_secs
        );

        write_pcm_wav(&captured, sample_rate)
    }
}

#[cfg(target_os = "linux")]
impl AudioRecorder for LinuxMicAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        self.record_once_with_chunks(
            Duration::from_millis(0),
            Duration::from_millis(0),
            Arc::new(AtomicBool::new(true)),
            |_, _, _| {},
        )
    }
}

#[cfg(not(target_os = "linux"))]
#[derive(Debug, Clone)]
pub struct LinuxMicAudioRecorder;

#[cfg(not(target_os = "linux"))]
impl LinuxMicAudioRecorder {
    pub fn new(_max_duration: Duration) -> Self {
        Self
    }

    pub fn stop(&self) {}

    pub fn is_recording(&self) -> bool {
        false
    }

    pub fn record_once_with_chunks<F>(
        &self,
        _chunk_interval: Duration,
        _silence_stop_timeout: Duration,
        _silence_stop_enabled: Arc<AtomicBool>,
        _on_snapshot: F,
    ) -> Result<Vec<u8>>
    where
        F: FnMut(u32, Vec<i16>, bool),
    {
        Err(VoiceInputError::Audio(
            "Linux 麦克风录音只支持 Linux".to_string(),
        ))
    }
}

#[cfg(not(target_os = "linux"))]
impl AudioRecorder for LinuxMicAudioRecorder {
    fn record_once(&self) -> Result<Vec<u8>> {
        Err(VoiceInputError::Audio(
            "Linux 麦克风录音只支持 Linux".to_string(),
        ))
    }
}
