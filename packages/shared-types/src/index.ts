// Hand-maintained mirror of the Rust types in src-tauri/src/application/{state,errors}.rs,
// storage/settings.rs and transcription/model.rs. Keep in sync (ts-rs generation is a later step).

export type ErrorKind =
  | "mic_permission"
  | "mic_unavailable"
  | "no_speech"
  | "model_missing"
  | "model_corrupt"
  | "model_download_failed"
  | "transcription_failed"
  | "refinement_failed"
  | "generation_failed"
  | "generation_unavailable"
  | "hotkey_conflict"
  | "clipboard_failed"
  | "busy"
  | "internal";

export type Recovery =
  | "retry"
  | "record_again"
  | "download_model"
  | "check_microphone"
  | "change_hotkey"
  | "copy_manually"
  | "open_settings"
  | "dismiss";

/** dictate = speech to text; compose = speech is an instruction and an LLM writes the content. */
export type Intent = "dictate" | "compose";

export interface Metrics {
  audioMs: number;
  transcribeMs: number;
  rtf: number;
  model: string;
  refineMs: number;
}

export type DictationState =
  | { status: "idle" }
  | { status: "recording"; session: number; startedAtMs: number; intent: Intent }
  | { status: "transcribing"; session: number }
  | { status: "refining"; session: number; intent: Intent }
  | {
      status: "completed";
      session: number;
      text: string;
      /** Original transcript, present only when `text` was refined. */
      rawText: string | null;
      /** Why refinement was skipped/failed; the raw transcript was delivered instead. */
      refinementNote: string | null;
      metrics: Metrics;
      /** Pasted at the cursor in the app the user was typing in. */
      pasted: boolean;
      /** Why it was not pasted (it was copied to the clipboard instead). */
      pasteError: string | null;
      /** The text is on the clipboard. */
      delivered: boolean;
      deliveryError: string | null;
    }
  | {
      status: "error";
      session: number | null;
      kind: ErrorKind;
      message: string;
      recovery: Recovery;
    };

export type DictationStatus = DictationState["status"];

export interface AppErrorPayload {
  kind: ErrorKind;
  message: string;
}

export type RecordingMode = "toggle" | "push_to_talk";
export type RefinementMode = "verbatim" | "clean" | "polished" | "developer" | "custom";

export interface Settings {
  hotkey: string;
  /** Empty string = content creation off. */
  composeHotkey: string;
  recordingMode: RecordingMode;
  model: string;
  language: string;
  refinement: RefinementMode;
  geminiModel: string;
  autoPaste: boolean;
  autoCopy: boolean;
  barVisible: boolean;
}

export type ModelStatus = "missing" | "corrupt" | "ready";

export interface ModelEntry {
  id: string;
  label: string;
  sizeBytes: number;
  multilingual: boolean;
  status: ModelStatus;
}

export interface HotkeyStatus {
  shortcut: string;
  registered: boolean;
  error: string | null;
}

export interface HotkeyStatuses {
  dictate: HotkeyStatus;
  compose: HotkeyStatus;
}

export interface RefinementStatus {
  keyConfigured: boolean;
}

export interface DownloadProgress {
  model: string;
  done: number;
  total: number;
}

export const EVENTS = {
  state: "dictation://state",
  downloadProgress: "model://download-progress",
  hotkeyStatus: "hotkey://status",
} as const;
