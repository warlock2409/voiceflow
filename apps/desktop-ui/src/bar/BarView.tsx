import type { DictationState, Recovery } from "@voiceflow/shared-types";
import { formatDuration, recoveryLabel, statusLabel } from "../state/labels";

export interface BarViewProps {
  state: DictationState;
  elapsedMs: number;
  hotkey: string;
  onToggle: () => void;
  onCancel: () => void;
  onRecovery: (r: Recovery) => void;
  onOpen: () => void;
  onHide: () => void;
}

/** Pure presentational pill; all behaviour arrives via props so it is unit-testable. */
export function BarView({ state, elapsedMs, hotkey, onToggle, onCancel, onRecovery, onOpen, onHide }: BarViewProps) {
  const label = statusLabel(state);
  const busy = state.status === "transcribing" || state.status === "refining";
  const recording = state.status === "recording";

  return (
    <div className={`pill pill--${state.status}`} role="toolbar" aria-label="VoiceFlow dictation">
      <span className="pill__indicator" aria-hidden="true">
        <StatusIcon status={state.status} />
      </span>

      <span className="pill__status" role="status" aria-live="polite" title={statusTitle(state, hotkey)}>
        {label}
        {recording && <span className="pill__timer"> {formatDuration(elapsedMs)}</span>}
      </span>

      {state.status === "error" ? (
        <button type="button" className="pill__btn pill__btn--primary" onClick={() => onRecovery(state.recovery)}>
          {recoveryLabel(state.recovery)}
        </button>
      ) : (
        <button
          type="button"
          className="pill__btn pill__btn--primary"
          onClick={onToggle}
          disabled={busy}
          aria-label={recording ? "Stop dictation" : "Start dictation"}
          aria-pressed={recording}
          title={`${recording ? "Stop" : "Start"} (${hotkey})`}
        >
          {recording ? "Stop" : busy ? "…" : "Dictate"}
        </button>
      )}

      {(recording || busy || state.status === "error") && (
        <button type="button" className="pill__btn" onClick={onCancel} aria-label="Cancel dictation" title="Cancel">
          ✕
        </button>
      )}

      <button type="button" className="pill__btn pill__btn--ghost" onClick={onOpen} aria-label="Open VoiceFlow window" title="Open VoiceFlow">
        ⋯
      </button>
      <button type="button" className="pill__btn pill__btn--ghost" onClick={onHide} aria-label="Hide bar" title="Hide bar (reopen from the main window)">
        –
      </button>
    </div>
  );
}

function statusTitle(state: DictationState, hotkey: string): string {
  if (state.status === "error") return state.message;
  if (state.status === "completed") return state.pasteError ?? state.deliveryError ?? state.text;
  if (state.status === "idle") return `Press ${hotkey} to dictate`;
  return statusLabel(state);
}

function StatusIcon({ status }: { status: DictationState["status"] }) {
  // Shape differs per state (not only colour): mic, pulsing dot, spinner, check, warning.
  switch (status) {
    case "recording":
      return <svg viewBox="0 0 16 16" width="14" height="14"><circle cx="8" cy="8" r="6" fill="currentColor" /></svg>;
    case "transcribing":
    case "refining":
      return (
        <svg viewBox="0 0 16 16" width="14" height="14" className="spin">
          <path d="M8 2a6 6 0 1 1-6 6" fill="none" stroke="currentColor" strokeWidth="2" />
        </svg>
      );
    case "completed":
      return <svg viewBox="0 0 16 16" width="14" height="14"><path d="M3 8l3 3 7-7" fill="none" stroke="currentColor" strokeWidth="2" /></svg>;
    case "error":
      return <svg viewBox="0 0 16 16" width="14" height="14"><path d="M8 1l7 13H1z M8 6v4 M8 12v1" fill="none" stroke="currentColor" strokeWidth="1.6" /></svg>;
    default:
      return (
        <svg viewBox="0 0 16 16" width="14" height="14">
          <rect x="5.5" y="1.5" width="5" height="8" rx="2.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
          <path d="M3 8a5 5 0 0 0 10 0 M8 13v2" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      );
  }
}
