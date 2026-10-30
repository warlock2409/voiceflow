import { useEffect, useState } from "react";
import type { DictationState } from "@voiceflow/shared-types";
import { ipc, onState } from "../ipc";

/** Renders purely from backend state: fetch once, then follow `dictation://state` events. */
export function useDictationState(): DictationState {
  const [state, setState] = useState<DictationState>({ status: "idle" });

  useEffect(() => {
    let disposed = false;
    const unlisten = onState((s) => {
      if (!disposed) setState(s);
    });
    ipc.getState().then((s) => {
      if (!disposed) setState(s);
    });
    return () => {
      disposed = true;
      unlisten.then((fn) => fn());
    };
  }, []);

  return state;
}

/** Ticking elapsed time for the recording indicator. */
export function useElapsed(startedAtMs: number | null): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (startedAtMs === null) return;
    const id = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(id);
  }, [startedAtMs]);
  return startedAtMs === null ? 0 : Math.max(0, now - startedAtMs);
}
