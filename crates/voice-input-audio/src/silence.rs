pub fn has_voice_activity(samples: &[i16]) -> bool {
    const RMS_THRESHOLD: f64 = 450.0;
    if samples.is_empty() {
        return false;
    }

    let energy = samples
        .iter()
        .map(|sample| {
            let value = i64::from(*sample);
            value * value
        })
        .sum::<i64>() as f64
        / samples.len() as f64;

    energy.sqrt() >= RMS_THRESHOLD
}

/// 峰值检测：任一采样绝对值超过阈值即认为有语音。
/// 与 has_voice_activity 的 RMS 语义不同——峰值对单个尖峰更敏感，
/// 用于整段录音的最终语音判定（对抗 AGC 底噪）。
pub fn has_peak_above(samples: &[i16], threshold: i16) -> bool {
    samples.iter().any(|s| s.abs() > threshold)
}

#[cfg(test)]
mod tests {
    use super::{has_peak_above, has_voice_activity};

    #[test]
    fn peak_detection_flags_loud_samples() {
        assert!(has_peak_above(&[0, 900, 0], 800));
        assert!(!has_peak_above(&[0, 800, 0], 800));
        assert!(!has_peak_above(&[], 800));
    }

    #[test]
    fn rms_detection_flags_sustained_energy() {
        assert!(has_voice_activity(&[500; 100]));
        assert!(!has_voice_activity(&[10; 100]));
    }
}
