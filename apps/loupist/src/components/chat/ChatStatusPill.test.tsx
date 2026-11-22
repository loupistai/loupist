import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ChatMessage } from "../../types";
import { Conversation, ConversationContent } from "../ai-elements/conversation";
import { ChatStatusPill, type ChatRunStatus } from "./ChatStatusPill";

const start = Date.UTC(2026, 9, 31, 9, 0, 0);
const streaming: ChatMessage = {
  id: "answer", role: "assistant", content: "Hanul's revenue grew", created_at: new Date(start).toISOString(), status: "streaming", phase: "tool",
  progress: { startedAtMs: start, items: [{ id: "compare", kind: "tool", category: "compute", operation: "calculate", action: "Compared utilization", running: "Comparing utilization", status: "running", target: null, href: null, startedAtMs: 0, finishedAtMs: null }], finishedAtMs: null },
};

/** A laid-out thread that opens at the end and lets the reader scroll away. */
function renderPill(message: ChatMessage | undefined, run?: ChatRunStatus) {
  const view = render(<Conversation aria-label="Conversation" footer={<ChatStatusPill message={message} run={run} />}><ConversationContent>Thread</ConversationContent></Conversation>);
  const element = screen.getByRole("log");
  let top = 0;
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, get: () => 1000 },
    clientHeight: { configurable: true, get: () => 400 },
    scrollTop: { configurable: true, get: () => top, set: (value) => { top = Math.max(0, Math.min(600, value)); } },
  });
  // jsdom had no layout at mount. Deliver the initial resize and let the thread settle before input.
  fireEvent.resize(window);
  act(() => {
    vi.advanceTimersToNextFrame();
    vi.advanceTimersToNextFrame();
  });
  return { ...view, scrollAway: () => {
    fireEvent.wheel(element, { deltaY: -400 });
    element.scrollTop = 200;
    fireEvent.scroll(element);
    act(() => { vi.advanceTimersToNextFrame(); });
  } };
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date", "requestAnimationFrame", "cancelAnimationFrame", "setInterval", "clearInterval"] });
  vi.setSystemTime(start + 42_000);
});
afterEach(() => vi.useRealTimers());

it("stays hidden while the end of the thread is in view", () => {
  renderPill(streaming, { running: true });
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
});

it("shows the live label and clock with Jump to latest once the end is out of view; Stop stays in the composer", () => {
  const { container, scrollAway } = renderPill(streaming, { running: true });
  scrollAway();
  expect(container.querySelector(".chat-status-pill-label")).toHaveTextContent("Comparing utilization · 0:42");
  expect(container.querySelector(".chat-status-pill")).not.toHaveTextContent("Working");
  expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  expect(screen.getByRole("button", { name: "Jump to latest" })).toBeVisible();
  act(() => { vi.advanceTimersByTime(15_000); });
  expect(container.querySelector(".chat-status-pill-label")).toHaveTextContent("Comparing utilization · 0:57");
  // The turn's footer is the one live region; the pill is quiet.
  expect(screen.queryByRole("status")).toBeNull();
});

it("names the latest note's sentence, which the pill's place does not show, and keeps the clock", () => {
  const noted: ChatMessage = { ...streaming, phase: "thinking", progress: { ...streaming.progress!, items: [
    { ...streaming.progress!.items[0]!, status: "succeeded" } as NonNullable<ChatMessage["progress"]>["items"][number],
    { id: "n1", kind: "commentary", text: "Utilization is up. Next I’ll compare it with inventory days over the last four quarters." },
  ] } };
  const { container, scrollAway } = renderPill(noted, { running: true });
  scrollAway();
  expect(container.querySelector(".chat-status-pill-text")).toHaveTextContent("Next I’ll compare it with inventory days over the last four quarters.");
  expect(container.querySelector(".chat-status-pill-clock")).toHaveTextContent("0:42");
});

it.each([
  ["a reference link", "Compared with [the filing][1].\n\n[1]: https://secret.example/a", "Compared with the filing."],
  ["an HTML anchor", 'Read <a href="https://secret.example/h">the filing</a>.', "Read the filing."],
  ["an autolink", "See <https://secret.example/token>.", "See secret.example."],
  ["a bare URL", "Details are at https://secret.example/path?token=abc.", "Details are at secret.example."],
])("names a note with %s as its reader sees it, never the link's target", (_name, text, shown) => {
  const noted: ChatMessage = { ...streaming, phase: "thinking", progress: { ...streaming.progress!, items: [
    { ...streaming.progress!.items[0]!, status: "succeeded" } as NonNullable<ChatMessage["progress"]>["items"][number],
    { id: "n1", kind: "commentary", text },
  ] } };
  const { container, scrollAway } = renderPill(noted, { running: true });
  scrollAway();
  expect(container.querySelector(".chat-status-pill-text")?.textContent).toBe(shown);
  expect(container.querySelector(".chat-status-pill")).not.toHaveTextContent(/secret\.example\/|https?:/u);
});

it("says a question waits for the reader, and that a failed reply could not finish", () => {
  const waiting: ChatMessage = { ...streaming, progress: { ...streaming.progress!, questions: [{ requestId: "q", itemId: "compare", question: "Which?", options: [{ label: "A" }], state: "waiting", requestedAtMs: start }] } };
  const first = renderPill(waiting, { running: true });
  first.scrollAway();
  expect(first.container.querySelector(".chat-status-pill-label")).toHaveTextContent("Waiting for you");
  expect(first.container.querySelector(".chat-status-pill-label")).toHaveAttribute("data-tone", "you");
  first.unmount();
  const failed = renderPill({ ...streaming, status: "error", failure_kind: "other" }, { running: false });
  failed.scrollAway();
  expect(failed.container.querySelector(".chat-status-pill-label")).toHaveTextContent("Couldn’t finish");
});

it("offers only Jump to latest when nothing runs", () => {
  const { container, scrollAway } = renderPill({ ...streaming, status: "complete" }, { running: false });
  scrollAway();
  expect(screen.getByRole("button", { name: "Jump to latest" })).toBeVisible();
  expect(container.querySelector(".chat-status-pill-label")).toBeNull();
});
