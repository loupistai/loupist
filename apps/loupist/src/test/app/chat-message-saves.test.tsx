import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage, ChatSaveState, ChatStreamEvent, ChatThread } from "../../types";
import { completedRun } from "../streamFixtures";

// A reply whose save waits on its own (docs/runtime/chat-turns.md#saves-that-wait-on-their-own).
const LOUPIST = "loupist-quality";
const THREAD = "thread-margin";
const SESSION = "How should we read the margin decline?";
const BLOCKER = "Review peer filings";
const model = { profile_id: "test", name: "Test", provider: "test", model: "test", thinking_level: "medium", run_options: {} };
const harness = { schema: "test", mode: "chat" as const, skill_ids: [], capability_ids: [], digest: "test" };
const activity = { startedAtMs: Date.now() - 5_000, finishedAtMs: null, waitedMs: 0 };

const pending = (runId: string, messageId: string): ChatSaveState => ({
  run_id: runId, thread_id: THREAD, message_id: messageId, waiting_for_other_chat: true,
  blocking_chat_id: "chat-blocker", save_activity: { ...activity, blockingChatId: "chat-blocker" },
});

const stored = (id: string, content: string, skipped?: "user"): ChatMessage => ({
  id, role: "assistant", content, created_at: new Date().toISOString(), status: "complete",
  progress: { startedAtMs: activity.startedAtMs, finishedAtMs: null, items: [], saveActivity: { ...activity, finishedAtMs: Date.now() } },
  memory_update: skipped
    ? { status: "no_change", commitId: null, changes: [], skipReason: skipped, discardedMemoryItems: [{ kind: "note", title: "Margin notes" }] }
    : { status: "no_change", commitId: null, changes: [] },
});

function setup(extra: ChatMessage[] = [], blocker: Pick<ChatThread, "title" | "messages"> = { title: BLOCKER, messages: [] }) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const selection = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
    const workspace = await selection(id);
    const session = workspace.threads.find((thread) => thread.id === THREAD);
    if (session) {
      session.messages = [...session.messages, ...extra];
      workspace.threads.push({ ...session, id: "chat-blocker", ...blocker });
    }
    return workspace;
  });
  return bridge;
}

it("frees the conversation once the answer is stored and keeps the wait and Skip saving on that reply", async () => {
  const user = userEvent.setup();
  const bridge = setup();
  const runs: string[] = [];
  const send = vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
    const index = runs.push(request.run_id);
    const messageId = `answer-${index}`;
    const text = index === 1 ? "Margins fell on input costs." : "Last year they held.";
    observer({ type: "started", run_id: request.run_id });
    observer({ type: "delta", run_id: request.run_id, text });
    // Rust reports the reply's own save before its stream ends with the stored answer.
    if (index === 1) act(() => bridge.emitChatSave({ loupist_id: LOUPIST, thread_id: THREAD, message_id: messageId, run_id: request.run_id, pending: pending(request.run_id, messageId) }));
    observer({
      ...completedRun, type: "completed", run_id: request.run_id, message_id: messageId, final_text: text,
      created_at: new Date().toISOString(), execution_model: model, agent_harness: harness,
      ...(index === 1 ? { save_activity: activity } : {}),
    } satisfies ChatStreamEvent);
    return { run_id: request.run_id };
  });
  const skip = vi.spyOn(bridge, "chatAbort").mockImplementation(async (runId) => {
    act(() => bridge.emitChatSave({ loupist_id: LOUPIST, thread_id: THREAD, message_id: "answer-1", run_id: runId,
      message: stored("answer-1", "Margins fell on input costs.", "user") }));
  });
  render(<App bridge={bridge} />);
  await user.click(await screen.findByRole("button", { name: SESSION }));
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Why did margins fall?{Enter}");

  const first = within((await screen.findByText("Margins fell on input costs.")).closest("article")!);
  const status = await first.findByRole("status");
  await waitFor(() => expect(status).toHaveTextContent(`Saving to Memory · waiting for “${BLOCKER}” to finish`));
  const skipHere = first.getByRole("button", { name: "Skip saving" });
  expect(skipHere).toHaveAttribute("title", "Keep the answer and skip saving it to Memory.");
  expect(first.getByRole("button", { name: "Helpful" })).toBeDisabled();
  // The conversation takes its next message while that reply waits to save.
  expect(screen.getByRole("button", { name: "Send" })).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Stop response" })).toBeNull();
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "And last year?{Enter}");
  expect(await screen.findByText("Last year they held.")).toBeVisible();
  expect(send).toHaveBeenCalledTimes(2);
  expect(status).toHaveTextContent(`Saving to Memory · waiting for “${BLOCKER}” to finish`);

  await user.click(skipHere);
  expect(skip).toHaveBeenCalledExactlyOnceWith(runs[0]);
  expect(await first.findByText("Not saved to Memory: you skipped saving.")).toBeVisible();
  expect(first.getByRole("list", { name: "Not kept" })).toHaveTextContent("Margin notes");
  expect(first.queryByRole("button", { name: "Skip saving" })).toBeNull();
  expect(screen.getByText("Last year they held.")).toBeVisible();
});

it("names the session it waits on from that session's request while its first reply runs", async () => {
  const user = userEvent.setup();
  // The 0.1.5 acceptance run: the research session's first answer was still running, untitled.
  const request = "Compare Coca-Cola and PepsiCo using each company's latest annual report on Form 10-K: revenue growth over the last three fiscal years, operating margin, and the three largest risk factors each one names.";
  const bridge = setup([
    { id: "ask-1", role: "user", content: "Why did margins fall?", created_at: new Date().toISOString(), status: "complete" },
    stored("answer-1", "Margins fell on input costs."),
  ], { title: "", messages: [{ id: "blocker-ask", role: "user", content: request, created_at: new Date().toISOString(), status: "complete" }] });
  bridge.emitChatSave({ loupist_id: LOUPIST, thread_id: THREAD, message_id: "answer-1", run_id: "run-1", pending: pending("run-1", "answer-1") });
  render(<App bridge={bridge} />);
  await user.click(await screen.findByRole("button", { name: SESSION }));
  const status = within((await screen.findByText("Margins fell on input costs.")).closest("article")!).getByRole("status");
  await waitFor(() => expect(status).toHaveTextContent("Saving to Memory · waiting for “Compare Coca-Cola and PepsiCo using…” to finish"));
  expect(within(status).getByRole("link", { name: "Compare Coca-Cola and PepsiCo using…" })).toBeVisible();
});

it("shows a save still waiting after the conversation is opened again, until its receipt arrives", async () => {
  const user = userEvent.setup();
  const bridge = setup([
    { id: "ask-1", role: "user", content: "Why did margins fall?", created_at: new Date().toISOString(), status: "complete" },
    { ...stored("answer-1", "Margins fell on input costs."), memory_update: { status: "no_change", commitId: null, changes: [], skipReason: "interrupted" } },
  ]);
  bridge.emitChatSave({ loupist_id: LOUPIST, thread_id: THREAD, message_id: "answer-1", run_id: "run-1", pending: pending("run-1", "answer-1") });
  render(<App bridge={bridge} />);
  await user.click(await screen.findByRole("button", { name: SESSION }));
  const card = within((await screen.findByText("Margins fell on input costs.")).closest("article")!);
  await waitFor(() => expect(card.getByRole("status")).toHaveTextContent(`Saving to Memory · waiting for “${BLOCKER}” to finish`));
  // The receipt a quit would leave stays hidden while the save still waits.
  expect(card.queryByText("Not saved to Memory: Loupist quit before saving finished.")).toBeNull();
  expect(card.getByRole("button", { name: "Skip saving" })).toBeEnabled();
  // Editing the request before it waits for the save.
  expect(screen.getByRole("button", { name: "Edit message" })).toBeDisabled();
  // Skip meets the save already writing: no error, and the receipt follows.
  const refused = vi.spyOn(bridge, "chatAbort").mockRejectedValue({ code: "conflict", message: "run completion already owns the terminal boundary" });
  await user.click(card.getByRole("button", { name: "Skip saving" }));
  expect(refused).toHaveBeenCalledExactlyOnceWith("run-1");
  await waitFor(() => expect(card.getByRole("button", { name: "Skip saving" })).toBeDisabled());
  expect(screen.queryByText("Could not skip saving. Try again.")).toBeNull();
  expect(screen.queryByText("run completion already owns the terminal boundary")).toBeNull();

  act(() => bridge.emitChatSave({ loupist_id: LOUPIST, thread_id: THREAD, message_id: "answer-1", run_id: "run-1",
    message: stored("answer-1", "Margins fell on input costs.") }));
  await waitFor(() => expect(card.queryByRole("button", { name: "Skip saving" })).toBeNull());
  expect(screen.getByRole("button", { name: "Edit message" })).toBeEnabled();
});
