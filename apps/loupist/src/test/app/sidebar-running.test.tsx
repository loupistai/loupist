import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "../../App";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { ChatMessage, ChatStreamEvent, ChatThread, HomeHandoff, InboxChangedEvent, RunActivity, StreamObserver } from "../../types";
import { scheduledRunFixture } from "../scheduledFixtures";
import { completedRun } from "../streamFixtures";
import { openApp, openChat, rowIsRunning, sidebarRow } from "../renderApp";

type User = ReturnType<typeof userEvent.setup>;
const MARGIN = "How should we read the margin decline?";
const CAPITAL = "Capital allocation checklist";
const HANDOFF = "Hynix HBM supply";
const terminal = { created_at: "2026-11-20T00:00:01.000Z",
  execution_model: { profile_id: "model-test", name: "GPT-5.6 Luna", provider: "openai-codex", model: "gpt-5.6-luna", thinking_level: "medium", run_options: {} },
  agent_harness: { schema: "loupist-agent-harness/1", mode: "chat" as const, skill_ids: [], capability_ids: [], digest: "b".repeat(64) },
};

const send = async (user: User, text: string) => {
  await user.type(screen.getByRole("textbox", { name: "Message Loupist" }), text);
  await user.click(screen.getByRole("button", { name: "Send" }));
};
const running = (title: string, expected = true) => waitFor(() => expect(rowIsRunning(title)).toBe(expected));

/**
 * A host that runs a research hand-off from Bot: while it runs, the session's thread carries the
 * run and Bot has no card; once it ends, Bot gets its card. Each change tells the window
 * (inbox-changed), as Rust does.
 */
function handoffHost(bridge: MockLoupistBridge) {
  const host: { card: Pick<HomeHandoff, "status" | "stopReason"> | null } = { card: null };
  const session: ChatThread = { id: "chat-handoff", loupist_id: "loupist-quality", title: HANDOFF, updated_at: new Date().toISOString(), use_memory: true, update_memory: true,
    messages: [{ id: "handoff-request", role: "user", status: "complete", created_at: new Date().toISOString(), content: "Who else can supply HBM4 in 2027?" }] };
  const select = bridge.loupistSelect.bind(bridge);
  vi.spyOn(bridge, "loupistSelect").mockImplementation(async (id) => {
    const workspace = await select(id);
    if (id !== "loupist-quality" || !host.card) return workspace;
    const home = workspace.threads.find((thread) => thread.home)!;
    const running = host.card.status === "running";
    if (!running) {
      const card: ChatMessage = { id: "handoff-card", role: "assistant", status: "complete", host_composed: true, created_at: new Date().toISOString(), content: "",
        handoff: { handoffId: "handoff-1", chatId: session.id, title: HANDOFF, ...host.card } };
      home.messages = [...home.messages, card];
    }
    const thread = { ...structuredClone(session), ...(running ? { handoff_run: { handoff_id: "handoff-1", run_id: "handoff-1a2b3c4d" } } : {}) };
    return { ...workspace, threads: [home, thread, ...workspace.threads.filter((thread) => !thread.home)] };
  });
  const listeners = new Set<(event: InboxChangedEvent) => void>();
  vi.spyOn(bridge, "onInboxChanged").mockImplementation((handler) => { listeners.add(handler); return () => { listeners.delete(handler); }; });
  return {
    card: async (card: Pick<HomeHandoff, "status" | "stopReason">) => {
      host.card = card;
      await act(async () => { for (const listener of listeners) listener({ loupistId: "loupist-quality" }); });
    },
  };
}

describe("Sidebar · a spinner on chats with a reply running", () => {
  it("spins a research session while a reply sent there runs, and stops when it finishes", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const release = bridge.pauseChatStreams();
    try {
      await openApp(bridge);
      expect(rowIsRunning(MARGIN)).toBe(false);
      await send(user, "Compare the last three quarters");
      // The open session spins too, beside its raised row; another session does not.
      await running(MARGIN);
      expect(rowIsRunning(CAPITAL)).toBe(false);
      release();
      await running(MARGIN, false);
    } finally {
      release();
    }
  });

  it("stops a session's spinner when the user stops the reply", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const release = bridge.pauseChatStreams();
    try {
      await openApp(bridge);
      await send(user, "Stop this one");
      await running(MARGIN);
      await user.click(await screen.findByRole("button", { name: "Stop response" }));
      await running(MARGIN, false);
    } finally {
      release();
    }
  });

  it("stops a session's spinner when the reply fails", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    let fail!: () => void;
    vi.spyOn(bridge, "chatSend").mockImplementation(async (request, observer) => {
      observer({ type: "started", run_id: request.run_id });
      fail = () => observer({ type: "error", failure_kind: "other", run_id: request.run_id, message: "Response could not be completed.", message_id: "assistant-error", final_text: "Response could not be completed.", placeholder: true, ...terminal });
      return { run_id: request.run_id };
    });
    await openApp(bridge);
    await send(user, "This one fails");
    await running(MARGIN);
    act(() => fail());
    expect(await screen.findByRole("alert")).toBeVisible();
    await running(MARGIN, false);
  });

  it.each([
    ["finishes", { status: "done" }],
    ["fails", { status: "failed" }],
    ["is stopped", { status: "stopped" }],
  ] as const)("spins a session Bot handed off, and Bot while another view is open, until the run %s", async (_ending, ending) => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const host = handoffHost(bridge);
    render(<App bridge={bridge} />);
    await openChat(MARGIN);
    await host.card({ status: "running" });
    // The session row spins whether or not this window has joined its run.
    await running(HANDOFF);
    await running("Bot");
    // Bot open: it posts nothing for the session at work, and its own row does not spin; the
    // session's row shows the work.
    await user.click(sidebarRow("Bot"));
    await screen.findByRole("heading", { name: "Bot", level: 1 });
    await running("Bot", false);
    expect(screen.queryByRole("article", { name: `Research: ${HANDOFF}` })).toBeNull();
    expect(rowIsRunning(HANDOFF)).toBe(true);
    await openChat(MARGIN);
    await running("Bot");
    await host.card(ending);
    await running(HANDOFF, false);
    await running("Bot", false);
    // Once it ends, Bot posts it.
    await user.click(sidebarRow("Bot"));
    expect(await screen.findByRole("article", { name: `Research: ${HANDOFF}` })).toHaveAttribute("data-status", ending.status);
  });

  it("keeps a joined hand-off spinning past the host's report until the stream's last event", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const host = handoffHost(bridge);
    let live!: StreamObserver<ChatStreamEvent>;
    vi.spyOn(bridge, "chatWatch").mockImplementation(async (request, observer) => {
      live = observer;
      observer({ type: "started", run_id: "handoff-1a2b3c4d" });
      observer({ type: "delta", run_id: "handoff-1a2b3c4d", text: "SK hynix " });
      const workspace = await bridge.loupistSelect("loupist-quality");
      return { run_id: "handoff-1a2b3c4d", thread: workspace.threads.find((thread) => thread.id === request.thread_id)! };
    });
    vi.spyOn(bridge, "chatUnwatch").mockResolvedValue();
    render(<App bridge={bridge} />);
    await screen.findByRole("heading", { name: "Bot" });
    await host.card({ status: "running" });
    await openChat(HANDOFF);
    await screen.findByText("SK hynix");
    await host.card({ status: "done" });
    expect(rowIsRunning(HANDOFF)).toBe(true);
    act(() => live({ type: "completed", ...completedRun, run_id: "handoff-1a2b3c4d", message_id: "handoff-answer", final_text: "SK hynix keeps the lead.", ...terminal }));
    await running(HANDOFF, false);
  });

  it("spins a listed session while a scheduled run writes into it, and stops when the run ends", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const stored = (await bridge.loupistSelect("loupist-quality")).threads.find((thread) => thread.title === CAPITAL)!;
    const run = scheduledRunFixture({ thread_id: stored.id, run_id: "chat-scheduled-capital" });
    vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue([run]);
    let live!: StreamObserver<ChatStreamEvent>;
    vi.spyOn(bridge, "scheduledTaskWatch").mockImplementation(async (_request, observer) => {
      live = observer;
      observer({ type: "started", run_id: run.run_id });
      observer({ type: "delta", run_id: run.run_id, text: "Reviewing sources" });
      return { run, thread: stored };
    });
    render(<App bridge={bridge} />);
    await screen.findByRole("heading", { name: "Bot" });
    await running(CAPITAL);
    act(() => live({ type: "completed", ...completedRun, run_id: run.run_id, message_id: "scheduled-answer", final_text: "The sources agree.", ...terminal }));
    await running(CAPITAL, false);
  });

  it("spins a session whose run the host still has after the window reloads, until it settles", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const host = { live: true };
    const activity: RunActivity = { run_id: "chat-reload-capital", loupist_id: "loupist-quality", kind: "chat", target: "thread-capital", started_at_ms: 1 };
    vi.spyOn(bridge, "runActivityList").mockImplementation(async () => host.live ? [activity] : []);
    render(<App bridge={bridge} />);
    await screen.findByRole("heading", { name: "Bot" });
    await running(CAPITAL);
    host.live = false;
    await running(CAPITAL, false);
  });

  it("spins nothing after an app restart: the host marked the cut-short hand-off interrupted", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const host = handoffHost(bridge);
    await host.card({ status: "stopped", stopReason: "interrupted" });
    render(<App bridge={bridge} />);
    await screen.findByRole("article", { name: `Research: ${HANDOFF}` });
    await openChat(HANDOFF);
    expect(rowIsRunning(HANDOFF)).toBe(false);
    expect(rowIsRunning("Bot")).toBe(false);
  });
});

describe("Sidebar · Bot's row", () => {
  it("spins for Bot's own reply only while another view is open, then leaves the finished dot", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const release = bridge.pauseChatStreams();
    try {
      render(<App bridge={bridge} />);
      await screen.findByRole("heading", { name: "Bot" });
      await send(user, "How did Hanul close?");
      await screen.findByRole("button", { name: "Stop response" });
      expect(rowIsRunning("Bot")).toBe(false);
      await openChat(MARGIN);
      await running("Bot");
      release();
      await running("Bot", false);
      // The reply finished out of sight: the unread dot takes the slot back.
      await waitFor(() => expect(sidebarRow("Bot")).toHaveAccessibleDescription("Chat and scheduled task results Finished while you were away"));
    } finally {
      release();
    }
  });

  it("shows the spinner over Bot's finished dot while a hand-off runs, and the dot again after", async () => {
    const user = userEvent.setup();
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const host = handoffHost(bridge);
    const release = bridge.pauseChatStreams();
    try {
      render(<App bridge={bridge} />);
      await screen.findByRole("heading", { name: "Bot" });
      await send(user, "Research who else can supply HBM4");
      await screen.findByRole("button", { name: "Stop response" });
      await openChat(MARGIN);
      release();
      await waitFor(() => expect(sidebarRow("Bot")).toHaveAccessibleDescription(/Finished while you were away$/));
      await host.card({ status: "running" });
      await running("Bot");
      expect(sidebarRow("Bot")).toHaveAccessibleDescription("Chat and scheduled task results Running");
      await host.card({ status: "done" });
      await running("Bot", false);
      expect(sidebarRow("Bot")).toHaveAccessibleDescription("Chat and scheduled task results Finished while you were away");
    } finally {
      release();
    }
  });
});

describe("Sidebar · Schedule's row", () => {
  it("names one running task in its tooltip and accessible name", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue([scheduledRunFixture()]);
    await openApp(bridge);
    const row = await screen.findByRole("button", { name: "Schedule · running Daily source review" });
    expect(row).toHaveAttribute("title", "Schedule · running Daily source review");
    expect(row.querySelector(".thread-running")).toBeInTheDocument();
  });

  it("takes the unread badge's place and restores the updated badge when the run finishes", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const run = scheduledRunFixture();
    const activity = vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue([]);
    const unread = vi.spyOn(bridge, "inboxUnreadCounts").mockResolvedValue([{ loupistId: run.loupist_id, count: 2 }]);
    const listeners = new Set<(event: InboxChangedEvent) => void>();
    vi.spyOn(bridge, "onInboxChanged").mockImplementation(handler => { listeners.add(handler); return () => { listeners.delete(handler); }; });
    await openApp(bridge);
    expect(sidebarRow("Schedule")).toHaveAccessibleDescription("2 unread scheduled results");

    activity.mockResolvedValue([run]);
    act(() => window.dispatchEvent(new Event("focus")));
    const row = await screen.findByRole("button", { name: "Schedule · running Daily source review" }, { timeout: 3000 });
    expect(row.querySelector(".thread-running")).toBeInTheDocument();
    expect(row.querySelector(".shell-unread")).toBeNull();
    expect(row).not.toHaveAttribute("aria-describedby");

    activity.mockResolvedValue([{ ...run, status: "succeeded" }]);
    unread.mockResolvedValue([{ loupistId: run.loupist_id, count: 3 }]);
    await act(async () => {
      for (const listener of listeners) listener({ loupistId: run.loupist_id });
      window.dispatchEvent(new Event("focus"));
    });
    const finished = await screen.findByRole("button", { name: "Schedule", description: "3 unread scheduled results" }, { timeout: 3000 });
    expect(finished.querySelector(".thread-running")).toBeNull();
    expect(finished.querySelector(".shell-unread")).toHaveTextContent("3");
  });

  it("shows how many runs are in progress when two tasks run", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue([
      scheduledRunFixture(),
      scheduledRunFixture({ id: "scheduled-run-two", task_id: "schedule-two", run_id: "chat-scheduled-two", task_title: "Weekly review" }),
    ]);
    await openApp(bridge);
    const row = await screen.findByRole("button", { name: "Schedule · 2 runs in progress" });
    expect(row).toHaveAttribute("title", "Schedule · 2 runs in progress");
    expect(row.querySelectorAll(".thread-running")).toHaveLength(1);
  });

  it("shows no spinner when the selected Loupist has no running task", async () => {
    const bridge = new MockLoupistBridge({ delay_ms: 0 });
    const activity = vi.spyOn(bridge, "scheduledTasksActivity").mockResolvedValue([scheduledRunFixture({ status: "failed" })]);
    await openApp(bridge);
    expect(activity).toHaveBeenCalledWith({ loupist_id: "loupist-quality" });
    expect(sidebarRow("Schedule").querySelector(".thread-running")).toBeNull();
  });
});
