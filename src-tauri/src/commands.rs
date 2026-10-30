//! Tauri command surface. Thin: validates input, delegates to the controller/stores.

use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::application::{AppError, AppResult, DictationState, ErrorKind, Intent};
use crate::platform::{self, hotkey, hotkey::HotkeyStatuses, secrets, windows};
use crate::refinement::{RefinementMode, RefinementProvider};
use crate::storage::Settings;
use crate::transcription::model::{self, ModelStatus, MODELS};
use crate::AppState;

pub const DOWNLOAD_EVENT: &str = "model://download-progress";

#[tauri::command]
pub fn get_state(state: State<'_, AppState>) -> DictationState {
    state.controller.state()
}

#[tauri::command]
pub fn toggle_dictation(state: State<'_, AppState>) -> AppResult<()> {
    state.controller.toggle(platform::foreground_window(), Intent::Dictate)
}

#[tauri::command]
pub fn start_dictation(state: State<'_, AppState>) -> AppResult<()> {
    state.controller.start(platform::foreground_window(), Intent::Dictate)
}

#[tauri::command]
pub fn stop_dictation(state: State<'_, AppState>) -> AppResult<()> {
    state.controller.stop()
}

#[tauri::command]
pub fn cancel_dictation(state: State<'_, AppState>) {
    state.controller.cancel()
}

#[tauri::command]
pub fn retry_dictation(state: State<'_, AppState>) -> AppResult<()> {
    state.controller.retry()
}

#[tauri::command]
pub fn dismiss_dictation(state: State<'_, AppState>) {
    state.controller.dismiss()
}

#[tauri::command]
pub fn copy_last(state: State<'_, AppState>) -> AppResult<()> {
    state.controller.copy_last()
}

/// Copies user-edited transcript text from the main window.
#[tauri::command]
pub fn copy_text(state: State<'_, AppState>, text: String) -> AppResult<()> {
    state.delivery.deliver(&text)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    model::find(&settings.model)?;
    if settings.hotkey.trim().is_empty() {
        return Err(AppError::new(ErrorKind::HotkeyConflict, "The dictation shortcut cannot be empty."));
    }
    hotkey::validate_pair(&settings.hotkey, &settings.compose_hotkey)
        .map_err(|m| AppError::new(ErrorKind::HotkeyConflict, m))?;

    let previous = state.settings();
    state
        .store
        .save(&settings)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("Could not save settings: {e}")))?;
    state.apply_settings(&settings);

    if previous.hotkey != settings.hotkey || previous.compose_hotkey != settings.compose_hotkey {
        let statuses = hotkey::register_all(&app, &settings.hotkey, &settings.compose_hotkey);
        *state.hotkey_status.lock().unwrap_or_else(|e| e.into_inner()) = statuses;
    }
    if previous.bar_visible != settings.bar_visible {
        windows::set_bar_visible(&app, settings.bar_visible);
    }
    Ok(settings)
}

#[tauri::command]
pub fn get_hotkey_status(state: State<'_, AppState>) -> HotkeyStatuses {
    state.hotkey_status.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    id: &'static str,
    label: &'static str,
    size_bytes: u64,
    multilingual: bool,
    status: ModelStatus,
}

#[tauri::command]
pub fn list_models(state: State<'_, AppState>) -> Vec<ModelEntry> {
    let dir = state.speech.models_dir();
    MODELS
        .iter()
        .map(|m| ModelEntry {
            id: m.id,
            label: m.label,
            size_bytes: m.size_bytes,
            multilingual: m.multilingual,
            status: model::status(dir, m),
        })
        .collect()
}

#[derive(Clone, Serialize)]
struct DownloadProgress<'a> {
    model: &'a str,
    done: u64,
    total: u64,
}

#[tauri::command]
pub async fn download_model(app: AppHandle, state: State<'_, AppState>, id: String) -> AppResult<()> {
    let info = model::find(&id)?;
    if state.downloading.swap(true, Ordering::SeqCst) {
        return Err(AppError::new(ErrorKind::Busy, "A model download is already running."));
    }
    let dir = state.speech.models_dir().clone();
    let result = model::download(&dir, info, |done, total| {
        let _ = app.emit(DOWNLOAD_EVENT, DownloadProgress { model: info.id, done, total });
    })
    .await;
    state.downloading.store(false, Ordering::SeqCst);
    result.map(|path| tracing::info!(model = info.id, path = %path.display(), "model downloaded"))
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) {
    windows::show_main(&app);
}

#[tauri::command]
pub fn set_bar_visible(app: AppHandle, state: State<'_, AppState>, visible: bool) -> AppResult<()> {
    let mut settings = state.settings();
    settings.bar_visible = visible;
    state
        .store
        .save(&settings)
        .map_err(|e| AppError::new(ErrorKind::Internal, format!("Could not save settings: {e}")))?;
    state.apply_settings(&settings);
    windows::set_bar_visible(&app, visible);
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefinementStatus {
    key_configured: bool,
}

/// Never returns the key itself, only whether one is stored.
#[tauri::command]
pub fn get_refinement_status(state: State<'_, AppState>) -> RefinementStatus {
    RefinementStatus { key_configured: state.gemini.is_available() }
}

/// Saves the key to Windows Credential Manager, then verifies it with one tiny request.
/// A key that Gemini rejects is removed again so a bad key is never left in place.
#[tauri::command]
pub async fn set_gemini_key(state: State<'_, AppState>, key: String) -> AppResult<String> {
    let key = key.trim().to_string();
    if key.len() < 20 || key.chars().any(char::is_whitespace) {
        return Err(AppError::new(ErrorKind::RefinementFailed, "That does not look like a Gemini API key."));
    }
    secrets::store(secrets::GEMINI_KEY, &key)?;
    drop(key);
    let gemini = state.gemini.clone();
    let check = tauri::async_runtime::spawn_blocking(move || gemini.refine("this are a test", RefinementMode::Clean))
        .await
        .map_err(|e| AppError::new(ErrorKind::Internal, e.to_string()))?;
    match check {
        Ok(sample) => {
            tracing::info!("gemini key verified");
            Ok(sample)
        }
        Err(e) => {
            if e.message.contains("API key") {
                let _ = secrets::delete(secrets::GEMINI_KEY);
            }
            Err(e)
        }
    }
}

#[tauri::command]
pub fn clear_gemini_key() -> AppResult<()> {
    secrets::delete(secrets::GEMINI_KEY)
}

#[tauri::command]
pub fn quit_app(app: AppHandle, state: State<'_, AppState>) {
    state.controller.cancel();
    app.exit(0);
}
