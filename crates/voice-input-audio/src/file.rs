use std::path::PathBuf;

use voice_input_core::{AudioRecorder, RecordedAudio, Result, VoiceInputError};

#[derive(Debug, Clone)]
pub struct FileAudioRecorder {
    path: PathBuf,
}

impl FileAudioRecorder {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }
}

fn average_to_mono(samples: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return samples.to_vec();
    }

    samples
        .chunks(channels)
        .map(|frame| {
            let sum = frame.iter().map(|s| i32::from(*s)).sum::<i32>();
            (sum / frame.len() as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

impl AudioRecorder for FileAudioRecorder {
    fn record_once(&self) -> Result<RecordedAudio> {
        let reader = hound::WavReader::open(&self.path).map_err(|e| {
            VoiceInputError::Audio(format!("读取音频文件失败 {}：{e}", self.path.display()))
        })?;
        let spec = reader.spec();
        let sample_rate = spec.sample_rate;
        let channels = usize::from(spec.channels.max(1));

        let samples: Vec<i16> = match spec.sample_format {
            hound::SampleFormat::Int => reader
                .into_samples::<i16>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| VoiceInputError::Audio(format!("解析 WAV 采样失败：{e}")))?,
            hound::SampleFormat::Float => reader
                .into_samples::<f32>()
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| VoiceInputError::Audio(format!("解析 WAV 采样失败：{e}")))?
                .iter()
                .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect(),
        };

        Ok(RecordedAudio {
            samples: average_to_mono(&samples, channels),
            sample_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::FileAudioRecorder;
    use crate::wav::write_pcm_wav;
    use voice_input_core::AudioRecorder;

    #[test]
    fn decodes_pcm_wav_back_to_mono_samples() {
        let wav = write_pcm_wav(&[100, -100, 300], 16000).expect("write wav");
        let path = std::env::temp_dir().join(format!(
            "voiceinput-test-{}-decode.wav",
            std::process::id()
        ));
        std::fs::write(&path, &wav).expect("write temp wav");

        let recorder = FileAudioRecorder::new(&path);
        let audio = recorder.record_once().expect("decode wav");

        std::fs::remove_file(&path).ok();

        assert_eq!(audio.samples, vec![100, -100, 300]);
        assert_eq!(audio.sample_rate, 16000);
    }
}
