//! Text refinement seam. `Verbatim` never calls a provider; other modes use the configured
//! provider (Gemini today) and fall back to the raw transcript if it is unavailable or fails.

use serde::{Deserialize, Serialize};

use crate::application::AppResult;

pub mod gemini;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefinementMode {
    /// No refinement: the transcript is delivered as recognised.
    Verbatim,
    Clean,
    #[default]
    Polished,
    Developer,
    Custom,
}

pub trait RefinementProvider: Send + Sync {
    /// Whether the provider can be used right now (e.g. an API key is configured).
    fn is_available(&self) -> bool;

    /// Blocking. Returns the refined text; errors are non-fatal to dictation.
    fn refine(&self, text: &str, mode: RefinementMode) -> AppResult<String>;
}

/// Writes new content from a spoken instruction ("create a 50% discount email").
pub trait ContentGenerator: Send + Sync {
    fn is_available(&self) -> bool;

    /// Blocking. Returns paste-ready content.
    fn generate(&self, instruction: &str) -> AppResult<String>;
}
