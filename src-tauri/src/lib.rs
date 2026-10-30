//! VoiceFlow Desktop. Module map (see docs/architecture.md):
//! application (state machine + controller) -> audio -> transcription -> refinement -> delivery,
//! with storage for settings and platform for OS integrations.

pub mod application;
pub mod audio;
mod commands;
pub mod delivery;
pub mod platform;
pub mod refinement;
pub mod storage;
pub mod transcription;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, RunEvent, Runtime, WindowEvent};

use application::{Controller, ControllerConfig, DictationState, StateSink};
use audio::capture::CpalSource;
use delivery::clipboard::ClipboardDelivery;
use delivery::paste::PasteInserter;
use delivery::TextDelivery;
use platform::hotkey::{self, HotkeyStatuses};
use platform::windows;
use storage::{Settings, SettingsStore};
use audio::vad::{SileroVad, SILERO_MODEL_FILE};
use audio::VoiceDetector;
use refinement::gemini::GeminiRefiner;
use transcription::whisper::WhisperProvider;
use transcription::SpeechProvider;

pub const STATE_EVENT: &str = "dictation://state";

pub struct AppState {
    pub controller: Arc<Controller>,
    pub speech: Arc<WhisperProvider>,
    pub gemini: Arc<GeminiRefiner>,
    pub delivery: Arc<dyn TextDelivery>,
    pub store: SettingsStore,
    settings: Mutex<Settings>,
    pub hotkey_status: Mutex<HotkeyStatuses>,
    pub downloading: AtomicBool,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Push settings into the running components (no restart needed).
    pub fn apply_settings(&self, s: &Settings) {
        self.speech.set_model(&s.model);
        self.gemini.set_model(&s.gemini_model);
        self.controller.set_config(controller_config(s));
        *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = s.clone();
    }
}

fn controller_config(s: &Settings) -> ControllerConfig {
    ControllerConfig {
        language: s.language.clone(),
        auto_copy: s.auto_copy,
        auto_paste: s.auto_paste,
        refinement: s.refinement,
        ..ControllerConfig::default()
    }
}

/// The bundled VAD is optional: without it dictation still works, just without the noise guard.
fn load_vad(resource_dir: Option<std::path::PathBuf>) -> Option<Arc<dyn VoiceDetector>> {
    let path = resource_dir?.join("resources").join(SILERO_MODEL_FILE);
    match SileroVad::load(&path.to_string_lossy()) {
        Ok(vad) => Some(Arc::new(vad)),
        Err(e) => {
            tracing::warn!(error = %e, path = %path.display(), "VAD unavailable; noise may be transcribed");
            None
        }
    }
}

/// Forwards controller state to every webview.
struct TauriSink<R: Runtime>(AppHandle<R>);

impl<R: Runtime> StateSink for TauriSink<R> {
    fn publish(&self, state: &DictationState) {
        if let Err(e) = self.0.emit(STATE_EVENT, state) {
            tracing::warn!(error = %e, "failed to emit state");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,voiceflow_lib=debug".into()),
        )
        .init();

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| hotkey::handle(app, shortcut, event))
                .build(),
        )
        .setup(|app| {
            let handle = app.handle().clone();
            let config_dir = app.path().app_config_dir()?;
            let models_dir = app.path().app_data_dir()?.join("models");
            let store = SettingsStore::new(&config_dir);
            let settings = store.load();
            tracing::info!(config = %config_dir.display(), models = %models_dir.display(), "starting");

            let speech = Arc::new(WhisperProvider::new(models_dir, &settings.model));
            let vad = load_vad(app.path().resource_dir().ok());
            let delivery: Arc<dyn TextDelivery> = Arc::new(ClipboardDelivery::new(handle.clone()));
            let audio_source: Arc<dyn audio::AudioSource> = Arc::new(CpalSource);
            #[cfg(feature = "e2e-audio")]
            let audio_source: Arc<dyn audio::AudioSource> = match std::env::var_os("VOICEFLOW_E2E_WAV") {
                Some(path) => {
                    tracing::warn!("E2E build: microphone replaced by test WAV");
                    Arc::new(audio::file_source::WavFileSource { path: path.into() })
                }
                None => audio_source,
            };
            let controller = Controller::new(
                audio_source,
                vad,
                speech.clone(),
                delivery.clone(),
                Arc::new(TauriSink(handle.clone())),
                controller_config(&settings),
            );
            let gemini = Arc::new(GeminiRefiner::new(
                Box::new(|| platform::secrets::load(platform::secrets::GEMINI_KEY)),
                &settings.gemini_model,
            ));
            controller.set_refiner(Some(gemini.clone()));
            controller.set_composer(Some(gemini.clone()));
            controller.set_inserter(Some(Arc::new(PasteInserter::new(handle.clone()))));
            let model_ready = speech.check_ready().is_ok();

            app.manage(AppState {
                controller,
                speech: speech.clone(),
                gemini,
                delivery,
                store,
                settings: Mutex::new(settings.clone()),
                hotkey_status: Mutex::new(HotkeyStatuses::default()),
                downloading: AtomicBool::new(false),
            });

            let statuses = hotkey::register_all(&handle, &settings.hotkey, &settings.compose_hotkey);
            let hotkey_failed = statuses.any_failed();
            *app.state::<AppState>().hotkey_status.lock().unwrap() = statuses;

            // First run (no model) or a hotkey problem: open the main window so the user can act.
            windows::create_main(&handle, !model_ready || hotkey_failed)?;
            windows::create_bar(&handle, settings.bar_visible)?;

            if model_ready {
                // Load the model in the background so the first dictation is not slowed by it.
                std::thread::spawn(move || {
                    if let Err(e) = speech.warm_up() {
                        tracing::warn!(error = %e, "model warm-up failed");
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the main window hides it; the app keeps running in the floating bar.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == windows::MAIN {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::toggle_dictation,
            commands::start_dictation,
            commands::stop_dictation,
            commands::cancel_dictation,
            commands::retry_dictation,
            commands::dismiss_dictation,
            commands::copy_last,
            commands::copy_text,
            commands::get_settings,
            commands::update_settings,
            commands::get_hotkey_status,
            commands::list_models,
            commands::download_model,
            commands::show_main_window,
            commands::set_bar_visible,
            commands::quit_app,
            commands::get_refinement_status,
            commands::set_gemini_key,
            commands::clear_gemini_key,
        ])
        .build(tauri::generate_context!())
        .expect("error while building VoiceFlow");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Release the microphone even if we exit mid-recording.
            if let Some(state) = handle.try_state::<AppState>() {
                state.controller.cancel();
            }
        }
    });
}
