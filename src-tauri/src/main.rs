#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{LogicalPosition, Manager, Monitor, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use typr_lib::audio;
use typr_lib::recorder::{Recorder, RecordingState};
use typr_lib::settings::Settings;

struct AppState {
    recorder: Recorder,
    settings: Mutex<Settings>,
    app_dir: PathBuf,
}

fn get_app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.typr.app")
}

const OVERLAY_SIZE: f64 = 50.0;
const MARGIN: f64 = 10.0;
const DOCK_HEIGHT: f64 = 80.0; // macOS dock typical logical height

// NSScreenSaverWindowLevel (1000) — above full-screen apps and all spaces
// NSFloatingWindowLevel (3)       — standard always-on-top (fallback)
const NS_SCREENSAVER_WINDOW_LEVEL: i64 = 1000;
const NS_FLOATING_WINDOW_LEVEL: i64 = 3;

// NSWindowCollectionBehaviorCanJoinAllSpaces    = 1 << 0
// NSWindowCollectionBehaviorIgnoresCycle        = 1 << 6
// NSWindowCollectionBehaviorFullScreenAuxiliary = 1 << 8
const COLLECTION_ALL_SPACES: u64 = 1 | 64 | 256;

#[cfg(target_os = "macos")]
fn configure_overlay(window: &WebviewWindow, always_on_top: bool) {
    // Tauri-level first (sets CanJoinAllSpaces internally)
    let _ = window.set_visible_on_all_workspaces(true);
    let _ = window.set_always_on_top(always_on_top);

    // Override with native NSWindow calls for stronger level + full-screen support
    if let Ok(ns_win) = window.ns_window() {
        unsafe {
            use objc::{msg_send, sel, sel_impl};
            let ns_win = ns_win as *mut objc::runtime::Object;
            let level: i64 = if always_on_top { NS_SCREENSAVER_WINDOW_LEVEL } else { NS_FLOATING_WINDOW_LEVEL };
            let _: () = msg_send![ns_win, setLevel: level];
            let _: () = msg_send![ns_win, setCollectionBehavior: COLLECTION_ALL_SPACES];
            // Force window to front regardless of app active state / current space
            let _: () = msg_send![ns_win, orderFrontRegardless];
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn configure_overlay(window: &WebviewWindow, always_on_top: bool) {
    let _ = window.set_always_on_top(always_on_top);
}

fn calc_overlay_pos(position: &str, monitor: Option<&Monitor>) -> (f64, f64) {
    let (w, h) = if let Some(m) = monitor {
        let size = m.size();
        let scale = m.scale_factor();
        (size.width as f64 / scale, size.height as f64 / scale)
    } else {
        (1440.0, 900.0)
    };
    let bottom_y = h - OVERLAY_SIZE - DOCK_HEIGHT - MARGIN;
    match position {
        "top-left"      => (MARGIN, MARGIN),
        "top-center"    => ((w - OVERLAY_SIZE) / 2.0, MARGIN),
        "top-right"     => (w - OVERLAY_SIZE - MARGIN, MARGIN),
        "bottom-left"   => (MARGIN, bottom_y),
        "bottom-center" => ((w - OVERLAY_SIZE) / 2.0, bottom_y),
        "bottom-right"  => (w - OVERLAY_SIZE - MARGIN, bottom_y),
        _               => (w - OVERLAY_SIZE - MARGIN, MARGIN),
    }
}

fn register_shortcuts(app: &tauri::AppHandle) -> Result<(), String> {
    let (toggle_key, ptt_key) = {
        let state = app.state::<AppState>();
        let s = state.settings.lock().unwrap();
        (s.hotkey_toggle.clone(), s.hotkey_ptt.clone())
    };

    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(toggle_key.as_str(), move |_app, _shortcut, event| {
            if event.state != ShortcutState::Pressed { return; }
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                match do_toggle_recording(&handle, state.inner()).await {
                    Ok(r) => println!("[Typr] Toggle: {}", r),
                    Err(e) => eprintln!("[Typr] Toggle error: {}", e),
                }
            });
        })
        .map_err(|e| format!("Failed to register toggle shortcut '{}': {}", toggle_key, e))?;

    let handle = app.clone();
    app.global_shortcut()
        .on_shortcut(ptt_key.as_str(), move |_app, _shortcut, event| {
            let handle = handle.clone();
            match event.state {
                ShortcutState::Pressed => {
                    tauri::async_runtime::spawn(async move {
                        let state = handle.state::<AppState>();
                        if state.recorder.get_state() == RecordingState::Ready {
                            let mic = state.settings.lock().unwrap().microphone.clone();
                            if let Err(e) = state.recorder.start_recording(&handle, &mic) {
                                eprintln!("[Typr] PTT start error: {}", e);
                            }
                        }
                    });
                }
                ShortcutState::Released => {
                    tauri::async_runtime::spawn(async move {
                        let state = handle.state::<AppState>();
                        if state.recorder.get_state() == RecordingState::Recording {
                            let settings = state.settings.lock().unwrap().clone();
                            match state.recorder.stop_and_transcribe(&handle, &settings, &state.app_dir).await {
                                Ok(r) => println!("[Typr] PTT result: {}", r),
                                Err(e) => eprintln!("[Typr] PTT error: {}", e),
                            }
                        }
                    });
                }
            }
        })
        .map_err(|e| format!("Failed to register PTT shortcut '{}': {}", ptt_key, e))?;

    println!("[Typr] Shortcuts registered: toggle={} ptt={}", toggle_key, ptt_key);
    Ok(())
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    settings.save(&state.app_dir)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn list_microphones() -> Vec<audio::MicDevice> {
    audio::list_microphones()
}

#[tauri::command]
fn get_recording_state(state: State<AppState>) -> RecordingState {
    state.recorder.get_state()
}

#[tauri::command]
async fn toggle_recording(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<String, String> {
    do_toggle_recording(&app, &state).await
}

#[tauri::command]
async fn update_shortcuts(app: tauri::AppHandle) -> Result<(), String> {
    app.global_shortcut().unregister_all().map_err(|e| e.to_string())?;
    register_shortcuts(&app)
}

#[tauri::command]
fn update_overlay_position(app: tauri::AppHandle, state: State<AppState>) -> Result<(), String> {
    let (position, always_on_top) = {
        let s = state.settings.lock().unwrap();
        (s.overlay_position.clone(), s.overlay_always_on_top)
    };
    if let Some(overlay) = app.get_webview_window("overlay") {
        let monitor = overlay.current_monitor().ok().flatten();
        let (x, y) = calc_overlay_pos(&position, monitor.as_ref());
        overlay.set_position(LogicalPosition::new(x, y)).map_err(|e| e.to_string())?;
        configure_overlay(&overlay, always_on_top);
    }
    Ok(())
}

async fn do_toggle_recording(app: &tauri::AppHandle, state: &AppState) -> Result<String, String> {
    match state.recorder.get_state() {
        RecordingState::Ready => {
            let mic = state.settings.lock().unwrap().microphone.clone();
            state.recorder.start_recording(app, &mic)?;
            Ok("recording".to_string())
        }
        RecordingState::Recording => {
            let settings = state.settings.lock().unwrap().clone();
            let result = state.recorder.stop_and_transcribe(app, &settings, &state.app_dir).await?;
            Ok(result)
        }
        RecordingState::Transcribing => Err("Currently transcribing, please wait".to_string()),
    }
}

fn main() {
    let app_dir = get_app_dir();
    let settings = Settings::load(&app_dir);
    let initial_position = settings.overlay_position.clone();
    let initial_always_on_top = settings.overlay_always_on_top;

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            recorder: Recorder::new(),
            settings: Mutex::new(settings),
            app_dir,
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_microphones,
            get_recording_state,
            toggle_recording,
            update_shortcuts,
            update_overlay_position,
        ])
        .setup(move |app| {
            let monitor = app.primary_monitor().ok().flatten();
            let (x, y) = calc_overlay_pos(&initial_position, monitor.as_ref());

            let _ = WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("src/overlay.html".into()))
                .title("")
                .inner_size(OVERLAY_SIZE, OVERLAY_SIZE)
                .position(x, y)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .shadow(false)
                .build();

            if let Some(overlay) = app.get_webview_window("overlay") {
                configure_overlay(&overlay, initial_always_on_top);
                // Deferred re-apply on main thread: NSWindow may not be fully ready at setup time
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    if let Some(w) = handle.get_webview_window("overlay") {
                        let w2 = w.clone();
                        let _ = w.run_on_main_thread(move || {
                            configure_overlay(&w2, initial_always_on_top);
                        });
                    }
                });
            }

            if let Err(e) = register_shortcuts(app.handle()) {
                eprintln!("[Typr] ERROR registering shortcuts: {}", e);
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
