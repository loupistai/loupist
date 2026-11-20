import { act, render, screen } from "@testing-library/react";
import i18n from "@/i18n";
import { LoupistChatItem } from "@/components/app/LoupistChatItem";
import { SidebarProvider } from "@/components/ui/sidebar";
import { MockLoupistBridge } from "../bridge/mockBridge";
import { chatTitle } from "./chatTitle";
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
