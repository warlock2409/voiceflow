import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import type { Recovery } from "@voiceflow/shared-types";
import { ipc } from "../ipc";
import { useDictationState, useElapsed } from "../state/useDictationState";
import { BarView } from "./BarView";
import "./bar.css";

function Bar() {
  const state = useDictationState();
  const elapsed = useElapsed(state.status === "recording" ? state.startedAtMs : null);
  const [hotkey, setHotkey] = useState("CapsLock");

  useEffect(() => {
    ipc.getSettings().then((s) => setHotkey(s.hotkey));
  }, []);

  const onRecovery = (r: Recovery) => {
    switch (r) {
      case "retry":
        return void ipc.retry();
      case "record_again":
        return void ipc.start();
      case "dismiss":
        return void ipc.dismiss();
      default:
        // Model download, mic and hotkey fixes live in the main window.
        return void ipc.showMain();
    }
  };

  return (
    <BarView
      state={state}
      elapsedMs={elapsed}
      hotkey={hotkey}
      onToggle={() => void ipc.toggle()}
      onCancel={() => void ipc.cancel()}
      onRecovery={onRecovery}
      onOpen={() => void ipc.showMain()}
      onHide={() => void ipc.setBarVisible(false)}
    />
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Bar />
  </StrictMode>,
);
