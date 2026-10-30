use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::refinement::RefinementMode;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingMode {
    #[default]
    Toggle,
    PushToTalk,
}

/// User settings. `#[serde(default)]` lets older/partial files load after upgrades
/// without overwriting what the user already chose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// Dictation: speech to text, pasted.
    pub hotkey: String,
    /// Content creation: speech is an instruction, an LLM writes the content. Empty = off.
    pub compose_hotkey: String,
    pub recording_mode: RecordingMode,
    pub model: String,
    pub language: String,
    pub refinement: RefinementMode,
    pub gemini_model: String,
    /// Paste the text at the cursor in the app you were typing in.
    pub auto_paste: bool,
    /// Also leave the text on the clipboard (otherwise the previous clipboard text is restored).
    pub auto_copy: bool,
    pub bar_visible: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Hold Caps Lock to dictate, release to transcribe (Windows keyboard hook).
            hotkey: "CapsLock".into(),
            compose_hotkey: "Ctrl+Space".into(),
            recording_mode: RecordingMode::PushToTalk,
            model: "base.en".into(),
            language: "en".into(),
            // Only takes effect once a Gemini key is saved; until then text is delivered as recognised.
            refinement: RefinementMode::Polished,
            gemini_model: crate::refinement::gemini::DEFAULT_MODEL.into(),
            auto_paste: true,
            // After pasting, put back what was on the clipboard (spec FR-07).
            auto_copy: false,
            bar_visible: true,
        }
    }
}

pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn new(dir: &Path) -> Self {
        Self { path: dir.join("settings.json") }
    }

    /// Never fails: a missing file yields defaults; a corrupt file is set aside
    /// as `settings.json.bak` so the user's data is not silently destroyed.
    pub fn load(&self) -> Settings {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(raw) => raw,
            Err(_) => return Settings::default(),
        };
        match serde_json::from_str(&raw) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "settings file unreadable; using defaults");
                let _ = std::fs::rename(&self.path, self.path.with_extension("json.bak"));
                Settings::default()
            }
        }
    }

    /// Atomic write: temp file then rename, so a crash cannot truncate settings.
    pub fn save(&self, settings: &Settings) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("voiceflow-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let store = SettingsStore::new(&temp_dir("missing"));
        assert_eq!(store.load(), Settings::default());
    }

    #[test]
    fn round_trip() {
        let store = SettingsStore::new(&temp_dir("roundtrip"));
        let s = Settings { hotkey: "Alt+F9".into(), auto_copy: false, ..Default::default() };
        store.save(&s).unwrap();
        assert_eq!(store.load(), s);
    }

    #[test]
    fn partial_file_keeps_user_values_and_fills_defaults() {
        let dir = temp_dir("partial");
        std::fs::write(dir.join("settings.json"), r#"{"hotkey":"Alt+F9"}"#).unwrap();
        let s = SettingsStore::new(&dir).load();
        assert_eq!(s.hotkey, "Alt+F9");
        assert_eq!(s.model, "base.en");
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = temp_dir("corrupt");
        std::fs::write(dir.join("settings.json"), "{not json").unwrap();
        assert_eq!(SettingsStore::new(&dir).load(), Settings::default());
        assert!(dir.join("settings.json.bak").exists());
    }
}
