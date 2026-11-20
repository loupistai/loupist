import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage, ChatThread, InboxChangedEvent } from "../../types";
import { completedRun } from "../streamFixtures";

const delivered: ChatMessage = {
  id: "delivery-1", role: "assistant", status: "complete", host_composed: true, created_at: new Date().toISOString(),
  content: "Hanul Precision's inventory days rose to 61.",
  delivery: { run_id: "run-1", task_id: "task-1", task_title: "Inventory check", task_kind: "custom", schedule: { kind: "weekly", days: [1], hour: 8, minute: 0, timezone: "Asia/Seoul" }, run_thread_id: "thread-margin", status: "succeeded", finished_at_ms: Date.now() },
};

it("lists the home Thread first, lands on it, and opens a delivered result's own Chat", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    const home = workspace.threads.find(thread => thread.home);
    if (home && id === "loupist-quality") home.messages = [delivered];
    return workspace;
  });
  const markRead = vi.spyOn(bridge, "inboxMarkRead").mockResolvedValue();
  render(<App bridge={bridge} />);

  expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
  const chats = document.querySelector<HTMLElement>(".loupist-navigation")!;
  expect(within(chats).getAllByRole("button")[0]).toHaveAccessibleName("Bot");
  // The home Thread has no rename or delete menu.
  expect(screen.queryByRole("button", { name: "Options for Thread" })).toBeNull();

  const result = await screen.findByRole("article", { name: "Result from Inventory check" });
  expect(within(result).getByText("Hanul Precision's inventory days rose to 61.")).toBeVisible();
  // The tag names the task; the foot says when it runs, without naming the task again.
  expect(within(result).getByText("Inventory check")).toBeVisible();
  expect(within(result).getByText(/8:00/)).toBeVisible();
  expect(within(result).queryByText(/^From:/)).toBeNull();
  await vi.waitFor(() => expect(markRead).toHaveBeenCalledWith({ loupistId: "loupist-quality", runIds: ["run-1"] }), { timeout: 2000 });

  await user.click(within(result).getByRole("button", { name: "Open" }));
  expect(await screen.findByRole("heading", { name: "How should we read the margin decline?" })).toBeVisible();
});

it("opens a result's task once its session went with the run, and says so once the task is gone too", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const task = await bridge.proposeScheduledTask({
    loupist_id: "loupist-quality", title: "Inventory check", prompt: "Check inventory days.", schedule: delivered.delivery!.schedule!,
    target: { kind: "new_chat" }, model_profile_id: "model-test", thinking_level: "medium", run_options: {}, use_memory: true, update_memory: false, status: "active",
  });
  // The run's session left with the run; the card in Bot still names it.
  const card: ChatMessage = { ...delivered, delivery: { ...delivered.delivery!, task_id: task.id, run_thread_id: "scheduled-chat-gone" } };
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    const home = workspace.threads.find(thread => thread.home);
    if (home && id === "loupist-quality") home.messages = [card];
    return workspace;
  });
  render(<App bridge={bridge} />);

  const result = await screen.findByRole("article", { name: "Result from Inventory check" });
  await user.click(within(result).getByRole("button", { name: "Open" }));
  expect(await screen.findByRole("article", { name: "Inventory check" })).toBeVisible();
  expect(screen.getByRole("heading", { name: "Schedule", level: 1 })).toBeInTheDocument();
  expect(screen.queryByRole("article", { name: "Result from Inventory check" })).toBeNull();

  await bridge.scheduledTaskDiscard({ loupist_id: "loupist-quality", task_id: task.id });
  await user.click(screen.getByRole("button", { name: "Bot" }));
  const again = await screen.findByRole("article", { name: "Result from Inventory check" });
  await user.click(within(again).getByRole("button", { name: "Open" }));
  expect(await within(again).findByRole("status")).toHaveTextContent("This run's record was removed.");
  expect(within(again).queryByRole("button", { name: "Open" })).toBeNull();
  expect(screen.getByRole("heading", { name: "Bot", level: 1 })).toBeInTheDocument();
});

it("leaves a run's open session for Bot once the host deletes it with its run", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const request: ChatMessage = {
    id: "run-request", role: "user", status: "complete", created_at: "2026-11-06T08:00:00.000Z", content: "Check inventory days.",
    scheduled: { task_id: "task-1", run_id: "run-1", task_title: "Inventory check", task_kind: "custom" },
  };
  const answer: ChatMessage = { id: "run-answer", role: "assistant", status: "complete", created_at: "2026-11-06T08:01:00.000Z", content: "Inventory days rose to 61." };
  const card: ChatMessage = { ...delivered, delivery: { ...delivered.delivery!, run_thread_id: "scheduled-chat-run" } };
  const stored = { session: true };
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    if (id !== "loupist-quality") return workspace;
    const home = workspace.threads.find(thread => thread.home)!;
    home.messages = [card];
    const session: ChatThread[] = stored.session ? [{ id: "scheduled-chat-run", loupist_id: id, title: "Inventory check", updated_at: answer.created_at, use_memory: true, update_memory: false, hide_from_recent: true, messages: [request, answer] }] : [];
    return { ...workspace, threads: [home, ...session, ...workspace.threads.filter(thread => !thread.home)] };
  });
  const listeners = new Set<(event: InboxChangedEvent) => void>();
  vi.spyOn(bridge, "onInboxChanged").mockImplementation(handler => { listeners.add(handler); return () => { listeners.delete(handler); }; });
  render(<App bridge={bridge} />);

  const result = await screen.findByRole("article", { name: "Result from Inventory check" });
  await user.click(within(result).getByRole("button", { name: "Open" }));
  expect(await screen.findByText(answer.content)).toBeVisible();

  stored.session = false;
  await act(async () => { for (const listener of listeners) listener({ loupistId: "loupist-quality" }); });
  expect(await screen.findByRole("heading", { name: "Bot", level: 1 })).toBeVisible();
  expect(screen.queryByText(answer.content)).toBeNull();
});

it("lists and selects a new topic chat under the Thread, and its title says which chat it is", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<App bridge={bridge} />);
  expect(await screen.findByRole("heading", { name: "Bot", level: 1 })).toBeVisible();
  const chats = document.querySelector<HTMLElement>(".loupist-navigation")!;
  await user.click(screen.getByRole("button", { name: "New research" }));
  expect(await screen.findByRole("heading", { name: "Untitled research session", level: 1 })).toBeVisible();
  const rows = [...chats.querySelectorAll(".loupist-chat-button[data-thread-id]")];
  expect(rows[0]).toHaveAccessibleName("Bot");
  expect(rows[1]).toHaveAccessibleName("Untitled research session");
  expect(within(chats).getByRole("button", { name: "Untitled research session" })).toHaveAttribute("aria-current", "page");
  // An unsent chat has nothing to rename or delete yet.
  expect(within(chats).queryByRole("button", { name: "Options for Untitled research session" })).toBeNull();
});

const handoffResult = (content: string): ChatMessage => ({
  id: "handoff-card", role: "assistant", status: "complete", host_composed: true, created_at: "2026-11-06T17:30:59.000Z", content,
  handoff: { handoffId: "handoff-1", chatId: "chat-handoff", title: "Hynix HBM supply", status: "done" },
});

it("posts nothing in Bot while a hand-off runs and its result once its session finishes, without reloading", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const ask: ChatMessage = { id: "bot-ask", role: "user", status: "complete", created_at: "2026-11-06T17:23:00.000Z", content: "Research who else can supply HBM4." };
  const reply: ChatMessage = { id: "bot-reply", role: "assistant", status: "complete", created_at: "2026-11-06T17:23:10.000Z", content: "I handed this to a research session.", research_session_id: "chat-handoff" };
  const request: ChatMessage = { id: "handoff-request", role: "user", status: "complete", created_at: "2026-11-06T17:23:14.000Z", content: "Who else can supply HBM4 in 2027?" };
  const answer: ChatMessage = { id: "handoff-answer", role: "assistant", status: "complete", created_at: "2026-11-06T17:30:59.000Z", content: "SK hynix keeps the lead through 2027." };
  const stored: { home: ChatMessage[]; session: ChatMessage[] | null; running: boolean; unreadable: boolean } = { home: [], session: null, running: false, unreadable: false };
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    if (id !== "loupist-quality") return workspace;
    if (stored.unreadable) { stored.unreadable = false; throw new Error("database is locked"); }
    const home = workspace.threads.find(thread => thread.home)!;
    home.messages = structuredClone(stored.home);
    const session: ChatThread[] = stored.session ? [{ id: "chat-handoff", loupist_id: id, title: "Hynix HBM supply", updated_at: request.created_at, use_memory: true, update_memory: true, messages: structuredClone(stored.session),
      ...(stored.running ? { handoff_run: { handoff_id: "handoff-1", run_id: "handoff-1a2b3c4d" } } : {}) }] : [];
    return { ...workspace, threads: [home, ...session, ...workspace.threads.filter(thread => !thread.home)] };
  });
  const listeners = new Set<(event: InboxChangedEvent) => void>();
  vi.spyOn(bridge, "onInboxChanged").mockImplementation(handler => { listeners.add(handler); return () => { listeners.delete(handler); }; });
  const hostWrote = () => act(async () => { for (const listener of listeners) listener({ loupistId: "loupist-quality" }); });
  vi.spyOn(bridge, "chatSend").mockImplementation(async (sent, observer) => {
    observer({ type: "started", run_id: sent.run_id });
    stored.home = [ask, reply];
    setTimeout(() => observer({
      type: "completed", ...completedRun, run_id: sent.run_id, message_id: reply.id, final_text: reply.content, created_at: reply.created_at, research_session_id: reply.research_session_id,
      execution_model: { profile_id: "model-test", name: "GPT-6.1 Sol", provider: "openai-codex", model: "gpt-6.1-sol", thinking_level: "medium", run_options: {} },
      agent_harness: { schema: "loupist-agent-harness/1", mode: "chat", skill_ids: [], capability_ids: [], digest: "b".repeat(64) },
    }), 0);
    return { run_id: sent.run_id };
  });
  render(<App bridge={bridge} />);
  expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), ask.content);
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText(reply.content)).toBeVisible();

  // The session appears under the acknowledgement before its separate result post exists.
  stored.home = [ask, reply];
  stored.session = [request];
  stored.running = true;
  await hostWrote();
  const acknowledgement = () => within(screen.getByText(reply.content).closest("article")!);
  const link = await acknowledgement().findByRole("button", { name: /Hynix HBM supply Researching it/ });
  expect(screen.queryByRole("article", { name: "Research: Hynix HBM supply" })).toBeNull();
  await user.click(link);
  expect(await screen.findByText(request.content)).toBeVisible();

  // The run finishes while the store is briefly unreadable; the next refresh still brings it.
  stored.home = [ask, reply, handoffResult(answer.content)];
  stored.session = [request, answer];
  stored.running = false;
  stored.unreadable = true;
  await hostWrote();
  expect(await screen.findByText(answer.content, undefined, { timeout: 5000 })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Bot" }));
  const finished = await screen.findByRole("article", { name: "Research: Hynix HBM supply" });
  expect(acknowledgement().getByRole("button", { name: "Hynix HBM supply" })).toBeVisible();
  expect(within(finished).getByText("Researched: Hynix HBM supply")).toBeVisible();
  expect(within(finished).getByText(answer.content)).toBeVisible();
  expect(within(finished).getByRole("button", { name: "Open" })).toBeVisible();
  expect(screen.getAllByRole("article", { name: "Research: Hynix HBM supply" })).toHaveLength(1);
});

it("answers a question about a followed stock with its sentences and sources and no card, streamed and once stored", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const first = render(<App bridge={bridge} />);
  expect(await screen.findByRole("heading", { name: "Bot" })).toBeVisible();
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), "Where does NVIDIA stand?");
  await user.click(screen.getByRole("button", { name: "Send" }));
  expect(await screen.findByText(/NVIDIA closed at \$162\.85 on Friday/)).toBeVisible();
  expect(await screen.findByRole("group", { name: "Sources and checked figures for this answer" })).toHaveTextContent(/\d+ sources?/u);
  expect(screen.queryByRole("article", { name: "NVIDIA snapshot" })).toBeNull();
  expect(screen.queryByRole("button", { name: "Take to research" })).toBeNull();
  first.unmount();

  // The stored answer, read back from the host, is the same text with the same sources.
  render(<App bridge={bridge} />);
  expect(await screen.findByText(/NVIDIA closed at \$162\.85 on Friday/)).toBeVisible();
  expect(await screen.findByRole("group", { name: "Sources and checked figures for this answer" })).toHaveTextContent(/\d+ sources?/u);
  expect(screen.queryByRole("article", { name: "NVIDIA snapshot" })).toBeNull();
});
