//! Voice activity detection using whisper.cpp's bundled Silero VAD.
//! Runs before transcription: no detected speech means no Whisper call at all,
//! which is the reliable way to stop Whisper inventing words from noise.

use std::sync::Mutex;

use whisper_rs::{WhisperVadContext, WhisperVadContextParams, WhisperVadParams};

use super::{SpeechSpan, VoiceDetector};
use crate::application::{AppError, AppResult, ErrorKind};

/// Bundled Silero VAD model (MIT), shipped as an app resource.
pub const SILERO_MODEL_FILE: &str = "ggml-silero-v5.1.2.bin";

pub struct SileroVad {
    ctx: Mutex<WhisperVadContext>,
}

impl SileroVad {
    pub fn load(model_path: &str) -> AppResult<Self> {
        let mut params = WhisperVadContextParams::default();
        params.set_n_threads(2);
        let ctx = WhisperVadContext::new(model_path, params)
            .map_err(|e| AppError::new(ErrorKind::ModelCorrupt, format!("Voice detection model failed to load ({e}).")))?;
        Ok(Self { ctx: Mutex::new(ctx) })
    }
}

impl VoiceDetector for SileroVad {
    fn speech_spans(&self, pcm: &[f32]) -> AppResult<Vec<SpeechSpan>> {
        let mut params = WhisperVadParams::default();
        params.set_min_speech_duration(200);
        let mut ctx = self.ctx.lock().unwrap_or_else(|e| e.into_inner());
        let segments = ctx
            .segments_from_samples(params, pcm)
            .map_err(|e| AppError::new(ErrorKind::TranscriptionFailed, format!("Voice detection failed ({e}).")))?;
        // Timestamps are in centiseconds.
        Ok(segments
            .map(|s| SpeechSpan { start_ms: (s.start * 10.0) as u32, end_ms: (s.end * 10.0) as u32 })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn vad() -> SileroVad {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources").join(SILERO_MODEL_FILE);
        SileroVad::load(p.to_str().unwrap()).unwrap()
    }

    fn lcg_noise(secs: usize, amplitude: f32) -> Vec<f32> {
        let mut seed: u32 = 12345;
        (0..16_000 * secs)
            .map(|_| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                ((seed >> 8) as f32 / (1u32 << 24) as f32 - 0.5) * 2.0 * amplitude
            })
            .collect()
    }

    #[test]
    fn no_speech_in_noise_or_silence() {
        let v = vad();
        assert!(v.speech_spans(&lcg_noise(3, 0.05)).unwrap().is_empty());
        assert!(v.speech_spans(&vec![0.0; 16_000 * 2]).unwrap().is_empty());
    }

    #[test]
    fn finds_speech_in_fixture() {
        let wav = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/jfk.wav");
        let pcm: Vec<f32> = hound::WavReader::open(wav)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32_768.0)
            .collect();
        let spans = vad().speech_spans(&pcm).unwrap();
        assert!(!spans.is_empty());
        let speech_ms: u32 = spans.iter().map(|s| s.end_ms - s.start_ms).sum();
        assert!(speech_ms > 5_000, "only {speech_ms} ms of speech found");
    }
}
