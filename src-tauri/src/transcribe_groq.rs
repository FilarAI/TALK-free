use reqwest::multipart;
use std::path::PathBuf;

use crate::settings::Settings;

// Codes offered in the UI, with the names Whisper may return as the detected language
const LANGUAGES: &[(&str, &str)] = &[
    ("pl", "polish"),
    ("en", "english"),
    ("de", "german"),
    ("es", "spanish"),
    ("fr", "french"),
    ("it", "italian"),
    ("uk", "ukrainian"),
    ("ru", "russian"),
];

fn is_language(detected: &str, code: &str) -> bool {
    let d = detected.to_lowercase();
    d == code || LANGUAGES.iter().any(|(c, name)| *c == code && d == *name)
}

pub async fn transcribe(settings: &Settings, audio_path: &PathBuf) -> Result<String, String> {
    let (url, model, api_key, provider) = if settings.engine == "groq" {
        ("https://api.groq.com/openai/v1/audio/transcriptions", "whisper-large-v3-turbo", &settings.groq_api_key, "Groq")
    } else {
        ("https://api.openai.com/v1/audio/transcriptions", "whisper-1", &settings.openai_api_key, "OpenAI")
    };

    if api_key.is_empty() {
        return Err(format!("{} API key not set. Please enter your API key in settings.", provider));
    }

    let audio_bytes = std::fs::read(audio_path)
        .map_err(|e| format!("Failed to read audio file: {}", e))?;

    let primary = settings.language.as_str();
    let secondary = settings.secondary_language.as_str();

    // Two languages: let Whisper detect, but accept only primary or secondary
    if primary != "auto" && secondary != "none" && secondary != primary {
        let (text, detected) = request(url, model, api_key, provider, &audio_bytes, None).await?;
        if is_language(&detected, primary) || is_language(&detected, secondary) {
            return Ok(text);
        }
        // ponytail: misdetection costs a second request, fine for short dictation
        return Ok(request(url, model, api_key, provider, &audio_bytes, Some(primary)).await?.0);
    }

    let language = if primary == "auto" { None } else { Some(primary) };
    Ok(request(url, model, api_key, provider, &audio_bytes, language).await?.0)
}

// Returns (text, detected language)
async fn request(
    url: &str,
    model: &str,
    api_key: &str,
    provider: &str,
    audio_bytes: &[u8],
    language: Option<&str>,
) -> Result<(String, String), String> {
    let file_part = multipart::Part::bytes(audio_bytes.to_vec())
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;

    let mut form = multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", "verbose_json")
        .part("file", file_part);
    if let Some(lang) = language {
        form = form.text("language", lang.to_string());
    }

    let client = reqwest::Client::new();
    let response = client
        .post(url)
        .header("Authorization", format!("Bearer {}", api_key))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("{} API request failed: {}", provider, e))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("{} API error ({}): {}", provider, status, body));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse {} response: {}", provider, e))?;

    let text = json["text"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or(format!("No 'text' field in {} response", provider))?;
    let detected = json["language"].as_str().unwrap_or("").to_string();
    Ok((text, detected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_empty_api_key() {
        let path = PathBuf::from("/tmp/test.wav");
        let result = transcribe(&Settings::default(), &path).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("API key not set"));
    }

    #[test]
    fn test_is_language() {
        assert!(is_language("polish", "pl"));
        assert!(is_language("Polish", "pl"));
        assert!(is_language("pl", "pl"));
        assert!(is_language("english", "en"));
        assert!(!is_language("russian", "pl"));
        assert!(!is_language("", "pl"));
    }
}
