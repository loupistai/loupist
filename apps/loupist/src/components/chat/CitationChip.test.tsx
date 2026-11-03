import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ChatEvidenceRef } from "../../types";
import { CitationChip, LONG_PRESS_MS } from "./CitationChip";

const evidence = {
  id: "e1", title: "Hanul Precision Q3 operating review", kind: "original", url: "https://hanul.example/q3", provider: "Hanul IR",
  tool_name: "web_fetch", retrieved_at: "2026-10-28T00:00:00Z", published_at: "2026-10-27",
  excerpt: "Revenue ₩184.2bn; operating margin 14.8%.", excerpt_truncated: false, content_sha256: "a".repeat(64), warnings: [],
} as ChatEvidenceRef;

/** The window width the media queries see: narrow is the 700 px layout. */
function windowWidth(width: number) {
  vi.mocked(window.matchMedia).mockImplementation((query: string) => {
    const max = /max-width:\s*(\d+)px/.exec(query);
    return { matches: max ? width <= Number(max[1]) : false, media: query, onchange: null, addEventListener: vi.fn(), removeEventListener: vi.fn(), addListener: vi.fn(), removeListener: vi.fn(), dispatchEvent: vi.fn() } as MediaQueryList;
  });
}

function renderChip() {
  const onOpen = vi.fn();
  render(<p>Margins fell <CitationChip evidence={evidence} label="Source 1" title={evidence.title} source="hanul.example" onOpen={onOpen}>1</CitationChip> after the line opened.</p>);
  return { onOpen, chip: screen.getByRole("button", { name: "Source 1" }) };
}

const press = (chip: HTMLElement, holdMs: number) => {
  fireEvent.pointerDown(chip, { button: 0, pointerType: "touch" });
  act(() => { vi.advanceTimersByTime(holdMs); });
  fireEvent.pointerUp(chip, { button: 0, pointerType: "touch" });
  fireEvent.click(chip);
};

describe("citation card at 700 px (chat-run §2 breakpoints)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    windowWidth(1280);
  });

  it("opens the card on a press and hold, without opening Evidence, and keeps it until Esc", () => {
    windowWidth(700);
    const { onOpen, chip } = renderChip();
    press(chip, LONG_PRESS_MS);
    const card = screen.getByRole("dialog", { name: evidence.title });
    expect(card).toHaveTextContent("hanul.example · Oct 27, 2026");
    expect(card).toHaveTextContent("operating margin 14.8%");
    expect(chip).toHaveAttribute("aria-expanded", "true");
    expect(onOpen).not.toHaveBeenCalled();
    // Lifting the finger off the chip does not close a card opened this way.
    fireEvent.pointerLeave(chip);
    act(() => { vi.advanceTimersByTime(1000); });
    expect(screen.getByRole("dialog", { name: evidence.title })).toBeInTheDocument();
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps a tap opening Evidence, and opens nothing on hover", () => {
    windowWidth(700);
    const { onOpen, chip } = renderChip();
    fireEvent.pointerEnter(chip);
    act(() => { vi.advanceTimersByTime(1000); });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
    press(chip, 120);
    act(() => { vi.advanceTimersByTime(LONG_PRESS_MS); });
    expect(onOpen).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    // The press after a long press is a fresh tap again.
    press(chip, LONG_PRESS_MS);
    expect(screen.getByRole("dialog", { name: evidence.title })).toBeInTheDocument();
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("leaves wide windows on hover: a long press there is an ordinary click", () => {
    windowWidth(1280);
    const { onOpen, chip } = renderChip();
    fireEvent.pointerEnter(chip);
    act(() => { vi.advanceTimersByTime(150); });
    expect(screen.getByRole("tooltip")).toHaveTextContent(evidence.title);
    press(chip, LONG_PRESS_MS);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });
});
