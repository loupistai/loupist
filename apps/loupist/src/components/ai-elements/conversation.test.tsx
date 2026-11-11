import { act, fireEvent, render, screen } from "@testing-library/react";
import { memo, type ReactNode } from "react";
import { ActivityRow } from "../chat/ActivityRow";
import { ChatStatusPill } from "../chat/ChatStatusPill";
import { deriveTurnView } from "../../chat/turnView";
import { Conversation, ConversationContent, useConversationScroll } from "./conversation";

let resize: () => void;
let disconnect: ReturnType<typeof vi.fn>;

beforeEach(() => {
  disconnect = vi.fn();
  vi.stubGlobal("ResizeObserver", class {
    constructor(callback: () => void) { resize = callback; }
    observe() {}
    disconnect = disconnect;
  });
});
afterEach(() => vi.unstubAllGlobals());

function InspectWork() {
  const conversation = useConversationScroll();
  return <button onClick={() => conversation?.holdScrollPosition()}>Inspect work</button>;
}

function conversation(extra?: ReactNode) {
  const content = (prompt: string, revision = "one") => <Conversation aria-label="Conversation" scrollRevision={revision} scrollResetKey={prompt} footer={<ChatStatusPill />}><ConversationContent><p>Answer</p><InspectWork />{extra}</ConversationContent></Conversation>;
  const view = render(content("first-prompt"));
  const element = screen.getByRole("log");
  let height = 1000;
  let viewport = 400;
  let top = 0;
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, get: () => height },
    clientHeight: { configurable: true, get: () => viewport },
    scrollTop: { configurable: true, get: () => top, set: (value) => { top = Math.max(0, Math.min(value, height - viewport)); } },
  });
  act(() => resize());
  return {
    element,
    sendPrompt: () => view.rerender(content("next-prompt")),
    stream: (revision: string) => view.rerender(content("first-prompt", revision)),
    grow: (next: number) => { height = next; },
    resizeViewport: (next: number) => { viewport = next; },
  };
}
const jump = () => screen.queryByRole("button", { name: "Jump to latest" });

it("updates the jump control without rerendering consumers of scroll actions", () => {
  const renders = vi.fn();
  const Actions = memo(function Actions() {
    renders();
    const scroll = useConversationScroll();
    return <button onClick={() => scroll?.scrollToBottom()}>Follow answer</button>;
  });
  conversation(<Actions />);
  const count = renders.mock.calls.length;
  fireEvent.click(screen.getByRole("button", { name: "Inspect work" }));
  expect(jump()).toBeVisible();
  expect(renders).toHaveBeenCalledTimes(count);
  fireEvent.click(screen.getByRole("button", { name: "Follow answer" }));
  expect(jump()).not.toBeInTheDocument();
  expect(renders).toHaveBeenCalledTimes(count);
});

it("follows deferred Markdown growth even when a queued scroll event arrives first", () => {
  const { element, grow, resizeViewport } = conversation();
  expect(element.scrollTop).toBe(600);
  grow(1200);
  fireEvent.scroll(element);
  act(() => resize());
  expect(element.scrollTop).toBe(800);
  expect(jump()).not.toBeInTheDocument();
  resizeViewport(300);
  act(() => resize());
  expect(element.scrollTop).toBe(900);
});

it("keeps the newest answer visible when a short thread starts overflowing after a window resize", () => {
  const { element, grow, resizeViewport } = conversation();
  grow(300);
  act(() => resize());
  expect(element.scrollTop).toBe(0);
  resizeViewport(200);
  grow(500);
  // A layout scroll can arrive before ResizeObserver in the native WebView.
  fireEvent.scroll(element);
  fireEvent.resize(window);
  expect(element.scrollTop).toBe(300);
  expect(jump()).not.toBeInTheDocument();
});

it("keeps following the answer when a Markdown reflow clamps the viewport before the next chunk grows it", () => {
  const { element, grow } = conversation();
  grow(920);
  element.scrollTop = 520; // The browser clamps the old 600px offset.
  grow(1120);
  fireEvent.scroll(element); // The queued scroll arrives after the new paragraph.
  act(() => resize());
  expect(element.scrollTop).toBe(720);
  expect(jump()).not.toBeInTheDocument();
});

it("keeps a reader's position during new content and resumes following on explicit return", () => {
  const { element, grow } = conversation();
  element.scrollTop = 250;
  fireEvent.scroll(element);
  grow(1500);
  act(() => resize());
  expect(element.scrollTop).toBe(250);
  fireEvent.click(jump()!);
  expect(element.scrollTop).toBe(1100);
  grow(1800);
  act(() => resize());
  expect(element.scrollTop).toBe(1400);
});

it("returns to the active reply when the reader submits another prompt", () => {
  const { element, grow, sendPrompt } = conversation();
  element.scrollTop = 200;
  fireEvent.scroll(element);
  grow(1500);
  sendPrompt();
  expect(element.scrollTop).toBe(1100);
  expect(jump()).not.toBeInTheDocument();
});

it("lets a reader scroll away one pixel at a time while the reply is growing", () => {
  const { element, grow, stream } = conversation();
  for (let i = 0; i < 5; i++) {
    element.scrollTop -= 1;
    fireEvent.scroll(element);
  }
  grow(1200);
  stream("more text");
  expect(element.scrollTop).toBe(595);
  expect(jump()).toBeVisible();
});

it("honors an upward wheel gesture even if the answer grows before the scroll event", () => {
  const { element, grow } = conversation();
  fireEvent.wheel(element, { deltaY: -15 });
  grow(1200);
  element.scrollTop = 585;
  fireEvent.scroll(element);
  act(() => resize());
  expect(element.scrollTop).toBe(585);
  expect(jump()).toBeVisible();
});

it("keeps an explicitly opened tool result in place while its lazy content arrives", () => {
  const { element, grow, sendPrompt } = conversation();
  expect(element.scrollTop).toBe(600);
  fireEvent.click(screen.getByRole("button", { name: "Inspect work" }));
  grow(1700);
  act(() => resize());
  expect(element.scrollTop).toBe(600);
  expect(jump()).toBeVisible();
  sendPrompt();
  expect(element.scrollTop).toBe(1300);
});

it("never moves the viewport when the reader expands the run header, even as the stream continues", () => {
  const progress = { startedAtMs: 1, finishedAtMs: 2000, items: [{ id: "read", kind: "tool" as const, category: "web" as const, operation: "read" as const, action: "Read the quarterly report", target: null, href: null, status: "succeeded" as const, startedAtMs: 1, finishedAtMs: 1500 }] };
  const view = deriveTurnView({ message: { id: "a", role: "assistant", status: "complete", content: "Done.", created_at: "2026-10-10T00:00:00Z", progress }, now: 3000, lastActivityAt: 3000, stopping: false, retry: { attempt: 1, offered: false } });
  const segment = view.segments.find((item) => item.kind === "activity")!;
  const { element, grow, stream } = conversation(<ActivityRow segment={segment} />);
  expect(element.scrollTop).toBe(600);
  fireEvent.click(screen.getByRole("button", { name: /^Source review/ }));
  grow(1400);
  act(() => resize());
  stream("two");
  expect(element.scrollTop).toBe(600);
  expect(jump()).toBeVisible();
  fireEvent.click(jump()!);
  expect(element.scrollTop).toBe(1000);
  grow(1600);
  stream("three");
  expect(element.scrollTop).toBe(1200);
});

it("flags the thread as scrolled while anything sits under the chat sub-header", () => {
  const { element } = conversation();
  // Pinned to the end of a long thread, the top of the thread is under the sub-header.
  expect(element).toHaveAttribute("data-scrolled");
  element.scrollTop = 0;
  fireEvent.scroll(element);
  expect(element).not.toHaveAttribute("data-scrolled");
  element.scrollTop = 1;
  fireEvent.scroll(element);
  expect(element).toHaveAttribute("data-scrolled");
});
