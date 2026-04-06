use std::sync::{Arc, Mutex};

pub fn push_mono_i16_f32(data: &[f32], channels: usize, sink: &Arc<Mutex<Vec<i16>>>) {
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

pub fn push_mono_i16_i16(data: &[i16], channels: usize, sink: &Arc<Mutex<Vec<i16>>>) {
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

pub fn push_mono_i16_u16(data: &[u16], channels: usize, sink: &Arc<Mutex<Vec<i16>>>) {
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
