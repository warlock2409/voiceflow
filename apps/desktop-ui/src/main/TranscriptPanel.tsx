import { useEffect, useState } from "react";
import type { DictationState } from "@voiceflow/shared-types";
import { formatDuration, recoveryLabel, statusLabel } from "../state/labels";

export interface TranscriptPanelProps {
  state: DictationState;
  elapsedMs: number;
  hotkey: string;
  /** False when the global shortcut failed to register (e.g. another app owns it). */
  hotkeyActive: boolean;
  pushToTalk: boolean;
  /** Active content-creation shortcut, or null when off/unavailable. */
  composeHotkey?: string | null;
  onToggle: () => void;
  onCancel: () => void;
  onRetry: () => void;
  onDismiss: () => void;
  onCopy: (text: string) => Promise<void>;
  onRecovery: () => void;
}

export function TranscriptPanel(props: TranscriptPanelProps) {
  const { state, elapsedMs, hotkey, hotkeyActive } = props;
  const [text, setText] = useState("");
  const [copyNote, setCopyNote] = useState<string | null>(null);

  // Show each new transcript; the user may then edit it before copying.
  const completedText = state.status === "completed" ? state.text : null;
  useEffect(() => {
    if (completedText !== null) setText(completedText);
  }, [completedText]);

  const recording = state.status === "recording";
  const busy = state.status === "transcribing" || state.status === "refining";

  const copy = async () => {
    try {
      await props.onCopy(text);
      setCopyNote("Copied to clipboard.");
    } catch (e) {
      setCopyNote(String((e as { message?: string })?.message ?? e));
    }
  };

  return (
    <section aria-labelledby="dictation-heading" className="panel">
      <h2 id="dictation-heading">Dictation</h2>

      <p className={`status status--${state.status}`} role="status" aria-live="polite">
        <strong>{statusLabel(state)}</strong>
        {recording && <span> · {formatDuration(elapsedMs)}</span>}
        {state.status === "idle" && hotkeyActive && (
          <span> · {props.pushToTalk ? `Hold ${hotkey} while speaking` : `Press ${hotkey} anywhere to start`}</span>
        )}
        {state.status === "idle" && props.composeHotkey && (
          <span> · {props.pushToTalk ? "Hold" : "Press"} {props.composeHotkey} to have AI write content</span>
        )}
      </p>

      {!hotkeyActive && (
        <p className="callout callout--error" role="alert">
          The shortcut {hotkey} is not available (another app may be using it). Use Start dictation below, or
          choose a different shortcut in Settings.
        </p>
      )}

      {state.status === "error" && (
        <div className="callout callout--error" role="alert">
          <p>{state.message}</p>
          <div className="row">
            {state.recovery === "retry" ? (
              <button type="button" onClick={props.onRetry}>Retry transcription</button>
            ) : state.recovery === "record_again" ? (
              <button type="button" onClick={props.onToggle}>Record again</button>
            ) : (
              <button type="button" onClick={props.onRecovery}>{recoveryLabel(state.recovery)}</button>
            )}
            <button type="button" className="secondary" onClick={props.onDismiss}>Dismiss</button>
          </div>
        </div>
      )}

      <div className="row">
        <button type="button" onClick={props.onToggle} disabled={busy} aria-pressed={recording}>
          {recording ? "Stop" : "Start dictation"}
        </button>
        {(recording || busy) && (
          <button type="button" className="secondary" onClick={props.onCancel}>Cancel</button>
        )}
      </div>

      <label htmlFor="transcript" className="label">Transcript</label>
      <textarea
        id="transcript"
        value={text}
        onChange={(e) => setText(e.target.value)}
        rows={7}
        placeholder="Your transcript will appear here."
      />
      <div className="row">
        <button type="button" onClick={copy} disabled={!text}>Copy</button>
        {copyNote && <span className="note" role="status">{copyNote}</span>}
      </div>

      {state.status === "completed" && state.rawText && (
        <details className="note">
          <summary>Written by Gemini. Show what you said</summary>
          <p>{state.rawText}</p>
        </details>
      )}
      {state.status === "completed" && state.refinementNote && (
        <p className="callout callout--error" role="status">
          AI refinement skipped: {state.refinementNote} The original transcript was used.
        </p>
      )}

      {state.status === "completed" && (
        <p className="metrics">
          {state.pasted ? "Pasted at your cursor. " : state.delivered ? "Copied to clipboard. " : ""}
          {state.pasteError && <span role="alert">Not pasted: {state.pasteError} Press Ctrl+V to paste it yourself. </span>}
          {state.deliveryError && <span role="alert">{state.deliveryError} </span>}
          Audio {(state.metrics.audioMs / 1000).toFixed(1)} s · processed in{" "}
          {(state.metrics.transcribeMs / 1000).toFixed(2)} s (RTF {state.metrics.rtf.toFixed(2)}) · model{" "}
          {state.metrics.model}
          {state.metrics.refineMs > 0 && <> · refined in {(state.metrics.refineMs / 1000).toFixed(2)} s</>}
        </p>
      )}
    </section>
  );
}
