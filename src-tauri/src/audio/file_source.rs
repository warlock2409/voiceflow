//! Test-only audio source (cargo feature `e2e-audio`): "records" a 16 kHz mono WAV file
//! instead of the microphone, so the full app (hotkey -> VAD -> Whisper -> refinement ->
//! paste) can be exercised deterministically. Never compiled into normal builds.

use std::path::PathBuf;

use super::{AudioSource, FaultCallback, Recording, TARGET_SAMPLE_RATE};
use crate::application::{AppError, AppResult, ErrorKind};

pub struct WavFileSource {
    pub path: PathBuf,
}

struct WavRecording(PathBuf);

impl AudioSource for WavFileSource {
    fn start(&self, _on_fault: FaultCallback) -> AppResult<Box<dyn Recording>> {
        Ok(Box::new(WavRecording(self.path.clone())))
    }
}

impl Recording for WavRecording {
    fn stop(self: Box<Self>) -> AppResult<Vec<f32>> {
        let err = |e: hound::Error| AppError::new(ErrorKind::MicUnavailable, format!("Test WAV unreadable: {e}"));
        let mut reader = hound::WavReader::open(&self.0).map_err(err)?;
        if reader.spec().sample_rate != TARGET_SAMPLE_RATE || reader.spec().channels != 1 {
            return Err(AppError::new(ErrorKind::MicUnavailable, "Test WAV must be 16 kHz mono."));
        }
        reader.samples::<i16>().map(|s| s.map(|v| v as f32 / 32_768.0).map_err(err)).collect()
    }
}
