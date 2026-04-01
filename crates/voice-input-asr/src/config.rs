use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunAsrConfig {
    pub source_url: String,
    pub model_dir: PathBuf,
    pub remote_code: PathBuf,
    pub device: String,
    pub language: String,
    pub itn: bool,
    pub hotwords: Vec<String>,
}

impl Default for FunAsrConfig {
    fn default() -> Self {
        Self {
            source_url: "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
                .to_string(),
            model_dir: PathBuf::from("./models/FunAudioLLM/Fun-ASR-Nano-2512"),
            remote_code: PathBuf::from("model.py"),
            device: "auto".to_string(),
            language: "中文".to_string(),
            itn: true,
            hotwords: Vec::new(),
        }
    }
}
