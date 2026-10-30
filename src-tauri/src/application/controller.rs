//! Recording lifecycle orchestration. Depends only on traits so every stage can be
//! swapped or mocked; knows nothing about Tauri, cpal or whisper.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::audio::{self, AudioSource, FaultCallback, Recording, VoiceDetector, TARGET_SAMPLE_RATE};
use crate::delivery::{TextDelivery, TextInserter};
use crate::refinement::{ContentGenerator, RefinementMode, RefinementProvider};
use crate::transcription::{SpeechProvider, TranscribeOptions};

use super::errors::{AppError, AppResult, ErrorKind};
use super::state::{can_transition, DictationState, Intent, Metrics, Phase, SessionId};

/// Receives every state change (the Tauri layer forwards it to the webviews).
pub trait StateSink: Send + Sync {
    fn publish(&self, state: &DictationState);
}

#[derive(Debug, Clone)]
pub struct ControllerConfig {
    pub language: String,
    /// Keep the final text on the clipboard (and copy it when not pasting).
    pub auto_copy: bool,
    /// Paste at the cursor of the window dictation started in.
    pub auto_paste: bool,
    pub max_recording: Duration,
    pub refinement: RefinementMode,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            language: "en".into(),
            auto_copy: true,
            auto_paste: false,
            max_recording: Duration::from_secs(120),
            refinement: RefinementMode::Verbatim,
        }
    }
}

struct Inner {
    state: DictationState,
    recording: Option<Box<dyn Recording>>,
    /// 16 kHz mono audio, held in memory only so a failed transcription can be retried.
    /// Cleared on success, cancel, dismiss and new recordings. Never written to disk.
    retry_audio: Option<Arc<Vec<f32>>>,
    last_text: Option<String>,
    /// Foreground window when dictation started (where the text is pasted).
    target_window: Option<isize>,
    /// Intent of the current/last session; reused by retry.
    intent: Intent,
}

pub struct Controller {
    inner: Mutex<Inner>,
    next_session: AtomicU64,
    audio: Arc<dyn AudioSource>,
    vad: Option<Arc<dyn VoiceDetector>>,
    speech: Arc<dyn SpeechProvider>,
    refiner: Mutex<Option<Arc<dyn RefinementProvider>>>,
    composer: Mutex<Option<Arc<dyn ContentGenerator>>>,
    inserter: Mutex<Option<Arc<dyn TextInserter>>>,
    delivery: Arc<dyn TextDelivery>,
    sink: Arc<dyn StateSink>,
    config: Mutex<ControllerConfig>,
}

#[derive(Default)]
struct DeliveryReport {
    pasted: bool,
    paste_error: Option<String>,
    on_clipboard: bool,
    clipboard_error: Option<String>,
}

/// Result of a successful dictation, before delivery.
struct Outcome {
    text: String,
    raw_text: Option<String>,
    refinement_note: Option<String>,
    metrics: Metrics,
}

/// Padding kept around detected speech so word onsets/endings are not clipped.
const SPEECH_PAD_MS: u32 = 300;

impl Controller {
    pub fn new(
        audio: Arc<dyn AudioSource>,
        vad: Option<Arc<dyn VoiceDetector>>,
        speech: Arc<dyn SpeechProvider>,
        delivery: Arc<dyn TextDelivery>,
        sink: Arc<dyn StateSink>,
        config: ControllerConfig,
    ) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                state: DictationState::Idle,
                recording: None,
                retry_audio: None,
                last_text: None,
                target_window: None,
                intent: Intent::Dictate,
            }),
            next_session: AtomicU64::new(1),
            audio,
            vad,
            speech,
            refiner: Mutex::new(None),
            composer: Mutex::new(None),
            inserter: Mutex::new(None),
            delivery,
            sink,
            config: Mutex::new(config),
        })
    }

    pub fn state(&self) -> DictationState {
        self.lock().state.clone()
    }

    pub fn target_window(&self) -> Option<isize> {
        self.lock().target_window
    }

    pub fn set_refiner(&self, refiner: Option<Arc<dyn RefinementProvider>>) {
        *self.refiner.lock().unwrap_or_else(|e| e.into_inner()) = refiner;
    }

    pub fn set_composer(&self, composer: Option<Arc<dyn ContentGenerator>>) {
        *self.composer.lock().unwrap_or_else(|e| e.into_inner()) = composer;
    }

    pub fn set_inserter(&self, inserter: Option<Arc<dyn TextInserter>>) {
        *self.inserter.lock().unwrap_or_else(|e| e.into_inner()) = inserter;
    }

    pub fn set_config(&self, config: ControllerConfig) {
        *self.config.lock().unwrap_or_else(|e| e.into_inner()) = config;
    }

    fn config(&self) -> ControllerConfig {
        self.config.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Hotkey behaviour: start when resting, stop when recording, ignore while processing.
    pub fn toggle(self: &Arc<Self>, target_window: Option<isize>, intent: Intent) -> AppResult<()> {
        match self.state().phase() {
            Phase::Recording => self.stop(),
            Phase::Transcribing | Phase::Refining => Ok(()),
            _ => self.start(target_window, intent),
        }
    }

    pub fn start(self: &Arc<Self>, target_window: Option<isize>, intent: Intent) -> AppResult<()> {
        let mut inner = self.lock();
        if !can_transition(inner.state.phase(), Phase::Recording) {
            return Err(AppError::new(ErrorKind::Busy, "Dictation is already in progress."));
        }
        let session = self.next_session.fetch_add(1, Ordering::SeqCst);
        inner.retry_audio = None;
        inner.target_window = target_window;
        inner.intent = intent;

        // Fail before opening the mic if the request cannot possibly succeed.
        let ready = self.speech.check_ready().and_then(|()| match intent {
            Intent::Compose if !self.composer_available() => Err(AppError::new(
                ErrorKind::GenerationUnavailable,
                "Content creation needs a Gemini API key. Add one under AI text refinement in the VoiceFlow window.",
            )),
            _ => Ok(()),
        });
        if let Err(err) = ready {
            self.set(&mut inner, DictationState::error(Some(session), &err));
            return Err(err);
        }

        let weak = Arc::downgrade(self);
        let on_fault: FaultCallback = Box::new(move |err: AppError| {
            if let Some(this) = weak.upgrade() {
                this.on_capture_fault(session, err);
            }
        });
        let recording = match self.audio.start(on_fault) {
            Ok(r) => r,
            Err(err) => {
                self.set(&mut inner, DictationState::error(Some(session), &err));
                return Err(err);
            }
        };
        inner.recording = Some(recording);
        self.set(
            &mut inner,
            DictationState::Recording { session, started_at_ms: now_ms(), intent },
        );
        drop(inner);

        // Hard cap so a forgotten toggle cannot record indefinitely.
        let weak = Arc::downgrade(self);
        let cap = self.config().max_recording;
        std::thread::spawn(move || {
            std::thread::sleep(cap);
            if let Some(this) = weak.upgrade() {
                if matches!(this.state(), DictationState::Recording { session: s, .. } if s == session) {
                    tracing::info!(session, "recording cap reached; stopping");
                    let _ = this.stop();
                }
            }
        });
        Ok(())
    }

    pub fn stop(self: &Arc<Self>) -> AppResult<()> {
        let mut inner = self.lock();
        let DictationState::Recording { session, .. } = inner.state else {
            return Err(AppError::new(ErrorKind::Busy, "Not recording."));
        };
        let Some(recording) = inner.recording.take() else {
            return Err(AppError::new(ErrorKind::Internal, "Recording handle missing."));
        };
        self.set(&mut inner, DictationState::Transcribing { session });
        drop(inner);

        let this = Arc::clone(self);
        std::thread::spawn(move || {
            let samples = match recording.stop() {
                Ok(s) => s,
                Err(err) => return this.finish(session, Err(err), None),
            };
            this.process(session, Arc::new(samples));
        });
        Ok(())
    }

    pub fn cancel(&self) {
        let mut inner = self.lock();
        // Dropping the handle stops the stream and discards the audio.
        inner.recording = None;
        inner.retry_audio = None;
        if inner.state.phase() != Phase::Idle {
            self.set(&mut inner, DictationState::Idle);
        }
    }

    /// Re-run transcription on the audio kept from a failed attempt.
    pub fn retry(self: &Arc<Self>) -> AppResult<()> {
        let mut inner = self.lock();
        if inner.state.phase() != Phase::Error {
            return Err(AppError::new(ErrorKind::Busy, "Nothing to retry."));
        }
        let Some(samples) = inner.retry_audio.clone() else {
            return Err(AppError::new(ErrorKind::NoSpeech, "No audio available; please record again."));
        };
        let session = self.next_session.fetch_add(1, Ordering::SeqCst);
        self.set(&mut inner, DictationState::Transcribing { session });
        drop(inner);

        let this = Arc::clone(self);
        std::thread::spawn(move || this.process(session, samples));
        Ok(())
    }

    pub fn dismiss(&self) {
        let mut inner = self.lock();
        if !inner.state.is_busy() {
            inner.retry_audio = None;
            self.set(&mut inner, DictationState::Idle);
        }
    }

    pub fn copy_last(&self) -> AppResult<()> {
        let text = self.lock().last_text.clone();
        match text {
            Some(t) => self.delivery.deliver(&t),
            None => Err(AppError::new(ErrorKind::Busy, "No transcript to copy yet.")),
        }
    }

    pub fn last_text(&self) -> Option<String> {
        self.lock().last_text.clone()
    }

    /// Runs on a worker thread: level gate, voice detection, transcription, delivery.
    fn process(&self, session: SessionId, samples: Arc<Vec<f32>>) {
        if let Err(err) = audio::check_has_speech(&samples, TARGET_SAMPLE_RATE) {
            return self.finish(session, Err(err), None);
        }
        // Whisper invents words ("you", "Thank you.") when fed noise, so it only runs
        // when the VAD finds speech, and only on the speech region.
        let started = Instant::now();
        let spans = match &self.vad {
            Some(vad) => match vad.speech_spans(&samples) {
                Ok(spans) if spans.is_empty() => {
                    let err = AppError::new(ErrorKind::NoSpeech, "No speech detected. Try again, closer to the microphone.");
                    return self.finish(session, Err(err), None);
                }
                Ok(spans) => spans,
                Err(e) => {
                    tracing::warn!(error = %e, "voice detection failed; transcribing full audio");
                    Vec::new()
                }
            },
            None => Vec::new(),
        };
        let speech = audio::crop_to_speech(&samples, &spans, TARGET_SAMPLE_RATE, SPEECH_PAD_MS);

        let opts = TranscribeOptions { language: self.config().language };
        let result = self.speech.transcribe(speech, &opts).map(|t| {
            let transcribe_ms = started.elapsed().as_millis() as u64;
            let audio_ms = samples.len() as u64 * 1000 / TARGET_SAMPLE_RATE as u64;
            let rtf = if audio_ms == 0 { 0.0 } else { transcribe_ms as f32 / audio_ms as f32 };
            tracing::info!(session, audio_ms, transcribe_ms, rtf, model = %t.model, "transcribed");
            (t.text, Metrics { audio_ms, transcribe_ms, rtf, model: t.model, refine_ms: 0 })
        });
        let result = result.and_then(|(text, metrics)| {
            if text.trim().is_empty() {
                Err(AppError::new(ErrorKind::NoSpeech, "No speech was recognised. Try again, closer to the microphone."))
            } else {
                Ok((text, metrics))
            }
        });
        let intent = self.lock().intent;
        let outcome = match result {
            Ok((text, metrics)) => {
                let staged = match intent {
                    Intent::Dictate => self.refine_stage(session, text, metrics).map(Ok),
                    Intent::Compose => self.compose_stage(session, text, metrics),
                };
                match staged {
                    Some(outcome) => outcome,
                    None => return, // cancelled while the LLM was working
                }
            }
            Err(e) => Err(e),
        };
        self.finish(session, outcome, Some(samples));
    }

    fn composer_available(&self) -> bool {
        self.composer.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|c| c.is_available())
    }

    /// Compose: the transcript is an instruction; the generated content is what gets pasted.
    /// Unlike refinement there is no raw-text fallback: pasting the instruction itself into
    /// an email would be wrong, so failures become a retryable error.
    fn compose_stage(&self, session: SessionId, instruction: String, mut metrics: Metrics) -> Option<AppResult<Outcome>> {
        let composer = self.composer.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let Some(composer) = composer else {
            return Some(Err(AppError::new(ErrorKind::GenerationUnavailable, "Content creation is not configured.")));
        };
        {
            let mut inner = self.lock();
            if !matches!(inner.state, DictationState::Transcribing { session: s } if s == session) {
                return None;
            }
            self.set(&mut inner, DictationState::Refining { session, intent: Intent::Compose });
        }
        let started = Instant::now();
        let result = composer.generate(&instruction);
        metrics.refine_ms = started.elapsed().as_millis() as u64;
        tracing::info!(session, generate_ms = metrics.refine_ms, ok = result.is_ok(), "generated");
        Some(result.map(|content| Outcome {
            text: content,
            raw_text: Some(instruction),
            refinement_note: None,
            metrics,
        }))
    }

    /// Optional cloud refinement. Never fails the dictation: on any problem the raw
    /// transcript is delivered with a note. Returns `None` if the session went stale.
    fn refine_stage(&self, session: SessionId, text: String, mut metrics: Metrics) -> Option<Outcome> {
        let mode = self.config().refinement;
        let refiner = self.refiner.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let raw = Outcome { text: text.clone(), raw_text: None, refinement_note: None, metrics: metrics.clone() };
        let Some(refiner) = refiner.filter(|_| mode != RefinementMode::Verbatim) else { return Some(raw) };
        if !refiner.is_available() {
            return Some(raw);
        }
        {
            let mut inner = self.lock();
            if !matches!(inner.state, DictationState::Transcribing { session: s } if s == session) {
                return None;
            }
            self.set(&mut inner, DictationState::Refining { session, intent: Intent::Dictate });
        }
        let started = Instant::now();
        let result = refiner.refine(&text, mode);
        metrics.refine_ms = started.elapsed().as_millis() as u64;
        tracing::info!(session, refine_ms = metrics.refine_ms, ok = result.is_ok(), "refined");
        Some(match result {
            Ok(refined) => Outcome { text: refined, raw_text: Some(text), refinement_note: None, metrics },
            Err(e) => Outcome { refinement_note: Some(e.message), metrics, ..raw },
        })
    }

    fn is_current(inner: &Inner, session: SessionId) -> bool {
        matches!(
            inner.state,
            DictationState::Transcribing { session: s } | DictationState::Refining { session: s, .. } if s == session
        )
    }

    fn finish(&self, session: SessionId, result: AppResult<Outcome>, samples: Option<Arc<Vec<f32>>>) {
        // Pasting waits on the OS (modifier release, clipboard), so it runs outside the lock;
        // the session is checked before and after so a cancel is still honoured.
        let delivery = match &result {
            Ok(outcome) => {
                let target = {
                    let inner = self.lock();
                    if !Self::is_current(&inner, session) {
                        tracing::debug!(session, "dropping stale transcription result");
                        return;
                    }
                    inner.target_window
                };
                Some(self.deliver_text(&outcome.text, target))
            }
            Err(_) => None,
        };

        let mut inner = self.lock();
        // A cancelled or superseded session must not overwrite the current state.
        if !Self::is_current(&inner, session) {
            tracing::debug!(session, "dropping stale transcription result");
            return;
        }
        match result {
            Ok(Outcome { text, raw_text, refinement_note, metrics }) => {
                let report = delivery.unwrap_or_default();
                inner.retry_audio = None;
                inner.last_text = Some(text.clone());
                self.set(
                    &mut inner,
                    DictationState::Completed {
                        session,
                        text,
                        raw_text,
                        refinement_note,
                        metrics,
                        pasted: report.pasted,
                        paste_error: report.paste_error,
                        delivered: report.on_clipboard,
                        delivery_error: report.clipboard_error,
                    },
                );
            }
            Err(err) => {
                inner.retry_audio = match err.kind {
                    ErrorKind::TranscriptionFailed
                    | ErrorKind::GenerationFailed
                    | ErrorKind::ModelCorrupt
                    | ErrorKind::ModelMissing => samples,
                    _ => None,
                };
                self.set(&mut inner, DictationState::error(Some(session), &err));
            }
        }
    }

    /// Paste at the cursor if enabled; otherwise (or if pasting is refused/fails) copy
    /// to the clipboard so the text is never lost.
    fn deliver_text(&self, text: &str, target: Option<isize>) -> DeliveryReport {
        let cfg = self.config();
        let mut report = DeliveryReport::default();
        if cfg.auto_paste {
            let inserter = self.inserter.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some(inserter) = inserter {
                match inserter.insert(text, target, cfg.auto_copy) {
                    Ok(()) => {
                        report.pasted = true;
                        // When kept, the inserter left the text on the clipboard.
                        report.on_clipboard = cfg.auto_copy;
                        return report;
                    }
                    Err(e) => {
                        tracing::info!(reason = %e.message, "paste skipped; copying instead");
                        report.paste_error = Some(e.message);
                    }
                }
            }
        }
        if cfg.auto_copy || report.paste_error.is_some() {
            match self.delivery.deliver(text) {
                Ok(()) => report.on_clipboard = true,
                Err(e) => report.clipboard_error = Some(e.message),
            }
        }
        report
    }

    fn on_capture_fault(&self, session: SessionId, err: AppError) {
        let mut inner = self.lock();
        if matches!(inner.state, DictationState::Recording { session: s, .. } if s == session) {
            tracing::warn!(session, kind = ?err.kind, "capture fault");
            inner.recording = None;
            self.set(&mut inner, DictationState::error(Some(session), &err));
        }
    }

    fn set(&self, inner: &mut MutexGuard<'_, Inner>, next: DictationState) {
        debug_assert!(
            can_transition(inner.state.phase(), next.phase()),
            "illegal transition {:?} -> {:?}",
            inner.state.phase(),
            next.phase()
        );
        tracing::debug!(from = ?inner.state.phase(), to = ?next.phase(), "state");
        inner.state = next;
        self.sink.publish(&inner.state);
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcription::Transcript;
    use std::sync::mpsc;

    struct FakeRecording(Vec<f32>);
    impl Recording for FakeRecording {
        fn stop(self: Box<Self>) -> AppResult<Vec<f32>> {
            Ok(self.0)
        }
    }

    struct FakeAudio {
        samples: Vec<f32>,
        fail: Option<AppError>,
        fault: Mutex<Option<FaultCallback>>,
    }
    impl AudioSource for FakeAudio {
        fn start(&self, on_fault: FaultCallback) -> AppResult<Box<dyn Recording>> {
            if let Some(e) = &self.fail {
                return Err(e.clone());
            }
            *self.fault.lock().unwrap() = Some(on_fault);
            Ok(Box::new(FakeRecording(self.samples.clone())))
        }
    }

    struct FakeSpeech {
        result: Mutex<Vec<AppResult<String>>>,
        gate: Mutex<Option<mpsc::Receiver<()>>>,
        /// Length of the audio passed to the last transcribe call (None = never called).
        seen_len: Mutex<Option<usize>>,
    }
    impl SpeechProvider for FakeSpeech {
        fn check_ready(&self) -> AppResult<()> {
            Ok(())
        }
        fn transcribe(&self, pcm: &[f32], _o: &TranscribeOptions) -> AppResult<Transcript> {
            *self.seen_len.lock().unwrap() = Some(pcm.len());
            if let Some(rx) = self.gate.lock().unwrap().as_ref() {
                let _ = rx.recv_timeout(Duration::from_secs(5));
            }
            let mut r = self.result.lock().unwrap();
            let next = if r.len() > 1 { r.remove(0) } else { r[0].clone() };
            next.map(|text| Transcript { text, model: "fake".into() })
        }
    }

    #[derive(Default)]
    struct FakeDelivery(Mutex<Vec<String>>);
    impl TextDelivery for FakeDelivery {
        fn deliver(&self, text: &str) -> AppResult<()> {
            self.0.lock().unwrap().push(text.to_string());
            Ok(())
        }
    }

    #[derive(Default)]
    struct Recorder(Mutex<Vec<DictationState>>);
    impl StateSink for Recorder {
        fn publish(&self, s: &DictationState) {
            self.0.lock().unwrap().push(s.clone());
        }
    }

    fn speech_samples() -> Vec<f32> {
        // 1 s of a 440 Hz tone at 0.3 amplitude: passes the silence gate.
        (0..TARGET_SAMPLE_RATE)
            .map(|i| 0.3 * (i as f32 * 440.0 * std::f32::consts::TAU / TARGET_SAMPLE_RATE as f32).sin())
            .collect()
    }

    struct Harness {
        ctl: Arc<Controller>,
        sink: Arc<Recorder>,
        delivery: Arc<FakeDelivery>,
        audio: Arc<FakeAudio>,
    }

    fn harness(samples: Vec<f32>, results: Vec<AppResult<String>>, gate: Option<mpsc::Receiver<()>>) -> Harness {
        harness_with_vad(samples, results, gate, None).0
    }

    fn harness_with_vad(
        samples: Vec<f32>,
        results: Vec<AppResult<String>>,
        gate: Option<mpsc::Receiver<()>>,
        vad: Option<Arc<dyn VoiceDetector>>,
    ) -> (Harness, Arc<FakeSpeech>) {
        let sink = Arc::new(Recorder::default());
        let delivery = Arc::new(FakeDelivery::default());
        let audio = Arc::new(FakeAudio { samples, fail: None, fault: Mutex::new(None) });
        let speech = Arc::new(FakeSpeech { result: Mutex::new(results), gate: Mutex::new(gate), seen_len: Mutex::new(None) });
        let ctl = Controller::new(audio.clone(), vad, speech.clone(), delivery.clone(), sink.clone(), ControllerConfig::default());
        (Harness { ctl, sink, delivery, audio }, speech)
    }

    struct FakeVad(Vec<audio::SpeechSpan>);
    impl VoiceDetector for FakeVad {
        fn speech_spans(&self, _pcm: &[f32]) -> AppResult<Vec<audio::SpeechSpan>> {
            Ok(self.0.clone())
        }
    }

    fn wait_for(ctl: &Controller, phase: Phase) -> DictationState {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let s = ctl.state();
            if s.phase() == phase {
                return s;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {phase:?}, at {:?}", s.phase());
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn full_cycle_completes_and_copies() {
        let h = harness(speech_samples(), vec![Ok("Hello world.".into())], None);
        h.ctl.toggle(Some(42), Intent::Dictate).unwrap();
        assert_eq!(h.ctl.state().phase(), Phase::Recording);
        assert_eq!(h.ctl.target_window(), Some(42));
        h.ctl.toggle(None, Intent::Dictate).unwrap();
        let DictationState::Completed { text, delivered, metrics, .. } = wait_for(&h.ctl, Phase::Completed) else {
            unreachable!()
        };
        assert_eq!(text, "Hello world.");
        assert!(delivered);
        assert_eq!(metrics.audio_ms, 1000);
        assert_eq!(h.delivery.0.lock().unwrap().as_slice(), ["Hello world."]);
        let phases: Vec<_> = h.sink.0.lock().unwrap().iter().map(|s| s.phase()).collect();
        assert_eq!(phases, [Phase::Recording, Phase::Transcribing, Phase::Completed]);
    }

    #[test]
    fn second_start_while_recording_is_rejected() {
        let h = harness(speech_samples(), vec![Ok("x".into())], None);
        h.ctl.start(None, Intent::Dictate).unwrap();
        let err = h.ctl.start(None, Intent::Dictate).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Busy);
        assert_eq!(h.ctl.state().phase(), Phase::Recording);
    }

    #[test]
    fn hotkey_during_transcription_is_ignored() {
        let (tx, rx) = mpsc::channel();
        let h = harness(speech_samples(), vec![Ok("done".into())], Some(rx));
        h.ctl.toggle(None, Intent::Dictate).unwrap();
        h.ctl.toggle(None, Intent::Dictate).unwrap();
        wait_for(&h.ctl, Phase::Transcribing);
        h.ctl.toggle(None, Intent::Dictate).unwrap();
        assert_eq!(h.ctl.state().phase(), Phase::Transcribing);
        tx.send(()).unwrap();
        wait_for(&h.ctl, Phase::Completed);
    }

    #[test]
    fn cancel_during_transcription_drops_stale_result() {
        let (tx, rx) = mpsc::channel();
        let h = harness(speech_samples(), vec![Ok("stale".into())], Some(rx));
        h.ctl.start(None, Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
        wait_for(&h.ctl, Phase::Transcribing);
        h.ctl.cancel();
        assert_eq!(h.ctl.state(), DictationState::Idle);
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(h.ctl.state(), DictationState::Idle);
        assert!(h.delivery.0.lock().unwrap().is_empty());
        assert_eq!(h.ctl.last_text(), None);
    }

    #[test]
    fn silence_is_reported_as_no_speech() {
        let h = harness(vec![0.0; TARGET_SAMPLE_RATE as usize], vec![Ok("hallucination".into())], None);
        h.ctl.start(None, Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
        let DictationState::Error { kind, .. } = wait_for(&h.ctl, Phase::Error) else { unreachable!() };
        assert_eq!(kind, ErrorKind::NoSpeech);
        assert!(h.delivery.0.lock().unwrap().is_empty());
    }

    struct FakeRefiner {
        available: bool,
        result: AppResult<String>,
        calls: Mutex<u32>,
        gate: Mutex<Option<mpsc::Receiver<()>>>,
    }
    impl FakeRefiner {
        fn new(available: bool, result: AppResult<String>) -> Arc<Self> {
            Arc::new(Self { available, result, calls: Mutex::new(0), gate: Mutex::new(None) })
        }
    }
    impl RefinementProvider for FakeRefiner {
        fn is_available(&self) -> bool {
            self.available
        }
        fn refine(&self, _text: &str, _mode: RefinementMode) -> AppResult<String> {
            *self.calls.lock().unwrap() += 1;
            if let Some(rx) = self.gate.lock().unwrap().as_ref() {
                let _ = rx.recv_timeout(Duration::from_secs(5));
            }
            self.result.clone()
        }
    }

    fn polished(h: &Harness, refiner: Arc<FakeRefiner>) {
        h.ctl.set_config(ControllerConfig { refinement: RefinementMode::Polished, ..Default::default() });
        h.ctl.set_refiner(Some(refiner));
    }

    fn dictate(h: &Harness) {
        h.ctl.start(None, Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
    }

    struct FakeInserter {
        result: AppResult<()>,
        calls: Mutex<Vec<(String, Option<isize>, bool)>>,
    }
    impl TextInserter for FakeInserter {
        fn insert(&self, text: &str, target: Option<isize>, keep: bool) -> AppResult<()> {
            self.calls.lock().unwrap().push((text.to_string(), target, keep));
            self.result.clone()
        }
    }

    fn with_paste(h: &Harness, result: AppResult<()>, auto_copy: bool) -> Arc<FakeInserter> {
        let ins = Arc::new(FakeInserter { result, calls: Mutex::new(vec![]) });
        h.ctl.set_config(ControllerConfig { auto_paste: true, auto_copy, ..Default::default() });
        h.ctl.set_inserter(Some(ins.clone()));
        ins
    }

    #[test]
    fn pastes_into_the_window_dictation_started_in() {
        let h = harness(speech_samples(), vec![Ok("Hello there.".into())], None);
        let ins = with_paste(&h, Ok(()), false);
        h.ctl.start(Some(777), Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
        let DictationState::Completed { pasted, paste_error, delivered, .. } = wait_for(&h.ctl, Phase::Completed)
        else {
            unreachable!()
        };
        assert!(pasted);
        assert_eq!(paste_error, None);
        assert!(!delivered, "clipboard restored, so the text is not left on it");
        assert_eq!(ins.calls.lock().unwrap().as_slice(), [("Hello there.".to_string(), Some(777), false)]);
        assert!(h.delivery.0.lock().unwrap().is_empty(), "no separate copy needed");
    }

    #[test]
    fn refused_paste_falls_back_to_clipboard() {
        let h = harness(speech_samples(), vec![Ok("Hello there.".into())], None);
        let refused = AppError::new(ErrorKind::ClipboardFailed, "You switched windows while VoiceFlow was working.");
        with_paste(&h, Err(refused), false);
        h.ctl.start(Some(1), Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
        let DictationState::Completed { pasted, paste_error, delivered, .. } = wait_for(&h.ctl, Phase::Completed)
        else {
            unreachable!()
        };
        assert!(!pasted);
        assert!(paste_error.unwrap().contains("switched windows"));
        assert!(delivered, "copied even though auto_copy is off, so the text is never lost");
        assert_eq!(h.delivery.0.lock().unwrap().as_slice(), ["Hello there."]);
    }

    #[test]
    fn keep_on_clipboard_is_passed_through() {
        let h = harness(speech_samples(), vec![Ok("x y".into())], None);
        let ins = with_paste(&h, Ok(()), true);
        dictate(&h);
        let DictationState::Completed { delivered, .. } = wait_for(&h.ctl, Phase::Completed) else { unreachable!() };
        assert!(delivered);
        assert!(ins.calls.lock().unwrap()[0].2);
    }

    struct FakeComposer {
        available: bool,
        results: Mutex<Vec<AppResult<String>>>,
        seen: Mutex<Vec<String>>,
    }
    impl ContentGenerator for FakeComposer {
        fn is_available(&self) -> bool {
            self.available
        }
        fn generate(&self, instruction: &str) -> AppResult<String> {
            self.seen.lock().unwrap().push(instruction.to_string());
            let mut r = self.results.lock().unwrap();
            if r.len() > 1 { r.remove(0) } else { r[0].clone() }
        }
    }

    fn composer(available: bool, results: Vec<AppResult<String>>) -> Arc<FakeComposer> {
        Arc::new(FakeComposer { available, results: Mutex::new(results), seen: Mutex::new(vec![]) })
    }

    #[test]
    fn compose_pastes_generated_content_not_the_instruction() {
        let h = harness(speech_samples(), vec![Ok("create 50% offer discount email".into())], None);
        let email = "Subject: 50% Off Everything\n\nHi [Name],\n\nEnjoy 50% off this week.";
        let c = composer(true, vec![Ok(email.into())]);
        h.ctl.set_composer(Some(c.clone()));
        // Dictation refinement is configured too, but must not run for compose.
        let refiner = FakeRefiner::new(true, Ok("WRONG".into()));
        polished(&h, refiner.clone());
        h.ctl.start(None, Intent::Compose).unwrap();
        assert!(matches!(h.ctl.state(), DictationState::Recording { intent: Intent::Compose, .. }));
        h.ctl.stop().unwrap();
        let DictationState::Completed { text, raw_text, .. } = wait_for(&h.ctl, Phase::Completed) else {
            unreachable!()
        };
        assert_eq!(text, email);
        assert_eq!(raw_text.as_deref(), Some("create 50% offer discount email"));
        assert_eq!(c.seen.lock().unwrap().as_slice(), ["create 50% offer discount email"]);
        assert_eq!(*refiner.calls.lock().unwrap(), 0);
        assert!(h.sink.0.lock().unwrap().iter().any(|s| matches!(s, DictationState::Refining { intent: Intent::Compose, .. })));
        assert_eq!(h.delivery.0.lock().unwrap().as_slice(), [email]);
    }

    #[test]
    fn compose_failure_pastes_nothing_and_retry_regenerates() {
        let h = harness(speech_samples(), vec![Ok("write a thank you note".into())], None);
        let c = composer(
            true,
            vec![Err(AppError::new(ErrorKind::GenerationFailed, "Gemini took too long to respond.")), Ok("Thank you!".into())],
        );
        h.ctl.set_composer(Some(c.clone()));
        h.ctl.start(None, Intent::Compose).unwrap();
        h.ctl.stop().unwrap();
        let DictationState::Error { kind, recovery, .. } = wait_for(&h.ctl, Phase::Error) else { unreachable!() };
        assert_eq!(kind, ErrorKind::GenerationFailed);
        assert_eq!(recovery, crate::application::errors::Recovery::Retry);
        assert!(h.delivery.0.lock().unwrap().is_empty(), "the raw instruction must not be pasted");
        h.ctl.retry().unwrap();
        let DictationState::Completed { text, .. } = wait_for(&h.ctl, Phase::Completed) else { unreachable!() };
        assert_eq!(text, "Thank you!");
        assert_eq!(c.seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn compose_without_key_fails_before_opening_the_mic() {
        let h = harness(speech_samples(), vec![Ok("x".into())], None);
        h.ctl.set_composer(Some(composer(false, vec![Ok("x".into())])));
        let err = h.ctl.start(None, Intent::Compose).unwrap_err();
        assert_eq!(err.kind, ErrorKind::GenerationUnavailable);
        assert!(h.audio.fault.lock().unwrap().is_none(), "microphone was never started");
        let DictationState::Error { recovery, .. } = h.ctl.state() else { panic!() };
        assert_eq!(recovery, crate::application::errors::Recovery::OpenSettings);
        // Dictation still works without a key.
        h.ctl.start(None, Intent::Dictate).unwrap();
    }

    #[test]
    fn refined_text_is_delivered_and_raw_kept() {
        let h = harness(speech_samples(), vec![Ok("um so the meeting is moved".into())], None);
        polished(&h, FakeRefiner::new(true, Ok("The meeting has been moved.".into())));
        dictate(&h);
        let DictationState::Completed { text, raw_text, refinement_note, .. } = wait_for(&h.ctl, Phase::Completed)
        else {
            unreachable!()
        };
        assert_eq!(text, "The meeting has been moved.");
        assert_eq!(raw_text.as_deref(), Some("um so the meeting is moved"));
        assert_eq!(refinement_note, None);
        assert_eq!(h.delivery.0.lock().unwrap().as_slice(), ["The meeting has been moved."]);
        let phases: Vec<_> = h.sink.0.lock().unwrap().iter().map(|s| s.phase()).collect();
        assert_eq!(phases, [Phase::Recording, Phase::Transcribing, Phase::Refining, Phase::Completed]);
    }

    #[test]
    fn refinement_failure_falls_back_to_raw_transcript() {
        let h = harness(speech_samples(), vec![Ok("raw words".into())], None);
        let err = AppError::new(ErrorKind::RefinementFailed, "Gemini took too long to respond.");
        polished(&h, FakeRefiner::new(true, Err(err)));
        dictate(&h);
        let DictationState::Completed { text, raw_text, refinement_note, delivered, .. } =
            wait_for(&h.ctl, Phase::Completed)
        else {
            unreachable!()
        };
        assert_eq!(text, "raw words");
        assert_eq!(raw_text, None);
        assert_eq!(refinement_note.as_deref(), Some("Gemini took too long to respond."));
        assert!(delivered, "user still gets their words");
    }

    #[test]
    fn verbatim_mode_and_missing_key_skip_the_provider() {
        let h = harness(speech_samples(), vec![Ok("as spoken".into())], None);
        let refiner = FakeRefiner::new(true, Ok("changed".into()));
        h.ctl.set_refiner(Some(refiner.clone())); // config default = Verbatim
        dictate(&h);
        let DictationState::Completed { text, .. } = wait_for(&h.ctl, Phase::Completed) else { unreachable!() };
        assert_eq!(text, "as spoken");
        assert_eq!(*refiner.calls.lock().unwrap(), 0);

        let h = harness(speech_samples(), vec![Ok("as spoken".into())], None);
        let no_key = FakeRefiner::new(false, Ok("changed".into()));
        polished(&h, no_key.clone());
        dictate(&h);
        let DictationState::Completed { text, refinement_note, .. } = wait_for(&h.ctl, Phase::Completed) else {
            unreachable!()
        };
        assert_eq!(text, "as spoken");
        assert_eq!(refinement_note, None);
        assert_eq!(*no_key.calls.lock().unwrap(), 0);
    }

    #[test]
    fn cancel_during_refinement_discards_result() {
        let (tx, rx) = mpsc::channel();
        let h = harness(speech_samples(), vec![Ok("raw".into())], None);
        let refiner = FakeRefiner::new(true, Ok("refined".into()));
        *refiner.gate.lock().unwrap() = Some(rx);
        polished(&h, refiner);
        dictate(&h);
        wait_for(&h.ctl, Phase::Refining);
        h.ctl.cancel();
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(h.ctl.state(), DictationState::Idle);
        assert!(h.delivery.0.lock().unwrap().is_empty());
    }

    #[test]
    fn loud_noise_without_detected_speech_never_reaches_the_model() {
        let (h, speech) = harness_with_vad(
            speech_samples(),
            vec![Ok("you".into())],
            None,
            Some(Arc::new(FakeVad(vec![]))),
        );
        h.ctl.start(None, Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
        let DictationState::Error { kind, .. } = wait_for(&h.ctl, Phase::Error) else { unreachable!() };
        assert_eq!(kind, ErrorKind::NoSpeech);
        assert_eq!(*speech.seen_len.lock().unwrap(), None, "model must not run");
        assert!(h.delivery.0.lock().unwrap().is_empty());
    }

    #[test]
    fn only_the_padded_speech_region_is_transcribed() {
        let span = audio::SpeechSpan { start_ms: 400, end_ms: 600 };
        let (h, speech) =
            harness_with_vad(speech_samples(), vec![Ok("hi".into())], None, Some(Arc::new(FakeVad(vec![span]))));
        h.ctl.start(None, Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
        wait_for(&h.ctl, Phase::Completed);
        // 400-300 .. 600+300 ms = 800 ms at 16 kHz.
        assert_eq!(*speech.seen_len.lock().unwrap(), Some(12_800));
    }

    #[test]
    fn failed_transcription_can_be_retried() {
        let h = harness(
            speech_samples(),
            vec![Err(AppError::new(ErrorKind::TranscriptionFailed, "boom")), Ok("second try".into())],
            None,
        );
        h.ctl.start(None, Intent::Dictate).unwrap();
        h.ctl.stop().unwrap();
        let DictationState::Error { recovery, .. } = wait_for(&h.ctl, Phase::Error) else { unreachable!() };
        assert_eq!(recovery, crate::application::errors::Recovery::Retry);
        h.ctl.retry().unwrap();
        let DictationState::Completed { text, .. } = wait_for(&h.ctl, Phase::Completed) else { unreachable!() };
        assert_eq!(text, "second try");
    }

    #[test]
    fn microphone_failure_surfaces_error() {
        let sink = Arc::new(Recorder::default());
        let audio = Arc::new(FakeAudio {
            samples: vec![],
            fail: Some(AppError::new(ErrorKind::MicUnavailable, "no mic")),
            fault: Mutex::new(None),
        });
        let speech = Arc::new(FakeSpeech {
            result: Mutex::new(vec![Ok("x".into())]),
            gate: Mutex::new(None),
            seen_len: Mutex::new(None),
        });
        let ctl = Controller::new(audio, None, speech, Arc::new(FakeDelivery::default()), sink, ControllerConfig::default());
        let err = ctl.start(None, Intent::Dictate).unwrap_err();
        assert_eq!(err.kind, ErrorKind::MicUnavailable);
        assert_eq!(ctl.state().phase(), Phase::Error);
        // The user can immediately try again from the error state.
        assert!(can_transition(ctl.state().phase(), Phase::Recording));
    }

    #[test]
    fn device_loss_mid_recording_moves_to_error() {
        let h = harness(speech_samples(), vec![Ok("x".into())], None);
        h.ctl.start(None, Intent::Dictate).unwrap();
        let cb = h.audio.fault.lock().unwrap().take().unwrap();
        cb(AppError::new(ErrorKind::MicUnavailable, "unplugged"));
        let DictationState::Error { kind, .. } = h.ctl.state() else { panic!("expected error") };
        assert_eq!(kind, ErrorKind::MicUnavailable);
        assert!(h.ctl.stop().is_err());
    }

    #[test]
    fn recording_cap_stops_automatically() {
        let h = harness(speech_samples(), vec![Ok("capped".into())], None);
        h.ctl.set_config(ControllerConfig { max_recording: Duration::from_millis(50), ..Default::default() });
        h.ctl.start(None, Intent::Dictate).unwrap();
        wait_for(&h.ctl, Phase::Completed);
    }
}
