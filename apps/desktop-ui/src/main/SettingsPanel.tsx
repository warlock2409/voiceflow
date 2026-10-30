import { useEffect, useState } from "react";
import type { HotkeyStatuses, Settings } from "@voiceflow/shared-types";

export interface SettingsPanelProps {
  settings: Settings;
  hotkeyStatus: HotkeyStatuses | null;
  onSave: (s: Settings) => Promise<void>;
}

export function SettingsPanel({ settings, hotkeyStatus, onSave }: SettingsPanelProps) {
  const [hotkey, setHotkey] = useState(settings.hotkey);
  const [composeHotkey, setComposeHotkey] = useState(settings.composeHotkey);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setHotkey(settings.hotkey), [settings.hotkey]);
  useEffect(() => setComposeHotkey(settings.composeHotkey), [settings.composeHotkey]);
  const dictateStatus = hotkeyStatus?.dictate;
  const composeStatus = hotkeyStatus?.compose;

  const save = async (patch: Partial<Settings>) => {
    setError(null);
    try {
      await onSave({ ...settings, ...patch });
    } catch (e) {
      setError(String((e as { message?: string })?.message ?? e));
    }
  };

  return (
    <section aria-labelledby="settings-heading" className="panel">
      <h2 id="settings-heading">Settings</h2>

      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          save({ hotkey: hotkey.trim() });
        }}
      >
        <label htmlFor="hotkey" className="label">Dictation shortcut (speech to text)</label>
        <input
          id="hotkey"
          value={hotkey}
          onChange={(e) => setHotkey(e.target.value)}
          aria-describedby="hotkey-help"
          spellCheck={false}
        />
        <button type="submit" disabled={hotkey.trim() === settings.hotkey}>Apply</button>
      </form>
      <p id="hotkey-help" className="hint">
        Examples: Ctrl+Shift+Space, Alt+F9, Ctrl+Alt+D (at least one modifier), or CapsLock on its own. With
        CapsLock, the key no longer toggles capitals; use Shift+CapsLock for that.
      </p>
      {!(settings.hotkey === "CapsLock" && settings.recordingMode === "push_to_talk") && (
        <button type="button" onClick={() => save({ hotkey: "CapsLock", recordingMode: "push_to_talk" })}>
          Use Caps Lock: hold to talk
        </button>
      )}
      {dictateStatus && !dictateStatus.registered && (
        <p className="callout callout--error" role="alert">
          {dictateStatus.error ?? "The shortcut is not active."} You can still dictate with the bar's button.
        </p>
      )}

      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          save({ composeHotkey: composeHotkey.trim() });
        }}
      >
        <label htmlFor="compose-hotkey" className="label">Content creation shortcut (AI writes from your instruction)</label>
        <input
          id="compose-hotkey"
          value={composeHotkey}
          onChange={(e) => setComposeHotkey(e.target.value)}
          aria-describedby="compose-help"
          spellCheck={false}
          placeholder="Leave empty to turn off"
        />
        <button type="submit" disabled={composeHotkey.trim() === settings.composeHotkey}>Apply</button>
      </form>
      <p id="compose-help" className="hint">
        Hold it and say what you want, e.g. "create a 50% off discount email". Gemini writes it and it is pasted at
        your cursor. Needs a Gemini key (below). Note: Ctrl+Space is also code completion in IDEs and an input-language
        switch in Windows; Ctrl+Alt+Space avoids those.
      </p>
      {composeStatus?.error && (
        <p className="callout callout--error" role="alert">{composeStatus.error}</p>
      )}

      <label className="label" htmlFor="mode">Recording mode</label>
      <select
        id="mode"
        value={settings.recordingMode}
        onChange={(e) => save({ recordingMode: e.target.value as Settings["recordingMode"] })}
      >
        <option value="toggle">Toggle: press to start, press again to stop</option>
        <option value="push_to_talk">Push-to-talk: hold the shortcut while speaking</option>
      </select>

      <label className="check">
        <input type="checkbox" checked={settings.autoPaste} onChange={(e) => save({ autoPaste: e.target.checked })} />
        Paste into the app I was typing in (at the cursor)
      </label>
      <label className="check">
        <input type="checkbox" checked={settings.autoCopy} onChange={(e) => save({ autoCopy: e.target.checked })} />
        {settings.autoPaste
          ? "Keep the text on the clipboard afterwards (otherwise your previous clipboard text is restored)"
          : "Copy the text to the clipboard"}
      </label>
      <label className="check">
        <input type="checkbox" checked={settings.barVisible} onChange={(e) => save({ barVisible: e.target.checked })} />
        Show floating bar
      </label>

      {error && <p className="callout callout--error" role="alert">{error}</p>}

      <p className="hint">
        Privacy: audio is processed in memory on this computer and is never saved to disk. No transcript history is
        stored in this version.
      </p>
    </section>
  );
}
