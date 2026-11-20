import { Chat } from "@ai-sdk/react";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import i18n from "../../i18n";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { TauriChatTransport, fromLoupistUIMessage, toLoupistUIMessage, type LoupistUIMessage } from "../../chat/ai-sdk";
import { useLoupistChat } from "../../chat/useLoupistChat";
import type { ChatMessage } from "../../types";
import { ChatConversation } from "./ChatConversation";

afterEach(async () => { await i18n.changeLanguage("en"); });

// A research request past 2,000 characters whose last sentence limits the answer.
const LIMIT = "Answer in one table and skip any forecasts.";
const REQUEST = `${"Compare the sealed suppliers' capacity, pricing and customer mix across the last three quarters. ".repeat(24)}${LIMIT}`;
const NOTE = { title: "Sealed supply", question: "Who else makes HBM4?", brief: "Check filings <img src=x onerror=alert(1)> first." };
const at = "2026-11-18T09:00:00Z";
const linkFirst: ChatMessage = {
  id: "first", role: "user", status: "complete", created_at: at,
  content: "The user chose “Look into it” on this Map link.",
  handoff_action: { kind: "link", connectionId: "con_1" },
  handoff_note: { title: "Link: Alpha and Gamma", linkPath: {
    seed: "Alpha", far: "Gamma", path: [
      { from: "Alpha", relation: "SUPPLIES", to: "Beta" },
      { from: "Gamma", relation: "USES", to: "Beta" },
    ],
  } },
};
const reply: ChatMessage = { id: "answer", role: "assistant", status: "complete", content: "Saved answer.", created_at: "2026-11-18T09:01:00Z" };

/** A research session opened from a hand-off, through the same stream adapter and hook the app reads it with. */
function session(first: ChatMessage, onEditUser = vi.fn()) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const chat = new Chat<LoupistUIMessage>({ transport: new TauriChatTransport(bridge), messages: [first, reply].map(toLoupistUIMessage) });
  function Conversation() {
    const { messages, messageKeys } = useLoupistChat({ bridge, chat, loupistId: "loupist-quality", activeThread: undefined, setThreads: vi.fn() });
    return <ChatConversation loupistId="loupist-quality" loupistName="Loupist" bridge={bridge} messages={messages} messageKeys={messageKeys}
      onSuggestion={vi.fn()} onEditUser={onEditUser} onReadAttachment={vi.fn()} />;
  }
  return { onEditUser, ...render(<Conversation />) };
}
const firstMessage = () => document.querySelector<HTMLElement>(".message.user")!;

it("keeps the stored note and chosen action through the stream adapter", () => {
  expect(fromLoupistUIMessage(toLoupistUIMessage(linkFirst))).toMatchObject({
    handoff_note: linkFirst.handoff_note, handoff_action: linkFirst.handoff_action,
  });
  expect(fromLoupistUIMessage(toLoupistUIMessage({ ...linkFirst, handoff_note: undefined, handoff_action: undefined }))).not.toHaveProperty("handoff_note");
});

it("shows the user's whole request, with the Bot's note closed under it as data", async () => {
  const { onEditUser } = session({ id: "first", role: "user", status: "complete", content: REQUEST, created_at: at, handoff_note: NOTE });
  const first = firstMessage();
  expect(within(first).getByText(REQUEST)).toBeVisible();
  expect(first.querySelector(".message-user-text")?.textContent?.endsWith(LIMIT)).toBe(true);
  // One user message: the note never reads as a second question from the user.
  expect(document.querySelectorAll(".message.user")).toHaveLength(1);
  const note = first.querySelector("details")!;
  expect(note).not.toHaveAttribute("open");
  const summary = within(first).getByText("From Bot");
  expect(within(note).getByText("Sealed supply")).not.toBeVisible();

  // A native summary: it takes focus, and Enter and Space activate it in the WebView
  // (jsdom has no key activation for summary; the native persistence journey presses Enter).
  const user = userEvent.setup();
  const trigger = summary.closest("summary")!;
  trigger.focus();
  expect(trigger).toHaveFocus();
  await user.click(trigger);
  expect(note).toHaveAttribute("open");
  expect(within(note).getByText("Sealed supply")).toBeVisible();
  expect(within(note).getByText("Who else makes HBM4?")).toBeVisible();
  expect(within(note).getByText("Check filings <img src=x onerror=alert(1)> first.")).toBeVisible();
  expect(note.querySelector("img")).toBeNull();
  await user.click(trigger);
  expect(note).not.toHaveAttribute("open");

  // The user wrote this request: it stays editable like any other.
  await user.click(within(first.parentElement!).getByRole("button", { name: "Edit message" }));
  expect(onEditUser).toHaveBeenCalledWith(expect.objectContaining({ content: REQUEST }), 0);
});

it("says the user chose a link action instead of showing the host's caption, and names the link context as the link's", async () => {
  session(linkFirst);
  const first = firstMessage();
  expect(within(first).getByText("You chose “Look into it” on a link in Bot.")).toBeVisible();
  expect(screen.queryByText(linkFirst.content)).toBeNull();
  expect(screen.queryByText("From Bot")).toBeNull();
  expect(screen.queryByRole("button", { name: "Edit message" })).toBeNull();
  const trigger = within(first).getByText("Link details");
  expect(trigger.closest("details")).not.toHaveAttribute("open");
  await userEvent.setup().click(trigger);
  expect(within(first).getByText("Start: Alpha")).toBeVisible();
  expect(within(first).getByText("Alpha → supplies → Beta")).toBeVisible();
  expect(within(first).getByText("Gamma → uses → Beta")).toBeVisible();
  expect(within(first).getByText("End: Gamma")).toBeVisible();

  await act(async () => { await i18n.changeLanguage("ko"); });
  expect(within(first).getByText("봇의 연결에서 ‘살펴보기’를 선택했습니다.")).toBeVisible();
  expect(within(first).getByText("연결 정보")).toBeVisible();
  expect(within(first).getByText("시작: Alpha")).toBeVisible();
  expect(within(first).getByText("Alpha → 공급 → Beta")).toBeVisible();
  expect(within(first).getByText("Gamma → 사용 → Beta")).toBeVisible();
  expect(within(first).getByText("끝: Gamma")).toBeVisible();
});

it("reads a session from before notes as it always did", async () => {
  session({ id: "first", role: "user", status: "complete", content: "Who else makes HBM4?\n\nCheck filings.", created_at: at });
  expect(firstMessage().querySelector("details")).toBeNull();
  expect(screen.queryByText("From Bot")).toBeNull();
  await act(async () => { await i18n.changeLanguage("ko"); });
  session({ id: "first-ko", role: "user", status: "complete", content: REQUEST, created_at: at, handoff_note: NOTE });
  expect(screen.getByText("봇이 전달한 내용")).toBeVisible();
});
