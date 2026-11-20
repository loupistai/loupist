import { act, render, renderHook, screen } from "@testing-library/react";
import type { LiveLine } from "../../chat/turnView";
import { DWELL_MS, STEP_HOLD_MS, TurnFooter, useShownLine } from "./TurnFooter";

const line = (kind: LiveLine["kind"], label: string, extra: Partial<LiveLine> = {}): LiveLine =>
  ({ kind, label, glyph: kind === "step" ? "web" : "phase", target: null, more: 0, sheen: kind !== "stopping" && kind !== "waiting", key: `${label}|${extra.target ?? ""}`, ...extra });

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.useRealTimers(); });

describe("the live line's label", () => {
  it("changes at most twice when five steps change within a second", () => {
    const { result, rerender } = renderHook(({ value }) => useShownLine(value), { initialProps: { value: line("step", "Step 0") } });
    const shown = new Set([result.current.label]);
    for (let index = 1; index <= 5; index += 1) {
      act(() => { vi.advanceTimersByTime(190); });
      rerender({ value: line("step", `Step ${index}`) });
      shown.add(result.current.label);
    }
    expect(shown.size).toBeLessThanOrEqual(2);
    act(() => { vi.advanceTimersByTime(DWELL_MS); });
    expect(result.current.label).toBe("Step 5");
  });

  it("never reads Thinking in a 300 ms gap between steps", () => {
    const { result, rerender } = renderHook(({ value }) => useShownLine(value), { initialProps: { value: line("step", "Reading") } });
    act(() => { vi.advanceTimersByTime(DWELL_MS + 100); });
    rerender({ value: line("thinking", "Thinking") });
    act(() => { vi.advanceTimersByTime(300); });
    expect(result.current.label).toBe("Reading");
    rerender({ value: line("step", "Calculating") });
    expect(result.current.label).toBe("Calculating");
  });

  it("holds a finished step's label for 800 ms before Thinking", () => {
    const { result, rerender } = renderHook(({ value }) => useShownLine(value), { initialProps: { value: line("step", "Reading") } });
    act(() => { vi.advanceTimersByTime(DWELL_MS + 100); });
    rerender({ value: line("thinking", "Thinking") });
    act(() => { vi.advanceTimersByTime(STEP_HOLD_MS - 50); });
    expect(result.current.label).toBe("Reading");
    act(() => { vi.advanceTimersByTime(100); });
    expect(result.current.label).toBe("Thinking");
  });

  it("follows a finished step's 800 ms hold with the batch line, never with Thinking", () => {
    const { result, rerender } = renderHook(({ value }) => useShownLine(value), { initialProps: { value: line("step", "Reading a web source") } });
    act(() => { vi.advanceTimersByTime(DWELL_MS + 100); });
    // The step ends: the host reports the last finished batch, not a phase.
    rerender({ value: line("batch", "Read a web source ×3", { glyph: "web" }) });
    const shown = new Set<string>();
    for (let elapsed = 0; elapsed < STEP_HOLD_MS - 50; elapsed += 50) {
      act(() => { vi.advanceTimersByTime(50); });
      shown.add(result.current.label);
    }
    expect([...shown]).toEqual(["Reading a web source"]);
    act(() => { vi.advanceTimersByTime(100); });
    expect(result.current).toMatchObject({ kind: "batch", label: "Read a web source ×3" });
    // A quick next step replaces it with no Thinking in between.
    rerender({ value: line("step", "Calculating") });
    act(() => { vi.advanceTimersByTime(DWELL_MS); });
    expect(result.current.label).toBe("Calculating");
  });

  it("shows a stop or a question at once, and +N follows the latest state", () => {
    const { result, rerender } = renderHook(({ value }) => useShownLine(value), { initialProps: { value: line("step", "Reading", { more: 1 }) } });
    rerender({ value: line("step", "Reading", { more: 2 }) });
    expect(result.current.more).toBe(2);
    rerender({ value: line("stopping", "Stopping") });
    expect(result.current.label).toBe("Stopping");
  });
});

describe("the footer slot", () => {
  it("shows one live line with its clock and no amber while running, then the actions in the same slot at commit", () => {
    vi.setSystemTime(10_000);
    const live = { kind: "live" as const, line: line("step", "Reading a web source", { target: "hanul.example", more: 2 }), sinceMs: 4_000 };
    const view = render(<TurnFooter footer={live} announce="Reading a web source · hanul.example"><button>Copy answer</button></TurnFooter>);
    const status = screen.getByLabelText("Response status");
    expect(status).toHaveTextContent("Reading a web source");
    expect(status).toHaveTextContent("+2");
    expect(status).toHaveTextContent("0:06");
    expect(status).toHaveAttribute("data-sheen");
    expect(view.container.querySelectorAll(".turn-live")).toHaveLength(1);
    expect(view.container.querySelector(".turn-live-dot, .chat-progress-live")).toBeNull();
    expect(screen.queryByRole("button", { name: "Copy answer" })).toBeNull();
    view.rerender(<TurnFooter footer={{ kind: "settled", sources: [], copyText: "Done.", feedback: true, runState: null }} announce="Worked 6s · 1 step"><button>Copy answer</button></TurnFooter>);
    expect(screen.getByRole("button", { name: "Copy answer" }).parentElement).toHaveAttribute("data-arriving");
    // The live line leaves (120 ms) while the actions arrive in its place.
    expect(view.container.querySelector(".turn-live[data-leaving]")).not.toBeNull();
    act(() => { vi.advanceTimersByTime(150); });
    expect(view.container.querySelector(".turn-live")).toBeNull();
  });

  it("does not repeat a note that is on the page above it, but names a note that is not", () => {
    const note = line("note", "Next I’ll compare margins.", { besideNote: line("thinking", "Thinking") });
    const footer = { kind: "live" as const, line: note, sinceMs: 0 };
    const onPage = render(<TurnFooter footer={footer} announce="Next I’ll compare margins." noteOnPage />);
    expect(screen.getByLabelText("Response status")).toHaveTextContent("Thinking");
    expect(screen.getByLabelText("Response status")).not.toHaveTextContent("compare margins");
    act(() => { vi.advanceTimersByTime(10); });
    expect(onPage.container.querySelector("[aria-live]")).toHaveTextContent("Thinking");
    onPage.unmount();
    render(<TurnFooter footer={footer} announce="Next I’ll compare margins." />);
    expect(screen.getByLabelText("Response status")).toHaveTextContent("Next I’ll compare margins.");
  });

  it("marks a waiting question with the amber dot and no sheen", () => {
    const view = render(<TurnFooter footer={{ kind: "live", line: line("waiting", "Waiting for you", { glyph: "waiting" }), sinceMs: 0 }} announce="Waiting for you" />);
    expect(screen.getByLabelText("Response status")).toHaveAttribute("data-tone", "you");
    expect(screen.getByLabelText("Response status")).not.toHaveAttribute("data-sheen");
    expect(view.container.querySelector(".turn-live-dot")).not.toBeNull();
  });

  it("draws a settled turn read later with no entrance", () => {
    render(<TurnFooter footer={{ kind: "settled", sources: [], copyText: "Done.", feedback: true, runState: null }} announce=""><button>Copy answer</button></TurnFooter>);
    expect(screen.getByRole("button", { name: "Copy answer" }).parentElement).not.toHaveAttribute("data-arriving");
  });
});
