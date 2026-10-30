use serde::Serialize;

use super::errors::{AppError, ErrorKind, Recovery};

pub type SessionId = u64;

/// What the user wants from this recording.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    /// Speech to text (optionally polished), pasted as said.
    #[default]
    Dictate,
    /// Speech is an instruction; an LLM writes the content, which is pasted.
    Compose,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub audio_ms: u64,
    pub transcribe_ms: u64,
    /// Real-time factor: processing time / audio duration. < 1.0 is faster than real time.
    pub rtf: f32,
    pub model: String,
    /// Time spent in cloud refinement (0 when not used).
    pub refine_ms: u64,
}

/// The single source of truth the UI renders. Serialized with a `status` tag.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DictationState {
    Idle,
    #[serde(rename_all = "camelCase")]
    Recording { session: SessionId, started_at_ms: u64, intent: Intent },
    Transcribing { session: SessionId },
    /// LLM stage: polishing a dictation, or writing content for a compose instruction.
    Refining { session: SessionId, intent: Intent },
    #[serde(rename_all = "camelCase")]
    Completed {
        session: SessionId,
        /// Final text (refined if refinement succeeded).
        text: String,
        /// The original transcript, present only when `text` was refined.
        raw_text: Option<String>,
        /// Why refinement was skipped or failed (the raw transcript was delivered instead).
        refinement_note: Option<String>,
        metrics: Metrics,
        /// Pasted at the cursor of the app the user was typing in.
        pasted: bool,
        /// Why it was not pasted (the text was copied instead).
        paste_error: Option<String>,
        /// The text is on the clipboard.
        delivered: bool,
        delivery_error: Option<String>,
    },
    Error {
        session: Option<SessionId>,
        kind: ErrorKind,
        message: String,
        recovery: Recovery,
    },
}

/// Discriminant-only view of [`DictationState`], used for transition rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
    Refining,
    Completed,
    Error,
}

impl DictationState {
    pub fn phase(&self) -> Phase {
        match self {
            Self::Idle => Phase::Idle,
            Self::Recording { .. } => Phase::Recording,
            Self::Transcribing { .. } => Phase::Transcribing,
            Self::Refining { .. } => Phase::Refining,
            Self::Completed { .. } => Phase::Completed,
            Self::Error { .. } => Phase::Error,
        }
    }

    pub fn error(session: Option<SessionId>, err: &AppError) -> Self {
        Self::Error {
            session,
            kind: err.kind,
            message: err.message.clone(),
            recovery: err.recovery(),
        }
    }

    /// True while the pipeline owns the microphone or a transcription job.
    pub fn is_busy(&self) -> bool {
        matches!(self.phase(), Phase::Recording | Phase::Transcribing | Phase::Refining)
    }
}

/// The allowed state graph. Everything else is rejected by the controller.
pub fn can_transition(from: Phase, to: Phase) -> bool {
    use Phase::*;
    match (from, to) {
        // A new recording may begin from any resting state.
        (Idle | Completed | Error, Recording) => true,
        (Recording, Transcribing) => true,
        (Transcribing, Refining) => true,
        (Transcribing | Refining, Completed) => true,
        // Retry re-enters transcription with the audio held in memory.
        (Error, Transcribing) => true,
        // Any state can fail, be cancelled or be dismissed.
        (_, Error) => true,
        (_, Idle) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Phase::*;

    #[test]
    fn happy_path_is_allowed() {
        assert!(can_transition(Idle, Recording));
        assert!(can_transition(Recording, Transcribing));
        assert!(can_transition(Transcribing, Refining));
        assert!(can_transition(Refining, Completed));
        assert!(can_transition(Transcribing, Completed));
        assert!(can_transition(Completed, Recording));
    }

    #[test]
    fn overlapping_recordings_are_rejected() {
        assert!(!can_transition(Recording, Recording));
        assert!(!can_transition(Transcribing, Recording));
        assert!(!can_transition(Refining, Recording));
    }

    #[test]
    fn cannot_complete_without_processing() {
        assert!(!can_transition(Idle, Completed));
        assert!(!can_transition(Recording, Completed));
        assert!(!can_transition(Idle, Transcribing));
    }

    #[test]
    fn every_state_can_fail_or_reset() {
        for p in [Idle, Recording, Transcribing, Refining, Completed, Error] {
            assert!(can_transition(p, Error));
            assert!(can_transition(p, Idle));
        }
    }

    #[test]
    fn serializes_with_status_tag() {
        let s = DictationState::Recording { session: 3, started_at_ms: 10, intent: Intent::Compose };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["status"], "recording");
        assert_eq!(json["startedAtMs"], 10);
        assert_eq!(json["intent"], "compose");
    }
}
