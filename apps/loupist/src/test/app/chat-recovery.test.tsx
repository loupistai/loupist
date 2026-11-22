import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage } from "../../types";
import { openChat } from "../renderApp";

const at = "2026-11-22T03:00:00Z";
const request: ChatMessage = { id: "recovered-request", role: "user", status: "complete", content: "Summarize cash flow.", created_at: at };
const stopped: ChatMessage = { id: "recovered-reply", role: "assistant", status: "aborted", content: "Response stopped.", created_at: at };

function renderRecovered(messages: ChatMessage[], home = false) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
    const workspace = await select(id);
    const thread = workspace.threads.find((value) => Boolean(value.home) === home)!;
    thread.messages = messages;
    return workspace;
  });
  render(<App bridge={bridge} />);
  return bridge;
}

const responseAnnouncement = (text: string) =>
  screen.findByText(text, { selector: '[role="status"][aria-live="polite"]' });

it.each([
  { content: stopped.content, title: "Stopped before the answer finished." },
  { content: "Cash flow rose.", title: "Stopped · kept the text so far" },
])("opens a stopped session with one notice and Try again ($content)", async ({ content, title }) => {
  renderRecovered([request, { ...stopped, content }]);
  await openChat();
  const answer = document.querySelector('[data-message-id="recovered-reply"]') as HTMLElement;
  expect(within(answer).getByText(title)).toBeVisible();
  expect(within(answer).getAllByRole("button", { name: "Try again" })).toHaveLength(1);
  if (content !== stopped.content) expect(within(answer).getByText(content)).toBeVisible();
  expect(await responseAnnouncement("Response stopped.")).toBeInTheDocument();
  expect(screen.queryByText("Response complete.")).toBeNull();
});

it("keeps Bot's stopped announcement and Try again when a separate card arrives later", async () => {
  renderRecovered([request, stopped, {
    id: "new-topic", role: "assistant", status: "complete", content: "Topic update", created_at: at,
    vocabulary: { changeId: "topic", producer: "chat", labels: ["Cash flow"] },
  }], true);
  expect(await screen.findByText("Stopped before the answer finished.")).toBeVisible();
  expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
  expect(await responseAnnouncement("Response stopped.")).toBeInTheDocument();
});

it("retries an interrupted World link as a scoped hand-off", async () => {
  const bridge = renderRecovered([{
    ...request,
    handoff_action: { kind: "link", connectionId: "con_cash_flow" },
    handoff_note: { title: "Cash flow" },
  }, stopped]);
  const restart = vi.spyOn(bridge, "researchHandoffStart").mockResolvedValue({ chatId: "thread-margin", handoffId: "new-handoff" });
  const retry = vi.spyOn(bridge, "chatRetryLast");
  await openChat();
  await userEvent.click(await screen.findByRole("button", { name: "Try again" }));
  expect(restart).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
    loupistId: "loupist-quality", connectionId: "con_cash_flow",
  }));
  expect(retry).not.toHaveBeenCalled();
});

it("keeps a delivered answer and the interrupted Memory receipt without offering a retry", async () => {
  renderRecovered([request, {
    ...stopped, content: "Cash flow rose.", status: "complete",
    memory_update: { status: "no_change", skipReason: "interrupted", commitId: null, changes: [] },
  }]);
  await openChat();
  expect(await screen.findByText("Cash flow rose.")).toBeVisible();
  expect(screen.getByText("Not saved to Memory: Loupist quit before saving finished.")).toBeVisible();
  expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  expect(await responseAnnouncement("Response complete.")).toBeInTheDocument();
});
