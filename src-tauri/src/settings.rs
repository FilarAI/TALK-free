use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub microphone: String,
    pub engine: String,
    #[serde(rename = "openaiApiKey")]
    pub openai_api_key: String,
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
}

fn default_true() -> bool { true }
fn default_theme() -> String { "system".to_string() }

impl Default for Settings {
    fn default() -> Self {
        Self {
            microphone: "default".to_string(),
            engine: "cloud".to_string(),
            openai_api_key: String::new(),
            hotkey_toggle: "CmdOrCtrl+Shift+Space".to_string(),
            hotkey_ptt: "Shift+Alt+Space".to_string(),
            overlay_position: "top-right".to_string(),
            overlay_always_on_top: true,
            theme: "system".to_string(),
        }
    }
}

impl Settings {
    pub fn config_path(app_dir: &PathBuf) -> PathBuf {
        app_dir.join("config.json")
    }

    pub fn load(app_dir: &PathBuf) -> Self {
        let path = Self::config_path(app_dir);
        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, app_dir: &PathBuf) -> Result<(), String> {
        let path = Self::config_path(app_dir);
        fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }
}
