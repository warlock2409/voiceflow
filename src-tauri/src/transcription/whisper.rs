//! whisper.cpp (via whisper-rs) speech provider. Fully local; the model stays loaded
//! after first use so later dictations skip the load cost.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, Once};

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use super::model::{self, ModelStatus};
use super::{SpeechProvider, TranscribeOptions, Transcript};
use crate::application::{AppError, AppResult, ErrorKind};

static SILENCE_NATIVE_LOGS: Once = Once::new();

pub struct WhisperProvider {
    models_dir: PathBuf,
    model_id: Mutex<String>,
    use_gpu: bool,
    loaded: Mutex<Option<(String, Arc<WhisperContext>)>>,
}

impl WhisperProvider {
    pub fn new(models_dir: PathBuf, model_id: impl Into<String>) -> Self {
        // Route whisper.cpp/ggml logging away from stderr (no tracing backend enabled).
        SILENCE_NATIVE_LOGS.call_once(whisper_rs::install_logging_hooks);
        Self {
            models_dir,
            model_id: Mutex::new(model_id.into()),
            use_gpu: cfg!(feature = "cuda"),
            loaded: Mutex::new(None),
        }
    }

    pub fn models_dir(&self) -> &PathBuf {
        &self.models_dir
    }

    pub fn set_model(&self, id: &str) {
        *self.model_id.lock().unwrap_or_else(|e| e.into_inner()) = id.to_string();
    }

    fn current_model(&self) -> String {
        self.model_id.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn context(&self) -> AppResult<(String, Arc<WhisperContext>)> {
        let id = self.current_model();
        let mut loaded = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((loaded_id, ctx)) = loaded.as_ref() {
            if *loaded_id == id {
                return Ok((id, Arc::clone(ctx)));
            }
        }
        let info = model::find(&id)?;
        let path = model::path_for(&self.models_dir, info);
        let started = std::time::Instant::now();
        let mut params = WhisperContextParameters::default();
        params.use_gpu(self.use_gpu);
        let ctx = WhisperContext::new_with_params(&path, params).map_err(|e| {
            AppError::new(
                ErrorKind::ModelCorrupt,
                format!("The speech model could not be loaded and may be damaged. Re-download it. ({e})"),
            )
        })?;
        tracing::info!(model = %id, load_ms = started.elapsed().as_millis() as u64, "model loaded");
        let ctx = Arc::new(ctx);
        *loaded = Some((id.clone(), Arc::clone(&ctx)));
        Ok((id, ctx))
    }

    /// Loads the model ahead of the first dictation (called in the background at startup).
    pub fn warm_up(&self) -> AppResult<()> {
        self.check_ready()?;
        self.context().map(|_| ())
    }
}

impl SpeechProvider for WhisperProvider {
    fn check_ready(&self) -> AppResult<()> {
        let info = model::find(&self.current_model())?;
        match model::status(&self.models_dir, info) {
            ModelStatus::Ready => Ok(()),
            ModelStatus::Missing => Err(AppError::new(
                ErrorKind::ModelMissing,
                format!("The speech model '{}' is not downloaded yet.", info.label),
            )),
            ModelStatus::Corrupt => Err(AppError::new(
                ErrorKind::ModelCorrupt,
                format!("The speech model '{}' is incomplete or damaged. Re-download it.", info.label),
            )),
        }
    }

    fn transcribe(&self, pcm: &[f32], opts: &TranscribeOptions) -> AppResult<Transcript> {
        let (model_id, ctx) = self.context()?;
        let failed = |e: whisper_rs::WhisperError| {
            AppError::new(ErrorKind::TranscriptionFailed, format!("Transcription failed: {e}"))
        };
        let mut state = ctx.create_state().map_err(failed)?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        let language = if opts.language == "auto" { None } else { Some(opts.language.as_str()) };
        params.set_language(language);
        params.set_n_threads(thread_count());
        params.set_translate(false);
        params.set_no_context(true);
        params.set_no_timestamps(true);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        state.full(params, pcm).map_err(failed)?;

        let mut text = String::new();
        for segment in state.as_iter() {
            let piece = segment.to_str_lossy().map_err(failed)?;
            text.push_str(&piece);
        }
        Ok(Transcript { text: clean(&text), model: model_id })
    }
}

/// `VOICEFLOW_THREADS` overrides the default for benchmarking and diagnostics.
fn thread_count() -> i32 {
    if let Some(n) = std::env::var("VOICEFLOW_THREADS").ok().and_then(|v| v.parse().ok()) {
        return n;
    }
    std::thread::available_parallelism().map(|n| n.get().min(8) as i32).unwrap_or(4)
}

/// Collapse whitespace and drop whisper's non-speech markers like "[BLANK_AUDIO]".
fn clean(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '[' => depth += 1,
            ']' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_markers_and_whitespace() {
        assert_eq!(clean(" [BLANK_AUDIO] "), "");
        assert_eq!(clean("  Hello,   world. [Music]  Bye."), "Hello, world. Bye.");
    }

    /// Real whisper.cpp provider for integration tests. Needs the model in the app's data
    /// dir (%APPDATA%\com.voiceflow.desktop\models) or VOICEFLOW_MODELS_DIR; tests skip otherwise.
    fn test_provider() -> Option<(String, WhisperProvider)> {
        let dir = std::env::var_os("VOICEFLOW_MODELS_DIR").map(PathBuf::from).or_else(|| {
            std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("com.voiceflow.desktop").join("models"))
        })?;
        let model = std::env::var("VOICEFLOW_TEST_MODEL").unwrap_or_else(|_| "tiny.en".into());
        let provider = WhisperProvider::new(dir, &model);
        match provider.check_ready() {
            Ok(()) => Some((model, provider)),
            Err(_) => {
                eprintln!("skipping: model {model} not present");
                None
            }
        }
    }

    /// End-to-end against the real whisper.cpp build.
    #[test]
    fn transcribes_jfk_fixture() {
        let Some((model, provider)) = test_provider() else { return };
        let wav = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/jfk.wav");
        let mut reader = hound::WavReader::open(wav).expect("fixture");
        assert_eq!(reader.spec().sample_rate, 16_000);
        let pcm: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32_768.0).collect();

        let started = std::time::Instant::now();
        provider.warm_up().unwrap();
        let load_ms = started.elapsed().as_millis();
        let started = std::time::Instant::now();
        let t = provider.transcribe(&pcm, &TranscribeOptions { language: "en".into() }).unwrap();
        let ms = started.elapsed().as_millis();
        let audio_ms = pcm.len() as u128 * 1000 / 16_000;
        eprintln!("BENCH system={}", whisper_rs::print_system_info().trim());
        eprintln!(
            "BENCH model={model} threads={} load_ms={load_ms} audio_ms={audio_ms} transcribe_ms={ms} text={:?}",
            thread_count(),
            t.text
        );
        let lower = t.text.to_lowercase();
        assert!(lower.contains("ask not what your country can do for you"), "{}", t.text);
    }
}
