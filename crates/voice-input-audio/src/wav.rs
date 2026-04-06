use std::io::Cursor;

use voice_input_core::{Result, VoiceInputError};

pub fn write_pcm_wav(samples: &[i16], sample_rate: u32) -> Result<Vec<u8>> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| VoiceInputError::Audio(format!("创建 WAV writer 失败：{e}")))?;
        for sample in samples {
            writer
                .write_sample(*sample)
                .map_err(|e| VoiceInputError::Audio(format!("写入 WAV 采样失败：{e}")))?;
        }
        writer
            .finalize()
            .map_err(|e| VoiceInputError::Audio(format!("完成 WAV 写入失败：{e}")))?;
    }

    Ok(cursor.into_inner())
}
