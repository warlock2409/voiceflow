import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type { HotkeyStatuses, RefinementStatus, Settings } from "@voiceflow/shared-types";
import { ipc, onHotkeyStatus } from "../ipc";
import { useDictationState, useElapsed } from "../state/useDictationState";
import { ModelPanel } from "./ModelPanel";
import { RefinementPanel } from "./RefinementPanel";
import { SettingsPanel } from "./SettingsPanel";
import { TranscriptPanel } from "./TranscriptPanel";
import "./main.css";

function App() {
  const state = useDictationState();
  const elapsed = useElapsed(state.status === "recording" ? state.startedAtMs : null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [hotkeyStatus, setHotkeyStatus] = useState<HotkeyStatuses | null>(null);
  const [refineStatus, setRefineStatus] = useState<RefinementStatus>({ keyConfigured: false });

  useEffect(() => {
    ipc.getSettings().then(setSettings);
    ipc.getRefinementStatus().then(setRefineStatus);
    ipc.getHotkeyStatus().then(setHotkeyStatus);
    const un = onHotkeyStatus(setHotkeyStatus);
    return () => {
      un.then((f) => f());
    };
  }, []);

  if (!settings) return null;

  const saveSettings = async (next: Settings) => {
    setSettings(await ipc.updateSettings(next));
  };

  return (
    <main>
      <header className="app-header">
        <h1>VoiceFlow</h1>
        <div className="row">
          {!settings.barVisible && (
            <button type="button" className="secondary" onClick={() => saveSettings({ ...settings, barVisible: true })}>
              Show bar
            </button>
          )}
          <button type="button" className="secondary" onClick={() => void ipc.quit()}>
            Quit VoiceFlow
          </button>
        </div>
      </header>

      <TranscriptPanel
        state={state}
        elapsedMs={elapsed}
        hotkey={settings.hotkey}
        hotkeyActive={hotkeyStatus?.dictate.registered ?? true}
        pushToTalk={settings.recordingMode === "push_to_talk"}
        composeHotkey={hotkeyStatus?.compose.registered ? settings.composeHotkey : null}
        onToggle={() => void ipc.toggle()}
        onCancel={() => void ipc.cancel()}
        onRetry={() => void ipc.retry()}
        onDismiss={() => void ipc.dismiss()}
        onCopy={ipc.copyText}
        onRecovery={() => {
          const target =
            state.status !== "error"
              ? "settings-heading"
              : state.recovery === "download_model"
                ? "model-heading"
                : state.recovery === "open_settings"
                  ? "refine-heading"
                  : "settings-heading";
          document.getElementById(target)?.scrollIntoView({ behavior: "smooth" });
        }}
      />
      <RefinementPanel
        settings={settings}
        status={refineStatus}
        onSave={saveSettings}
        onSetKey={async (key) => {
          try {
            return await ipc.setGeminiKey(key);
          } finally {
            setRefineStatus(await ipc.getRefinementStatus());
          }
        }}
        onClearKey={async () => {
          await ipc.clearGeminiKey();
          setRefineStatus(await ipc.getRefinementStatus());
        }}
      />
      <ModelPanel selected={settings.model} onSelect={(model) => void saveSettings({ ...settings, model })} />
      <SettingsPanel settings={settings} hotkeyStatus={hotkeyStatus} onSave={saveSettings} />
    </main>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
