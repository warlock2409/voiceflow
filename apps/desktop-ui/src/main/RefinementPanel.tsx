import { useState } from "react";
import type { RefinementMode, RefinementStatus, Settings } from "@voiceflow/shared-types";

export interface RefinementPanelProps {
  settings: Settings;
  status: RefinementStatus;
  onSave: (s: Settings) => Promise<void>;
  /** Stores + verifies the key; resolves with Gemini's edit of a sample sentence. */
  onSetKey: (key: string) => Promise<string>;
  onClearKey: () => Promise<void>;
}

const MODES: { value: RefinementMode; label: string }[] = [
  { value: "verbatim", label: "Off: paste exactly what was recognised" },
  { value: "clean", label: "Clean: fix grammar, punctuation and filler words" },
  { value: "polished", label: "Polished: clear, meaningful sentences" },
  { value: "developer", label: "Developer: keep code terms, identifiers and paths" },
];

export function RefinementPanel({ settings, status, onSave, onSetKey, onClearKey }: RefinementPanelProps) {
  const [key, setKey] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);

  const saveKey = async () => {
    setBusy(true);
    setMessage(null);
    try {
      const sample = await onSetKey(key);
      setKey("");
      setMessage({ ok: true, text: `Key saved and working. Test edit: "this are a test" → "${sample}"` });
    } catch (e) {
      setMessage({ ok: false, text: String((e as { message?: string })?.message ?? e) });
    } finally {
      setBusy(false);
    }
  };

  const clearKey = async () => {
    await onClearKey();
    setMessage({ ok: true, text: "Key removed from this computer." });
  };

  return (
    <section aria-labelledby="refine-heading" className="panel">
      <h2 id="refine-heading">AI text refinement (Google Gemini)</h2>

      <p className="hint">
        When on, the <strong>transcript text</strong> (never audio) is sent to Google Gemini to fix grammar and make
        it read clearly. If Gemini is slow, offline or fails, you still get the original transcript.
      </p>

      <p role="status">
        API key: <strong>{status.keyConfigured ? "saved (Windows Credential Manager)" : "not set"}</strong>
      </p>

      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          if (key.trim()) void saveKey();
        }}
      >
        <label htmlFor="gemini-key" className="label">
          {status.keyConfigured ? "Replace API key" : "Gemini API key"}
        </label>
        <input
          id="gemini-key"
          type="password"
          autoComplete="off"
          spellCheck={false}
          value={key}
          onChange={(e) => setKey(e.target.value)}
          placeholder="Paste your key from Google AI Studio"
        />
        <button type="submit" disabled={busy || !key.trim()}>
          {busy ? "Testing…" : "Save & test"}
        </button>
        {status.keyConfigured && (
          <button type="button" className="secondary" onClick={() => void clearKey()} disabled={busy}>
            Remove key
          </button>
        )}
      </form>
      {message && (
        <p className={message.ok ? "note" : "callout callout--error"} role={message.ok ? "status" : "alert"}>
          {message.text}
        </p>
      )}

      <label htmlFor="refine-mode" className="label">Refinement</label>
      <select
        id="refine-mode"
        value={settings.refinement}
        onChange={(e) => void onSave({ ...settings, refinement: e.target.value as RefinementMode })}
      >
        {MODES.map((m) => (
          <option key={m.value} value={m.value}>{m.label}</option>
        ))}
      </select>

      <label htmlFor="gemini-model" className="label">Gemini model</label>
      <input
        id="gemini-model"
        defaultValue={settings.geminiModel}
        spellCheck={false}
        onBlur={(e) => {
          const model = e.target.value.trim();
          if (model && model !== settings.geminiModel) void onSave({ ...settings, geminiModel: model });
        }}
      />
      <p className="hint">Default gemini-3.5-flash-lite is the fastest. gemini-3.8-flash may write better but is slower.</p>
    </section>
  );
}
