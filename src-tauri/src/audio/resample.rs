use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::application::{AppError, AppResult, ErrorKind};

/// Average interleaved channels into mono.
pub fn to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Offline whole-clip resample of mono audio. `process_all` trims the filter delay.
pub fn resample(mono: &[f32], from_hz: u32, to_hz: u32) -> AppResult<Vec<f32>> {
    if from_hz == to_hz || mono.is_empty() {
        return Ok(mono.to_vec());
    }
    let err = |e: String| AppError::new(ErrorKind::Internal, format!("Audio resampling failed: {e}"));
    let mut resampler = Fft::<f32>::new(from_hz as usize, to_hz as usize, 1024, 1, FixedSync::Input)
        .map_err(|e| err(e.to_string()))?;
    let input = InterleavedSlice::new(mono, 1, mono.len()).map_err(|e| err(e.to_string()))?;
    let out = resampler
        .process_all(&input, mono.len(), None)
        .map_err(|e| err(e.to_string()))?;
    Ok(out.take_data())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| (i as f32 * freq * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    /// Frequency estimate from zero crossings (robust enough for a pure tone).
    fn estimate_freq(s: &[f32], rate: u32) -> f32 {
        let crossings = s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        crossings as f32 * rate as f32 / s.len() as f32
    }

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(to_mono(&[0.1, 0.2], 1), vec![0.1, 0.2]);
    }

    #[test]
    fn resample_48k_to_16k_preserves_length_and_pitch() {
        let input = sine(440.0, 48_000, 1.0);
        let out = resample(&input, 48_000, 16_000).unwrap();
        assert!((out.len() as i64 - 16_000).abs() <= 2, "len {}", out.len());
        let f = estimate_freq(&out, 16_000);
        assert!((f - 440.0).abs() < 5.0, "freq {f}");
    }

    #[test]
    fn resample_44100_to_16k() {
        let input = sine(1000.0, 44_100, 0.5);
        let out = resample(&input, 44_100, 16_000).unwrap();
        assert!((out.len() as i64 - 8_000).abs() <= 2, "len {}", out.len());
        assert!((estimate_freq(&out, 16_000) - 1000.0).abs() < 10.0);
    }

    #[test]
    fn same_rate_is_passthrough() {
        let input = sine(440.0, 16_000, 0.1);
        assert_eq!(resample(&input, 16_000, 16_000).unwrap(), input);
    }
}
