import { Chat } from "@ai-sdk/react";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { TauriChatTransport, fromLoupistUIMessage, toLoupistUIMessage, type LoupistUIMessage } from "../../chat/ai-sdk";
import { useLoupistChat } from "../../chat/useLoupistChat";
import type { ChatMessage } from "../../types";
import { OpenProvider } from "../navigation/Open";
import { ChatConversation } from "./ChatConversation";

afterEach(async () => { await i18n.changeLanguage("en"); });

const at = "2026-11-19T08:00:00Z";
const PROMPT = "Check Samsung Electronics and SK hynix against their 52-week lows and report any that crossed.";
const custom: ChatMessage = {
  id: "request", role: "user", status: "complete", created_at: at, content: PROMPT,
  scheduled: { task_id: "scheduled-task-1", run_id: "scheduled-run-1", task_title: "Daily chip check", task_kind: "custom" },
};
const briefing: ChatMessage = {
  id: "request", role: "user", status: "complete", created_at: at,
  content: "This is a scheduled briefing. Write a factual research briefing from the JSON below.",
  scheduled: { task_id: "briefing-loupist-quality", run_id: "scheduled-run-2", task_title: "Briefings", task_kind: "briefing" },
};
const reply: ChatMessage = { id: "answer", role: "assistant", status: "complete", content: "Neither crossed today.", created_at: "2026-11-19T08:02:00Z" };

/** A run's session, through the same stream adapter and hook the app reads it with. */
function session(first: ChatMessage) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const chat = new Chat<LoupistUIMessage>({ transport: new TauriChatTransport(bridge), messages: [first, reply].map(toLoupistUIMessage) });
  function Conversation() {
    const { messages, messageKeys } = useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: undefined, setThreads: vi.fn() });
    return <ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} messages={messages} messageKeys={messageKeys}
      onSuggestion={vi.fn()} onEditUser={vi.fn()} onReadAttachment={vi.fn()} />;
  }
  return render(<Conversation />);
}

it("keeps which run sent the request through the stream adapter", () => {
  expect(fromLoupistUIMessage(toLoupistUIMessage(custom))).toMatchObject({ scheduled: custom.scheduled });
  expect(fromLoupistUIMessage(toLoupistUIMessage({ ...custom, scheduled: undefined }))).not.toHaveProperty("scheduled");
});

it("names the task that sent the request, with its saved prompt closed under it, and offers no Edit", async () => {
  session(custom);
  const row = document.querySelector<HTMLElement>(".chat-scheduled-turn")!;
  expect(within(row).getByText("Scheduled run: Daily chip check")).toBeVisible();
  // The user did not write it: it is not their bubble, and it cannot be edited as theirs.
  expect(document.querySelector(".message.user")).toBeNull();
  expect(screen.queryByText("You")).toBeNull();
  expect(screen.queryByRole("button", { name: "Edit message" })).toBeNull();
  const request = row.querySelector("details")!;
  expect(request).not.toHaveAttribute("open");
  expect(within(request).getByText(PROMPT)).not.toBeVisible();
  await userEvent.setup().click(within(row).getByText("Request"));
  expect(within(request).getByText(PROMPT)).toBeVisible();
  expect(screen.getByText("Neither crossed today.")).toBeVisible();
});

it("runs a scheduled run that did not finish again through its task, not as the user's turn", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const runNow = vi.spyOn(bridge, "scheduledTaskRunNow").mockResolvedValue({} as Awaited<ReturnType<typeof bridge.scheduledTaskRunNow>>);
  const open = vi.fn(async (_to: unknown, _how?: unknown) => undefined);
  const onRetry = vi.fn();
  for (const status of ["error", "aborted"] as const) {
    const failed: ChatMessage = { id: `answer-${status}`, role: "assistant", status, content: "Response could not be completed.", created_at: "2026-11-19T08:02:00Z", ...(status === "error" ? { placeholder: true } : {}) };
    const chat = new Chat<LoupistUIMessage>({ transport: new TauriChatTransport(bridge), messages: [custom, failed].map(toLoupistUIMessage) });
    function Conversation() {
      const { messages, messageKeys } = useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: undefined, setThreads: vi.fn() });
      return <OpenProvider open={open}><ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} messages={messages} messageKeys={messageKeys}
        onSuggestion={vi.fn()} onEditUser={vi.fn()} onReadAttachment={vi.fn()} onRetry={onRetry} /></OpenProvider>;
    }
    const { unmount } = render(<Conversation />);
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
    await user.click(await screen.findByRole("button", { name: "Run now" }));
    expect(runNow).toHaveBeenLastCalledWith({ loupist_id: "loupist-quality", task_id: "scheduled-task-1" });
    // The task opens in Schedule, where the new run appears.
    await vi.waitFor(() => expect(open.mock.lastCall?.[0]).toEqual({ kind: "schedule", taskId: "scheduled-task-1" }));
    unmount();
  }
  expect(onRetry).not.toHaveBeenCalled();

  // A run that cannot start says so beside the control.
  runNow.mockRejectedValueOnce(new Error("busy"));
  const failed: ChatMessage = { id: "answer-busy", role: "assistant", status: "aborted", content: "Stopped.", created_at: "2026-11-19T08:02:00Z" };
  const chat = new Chat<LoupistUIMessage>({ transport: new TauriChatTransport(bridge), messages: [custom, failed].map(toLoupistUIMessage) });
  function Busy() {
    const { messages, messageKeys } = useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: undefined, setThreads: vi.fn() });
    return <ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} messages={messages} messageKeys={messageKeys}
      onSuggestion={vi.fn()} onEditUser={vi.fn()} onReadAttachment={vi.fn()} onRetry={onRetry} />;
  }
  render(<Busy />);
  await user.click(await screen.findByRole("button", { name: "Run now" }));
  expect(await screen.findByText("Couldn't start this task. Try again.")).toHaveAttribute("role", "alert");
});

it("leaves the briefing's host request out and names the briefing in the reader's language", async () => {
  session(briefing);
  const row = document.querySelector<HTMLElement>(".chat-scheduled-turn")!;
  expect(within(row).getByText("Scheduled run: Briefing")).toBeVisible();
  expect(row.querySelector("details")).toBeNull();
  expect(screen.queryByText(/This is a scheduled briefing/)).toBeNull();
  await act(async () => { await i18n.changeLanguage("ko"); });
  expect(within(row).getByText("예약 실행: 브리핑")).toBeVisible();
});
