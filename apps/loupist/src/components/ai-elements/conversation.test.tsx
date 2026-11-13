import { act, fireEvent, render, screen } from "@testing-library/react";
import { memo, useEffect, type ReactNode } from "react";
import { ChatStatusPill } from "../chat/ChatStatusPill";
import { anchorDelta, Conversation, ConversationContent, pillVisible, roomDelta, useConversationScroll } from "./conversation";

let resize: () => void;

const view = { top: 0, bottom: 400, height: 400 };

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame", "performance", "setTimeout", "clearTimeout"] });
  // Reduced motion makes every glide land in one step, so positions can be read at once.
  vi.stubGlobal("matchMedia", (query: string) => ({ matches: query.includes("reduce"), media: query, addEventListener() {}, removeEventListener() {} }));
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resize = callback; }
    observe() {}
    disconnect() {}
  });
});
afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });

describe("where the page goes", () => {
  it("puts a sent question 16 px under the top, or a tall one's bottom at 40%", () => {
    expect(anchorDelta({ top: 300, bottom: 340, height: 40 }, view)).toBe(284);
    expect(anchorDelta({ top: 300, bottom: 600, height: 300 }, view)).toBe(440);
  });

  it("gives new prose the lower 60% of the viewport, shows a card whole with room for the live line, and never moves up", () => {
    expect(roomDelta("prose", { top: 300, bottom: 326, height: 26 }, view)).toBe(140);
    expect(roomDelta("prose", { top: 120, bottom: 146, height: 26 }, view)).toBe(0);
    expect(roomDelta("card", { top: 200, bottom: 440, height: 240 }, view)).toBe(96);
    expect(roomDelta("card", { top: 200, bottom: 700, height: 500 }, view)).toBe(184);
    expect(roomDelta("card", { top: 20, bottom: 60, height: 40 }, view)).toBe(0);
  });

  it("shows the pill only while the end is more than 12 px below the fold and the reader is not following", () => {
    expect(pillVisible(13, false)).toBe(true);
    expect(pillVisible(12, false)).toBe(false);
    expect(pillVisible(200, true)).toBe(false);
  });
});

function Blocks({ onReady }: { onReady: (append: (kind: "prose" | "card") => void, hold: () => void, jump: () => void) => void }) {
  const conversation = useConversationScroll();
  useEffect(() => {
    onReady((kind) => conversation?.blockAppended(document.getElementById("block")!, kind), () => conversation?.holdScrollPosition(), () => conversation?.jumpToLatest());
  });
  return null;
}

function thread({ running = false, wholePixels = false }: { running?: boolean; wholePixels?: boolean } = {}) {
  const actions = { append: (_kind: "prose" | "card") => {}, hold: () => {}, jump: () => {} };
  const content = (prompt: string, live = running, extra?: ReactNode) => <Conversation aria-label="Conversation" scrollResetKey={prompt} running={live} footer={<ChatStatusPill />}>
    <ConversationContent>
      <section data-turn="" data-turn-latest=""><article className="message user">Question</article><div id="block" />{extra}</section>
      <Blocks onReady={(append, hold, jump) => Object.assign(actions, { append, hold, jump })} />
    </ConversationContent>
  </Conversation>;
  let height = 1000;
  let viewport = 400;
  let top = 0;
  // Where the scroller's top edge sits: context above the thread moves it.
  let edgeAt = 0;
  let questionAt = 900;
  let blockAt = 950;
  const define = (element: Element) => {
    Object.defineProperties(element, {
      scrollHeight: { configurable: true, get: () => height },
      clientHeight: { configurable: true, get: () => viewport },
      scrollTop: { configurable: true, get: () => top, // WKWebView keeps scrollTop in whole pixels: a smaller step is dropped.
      set: (value) => { top = Math.max(0, Math.min(wholePixels ? Math.floor(value) : value, height - viewport)); } },
      getBoundingClientRect: { configurable: true, value: () => ({ top: edgeAt, bottom: edgeAt + viewport, height: viewport, left: 0, right: 600, width: 600 }) },
    });
  };
  // jsdom has no layout: give the scroller, the question and the appended block their geometry before the first layout effect.
  const original = HTMLElement.prototype.getBoundingClientRect;
  Object.defineProperty(HTMLElement.prototype, "offsetTop", { configurable: true, get() { return (this as HTMLElement).id === "block" ? blockAt : (this as HTMLElement).classList.contains("user") ? questionAt : 0; } });
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", { configurable: true, get() { return (this as HTMLElement).id === "block" ? 26 : 40; } });
  HTMLElement.prototype.getBoundingClientRect = function () {
    if ((this as HTMLElement).getAttribute("role") === "log") return { top: edgeAt, bottom: edgeAt + viewport, height: viewport, left: 0, right: 600, width: 600, x: 0, y: 0, toJSON() {} } as DOMRect;
    if ((this as HTMLElement).classList.contains("turn-footer")) return { top: height - top - 30, bottom: height - top, height: 30, left: 0, right: 0, width: 0, x: 0, y: 0, toJSON() {} } as DOMRect;
    return original.call(this);
  };
  const result = render(content("first"));
  const element = screen.getByRole("log");
  define(element);
  return {
    element,
    actions,
    sendPrompt: () => { questionAt = 1000; height = 1400; result.rerender(content("second", true)); },
    rerender: (live: boolean, extra?: ReactNode) => result.rerender(content("second", live, extra)),
    grow: (next: number) => { height = next; act(() => resize()); },
    moveBlock: (next: number) => { blockAt = next; },
    /** Context above the thread takes `rows` px: the scroller's top edge moves down and its viewport shrinks by as much. */
    contextAbove: (rows: number) => { viewport += edgeAt - rows; edgeAt = rows; },
    restore: () => { HTMLElement.prototype.getBoundingClientRect = original; delete (HTMLElement.prototype as { offsetTop?: number }).offsetTop; delete (HTMLElement.prototype as { offsetHeight?: number }).offsetHeight; },
  };
}

const jump = () => screen.queryByRole("button", { name: "Jump to latest" });

describe("anchored reading", () => {
  it("puts the sent question at the top, then stays put while the reply grows", () => {
    const t = thread();
    try {
      t.sendPrompt();
      expect(t.element.scrollTop).toBe(1000 - 16);
      t.grow(1600);
      expect(t.element.scrollTop).toBe(984);
    } finally { t.restore(); }
  });

  it("makes room once when a block is appended, and not at all after the reader scrolls", () => {
    const t = thread();
    try {
      t.sendPrompt();
      t.grow(2000);
      t.moveBlock(984 + 300);
      act(() => t.actions.append("prose"));
      // The new prose starts no lower than 40% of the viewport.
      expect(t.element.scrollTop).toBe(984 + 300 - 160);
      fireEvent.wheel(t.element, { deltaY: 40 });
      t.moveBlock(1124 + 380);
      act(() => t.actions.append("card"));
      expect(t.element.scrollTop).toBe(1124);
    } finally { t.restore(); }
  });

  it("follows the end after Jump to latest and stops on upward input", () => {
    const t = thread();
    try {
      t.sendPrompt();
      t.grow(2400);
      fireEvent.scroll(t.element);
      fireEvent.click(jump()!);
      act(() => { vi.advanceTimersByTime(50); });
      expect(t.element.scrollTop).toBe(2000);
      expect(jump()).toBeNull();
      t.grow(2600);
      act(() => { vi.advanceTimersByTime(50); });
      expect(t.element.scrollTop).toBe(2200);
      fireEvent.wheel(t.element, { deltaY: -40 });
      t.grow(2800);
      act(() => { vi.advanceTimersByTime(50); });
      expect(t.element.scrollTop).toBe(2200);
    } finally { t.restore(); }
  });

  it("reaches the end while following when the scroller keeps whole pixels, as WKWebView does", () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
    const t = thread({ wholePixels: true });
    try {
      t.sendPrompt();
      act(() => { vi.advanceTimersByTime(400); });
      t.grow(2400);
      fireEvent.scroll(t.element);
      fireEvent.click(jump()!);
      act(() => { vi.advanceTimersByTime(3_000); });
      expect(t.element.scrollTop).toBe(2000);
    } finally { t.restore(); }
  });

  it("never scrolls when the reader opens something or when it grows; only a new block makes room", () => {
    const t = thread();
    try {
      t.sendPrompt();
      t.grow(2400);
      fireEvent.click(jump()!);
      act(() => t.actions.hold());
      act(() => { vi.advanceTimersByTime(50); });
      const held = t.element.scrollTop;
      t.grow(3000);
      act(() => { vi.advanceTimersByTime(50); });
      expect(t.element.scrollTop).toBe(held);
    } finally { t.restore(); }
  });

  it("keeps what the reader sees in place when context above the thread grows or goes, also when the browser clamps first", () => {
    const t = thread();
    try {
      t.sendPrompt();
      expect(t.element.scrollTop).toBe(984);
      // A 62 px row appears above the thread (the inline rail's "Needs your answer"): the page moves down with it.
      t.contextAbove(62);
      act(() => resize());
      expect(t.element.scrollTop).toBe(984 + 62);
      // The row goes once the question is answered. WKWebView first clamps the position to the larger viewport's range
      // (1400 - 400) and reports a scroll; the page goes back to where the reader's content stays put, and that is not the reader's scroll.
      t.contextAbove(0);
      t.element.scrollTop = t.element.scrollTop;
      expect(t.element.scrollTop).toBe(1000);
      fireEvent.scroll(t.element);
      expect(t.element.scrollTop).toBe(984);
      // Still the page's own position: a new block makes room as before.
      t.grow(2000);
      t.moveBlock(984 + 300);
      act(() => t.actions.append("prose"));
      expect(t.element.scrollTop).toBe(984 + 300 - 160);
    } finally { t.restore(); }
  });

  it("corrects in the commit that changes the context above the thread, before a resize is observed", () => {
    const t = thread();
    try {
      t.sendPrompt();
      t.contextAbove(45);
      t.rerender(true);
      expect(t.element.scrollTop).toBe(984 + 45);
      // The turn's room is sized for the smaller viewport in the same commit, so a later resize cannot clamp the position.
      expect(t.element.style.getPropertyValue("--turn-viewport")).toBe("355px");
    } finally { t.restore(); }
  });

  it("keeps the page still when context above it changes while a glide makes room, then finishes that glide", () => {
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
    const t = thread();
    try {
      t.sendPrompt();
      act(() => { vi.advanceTimersByTime(400); });
      expect(t.element.scrollTop).toBe(984);
      t.grow(2400);
      t.moveBlock(984 + 300);
      act(() => t.actions.append("prose"));
      // The question card's "Needs your answer" row arrives in the same moment: the page starts from where the content stays.
      t.contextAbove(45);
      t.rerender(true);
      expect(t.element.scrollTop).toBe(984 + 45);
      act(() => { vi.advanceTimersByTime(400); });
      expect(t.element.scrollTop).toBe(984 + 300 - 160);
    } finally { t.restore(); }
  });

  it("keeps a thread opened at its end at its end when context above it changes", () => {
    const t = thread();
    try {
      act(() => resize());
      expect(t.element.scrollTop).toBe(600);
      t.contextAbove(45);
      act(() => resize());
      expect(t.element.scrollTop).toBe(1000 - 355);
    } finally { t.restore(); }
  });

  it("opens a settled thread at its end and keeps it there while late content arrives", () => {
    const t = thread();
    try {
      expect(t.element.scrollTop).toBe(0);
      act(() => resize());
      expect(t.element.scrollTop).toBe(600);
      t.grow(1300);
      expect(t.element.scrollTop).toBe(900);
    } finally { t.restore(); }
  });
});

it("draws the pill over the thread in a dock that takes no height, and updates it without rerendering consumers of scroll actions", () => {
  const renders = vi.fn();
  const Actions = memo(function Actions() {
    renders();
    useConversationScroll();
    return null;
  });
  const t = thread();
  try {
    t.rerender(false, <Actions />);
    const count = renders.mock.calls.length;
    act(() => t.actions.hold());
    t.grow(1600);
    fireEvent.scroll(t.element);
    expect(jump()).toBeVisible();
    expect(jump()!.closest(".conversation-dock")).not.toBeNull();
    expect(renders).toHaveBeenCalledTimes(count);
  } finally { t.restore(); }
});

it("flags the thread as scrolled while anything sits under the chat sub-header", () => {
  const t = thread();
  try {
    act(() => resize());
    expect(t.element).toHaveAttribute("data-scrolled");
    t.element.scrollTop = 0;
    fireEvent.scroll(t.element);
    expect(t.element).not.toHaveAttribute("data-scrolled");
  } finally { t.restore(); }
});
