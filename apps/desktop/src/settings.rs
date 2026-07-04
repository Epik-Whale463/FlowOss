//! User settings, persisted to `~/.config/flowoss/config.toml` (PRD 11.8).
//! Every field has a sensible default; the file is optional.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Input device name; `None` = system default.
    pub device: Option<String>,
    /// "auto" (clipboard + simulated paste) or "copy".
    pub paste_mode: String,
    /// "raw" or "basic".
    pub cleanup: String,
    pub model_dir: PathBuf,
    pub vad_model: PathBuf,
    pub threads: i32,
    /// Show live words in the overlay while speaking (needs the streaming
    /// preview model).
    pub live_preview: bool,
    pub streaming_model_dir: PathBuf,
    /// AI backend for highlighted-text assist mode: ollama | anthropic | openai.
    pub assist_provider: String,
    pub assist_model: String,
    pub assist_base_url: String,
    pub assist_api_key: String,
    /// Let the assistant call the free DuckDuckGo web_search tool.
    pub assist_web_search: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device: None,
            paste_mode: "auto".into(),
            cleanup: "basic".into(),
            model_dir: flowoss_core::default_stt_model_dir(),
            vad_model: flowoss_core::default_vad_model_path(),
            threads: 4,
            live_preview: false,
            streaming_model_dir: flowoss_core::models_dir()
                .join("sherpa-onnx-streaming-zipformer-en-20M-2023-02-17"),
            assist_provider: "ollama".into(),
            assist_model: "gemma3:4b".into(),
            assist_base_url: "http://localhost:11434".into(),
            assist_api_key: String::new(),
            assist_web_search: true,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read_to_string(flowoss_core::config_path())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = flowoss_core::config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("failed to write {}", path.display()))
    }

    pub fn paste_mode(&self) -> flowoss_insertion::PasteMode {
        self.paste_mode
            .parse()
            .unwrap_or(flowoss_insertion::PasteMode::Auto)
    }

    pub fn cleanup_mode(&self) -> flowoss_text_cleanup::CleanupMode {
        self.cleanup.parse().unwrap_or_default()
    }

    pub fn assist_config(&self) -> flowoss_assist::AssistConfig {
        flowoss_assist::AssistConfig {
            provider: flowoss_assist::Provider::from_str_lossy(&self.assist_provider),
            model: self.assist_model.trim().to_string(),
            base_url: self.assist_base_url.trim().to_string(),
            api_key: self.assist_api_key.trim().to_string(),
            web_search: self.assist_web_search,
        }
    }
}
