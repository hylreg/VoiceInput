use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrBackend {
    FunAsr,
    QwenAsr,
}

impl AsrBackend {
    pub fn from_model_id(model_id: &str) -> Self {
        let normalized = model_id.trim().to_ascii_lowercase();
        if normalized.contains("qwen/qwen3-asr") {
            Self::QwenAsr
        } else {
            Self::FunAsr
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FunAsr => "funasr",
            Self::QwenAsr => "qwen",
        }
    }
}

impl Default for AsrBackend {
    fn default() -> Self {
        Self::FunAsr
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ModelCatalog {
    aliases: std::collections::HashMap<String, String>,
    models: std::collections::HashMap<String, ModelSpec>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelSpec {
    backend: String,
    model_id: String,
    source_url: String,
    model_dir: String,
    #[serde(default)]
    remote_code: String,
}

static MODEL_CATALOG: OnceLock<ModelCatalog> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunAsrConfig {
    pub backend: AsrBackend,
    pub model_id: String,
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
        Self::funasr_default()
    }
}

impl FunAsrConfig {
    fn from_spec(spec: &ModelSpec) -> Self {
        Self {
            backend: match spec.backend.as_str() {
                "qwen" => AsrBackend::QwenAsr,
                _ => AsrBackend::FunAsr,
            },
            model_id: spec.model_id.clone(),
            source_url: spec.source_url.clone(),
            model_dir: PathBuf::from(&spec.model_dir),
            remote_code: PathBuf::from(&spec.remote_code),
            device: "auto".to_string(),
            language: "中文".to_string(),
            itn: true,
            hotwords: Vec::new(),
        }
    }

    fn builtin_catalog() -> ModelCatalog {
        let catalog_json = r#"{
            "aliases": {
                "fun": "funasr",
                "funasr": "funasr",
                "qwen": "qwen",
                "qwen3": "qwen",
                "qwen-asr": "qwen",
                "qwen-0.6b": "qwen-0.6b",
                "qwen0.6b": "qwen-0.6b",
                "qwen06": "qwen-0.6b",
                "qwen3-0.6b": "qwen-0.6b",
                "qwen3-asr-0.6b": "qwen-0.6b"
            },
            "models": {
                "funasr": {
                    "backend": "funasr",
                    "model_id": "FunAudioLLM/Fun-ASR-Nano-2512",
                    "source_url": "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512",
                    "model_dir": "./models/FunAudioLLM/Fun-ASR-Nano-2512",
                    "remote_code": "./models/FunAudioLLM/Fun-ASR-Nano-2512/model.py"
                },
                "qwen": {
                    "backend": "qwen",
                    "model_id": "Qwen/Qwen3-ASR-1.7B",
                    "source_url": "https://www.modelscope.cn/collections/Qwen/Qwen3-ASR",
                    "model_dir": "./models/Qwen/Qwen3-ASR-1.7B",
                    "remote_code": ""
                },
                "qwen-0.6b": {
                    "backend": "qwen",
                    "model_id": "Qwen/Qwen3-ASR-0.6B",
                    "source_url": "https://www.modelscope.cn/collections/Qwen/Qwen3-ASR",
                    "model_dir": "./models/Qwen/Qwen3-ASR-0.6B",
                    "remote_code": ""
                }
            }
        }"#;
        serde_json::from_str(catalog_json).expect("builtin model catalog should be valid")
    }

    fn config_catalog_path() -> Option<PathBuf> {
        let mut current = env::current_dir().ok()?;
        loop {
            let candidate = current.join("config/models.json");
            if candidate.exists() {
                return Some(candidate);
            }
            if !current.pop() {
                break;
            }
        }
        None
    }

    fn load_catalog() -> ModelCatalog {
        MODEL_CATALOG
            .get_or_init(|| {
                let Some(path) = Self::config_catalog_path() else {
                    return Self::builtin_catalog();
                };

                let Ok(content) = fs::read_to_string(path) else {
                    return Self::builtin_catalog();
                };

                serde_json::from_str(&content).unwrap_or_else(|_| Self::builtin_catalog())
            })
            .clone()
    }

    fn model_spec_by_alias(alias: &str) -> Option<ModelSpec> {
        let catalog = Self::load_catalog();
        let normalized = catalog.aliases.get(&alias.trim().to_ascii_lowercase())?;
        catalog.models.get(normalized).cloned()
    }

    pub fn funasr_default() -> Self {
        Self::model_spec_by_alias("funasr")
            .map(|spec| Self::from_spec(&spec))
            .unwrap_or_else(|| Self {
                backend: AsrBackend::FunAsr,
                model_id: "FunAudioLLM/Fun-ASR-Nano-2512".to_string(),
                source_url: "https://www.modelscope.cn/models/FunAudioLLM/Fun-ASR-Nano-2512"
                    .to_string(),
                model_dir: PathBuf::from("./models/FunAudioLLM/Fun-ASR-Nano-2512"),
                remote_code: PathBuf::from("./models/FunAudioLLM/Fun-ASR-Nano-2512/model.py"),
                device: "auto".to_string(),
                language: "中文".to_string(),
                itn: true,
                hotwords: Vec::new(),
            })
    }

    pub fn qwen3_asr_1_7b_default() -> Self {
        Self::model_spec_by_alias("qwen")
            .map(|spec| Self::from_spec(&spec))
            .unwrap_or_else(|| Self {
                backend: AsrBackend::QwenAsr,
                model_id: "Qwen/Qwen3-ASR-1.7B".to_string(),
                source_url: "https://www.modelscope.cn/collections/Qwen/Qwen3-ASR".to_string(),
                model_dir: PathBuf::from("./models/Qwen/Qwen3-ASR-1.7B"),
                remote_code: PathBuf::new(),
                device: "auto".to_string(),
                language: "中文".to_string(),
                itn: true,
                hotwords: Vec::new(),
            })
    }

    pub fn qwen3_asr_0_6b_default() -> Self {
        Self::model_spec_by_alias("qwen-0.6b")
            .map(|spec| Self::from_spec(&spec))
            .unwrap_or_else(|| Self {
                backend: AsrBackend::QwenAsr,
                model_id: "Qwen/Qwen3-ASR-0.6B".to_string(),
                source_url: "https://www.modelscope.cn/collections/Qwen/Qwen3-ASR".to_string(),
                model_dir: PathBuf::from("./models/Qwen/Qwen3-ASR-0.6B"),
                remote_code: PathBuf::new(),
                device: "auto".to_string(),
                language: "中文".to_string(),
                itn: true,
                hotwords: Vec::new(),
            })
    }

    pub fn for_model_id(model_id: impl Into<String>) -> Self {
        let model_id = model_id.into();
        match AsrBackend::from_model_id(&model_id) {
            AsrBackend::FunAsr => {
                let mut config = Self::funasr_default();
                config.model_id = model_id;
                config
            }
            AsrBackend::QwenAsr => {
                let mut config = if model_id
                    .to_ascii_lowercase()
                    .contains("qwen/qwen3-asr-0.6b")
                {
                    Self::qwen3_asr_0_6b_default()
                } else {
                    Self::qwen3_asr_1_7b_default()
                };
                config.model_id = model_id;
                config
            }
        }
    }

    pub fn from_env() -> Self {
        let mut config = if let Ok(model_id) = env::var("VOICEINPUT_ASR_MODEL_ID") {
            Self::for_model_id(model_id)
        } else {
            Self::default()
        };

        if let Ok(model_name) = env::var("VOICEINPUT_ASR_MODEL") {
            if let Some(spec) = Self::model_spec_by_alias(&model_name) {
                config = Self::from_spec(&spec);
            }
        }

        if let Ok(backend) = env::var("VOICEINPUT_ASR_BACKEND") {
            match backend.trim().to_ascii_lowercase().as_str() {
                "funasr" => {
                    config.backend = AsrBackend::FunAsr;
                }
                "qwen" | "qwen3" | "qwen-asr" => {
                    config.backend = AsrBackend::QwenAsr;
                }
                _ => {}
            }
        }

        if let Ok(model_id) = env::var("VOICEINPUT_ASR_MODEL_ID") {
            config.model_id = model_id;
        }
        if let Ok(source_url) = env::var("VOICEINPUT_ASR_SOURCE_URL") {
            config.source_url = source_url;
        }
        if let Ok(model_dir) = env::var("VOICEINPUT_ASR_MODEL_DIR") {
            config.model_dir = PathBuf::from(model_dir);
        }
        if let Ok(remote_code) = env::var("VOICEINPUT_ASR_REMOTE_CODE") {
            config.remote_code = PathBuf::from(remote_code);
        }
        if let Ok(device) = env::var("VOICEINPUT_ASR_DEVICE") {
            config.device = device;
        }
        if let Ok(language) = env::var("VOICEINPUT_ASR_LANGUAGE") {
            config.language = language;
        }
        if let Ok(itn) = env::var("VOICEINPUT_ASR_ITN") {
            config.itn = !matches!(
                itn.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no"
            );
        }
        if let Ok(hotwords) = env::var("VOICEINPUT_ASR_HOTWORDS") {
            config.hotwords = hotwords
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect();
        }

        if config.model_id.trim().is_empty() {
            config.model_id = match config.backend {
                AsrBackend::FunAsr => "FunAudioLLM/Fun-ASR-Nano-2512".to_string(),
                AsrBackend::QwenAsr => "Qwen/Qwen3-ASR-1.7B".to_string(),
            };
        }

        config
    }

    pub fn is_qwen(&self) -> bool {
        matches!(self.backend, AsrBackend::QwenAsr)
    }

    pub fn qwen_language(&self) -> Option<String> {
        let language = self.language.trim();
        if language.is_empty()
            || language.eq_ignore_ascii_case("auto")
            || language.eq_ignore_ascii_case("automatic")
            || language.eq_ignore_ascii_case("自动")
        {
            return None;
        }

        let lower = language.to_ascii_lowercase();
        let normalized = match lower.as_str() {
            "中文" | "zh" | "zh-cn" | "zh-hans" | "chinese" => "Chinese",
            "英文" | "en" | "english" => "English",
            "日文" | "ja" | "japanese" => "Japanese",
            "韩文" | "ko" | "korean" => "Korean",
            "粤语" | "cantonese" => "Cantonese",
            "法语" | "french" => "French",
            "德语" | "german" => "German",
            "西班牙语" | "spanish" => "Spanish",
            "葡萄牙语" | "portuguese" => "Portuguese",
            _ => language,
        };

        Some(normalized.to_string())
    }
}
