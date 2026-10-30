use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

use super::TextDelivery;
use crate::application::{AppError, AppResult, ErrorKind};

pub struct ClipboardDelivery<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> ClipboardDelivery<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> TextDelivery for ClipboardDelivery<R> {
    fn deliver(&self, text: &str) -> AppResult<()> {
        self.app.clipboard().write_text(text.to_string()).map_err(|e| {
            AppError::new(
                ErrorKind::ClipboardFailed,
                format!("Could not copy to the clipboard; the transcript is shown in the VoiceFlow window. ({e})"),
            )
        })
    }
}
