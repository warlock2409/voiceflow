import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { DictationState } from "@voiceflow/shared-types";
import { BarView, type BarViewProps } from "./BarView";

function renderBar(state: DictationState, overrides: Partial<BarViewProps> = {}) {
  const props: BarViewProps = {
    state,
    elapsedMs: 0,
    hotkey: "Ctrl+Shift+Space",
    onToggle: vi.fn(),
    onCancel: vi.fn(),
    onRecovery: vi.fn(),
    onOpen: vi.fn(),
    onHide: vi.fn(),
    ...overrides,
  };
  render(<BarView {...props} />);
  return props;
}

describe("BarView", () => {
  it("idle: labelled start control and text status", () => {
    const p = renderBar({ status: "idle" });
    expect(screen.getByRole("status")).toHaveTextContent("Ready");
    fireEvent.click(screen.getByRole("button", { name: "Start dictation" }));
    expect(p.onToggle).toHaveBeenCalledOnce();
    expect(screen.queryByRole("button", { name: "Cancel dictation" })).toBeNull();
  });

  it("recording: shows text label and timer, stop and cancel controls", () => {
    const p = renderBar({ status: "recording", session: 1, startedAtMs: 0, intent: "dictate" }, { elapsedMs: 65_000 });
    expect(screen.getByRole("status")).toHaveTextContent("Listening 1:05");
    const stop = screen.getByRole("button", { name: "Stop dictation" });
    expect(stop).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByRole("button", { name: "Cancel dictation" }));
    expect(p.onCancel).toHaveBeenCalledOnce();
  });

  it("content creation is labelled differently from dictation", () => {
    const { unmount } = render(
      <BarView
        state={{ status: "recording", session: 2, startedAtMs: 0, intent: "compose" }}
        elapsedMs={3000}
        hotkey="CapsLock"
        onToggle={vi.fn()}
        onCancel={vi.fn()}
        onRecovery={vi.fn()}
        onOpen={vi.fn()}
        onHide={vi.fn()}
      />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Listening (create) 0:03");
    unmount();
    renderBar({ status: "refining", session: 2, intent: "compose" });
    expect(screen.getByRole("status")).toHaveTextContent("Writing…");
  });

  it("missing key for content creation offers to add one", () => {
    const p = renderBar({
      status: "error",
      session: 3,
      kind: "generation_unavailable",
      message: "Content creation needs a Gemini API key.",
      recovery: "open_settings",
    });
    fireEvent.click(screen.getByRole("button", { name: "Add key" }));
    expect(p.onRecovery).toHaveBeenCalledWith("open_settings");
  });

  it("transcribing: primary control disabled to prevent overlapping sessions", () => {
    renderBar({ status: "transcribing", session: 1 });
    expect(screen.getByRole("status")).toHaveTextContent("Transcribing");
    expect(screen.getByRole("button", { name: "Start dictation" })).toBeDisabled();
  });

  it("completed: reports copy result in text", () => {
    renderBar({
      status: "completed",
      session: 1,
      text: "Hello.",
      rawText: null,
      refinementNote: null,
      metrics: { audioMs: 1000, transcribeMs: 200, rtf: 0.2, model: "base.en", refineMs: 0 },
      pasted: false,
      pasteError: null,
      delivered: true,
      deliveryError: null,
    });
    expect(screen.getByRole("status")).toHaveTextContent("Copied");
  });

  it("completed via paste says Pasted", () => {
    renderBar({
      status: "completed",
      session: 1,
      text: "Hello.",
      rawText: null,
      refinementNote: null,
      metrics: { audioMs: 1000, transcribeMs: 200, rtf: 0.2, model: "base.en", refineMs: 0 },
      pasted: true,
      pasteError: null,
      delivered: false,
      deliveryError: null,
    });
    expect(screen.getByRole("status")).toHaveTextContent("Pasted");
  });

  it("error: offers the recovery action", () => {
    const p = renderBar({
      status: "error",
      session: 1,
      kind: "model_missing",
      message: "Model not downloaded",
      recovery: "download_model",
    });
    expect(screen.getByRole("status")).toHaveTextContent("Needs attention");
    fireEvent.click(screen.getByRole("button", { name: "Get model" }));
    expect(p.onRecovery).toHaveBeenCalledWith("download_model");
  });

  it("hide and open controls are labelled", () => {
    const p = renderBar({ status: "idle" });
    fireEvent.click(screen.getByRole("button", { name: "Hide bar" }));
    fireEvent.click(screen.getByRole("button", { name: "Open VoiceFlow window" }));
    expect(p.onHide).toHaveBeenCalledOnce();
    expect(p.onOpen).toHaveBeenCalledOnce();
  });
});
