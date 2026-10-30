//! Text delivery: where finished text goes.
//! - `TextDelivery`: put text on the clipboard (always available; the fallback).
//! - `TextInserter`: insert text into the app the user was typing in.

use crate::application::AppResult;

pub mod clipboard;
pub mod paste;

pub trait TextDelivery: Send + Sync {
    fn deliver(&self, text: &str) -> AppResult<()>;
}

pub trait TextInserter: Send + Sync {
    /// Insert `text` at the cursor of `target` (the window that was focused when dictation
    /// started). Must refuse rather than insert into a different window.
    /// `keep_on_clipboard`: leave the text on the clipboard afterwards instead of restoring
    /// what was there before.
    fn insert(&self, text: &str, target: Option<isize>, keep_on_clipboard: bool) -> AppResult<()>;
}
