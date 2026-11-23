import { act, render, screen } from "@testing-library/react";
import i18n from "@/i18n";
import { LoupistChatItem } from "@/components/app/LoupistChatItem";
import { SidebarProvider } from "@/components/ui/sidebar";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { chatTitle, waitedOnChatTitle } from "./chatTitle";
import { requestTitle } from "../lib/requestTitle";
import type { ChatMessage } from "../types";
import { emptyChatThread } from "./emptyChat";
import { useAppDialogs } from "./AppDialogs";
import { renderHook } from "@testing-library/react";

afterEach(async () => { await i18n.changeLanguage("en"); });

it("localizes untitled saved chats when the language changes", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const thread = await bridge.chatCreate({ loupist_id: "loupist-quality" });
  expect(thread.title).toBe("");
  render(<SidebarProvider><LoupistChatItem thread={thread} active running={false} onSelect={vi.fn()} onRename={vi.fn()} onDelete={vi.fn()} /></SidebarProvider>);
  expect(screen.getByRole("button", { name: i18n.t("app:emptyChat.newChat") })).toBeVisible();
  await act(async () => { await i18n.changeLanguage("ko"); });
  expect(screen.getByRole("button", { name: "제목 없는 리서치 세션" })).toBeVisible();
  expect(thread.title).toBe("");
});

it("uses the localized label for blank titles and preserves user titles", async () => {
  await i18n.changeLanguage("ko");
  expect(emptyChatThread("loupist").title).toBe("");
  expect(chatTitle({ title: " \t\n" })).toBe("제목 없는 리서치 세션");
  expect(chatTitle({ title: "  My research  " })).toBe("  My research  ");
  const { result } = renderHook(() => useAppDialogs());
  act(() => result.current.openRenameThread(emptyChatThread("loupist")));
  expect(result.current.threadNameDraft).toBe("제목 없는 리서치 세션");
});

it("names the briefing's session as the briefing reads in the interface language", async () => {
  const request = { id: "request", role: "user" as const, content: "This is a scheduled briefing.", created_at: "2026-11-19T08:00:00Z",
    scheduled: { task_id: "briefing-loupist", run_id: "scheduled-run", task_title: "Briefings", task_kind: "briefing" } };
  const briefing = { title: "Briefings", home: false, messages: [request] };
  expect(chatTitle(briefing)).toBe("Briefing");
  await i18n.changeLanguage("ko");
  expect(chatTitle(briefing)).toBe("브리핑");
  // A name the user gave it is kept, and a custom task's session keeps its task's title.
  expect(chatTitle({ ...briefing, title: "Morning notes" })).toBe("Morning notes");
  const custom = { ...request, scheduled: { ...request.scheduled, task_title: "Daily chip check", task_kind: "custom" } };
  expect(chatTitle({ title: "Daily chip check", home: false, messages: [custom] })).toBe("Daily chip check");
});

it("names a session whose first reply still runs from its request, as the host titles it", async () => {
  // Requests and titles the host stored in the 0.1.5 acceptance run.
  const request = (content: string, extra: Partial<ChatMessage> = {}): ChatMessage => ({ id: `ask-${content.length}`, role: "user", content, created_at: "2026-11-23T04:32:09Z", ...extra });
  const running = (...messages: ChatMessage[]) => ({ title: "", home: false, messages });
  expect(waitedOnChatTitle(running(request("Compare Coca-Cola and PepsiCo using each company's latest annual report on Form 10-K: revenue growth over the last three fiscal years, operating margin, and the three largest risk factors each one names. Read both filings before answering and finish with a comparison table."))))
    .toBe("Compare Coca-Cola and PepsiCo using…");
  expect(waitedOnChatTitle(running(request("Write a 3000-word primer on how to read a cash flow statement, section by section, with worked examples, without using any tools."))))
    .toBe("Write a 3000-word primer on how to read…");
  // The first line only, its spacing collapsed; a steer is not the request.
  expect(waitedOnChatTitle(running(request("  Review   peer filings\nthen compare margins"), request("Also check Pepsi", { steer: true })))).toBe("Review peer filings");
  // A request that is only a file takes the file's name.
  expect(waitedOnChatTitle(running(request("", { attachments: [{ id: "file", filename: "annual-report.pdf", media_type: "application/pdf", size_bytes: 4 }] })))).toBe("annual-report.pdf");
  // A cut never splits a grapheme.
  expect(requestTitle(`${"a".repeat(38)}👩🏽‍💻bc`)).toBe(`${"a".repeat(38)}…`);
  expect(requestTitle("x".repeat(40))).toBe("x".repeat(40));
  // A title, the Bot and a session with no request keep their usual names.
  expect(waitedOnChatTitle({ title: "Peer margins", home: false, messages: [request("Compare margins")] })).toBe("Peer margins");
  expect(waitedOnChatTitle({ title: "", home: true, messages: [request("Compare margins")] })).toBe(chatTitle({ title: "", home: true }));
  expect(waitedOnChatTitle(running())).toBe("Untitled research session");
  await i18n.changeLanguage("ko");
  expect(waitedOnChatTitle(running(request("   ")))).toBe("제목 없는 리서치 세션");
});
