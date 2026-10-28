use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::keys;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub microphone: String,
    // "groq" or "openai" (legacy "cloud" = openai)
    pub engine: String,
    // Keys are kept in memory and the Keychain only: read from old config.json
    // for migration, never written to disk or sent to the UI
    #[serde(rename = "openaiApiKey", default, skip_serializing)]
    pub openai_api_key: String,
    #[serde(rename = "groqApiKey", default, skip_serializing)]
    pub groq_api_key: String,
    // ISO code ("pl", "en", ...) or "auto"
    #[serde(default = "default_language")]
    pub language: String,
    // ISO code or "none"
    #[serde(rename = "secondaryLanguage", default = "default_secondary_language")]
    pub secondary_language: String,
    #[serde(rename = "hotkeyToggle")]
    pub hotkey_toggle: String,
    #[serde(rename = "hotkeyPtt")]
    pub hotkey_ptt: String,
    #[serde(rename = "overlayPosition")]
    pub overlay_position: String,
    #[serde(rename = "overlayAlwaysOnTop", default = "default_true")]
    pub overlay_always_on_top: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    // false = only copy to clipboard
    #[serde(rename = "autoPaste", default = "default_true")]
    pub auto_paste: bool,
}

fn default_true() -> bool { true }
fn default_theme() -> String { "system".to_string() }
fn default_language() -> String { "pl".to_string() }
fn default_secondary_language() -> String { "en".to_string() }

impl Default for Settings {
    fn default() -> Self {
        Self {
            microphone: "default".to_string(),
            engine: "groq".to_string(),
            openai_api_key: String::new(),
            groq_api_key: String::new(),
            language: default_language(),
            secondary_language: default_secondary_language(),
            hotkey_toggle: "CmdOrCtrl+Shift+Space".to_string(),
            hotkey_ptt: "Shift+Alt+Space".to_string(),
            overlay_position: "top-right".to_string(),
            overlay_always_on_top: true,
            theme: "system".to_string(),
            auto_paste: true,
        }
    }
}

impl Settings {
    pub fn config_path(app_dir: &PathBuf) -> PathBuf {
        app_dir.join("config.json")
    }

    pub fn load(app_dir: &PathBuf) -> Self {
        let path = Self::config_path(app_dir);
        let mut settings: Self = match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        };

        // Move plaintext keys from old config.json into the Keychain
        let legacy = !settings.openai_api_key.is_empty() || !settings.groq_api_key.is_empty();
        let mut migrated = true;
        for (provider, key) in [("openai", &mut settings.openai_api_key), ("groq", &mut settings.groq_api_key)] {
            if !key.is_empty() {
                if let Err(e) = keys::set(provider, key) {
                    eprintln!("[Typr] Keychain migration failed for {}: {}", provider, e);
                    migrated = false;
                }
            } else {
                *key = keys::get(provider);
            }
        }
        if legacy && migrated {
            // Rewrites config.json without the keys; on failure keep the old file so no key is lost
            let _ = settings.save(app_dir);
        }
        settings
    }

    pub fn save(&self, app_dir: &PathBuf) -> Result<(), String> {
        let path = Self::config_path(app_dir);
        fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }
}
