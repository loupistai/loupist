import { act, fireEvent, render, screen } from "@testing-library/react";
import type { ChatMessage } from "../../types";
import { Conversation, ConversationContent, useConversationScroll } from "../ai-elements/conversation";
import { ChatStatusPill, type ChatRunStatus } from "./ChatStatusPill";

const start = Date.UTC(2026, 9, 31, 9, 0, 0);
const streaming: ChatMessage = {
  id: "answer", role: "assistant", content: "Hanul's revenue grew", created_at: new Date(start).toISOString(), status: "streaming", phase: "tool",
  progress: { startedAtMs: start, items: [{ id: "compare", kind: "tool", category: "compute", operation: "calculate", action: "Compared utilization", running: "Comparing utilization", status: "running" }] },
};

function ScrollAway() {
  const conversation = useConversationScroll();
  return <button onClick={() => conversation?.holdScrollPosition()}>Read earlier</button>;
}

function renderPill(message: ChatMessage | undefined, run?: ChatRunStatus) {
  const view = render(<Conversation aria-label="Conversation" footer={<ChatStatusPill message={message} run={run} />}><ConversationContent><ScrollAway /></ConversationContent></Conversation>);
  return { ...view, scrollAway: () => fireEvent.click(screen.getByRole("button", { name: "Read earlier" })) };
}

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(start + 42_000); });
afterEach(() => vi.useRealTimers());

it("stays hidden while the reader follows the end of the thread", () => {
  renderPill(streaming, { running: true });
  expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
});

it("shows the live run and Jump to latest once the reader scrolls up; Stop stays in the composer", () => {
  const { container, scrollAway } = renderPill(streaming, { running: true });
  scrollAway();
  expect(container.querySelector(".chat-status-pill-label")).toHaveTextContent("Working · 0:42 · Comparing utilization");
  expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  expect(screen.getByRole("button", { name: "Jump to latest" })).toBeVisible();
  act(() => { vi.advanceTimersByTime(15_000); });
  expect(container.querySelector(".chat-status-pill-label")).toHaveTextContent("Still working · 0:57 · Comparing utilization");
  // The turn's footer is the one live region; the pill is quiet.
  expect(screen.queryByRole("status")).toBeNull();
});

it("offers only Jump to latest when nothing runs", () => {
  const { container, scrollAway } = renderPill({ ...streaming, status: "complete" }, { running: false });
  scrollAway();
  expect(screen.getByRole("button", { name: "Jump to latest" })).toBeVisible();
  expect(screen.queryByRole("button", { name: /Stop/ })).toBeNull();
  expect(container.querySelector(".chat-status-pill-label")).toBeNull();
});
