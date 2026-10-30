//! Whisper model registry, presence checks and verified download.
//! The only network access in Phase 1 is the user-initiated model download.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::application::{AppError, AppResult, ErrorKind};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub multilingual: bool,
}

/// Pinned to the official whisper.cpp conversions; hashes match the Hugging Face LFS pointers.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "tiny.en",
        label: "Tiny (English, fastest)",
        file: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        size_bytes: 77_704_715,
        multilingual: false,
    },
    ModelInfo {
        id: "base.en",
        label: "Base (English, recommended)",
        file: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        size_bytes: 147_964_211,
        multilingual: false,
    },
];

pub fn find(id: &str) -> AppResult<&'static ModelInfo> {
    MODELS
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| AppError::new(ErrorKind::ModelMissing, format!("Unknown speech model '{id}'.")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelStatus {
    Missing,
    /// File exists but its size is wrong (interrupted copy, wrong file).
    Corrupt,
    Ready,
}

pub fn path_for(dir: &Path, model: &ModelInfo) -> PathBuf {
    dir.join(model.file)
}

/// Fast check used on every recording. The full SHA-256 is verified at download time.
pub fn status(dir: &Path, model: &ModelInfo) -> ModelStatus {
    match std::fs::metadata(path_for(dir, model)) {
        Err(_) => ModelStatus::Missing,
        Ok(m) if m.len() != model.size_bytes => ModelStatus::Corrupt,
        Ok(_) => ModelStatus::Ready,
    }
}

/// Streams the model to `<file>.part`, hashing as it goes, and only renames it into
/// place if the hash matches. `progress(done, total)` is called periodically.
pub async fn download(
    dir: &Path,
    model: &ModelInfo,
    mut progress: impl FnMut(u64, u64) + Send,
) -> AppResult<PathBuf> {
    let fail = |msg: String| AppError::new(ErrorKind::ModelDownloadFailed, msg);
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| fail(format!("Cannot create model folder: {e}")))?;
    let final_path = path_for(dir, model);
    let part_path = final_path.with_extension("bin.part");

    let resp = reqwest::Client::new()
        .get(model.url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| fail(format!("Download failed. Check your internet connection. ({e})")))?;
    let total = resp.content_length().unwrap_or(model.size_bytes);

    let mut file = tokio::fs::File::create(&part_path)
        .await
        .map_err(|e| fail(format!("Cannot write model file: {e}")))?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut last_report: u64 = 0;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| fail(format!("Download interrupted: {e}")))?;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| fail(format!("Cannot write model file (disk full?): {e}")))?;
        done += chunk.len() as u64;
        if done - last_report >= 1_000_000 || done == total {
            progress(done, total);
            last_report = done;
        }
    }
    file.flush().await.map_err(|e| fail(e.to_string()))?;
    drop(file);

    let digest = hex::encode(hasher.finalize());
    if digest != model.sha256 {
        let _ = tokio::fs::remove_file(&part_path).await;
        return Err(AppError::new(
            ErrorKind::ModelCorrupt,
            "The downloaded model failed its integrity check and was discarded. Please try again.",
        ));
    }
    tokio::fs::rename(&part_path, &final_path)
        .await
        .map_err(|e| fail(format!("Cannot finalise model file: {e}")))?;
    progress(done, total);
    Ok(final_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_lookup() {
        assert_eq!(find("base.en").unwrap().file, "ggml-base.en.bin");
        assert_eq!(find("nope").unwrap_err().kind, ErrorKind::ModelMissing);
    }

    /// Network test (77 MB): run with `cargo test -- --ignored downloads_and_verifies`.
    #[tokio::test]
    #[ignore]
    async fn downloads_and_verifies_tiny() {
        let dir = std::env::temp_dir().join(format!("voiceflow-dl-{}", std::process::id()));
        let m = find("tiny.en").unwrap();
        let mut calls = 0u32;
        let path = download(&dir, m, |_, _| calls += 1).await.unwrap();
        assert_eq!(status(&dir, m), ModelStatus::Ready);
        assert!(calls > 10, "progress reported {calls} times");
        assert!(!path.with_extension("bin.part").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn status_detects_missing_and_wrong_size() {
        let dir = std::env::temp_dir().join(format!("voiceflow-model-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let m = find("tiny.en").unwrap();
        let _ = std::fs::remove_file(path_for(&dir, m));
        assert_eq!(status(&dir, m), ModelStatus::Missing);
        std::fs::write(path_for(&dir, m), b"truncated").unwrap();
        assert_eq!(status(&dir, m), ModelStatus::Corrupt);
    }
}
