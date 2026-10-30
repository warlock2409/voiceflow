import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Settings } from "@voiceflow/shared-types";
import { RefinementPanel, type RefinementPanelProps } from "./RefinementPanel";

const settings: Settings = {
  hotkey: "CapsLock",
  composeHotkey: "Ctrl+Space",
  recordingMode: "push_to_talk",
  model: "base.en",
  language: "en",
  refinement: "polished",
  geminiModel: "gemini-3.5-flash-lite",
  autoPaste: true,
  autoCopy: false,
  barVisible: true,
};

function renderPanel(overrides: Partial<RefinementPanelProps> = {}) {
  const props: RefinementPanelProps = {
    settings,
    status: { keyConfigured: false },
    onSave: vi.fn().mockResolvedValue(undefined),
    onSetKey: vi.fn().mockResolvedValue("This is a test."),
    onClearKey: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
  render(<RefinementPanel {...props} />);
  return props;
}

describe("RefinementPanel", () => {
  it("key field is masked and states that only text is sent", () => {
    renderPanel();
    expect(screen.getByLabelText("Gemini API key")).toHaveAttribute("type", "password");
    expect(screen.getByText(/never audio/)).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("not set");
  });

  it("saves, clears the field and shows the test result", async () => {
    const p = renderPanel();
    const input = screen.getByLabelText("Gemini API key");
    fireEvent.change(input, { target: { value: "AIza-test-key-0123456789" } });
    fireEvent.click(screen.getByRole("button", { name: "Save & test" }));
    expect(await screen.findByText(/Key saved and working/)).toHaveTextContent("This is a test.");
    expect(p.onSetKey).toHaveBeenCalledWith("AIza-test-key-0123456789");
    expect(input).toHaveValue("");
  });

  it("shows a rejected key error", async () => {
    renderPanel({ onSetKey: vi.fn().mockRejectedValue({ message: "Gemini rejected the API key." }) });
    fireEvent.change(screen.getByLabelText("Gemini API key"), { target: { value: "bad-key-0123456789abcdef" } });
    fireEvent.click(screen.getByRole("button", { name: "Save & test" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("rejected the API key");
  });

  it("mode can be turned off", () => {
    const p = renderPanel({ status: { keyConfigured: true } });
    fireEvent.change(screen.getByLabelText("Refinement"), { target: { value: "verbatim" } });
    expect(p.onSave).toHaveBeenCalledWith({ ...settings, refinement: "verbatim" });
    expect(screen.getByRole("button", { name: "Remove key" })).toBeInTheDocument();
  });
});
