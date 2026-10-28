// API keys live in the macOS Keychain, never in config.json or the settings UI.
use keyring::Entry;

const SERVICE: &str = "com.typr.app";

fn entry(provider: &str) -> Result<Entry, String> {
    Entry::new(SERVICE, provider).map_err(|e| e.to_string())
}

pub fn get(provider: &str) -> String {
    entry(provider).and_then(|e| e.get_password().map_err(|e| e.to_string())).unwrap_or_default()
}

pub fn set(provider: &str, key: &str) -> Result<(), String> {
    let e = entry(provider)?;
    if key.is_empty() {
        return match e.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.to_string()),
        };
    }
    e.set_password(key).map_err(|e| e.to_string())
}

// What the UI may see: last 4 characters only
pub fn hint(key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let tail: String = key.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
    Some(format!("…{}", tail))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hint() {
        assert_eq!(hint(""), None);
        assert_eq!(hint("gsk_abcdef1234"), Some("…1234".to_string()));
        assert_eq!(hint("ab"), Some("…ab".to_string()));
    }
}
