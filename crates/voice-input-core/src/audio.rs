/// 已录制的单声道 PCM 音频。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedAudio {
    pub samples: Vec<i16>,
    pub sample_rate: u32,
}
