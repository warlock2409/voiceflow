use serde::Serialize;

/// Machine-readable error category, mirrored in `packages/shared-types`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    MicPermission,
    MicUnavailable,
    NoSpeech,
    ModelMissing,
    ModelCorrupt,
    ModelDownloadFailed,
    TranscriptionFailed,
    /// Cloud refinement failed; dictation still completes with the raw transcript.
    RefinementFailed,
    /// Content generation failed (nothing is pasted; the recording can be retried).
    GenerationFailed,
    /// Content generation needs an API key that is not configured.
    GenerationUnavailable,
    HotkeyConflict,
    ClipboardFailed,
    Busy,
    Internal,
}

/// The safe next step the UI should offer for an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Recovery {
    /// Re-run transcription on the audio still held in memory.
    Retry,
    RecordAgain,
    DownloadModel,
    CheckMicrophone,
    ChangeHotkey,
    CopyManually,
    OpenSettings,
    Dismiss,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{message}")]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
}

impl AppError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }

    pub fn recovery(&self) -> Recovery {
        match self.kind {
            ErrorKind::MicPermission | ErrorKind::MicUnavailable => Recovery::CheckMicrophone,
            ErrorKind::NoSpeech => Recovery::RecordAgain,
            ErrorKind::ModelMissing | ErrorKind::ModelCorrupt | ErrorKind::ModelDownloadFailed => {
                Recovery::DownloadModel
            }
            ErrorKind::TranscriptionFailed | ErrorKind::GenerationFailed => Recovery::Retry,
            ErrorKind::GenerationUnavailable => Recovery::OpenSettings,
            ErrorKind::HotkeyConflict => Recovery::ChangeHotkey,
            ErrorKind::ClipboardFailed => Recovery::CopyManually,
            ErrorKind::RefinementFailed | ErrorKind::Busy | ErrorKind::Internal => Recovery::Dismiss,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
