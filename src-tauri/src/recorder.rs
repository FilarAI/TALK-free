use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::paste::paste_text;
use crate::settings::Settings;
use crate::transcribe_groq;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum RecordingState {
    Ready,
    Recording,
    Transcribing,
}

// The overlay page exposes setState(name) and setLevel(0..1)
fn overlay_eval(app: &AppHandle, js: &str) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let _ = overlay.eval(js);
    }
}

fn update_overlay(app: &AppHandle, state: &RecordingState) {
    let name = match state {
        RecordingState::Ready => "ready",
        RecordingState::Recording => "recording",
        RecordingState::Transcribing => "transcribing",
    };
    overlay_eval(app, &format!("window.setState && window.setState('{}');", name));
}

// Shows the error in the settings window and flashes the overlay
fn report_error(app: &AppHandle, error: &str) {
    eprintln!("[Typr] {}", error);
    let _ = app.emit("recording-error", error);
    overlay_eval(app, "window.setState && window.setState('error');");
}

pub struct Recorder {
    state: Arc<Mutex<RecordingState>>,
    audio_recorder: Arc<Mutex<AudioRecorder>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
        }
    }

    pub fn get_state(&self) -> RecordingState {
        self.state.lock().unwrap().clone()
    }

    pub fn start_recording(&self, app: &AppHandle, mic_name: &str) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if *state != RecordingState::Ready {
            return Err("Already recording or transcribing".to_string());
        }

        let mut recorder = self.audio_recorder.lock().unwrap();
        if let Err(e) = recorder.start(mic_name) {
            report_error(app, &e);
            return Err(e);
        }

        *state = RecordingState::Recording;
        let _ = app.emit("recording-state", RecordingState::Recording);
        update_overlay(app, &RecordingState::Recording);

        // Feed the mic level to the overlay animation while recording (~20 fps)
        let level = recorder.level();
        let state_ref = self.state.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            while *state_ref.lock().unwrap() == RecordingState::Recording {
                let rms = f32::from_bits(level.load(Ordering::Relaxed));
                // Speech RMS is roughly 0.01–0.2: gate room noise, sqrt so quiet speech still moves the bars
                let normalized = ((rms - 0.004).max(0.0) * 14.0).sqrt().min(1.0);
                overlay_eval(&app, &format!("window.setLevel && window.setLevel({:.3});", normalized));
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
        Ok(())
    }

    pub async fn stop_and_transcribe(
        &self,
        app: &AppHandle,
        settings: &Settings,
        app_dir: &PathBuf,
    ) -> Result<String, String> {
        {
            let mut state = self.state.lock().unwrap();
            if *state != RecordingState::Recording {
                return Err("Not currently recording".to_string());
            }
            *state = RecordingState::Transcribing;
            let _ = app.emit("recording-state", RecordingState::Transcribing);
            update_overlay(app, &RecordingState::Transcribing);
        }

        let result = self.save_and_transcribe(settings, app_dir).await;

        // Always return to Ready, otherwise one API error blocks recording until restart
        {
            let mut state = self.state.lock().unwrap();
            *state = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
            update_overlay(app, &RecordingState::Ready);
        }

        match &result {
            Err(e) => report_error(app, e),
            Ok(text) if !text.is_empty() && !settings.auto_paste => {
                overlay_eval(app, "window.setState && window.setState('copied');");
            }
            _ => {}
        }

        result
    }

    async fn save_and_transcribe(&self, settings: &Settings, app_dir: &PathBuf) -> Result<String, String> {
        let temp_path = app_dir.join("temp_recording.wav");

        {
            let mut recorder = self.audio_recorder.lock().unwrap();
            recorder.stop_and_save(&temp_path)?;
        }

        let raw_text = transcribe_groq::transcribe(settings, &temp_path).await;
        let _ = std::fs::remove_file(&temp_path);

        let cleaned = cleanup_text(&raw_text?);

        if !cleaned.is_empty() {
            paste_text(&cleaned, settings.auto_paste)?;
        }

        Ok(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_ready() {
        let recorder = Recorder::new();
        assert_eq!(recorder.get_state(), RecordingState::Ready);
    }
}
