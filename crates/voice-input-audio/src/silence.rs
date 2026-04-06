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
