import type { DictationState, Recovery } from "@voiceflow/shared-types";

/** Text label for every state, so status is never conveyed by colour alone. */
export function statusLabel(state: DictationState): string {
  switch (state.status) {
    case "idle":
      return "Ready";
    case "recording":
      return state.intent === "compose" ? "Listening (create)" : "Listening";
    case "transcribing":
      return "Transcribing";
    case "refining":
      return state.intent === "compose" ? "Writing…" : "Polishing";
    case "completed":
      return state.pasted ? "Pasted" : state.delivered ? "Copied" : "Done";
    case "error":
      return state.kind === "no_speech" ? "No speech" : "Needs attention";
  }
}

export function recoveryLabel(recovery: Recovery): string {
  switch (recovery) {
    case "retry":
      return "Retry";
    case "record_again":
      return "Record again";
    case "download_model":
      return "Get model";
    case "check_microphone":
      return "Mic settings";
    case "change_hotkey":
      return "Change shortcut";
    case "copy_manually":
      return "Open transcript";
    case "open_settings":
      return "Add key";
    case "dismiss":
      return "Dismiss";
  }
}

export function formatDuration(ms: number): string {
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}
