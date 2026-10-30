//! Insert text at the user's cursor: clipboard + synthetic Ctrl+V.
//!
//! This is the most compatible method on Windows (browsers, Electron apps like Slack,
//! IDEs, Office, terminals). Typing characters one by one is slower and breaks on
//! auto-complete/IME; UI Automation is not supported by many apps.

use std::time::Duration;

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use super::TextInserter;
use crate::application::{AppError, AppResult, ErrorKind};
use crate::platform::{self, input};

/// How long the target app gets to read the clipboard before we restore the old contents.
const RESTORE_DELAY: Duration = Duration::from_millis(600);
const MODIFIER_WAIT: Duration = Duration::from_millis(1500);

pub struct PasteInserter<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> PasteInserter<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

/// Why pasting into `current` would be unsafe, given the window dictation started in.
pub fn target_problem(target: Option<isize>, current: Option<isize>, own_window: bool) -> Option<&'static str> {
    match (target, current) {
        (_, None) => Some("No window is active to paste into."),
        (_, Some(_)) if own_window => Some("The VoiceFlow window is active, so there is nowhere to paste."),
        (Some(t), Some(c)) if t != c => {
            Some("You switched windows while VoiceFlow was working, so it did not paste into the wrong place.")
        }
        _ => None,
    }
}

impl<R: Runtime> TextInserter for PasteInserter<R> {
    fn insert(&self, text: &str, target: Option<isize>, keep_on_clipboard: bool) -> AppResult<()> {
        let fail = |msg: &str| AppError::new(ErrorKind::ClipboardFailed, msg);
        let current = platform::foreground_window();
        if let Some(problem) = target_problem(target, current, current.is_some_and(input::is_own_window)) {
            return Err(fail(problem));
        }
        if !input::wait_for_modifiers_released(MODIFIER_WAIT) {
            return Err(fail("A modifier key (Ctrl/Shift/Alt/Win) was still held, so VoiceFlow did not paste."));
        }

        let clipboard = self.app.clipboard();
        // Only plain text can be restored; images/files on the clipboard are left replaced.
        let previous = if keep_on_clipboard { None } else { clipboard.read_text().ok() };
        clipboard
            .write_text(text.to_string())
            .map_err(|e| fail(&format!("Could not use the clipboard ({e}).")))?;
        std::thread::sleep(Duration::from_millis(30));

        // Re-check: focus could have moved while we waited for modifiers.
        if platform::foreground_window() != current {
            return Err(fail("The active window changed just before pasting, so VoiceFlow did not paste."));
        }
        input::send_ctrl_v().map_err(|e| fail(&e))?;

        if let Some(previous) = previous {
            let app = self.app.clone();
            let ours = text.to_string();
            std::thread::spawn(move || {
                std::thread::sleep(RESTORE_DELAY);
                let clipboard = app.clipboard();
                // Don't clobber something the user copied in the meantime.
                if clipboard.read_text().ok().as_deref() == Some(ours.as_str()) {
                    let _ = clipboard.write_text(previous);
                }
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pastes_only_into_the_window_dictation_started_in() {
        assert_eq!(target_problem(Some(10), Some(10), false), None);
        assert!(target_problem(Some(10), Some(11), false).unwrap().contains("switched windows"));
        assert!(target_problem(Some(10), None, false).is_some());
        assert!(target_problem(Some(10), Some(10), true).unwrap().contains("VoiceFlow window"));
        // Started from the bar with no known target: paste into whatever is active.
        assert_eq!(target_problem(None, Some(5), false), None);
    }
}
