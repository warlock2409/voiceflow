//! Global shortcuts: one for dictation, one for content creation. Desktop only.
//! `CapsLock` is handled by a keyboard hook (see `caps_lock.rs`); every other shortcut
//! must include a modifier and goes through the global-shortcut plugin.

use std::str::FromStr;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::application::{DictationState, Intent};
use crate::storage::RecordingMode;
use crate::AppState;

pub const STATUS_EVENT: &str = "hotkey://status";

/// Special shortcut value: Caps Lock alone, handled by a keyboard hook (see `caps_lock.rs`).
pub const CAPS_LOCK: &str = "CapsLock";

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    pub shortcut: String,
    pub registered: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatuses {
    pub dictate: HotkeyStatus,
    pub compose: HotkeyStatus,
}

impl HotkeyStatuses {
    pub fn any_failed(&self) -> bool {
        self.dictate.error.is_some() || self.compose.error.is_some()
    }
}

/// Plugin shortcut id -> what it does.
static PLUGIN_INTENTS: Mutex<Vec<(u32, Intent)>> = Mutex::new(Vec::new());

pub fn is_caps_lock(shortcut: &str) -> bool {
    let s: String = shortcut.chars().filter(|c| !c.is_whitespace()).collect();
    s.eq_ignore_ascii_case(CAPS_LOCK)
}

/// Validate a shortcut string without touching OS registration.
pub fn parse(shortcut: &str) -> Result<Shortcut, String> {
    let parsed = Shortcut::from_str(shortcut).map_err(|e| format!("'{shortcut}' is not a valid shortcut ({e})."))?;
    if parsed.mods.is_empty() {
        // A bare key would hijack normal typing system-wide.
        return Err("Use at least one modifier (Ctrl, Alt, Shift or Win), or CapsLock on its own.".into());
    }
    Ok(parsed)
}

/// Accepts a modifier combo, or `CapsLock` where the platform supports it.
pub fn validate(shortcut: &str) -> Result<(), String> {
    if is_caps_lock(shortcut) {
        return if cfg!(target_os = "windows") {
            Ok(())
        } else {
            Err("CapsLock dictation is only available on Windows.".into())
        };
    }
    parse(shortcut).map(|_| ())
}

/// Validate both shortcuts together. An empty compose shortcut disables content creation.
pub fn validate_pair(dictate: &str, compose: &str) -> Result<(), String> {
    validate(dictate).map_err(|e| format!("Dictation shortcut: {e}"))?;
    if compose.trim().is_empty() {
        return Ok(());
    }
    validate(compose).map_err(|e| format!("Content creation shortcut: {e}"))?;
    if same_shortcut(dictate, compose) {
        return Err("The dictation and content creation shortcuts must be different.".into());
    }
    Ok(())
}

fn same_shortcut(a: &str, b: &str) -> bool {
    if is_caps_lock(a) || is_caps_lock(b) {
        return is_caps_lock(a) && is_caps_lock(b);
    }
    match (Shortcut::from_str(a), Shortcut::from_str(b)) {
        (Ok(x), Ok(y)) => x.id() == y.id(),
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// Replace all registrations. Failure usually means another app owns the combo.
pub fn register_all<R: Runtime>(app: &AppHandle<R>, dictate: &str, compose: &str) -> HotkeyStatuses {
    let _ = app.global_shortcut().unregister_all();
    super::caps_lock::uninstall();
    PLUGIN_INTENTS.lock().unwrap_or_else(|e| e.into_inner()).clear();

    let dictate_status = register_one(app, dictate, Intent::Dictate);
    let compose_status = if compose.trim().is_empty() {
        HotkeyStatus { shortcut: String::new(), registered: false, error: None }
    } else if same_shortcut(dictate, compose) {
        HotkeyStatus {
            shortcut: compose.to_string(),
            registered: false,
            error: Some("Same as the dictation shortcut.".into()),
        }
    } else {
        register_one(app, compose, Intent::Compose)
    };
    let statuses = HotkeyStatuses { dictate: dictate_status, compose: compose_status };
    let _ = app.emit(STATUS_EVENT, &statuses);
    statuses
}

fn register_one<R: Runtime>(app: &AppHandle<R>, shortcut: &str, intent: Intent) -> HotkeyStatus {
    let result = if is_caps_lock(shortcut) {
        let app = app.clone();
        super::caps_lock::install(move |pressed| on_trigger(&app, pressed, intent))
    } else {
        parse(shortcut).and_then(|s| {
            app.global_shortcut()
                .register(s)
                .map(|()| PLUGIN_INTENTS.lock().unwrap_or_else(|e| e.into_inner()).push((s.id(), intent)))
                .map_err(|e| {
                    format!("'{shortcut}' could not be registered; another app may already use it. Choose a different shortcut. ({e})")
                })
        })
    };
    if let Err(err) = &result {
        tracing::warn!(%err, ?intent, "hotkey registration failed");
    }
    HotkeyStatus { shortcut: shortcut.to_string(), registered: result.is_ok(), error: result.err() }
}

/// Plugin handler for modifier-combo shortcuts.
pub fn handle<R: Runtime>(app: &AppHandle<R>, shortcut: &Shortcut, event: ShortcutEvent) {
    let intent = PLUGIN_INTENTS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|(id, _)| *id == shortcut.id())
        .map(|(_, i)| *i)
        .unwrap_or_default();
    on_trigger(app, event.state() == ShortcutState::Pressed, intent);
}

/// Shared by every trigger source: toggle on press, or press/release in push-to-talk mode.
fn on_trigger<R: Runtime>(app: &AppHandle<R>, pressed: bool, intent: Intent) {
    let state = app.state::<AppState>();
    let mode = state.settings().recording_mode;
    let ctl = &state.controller;
    let result = match (mode, pressed) {
        (RecordingMode::Toggle, true) => ctl.toggle(super::foreground_window(), intent),
        (RecordingMode::PushToTalk, true) => ctl.start(super::foreground_window(), intent),
        // Releasing one shortcut must not stop a recording started by the other.
        (RecordingMode::PushToTalk, false) => match ctl.state() {
            DictationState::Recording { intent: i, .. } if i == intent => ctl.stop(),
            _ => Ok(()),
        },
        (RecordingMode::Toggle, false) => Ok(()),
    };
    if let Err(e) = result {
        tracing::debug!(kind = ?e.kind, "hotkey action ignored");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_rejects_bad_input() {
        assert!(parse("Ctrl+Shift+Space").is_ok());
        assert!(parse("Ctrl+Space").is_ok());
        assert!(parse("Alt+F9").is_ok());
        assert!(parse("Space").is_err(), "bare key must be rejected");
        assert!(parse("Ctrl+Nonsense").is_err());
    }

    #[test]
    fn caps_lock_is_the_one_allowed_bare_key() {
        assert!(is_caps_lock("CapsLock"));
        assert!(is_caps_lock("caps lock"));
        assert!(!is_caps_lock("Ctrl+CapsLock"));
        assert_eq!(validate("CapsLock").is_ok(), cfg!(target_os = "windows"));
        assert!(validate("Space").is_err());
        assert!(validate("Ctrl+Shift+Space").is_ok());
    }

    #[test]
    fn shortcut_pair_must_differ() {
        assert!(validate_pair("CapsLock", "Ctrl+Space").is_ok());
        assert!(validate_pair("CapsLock", "").is_ok(), "empty compose = disabled");
        assert!(validate_pair("CapsLock", "caps lock").is_err());
        assert!(validate_pair("Ctrl+Space", "control+space").is_err());
        assert!(validate_pair("CapsLock", "Space").unwrap_err().starts_with("Content creation shortcut"));
    }
}
