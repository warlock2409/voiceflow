//! Speech-to-text seam. The controller only sees [`SpeechProvider`]; whisper.cpp is
//! one implementation, and hosted or other local engines can be added alongside it.

use crate::application::AppResult;

pub mod model;
pub mod whisper;

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    /// ISO-639-1 code, or "auto" for detection on multilingual models.
    pub language: String,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
    pub model: String,
}

pub trait SpeechProvider: Send + Sync {
    /// Cheap pre-flight check (e.g. model file present) run before the mic opens.
    fn check_ready(&self) -> AppResult<()>;

    /// Blocking. `pcm` is 16 kHz mono f32 in [-1, 1].
    fn transcribe(&self, pcm: &[f32], opts: &TranscribeOptions) -> AppResult<Transcript>;
}
