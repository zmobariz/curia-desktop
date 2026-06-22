//! Local settings, persisted as JSON in the OS per-user app-config directory.
//! Nothing here needs admin rights or touches the registry/Program Files.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// User-editable settings. `api_key` is stored in plain text in the per-user
/// config file — acceptable for a single-user desktop machine; a future version
/// can move this to the OS keyring.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
    /// Base URL of the Lex API.
    pub lex_base_url: String,
    /// Optional contact string, sent in the User-Agent for polite API use.
    pub contact: String,
    /// Whether AI ("Explain") features are enabled.
    pub llm_enabled: bool,
    /// One of: "openai", "anthropic", "openai_compatible", "ollama".
    pub provider: String,
    /// Base URL for "openai_compatible" / "ollama" providers.
    pub base_url: String,
    /// Model name (provider-specific).
    pub model: String,
    /// The user's own API key for their chosen provider.
    pub api_key: String,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            lex_base_url: "https://lex.lab.i.ai.gov.uk".into(),
            contact: String::new(),
            llm_enabled: false,
            provider: "openai".into(),
            base_url: String::new(),
            model: String::new(),
            api_key: String::new(),
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    if let Ok(path) = settings_path(app) {
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<Settings>(&text) {
                return cfg;
            }
        }
    }
    Settings::default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = settings_path(app)?;
    let data = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())
}
