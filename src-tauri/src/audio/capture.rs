//! cpal (WASAPI on Windows, CoreAudio on macOS) microphone capture.
//! The stream lives on its own thread; the handle is just channels, so it is `Send`.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};

use super::{resample, AudioSource, FaultCallback, Recording, TARGET_SAMPLE_RATE};
use crate::application::{AppError, AppResult, ErrorKind};

/// Safety net slightly above the controller's 2 min cap, so memory stays bounded.
const MAX_CAPTURE_SECS: usize = 130;

#[derive(Default)]
pub struct CpalSource;

struct Raw {
    samples: Vec<f32>,
    sample_rate: u32,
    channels: u16,
}

pub struct CpalRecording {
    stop_tx: Option<mpsc::Sender<()>>,
    thread: Option<JoinHandle<Raw>>,
}

impl AudioSource for CpalSource {
    fn start(&self, on_fault: FaultCallback) -> AppResult<Box<dyn Recording>> {
        let (stop_tx, stop_rx) = mpsc::channel::<()>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<AppResult<()>>(1);
        let on_fault: Arc<FaultCallback> = Arc::new(on_fault);

        let thread = std::thread::Builder::new()
            .name("voiceflow-capture".into())
            .spawn(move || {
                let buffer = Arc::new(Mutex::new(Vec::<f32>::new()));
                let opened = open_stream(Arc::clone(&buffer), on_fault);
                let (stream, sample_rate, channels) = match opened {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return Raw { samples: vec![], sample_rate: TARGET_SAMPLE_RATE, channels: 1 };
                    }
                };
                let _ = ready_tx.send(Ok(()));
                // Blocks until stop() sends or the handle is dropped (cancel).
                let _ = stop_rx.recv();
                drop(stream); // releases the microphone
                let samples = std::mem::take(&mut *buffer.lock().unwrap_or_else(|e| e.into_inner()));
                Raw { samples, sample_rate, channels }
            })
            .map_err(|e| AppError::new(ErrorKind::Internal, format!("Cannot start audio thread: {e}")))?;

        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Box::new(CpalRecording { stop_tx: Some(stop_tx), thread: Some(thread) })),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(AppError::new(ErrorKind::MicUnavailable, "The microphone did not respond in time.")),
        }
    }
}

impl Recording for CpalRecording {
    fn stop(mut self: Box<Self>) -> AppResult<Vec<f32>> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        let raw = self
            .thread
            .take()
            .and_then(|t| t.join().ok())
            .ok_or_else(|| AppError::new(ErrorKind::Internal, "Audio thread crashed."))?;
        let mono = resample::to_mono(&raw.samples, raw.channels as usize);
        resample::resample(&mono, raw.sample_rate, TARGET_SAMPLE_RATE)
    }
}

impl Drop for CpalRecording {
    fn drop(&mut self) {
        // Cancel path: closing the channel ends the capture thread, which drops the stream.
        // Never join here: drop may run on the cpal callback thread (device fault).
        self.stop_tx.take();
    }
}

type Opened = (cpal::Stream, u32, u16);

fn open_stream(buffer: Arc<Mutex<Vec<f32>>>, on_fault: Arc<FaultCallback>) -> AppResult<Opened> {
    let host = cpal::default_host();
    let device = host.default_input_device().ok_or_else(|| {
        AppError::new(ErrorKind::MicUnavailable, "No microphone found. Connect one or enable it in Windows sound settings.")
    })?;
    let supported = device.default_input_config().map_err(map_cpal_error)?;
    let sample_rate = supported.sample_rate();
    let channels = supported.channels();
    let max_samples = sample_rate as usize * channels as usize * MAX_CAPTURE_SECS;
    tracing::info!(device = %device, sample_rate, channels, format = ?supported.sample_format(), "opening microphone");

    let config: cpal::StreamConfig = supported.config();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, buffer, max_samples, on_fault),
        SampleFormat::I16 => build::<i16>(&device, config, buffer, max_samples, on_fault),
        SampleFormat::I32 => build::<i32>(&device, config, buffer, max_samples, on_fault),
        SampleFormat::U16 => build::<u16>(&device, config, buffer, max_samples, on_fault),
        other => Err(AppError::new(
            ErrorKind::MicUnavailable,
            format!("Unsupported microphone sample format {other:?}."),
        )),
    }?;
    stream.play().map_err(map_cpal_error)?;
    Ok((stream, sample_rate, channels))
}

fn build<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    buffer: Arc<Mutex<Vec<f32>>>,
    max_samples: usize,
    on_fault: Arc<FaultCallback>,
) -> AppResult<cpal::Stream>
where
    T: SizedSample + ToF32,
{
    device
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mut buf = buffer.lock().unwrap_or_else(|e| e.into_inner());
                let room = max_samples.saturating_sub(buf.len());
                buf.extend(data.iter().take(room).map(|s| s.to_f32()));
            },
            move |err: cpal::Error| {
                use cpal::ErrorKind as K;
                match err.kind() {
                    K::DeviceNotAvailable | K::StreamInvalidated | K::PermissionDenied => {
                        let app_err = map_cpal_error(err);
                        let cb = Arc::clone(&on_fault);
                        // Hop off the audio thread before touching controller state.
                        std::thread::spawn(move || cb(app_err));
                    }
                    _ => tracing::warn!(error = %err, "audio stream warning"),
                }
            },
            None,
        )
        .map_err(map_cpal_error)
}

fn map_cpal_error(err: cpal::Error) -> AppError {
    use cpal::ErrorKind as K;
    match err.kind() {
        K::PermissionDenied => AppError::new(
            ErrorKind::MicPermission,
            "Microphone access is blocked. Allow desktop apps to use the microphone in Windows Settings > Privacy & security > Microphone.",
        ),
        K::DeviceBusy => AppError::new(ErrorKind::MicUnavailable, "The microphone is in exclusive use by another app."),
        K::DeviceNotAvailable | K::DeviceChanged | K::StreamInvalidated => {
            AppError::new(ErrorKind::MicUnavailable, "The microphone was disconnected or became unavailable.")
        }
        _ => AppError::new(ErrorKind::MicUnavailable, format!("Microphone error: {err}")),
    }
}

/// Normalises integer and float PCM to f32 in [-1, 1].
pub trait ToF32: Copy {
    fn to_f32(self) -> f32;
}
impl ToF32 for f32 {
    fn to_f32(self) -> f32 {
        self
    }
}
impl ToF32 for i16 {
    fn to_f32(self) -> f32 {
        self as f32 / 32_768.0
    }
}
impl ToF32 for i32 {
    fn to_f32(self) -> f32 {
        self as f32 / 2_147_483_648.0
    }
}
impl ToF32 for u16 {
    fn to_f32(self) -> f32 {
        (self as f32 - 32_768.0) / 32_768.0
    }
}
