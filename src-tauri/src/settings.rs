//! Local settings, persisted as JSON in the OS per-user app-config directory.
//! Nothing here needs admin rights or touches the registry/Program Files.
//!
//! The provider API key is NOT written to the JSON file. It lives in the OS
//! credential store (Windows Credential Manager), and is never sent back to the
//! web UI — the UI only learns whether a key is saved (`api_key_set`).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const KEYRING_SERVICE: &str = "org.curia.desktop";
const KEYRING_USER: &str = "llm-api-key";

/// User-editable settings.
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
    /// Inbound from the UI on save (blank = keep existing). Filled from the
    /// credential store only for backend use; always blank on disk and in
    /// anything returned to the UI. (Older versions stored it here in plain
    /// text; `load` migrates such keys into the credential store.)
    pub api_key: String,
    /// Outbound to the UI only: whether a key is saved in the credential store.
    pub api_key_set: bool,
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
            api_key_set: false,
        }
    }
}

fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("settings.json"))
}

fn key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER).map_err(|e| e.to_string())
}

fn read_key() -> Option<String> {
    key_entry()
        .ok()?
        .get_password()
        .ok()
        .filter(|k| !k.trim().is_empty())
}

fn store_key(key: &str) -> Result<(), String> {
    key_entry()?
        .set_password(key)
        .map_err(|e| format!("Could not save the API key to Windows Credential Manager: {e}"))
}

pub fn clear_key() -> Result<(), String> {
    match key_entry()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn write_file(app: &AppHandle, s: &Settings) -> Result<(), String> {
    let mut on_disk = s.clone();
    on_disk.api_key = String::new();
    on_disk.api_key_set = false;
    let path = settings_path(app)?;
    let data = serde_json::to_string_pretty(&on_disk).map_err(|e| e.to_string())?;
    fs::write(&path, data).map_err(|e| e.to_string())
}

fn load_file(app: &AppHandle) -> Settings {
    if let Ok(path) = settings_path(app) {
        if let Ok(text) = fs::read_to_string(&path) {
            if let Ok(mut cfg) = serde_json::from_str::<Settings>(&text) {
                // Migrate a plain-text key left by an older version.
                if !cfg.api_key.trim().is_empty() && store_key(cfg.api_key.trim()).is_ok() {
                    cfg.api_key = String::new();
                    let _ = write_file(app, &cfg);
                }
                return cfg;
            }
        }
    }
    Settings::default()
}

/// Settings as shown to the UI: never contains the key itself.
pub fn load_public(app: &AppHandle) -> Settings {
    let mut s = load_file(app);
    s.api_key = String::new();
    s.api_key_set = read_key().is_some();
    s
}

/// Settings for backend use, with the API key filled in from the credential store.
pub fn load(app: &AppHandle) -> Settings {
    let mut s = load_file(app);
    if s.api_key.trim().is_empty() {
        if let Some(k) = read_key() {
            s.api_key = k;
        }
    }
    s.api_key_set = !s.api_key.trim().is_empty();
    s
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    validate(settings)?;
    let key = settings.api_key.trim();
    if !key.is_empty() {
        store_key(key)?;
    }
    write_file(app, settings)
}

/// True for https URLs, or plain http only to this machine (e.g. local Ollama).
pub fn is_safe_endpoint(url: &str) -> bool {
    let u = url.trim().to_ascii_lowercase();
    u.starts_with("https://")
        || u.starts_with("http://localhost:")
        || u.starts_with("http://localhost/")
        || u == "http://localhost"
        || u.starts_with("http://127.0.0.1:")
        || u.starts_with("http://127.0.0.1/")
        || u.starts_with("http://[::1]:")
}

/// Reject settings that would send research queries or API keys in clear text
/// over the network.
pub fn validate(s: &Settings) -> Result<(), String> {
    if !is_safe_endpoint(&s.lex_base_url) {
        return Err("Lex base URL must use https:// (or http://localhost).".into());
    }
    if !s.base_url.trim().is_empty() && !is_safe_endpoint(&s.base_url) {
        return Err(
            "Provider Base URL must use https:// (plain http is only allowed for localhost).".into(),
        );
    }
    Ok(())
}
