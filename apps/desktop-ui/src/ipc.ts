import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  EVENTS,
  type AppErrorPayload,
  type DictationState,
  type DownloadProgress,
  type HotkeyStatuses,
  type ModelEntry,
  type RefinementStatus,
  type Settings,
} from "@voiceflow/shared-types";

// Thin typed wrappers over the Rust commands registered in src-tauri/src/commands.rs.
export const ipc = {
  getState: () => invoke<DictationState>("get_state"),
  toggle: () => invoke<void>("toggle_dictation"),
  start: () => invoke<void>("start_dictation"),
  stop: () => invoke<void>("stop_dictation"),
  cancel: () => invoke<void>("cancel_dictation"),
  retry: () => invoke<void>("retry_dictation"),
  dismiss: () => invoke<void>("dismiss_dictation"),
  copyLast: () => invoke<void>("copy_last"),
  copyText: (text: string) => invoke<void>("copy_text", { text }),
  quit: () => invoke<void>("quit_app"),
  getRefinementStatus: () => invoke<RefinementStatus>("get_refinement_status"),
  /** Stores the key in Windows Credential Manager and verifies it; resolves with a sample edit. */
  setGeminiKey: (key: string) => invoke<string>("set_gemini_key", { key }),
  clearGeminiKey: () => invoke<void>("clear_gemini_key"),
  getSettings: () => invoke<Settings>("get_settings"),
  updateSettings: (settings: Settings) => invoke<Settings>("update_settings", { settings }),
  getHotkeyStatus: () => invoke<HotkeyStatuses>("get_hotkey_status"),
  listModels: () => invoke<ModelEntry[]>("list_models"),
  downloadModel: (id: string) => invoke<void>("download_model", { id }),
  showMain: () => invoke<void>("show_main_window"),
  setBarVisible: (visible: boolean) => invoke<void>("set_bar_visible", { visible }),
};

export const onState = (cb: (s: DictationState) => void): Promise<UnlistenFn> =>
  listen<DictationState>(EVENTS.state, (e) => cb(e.payload));

export const onDownloadProgress = (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
  listen<DownloadProgress>(EVENTS.downloadProgress, (e) => cb(e.payload));

export const onHotkeyStatus = (cb: (s: HotkeyStatuses) => void): Promise<UnlistenFn> =>
  listen<HotkeyStatuses>(EVENTS.hotkeyStatus, (e) => cb(e.payload));

/** Commands reject with the serialized Rust `AppError`; normalise to a readable message. */
export function errorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as AppErrorPayload).message);
  }
  return String(err);
}
