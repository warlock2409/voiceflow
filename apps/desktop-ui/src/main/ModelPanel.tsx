import { useCallback, useEffect, useState } from "react";
import type { DownloadProgress, ModelEntry } from "@voiceflow/shared-types";
import { errorMessage, ipc, onDownloadProgress } from "../ipc";

export function ModelPanel({ selected, onSelect }: { selected: string; onSelect: (id: string) => void }) {
  const [models, setModels] = useState<ModelEntry[]>([]);
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    ipc.listModels().then(setModels);
  }, []);

  useEffect(() => {
    refresh();
    const un = onDownloadProgress(setProgress);
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  const download = async (id: string) => {
    setError(null);
    setProgress({ model: id, done: 0, total: 1 });
    try {
      await ipc.downloadModel(id);
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setProgress(null);
      refresh();
    }
  };

  return (
    <section aria-labelledby="model-heading" className="panel">
      <h2 id="model-heading">Speech model</h2>
      <p className="hint">
        Transcription runs entirely on this computer. Downloading a model is the only time VoiceFlow uses the internet.
      </p>
      <ul className="models">
        {models.map((m) => {
          const downloading = progress?.model === m.id;
          const pct = downloading && progress ? Math.floor((progress.done / Math.max(progress.total, 1)) * 100) : 0;
          return (
            <li key={m.id}>
              <label>
                <input
                  type="radio"
                  name="model"
                  value={m.id}
                  checked={selected === m.id}
                  onChange={() => onSelect(m.id)}
                />{" "}
                {m.label} · {(m.sizeBytes / 1e6).toFixed(0)} MB ·{" "}
                <span className={`badge badge--${m.status}`}>
                  {m.status === "ready" ? "Downloaded" : m.status === "corrupt" ? "Damaged" : "Not downloaded"}
                </span>
              </label>
              {m.status !== "ready" &&
                (downloading ? (
                  <progress max={100} value={pct} aria-label={`Downloading ${m.label}`}>
                    {pct}%
                  </progress>
                ) : (
                  <button type="button" onClick={() => download(m.id)} disabled={progress !== null}>
                    Download
                  </button>
                ))}
            </li>
          );
        })}
      </ul>
      {error && (
        <p className="callout callout--error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
