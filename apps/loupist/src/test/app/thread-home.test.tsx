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
  expect(within(result).getByText(/^From: Inventory check · /)).toBeVisible();
  await vi.waitFor(() => expect(markRead).toHaveBeenCalledWith({ loupistId: "loupist-quality", runIds: ["run-1"] }), { timeout: 2000 });

  await user.click(within(result).getByRole("button", { name: "Open" }));
  expect(await screen.findByRole("heading", { name: "How should we read the margin decline?" })).toBeVisible();
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

const handoffCard = (status: "running" | "done", content: string): ChatMessage => ({
  id: "handoff-card", role: "assistant", status: "complete", host_composed: true, created_at: "2026-11-06T17:23:14.000Z", content,
  handoff: { handoffId: "handoff-1", chatId: "chat-handoff", title: "Hynix HBM supply", status },
});

it("shows a hand-off the host finished in the open Bot and its session without reloading", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const ask: ChatMessage = { id: "bot-ask", role: "user", status: "complete", created_at: "2026-11-06T17:23:00.000Z", content: "Research who else can supply HBM4." };
  const reply: ChatMessage = { id: "bot-reply", role: "assistant", status: "complete", created_at: "2026-11-06T17:23:10.000Z", content: "I handed this to a research session." };
  const request: ChatMessage = { id: "handoff-request", role: "user", status: "complete", created_at: "2026-11-06T17:23:14.000Z", content: "Who else can supply HBM4 in 2027?" };
  const answer: ChatMessage = { id: "handoff-answer", role: "assistant", status: "complete", created_at: "2026-11-06T17:30:59.000Z", content: "SK hynix keeps the lead through 2027." };
  const stored: { home: ChatMessage[]; session: ChatMessage[] | null; unreadable: boolean } = { home: [], session: null, unreadable: false };
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async id => {
    const workspace = await select(id);
    if (id !== "loupist-quality") return workspace;
    if (stored.unreadable) { stored.unreadable = false; throw new Error("database is locked"); }
    const home = workspace.threads.find(thread => thread.home)!;
    home.messages = structuredClone(stored.home);
    const session: ChatThread[] = stored.session ? [{ id: "chat-handoff", loupist_id: id, title: "Hynix HBM supply", updated_at: request.created_at, use_memory: true, update_memory: true, messages: structuredClone(stored.session) }] : [];
    return { ...workspace, threads: [home, ...session, ...workspace.threads.filter(thread => !thread.home)] };
  });
  const listeners = new Set<(event: InboxChangedEvent) => void>();
  vi.spyOn(bridge, "onInboxChanged").mockImplementation(handler => { listeners.add(handler); return () => { listeners.delete(handler); }; });
  const hostWrote = () => act(async () => { for (const listener of listeners) listener({ loupistId: "loupist-quality" }); });
  vi.spyOn(bridge, "chatSend").mockImplementation(async (sent, observer) => {
    observer({ type: "started", run_id: sent.run_id });
    stored.home = [ask, reply];
    setTimeout(() => observer({
      type: "completed", ...completedRun, run_id: sent.run_id, message_id: reply.id, final_text: reply.content, created_at: reply.created_at,
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

  stored.home = [ask, reply, handoffCard("running", "Researching: Hynix HBM supply.")];
  stored.session = [request];
  await hostWrote();
  const card = await screen.findByRole("article", { name: "Research: Hynix HBM supply" });
  expect(within(card).getByText("Running")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Hynix HBM supply" }));
  expect(await screen.findByText(request.content)).toBeVisible();

  // The run finishes while the store is briefly unreadable; the next refresh still brings it.
  stored.home = [ask, reply, handoffCard("done", answer.content)];
  stored.session = [request, answer];
  stored.unreadable = true;
  await hostWrote();
  expect(await screen.findByText(answer.content, undefined, { timeout: 5000 })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Bot" }));
  const finished = await screen.findByRole("article", { name: "Research: Hynix HBM supply" });
  expect(within(finished).getByText("Done")).toBeVisible();
  expect(within(finished).getByText(answer.content)).toBeVisible();
});
