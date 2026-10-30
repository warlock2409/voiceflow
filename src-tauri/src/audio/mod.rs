//! Microphone capture seam. Implementations deliver 16 kHz mono f32 so every
//! speech provider receives one canonical format.

use crate::application::{AppError, AppResult, ErrorKind};

pub mod capture;
#[cfg(feature = "e2e-audio")]
pub mod file_source;
pub mod resample;
pub mod vad;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Invoked from the audio thread when the device fails mid-recording.
pub type FaultCallback = Box<dyn Fn(AppError) + Send + Sync>;

pub trait AudioSource: Send + Sync {
    fn start(&self, on_fault: FaultCallback) -> AppResult<Box<dyn Recording>>;
}

/// A live capture. Dropping it without `stop` discards the audio (cancel).
pub trait Recording: Send {
    fn stop(self: Box<Self>) -> AppResult<Vec<f32>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeechSpan {
    pub start_ms: u32,
    pub end_ms: u32,
}

/// Optional pipeline stage between capture and transcription.
pub trait VoiceDetector: Send + Sync {
    /// Speech regions in 16 kHz mono audio; empty means no speech.
    fn speech_spans(&self, pcm: &[f32]) -> AppResult<Vec<SpeechSpan>>;
}

/// Trim leading/trailing non-speech (keeping `pad_ms` either side) so Whisper processes less audio.
pub fn crop_to_speech<'a>(pcm: &'a [f32], spans: &[SpeechSpan], sample_rate: u32, pad_ms: u32) -> &'a [f32] {
    let (Some(first), Some(last)) = (spans.first(), spans.last()) else { return pcm };
    let to_idx = |ms: u32| (ms as usize * sample_rate as usize / 1000).min(pcm.len());
    let start = to_idx(first.start_ms.saturating_sub(pad_ms));
    let end = to_idx(last.end_ms.saturating_add(pad_ms));
    if start >= end { pcm } else { &pcm[start..end] }
}

const MIN_SPEECH_MS: usize = 300;
const FRAME_MS: usize = 30;
/// Loudest 30 ms frame must exceed this RMS. Typical room noise on a laptop mic
/// sits well below it; normal speech is roughly 0.02-0.3.
const SPEECH_RMS_THRESHOLD: f32 = 0.008;

/// Rejects empty, too-short or silent recordings before they reach the model,
/// because Whisper tends to hallucinate text ("Thank you.") on silence.
pub fn check_has_speech(samples: &[f32], sample_rate: u32) -> AppResult<()> {
    let min_len = sample_rate as usize * MIN_SPEECH_MS / 1000;
    if samples.len() < min_len {
        return Err(AppError::new(ErrorKind::NoSpeech, "Recording was too short. Hold on a little longer."));
    }
    let frame = (sample_rate as usize * FRAME_MS / 1000).max(1);
    let loudest = samples
        .chunks(frame)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / c.len() as f32).sqrt())
        .fold(0.0f32, f32::max);
    tracing::debug!(peak_frame_rms = loudest, "speech gate");
    if loudest < SPEECH_RMS_THRESHOLD {
        return Err(AppError::new(
            ErrorKind::NoSpeech,
            "No speech detected. Check that the right microphone is selected and unmuted.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_short_audio() {
        let err = check_has_speech(&[0.5; 100], 16_000).unwrap_err();
        assert_eq!(err.kind, ErrorKind::NoSpeech);
    }

    #[test]
    fn rejects_silence_and_low_noise() {
        assert!(check_has_speech(&[0.0; 16_000], 16_000).is_err());
        let noise: Vec<f32> = (0..16_000).map(|i| if i % 2 == 0 { 0.002 } else { -0.002 }).collect();
        assert!(check_has_speech(&noise, 16_000).is_err());
    }

    #[test]
    fn crop_keeps_padded_speech_region() {
        let pcm: Vec<f32> = (0..16_000).map(|i| i as f32).collect(); // 1 s
        let spans = [SpeechSpan { start_ms: 300, end_ms: 400 }, SpeechSpan { start_ms: 600, end_ms: 700 }];
        let out = crop_to_speech(&pcm, &spans, 16_000, 100);
        assert_eq!(out.first().copied(), Some(3_200.0)); // 200 ms
        assert_eq!(out.len(), 9_600); // 200..800 ms
    }

    #[test]
    fn crop_clamps_to_bounds_and_handles_no_spans() {
        let pcm = vec![0.0; 16_000];
        let spans = [SpeechSpan { start_ms: 50, end_ms: 990 }];
        assert_eq!(crop_to_speech(&pcm, &spans, 16_000, 200).len(), 16_000);
        assert_eq!(crop_to_speech(&pcm, &[], 16_000, 200).len(), 16_000);
    }

    #[test]
    fn accepts_brief_loud_segment_in_quiet_audio() {
        let mut s = vec![0.0; 16_000];
        for (i, v) in s[8_000..9_600].iter_mut().enumerate() {
            *v = 0.2 * (i as f32 * 0.2).sin();
        }
        assert!(check_has_speech(&s, 16_000).is_ok());
    }
}
