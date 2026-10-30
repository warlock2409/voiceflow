import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { DictationState } from "@voiceflow/shared-types";
import { TranscriptPanel, type TranscriptPanelProps } from "./TranscriptPanel";

function renderPanel(state: DictationState, overrides: Partial<TranscriptPanelProps> = {}) {
  const props: TranscriptPanelProps = {
    state,
    elapsedMs: 0,
    hotkey: "Ctrl+Shift+Space",
    hotkeyActive: true,
    pushToTalk: false,
    onToggle: vi.fn(),
    onCancel: vi.fn(),
    onRetry: vi.fn(),
    onDismiss: vi.fn(),
    onCopy: vi.fn().mockResolvedValue(undefined),
    onRecovery: vi.fn(),
    ...overrides,
  };
  const utils = render(<TranscriptPanel {...props} />);
  return { props, ...utils };
}

const completed: DictationState = {
  status: "completed",
  session: 2,
  text: "Ship the release today.",
  rawText: null,
  refinementNote: null,
  metrics: { audioMs: 2500, transcribeMs: 500, rtf: 0.2, model: "base.en", refineMs: 0 },
  pasted: false,
  pasteError: null,
  delivered: false,
  deliveryError: "Clipboard busy",
};

describe("TranscriptPanel", () => {
  it("shows the transcript, metrics and a delivery failure", () => {
    renderPanel(completed);
    expect(screen.getByLabelText("Transcript")).toHaveValue("Ship the release today.");
    expect(screen.getByText(/RTF 0.20/)).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Clipboard busy");
  });

  it("refined result offers the original transcript", () => {
    renderPanel({
      ...completed,
      deliveryError: null,
      text: "We should move the meeting to tomorrow.",
      rawText: "um we should uh move the meeting to tomorrow",
      metrics: { ...completed.metrics, refineMs: 640 },
    } as DictationState);
    expect(screen.getByLabelText("Transcript")).toHaveValue("We should move the meeting to tomorrow.");
    expect(screen.getByText(/Show what you said/)).toBeInTheDocument();
    expect(screen.getByText("um we should uh move the meeting to tomorrow")).toBeInTheDocument();
    expect(screen.getByText(/refined in 0.64 s/)).toBeInTheDocument();
  });

  it("refinement failure is explained and raw text kept", () => {
    renderPanel({ ...completed, deliveryError: null, refinementNote: "Gemini took too long to respond." } as DictationState);
    expect(screen.getByText(/AI refinement skipped: Gemini took too long/)).toBeInTheDocument();
    expect(screen.getByLabelText("Transcript")).toHaveValue("Ship the release today.");
  });

  it("reports pasted, and explains a refused paste", () => {
    const { unmount } = renderPanel({ ...completed, deliveryError: null, pasted: true } as DictationState);
    expect(screen.getByText(/Pasted at your cursor/)).toBeInTheDocument();
    unmount();
    renderPanel({
      ...completed,
      deliveryError: null,
      delivered: true,
      pasteError: "You switched windows while VoiceFlow was working.",
    } as DictationState);
    expect(screen.getByRole("alert")).toHaveTextContent("Not pasted: You switched windows");
    expect(screen.getByText(/Copied to clipboard/)).toBeInTheDocument();
  });

  it("copies the edited text", async () => {
    const { props } = renderPanel(completed);
    fireEvent.change(screen.getByLabelText("Transcript"), { target: { value: "Edited." } });
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(props.onCopy).toHaveBeenCalledWith("Edited."));
    expect(await screen.findByText("Copied to clipboard.")).toBeInTheDocument();
  });

  it("retryable error offers retry and dismiss", () => {
    const { props } = renderPanel({
      status: "error",
      session: 3,
      kind: "transcription_failed",
      message: "Transcription failed: boom",
      recovery: "retry",
    });
    expect(screen.getByRole("alert")).toHaveTextContent("Transcription failed: boom");
    fireEvent.click(screen.getByRole("button", { name: "Retry transcription" }));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(props.onRetry).toHaveBeenCalledOnce();
    expect(props.onDismiss).toHaveBeenCalledOnce();
  });

  it("idle explains the shortcut", () => {
    renderPanel({ status: "idle" });
    expect(screen.getByRole("status")).toHaveTextContent("Press Ctrl+Shift+Space anywhere to start");
  });

  it("push-to-talk hint says hold", () => {
    renderPanel({ status: "idle" }, { hotkey: "CapsLock", pushToTalk: true });
    expect(screen.getByRole("status")).toHaveTextContent("Hold CapsLock while speaking");
  });

  it("unregistered shortcut is flagged instead of advertised", () => {
    renderPanel({ status: "idle" }, { hotkeyActive: false });
    expect(screen.getByRole("status")).not.toHaveTextContent("Press Ctrl+Shift+Space");
    expect(screen.getByRole("alert")).toHaveTextContent("not available");
  });
});
