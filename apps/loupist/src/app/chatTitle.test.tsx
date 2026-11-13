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
