import { chatTitle } from "@/app/chatTitle";
import { emptyChatThread, isEmptyChatThreadId } from "../../app/emptyChat";
import type { ChatThread, LoupistSummary } from "../../types";

const RECENT_CHAT_COUNT = 5;

/** The group and the empty-search message must use the same saved-chat list. */
export function getLoupistChatList(loupist: LoupistSummary, threads: ChatThread[] = [], query: string) {
  // The home Thread leads; topic chats follow, newest first.
  const saved = threads.filter((thread) => !isEmptyChatThreadId(thread.id));
  const chats = [
    ...saved.filter((thread) => thread.home),
    ...saved.filter((thread) => !thread.home).sort((a, b) => b.updated_at.localeCompare(a.updated_at)),
  ];
  const matchesLoupist = `${loupist.name} ${loupist.workspace_path}`.toLowerCase().includes(query);
  const matches = matchesLoupist
    ? chats
    : chats.filter((thread) => chatTitle(thread).toLowerCase().includes(query));

  return { chats, matches, matchesSearch: matchesLoupist || matches.length > 0 };
}

export type LoupistChatList = ReturnType<typeof getLoupistChatList>;

export function recentLoupistChats(chats: ChatThread[], activeThreadId: string | null, loupistId = chats[0]?.loupist_id ?? "") {
  // An unsent topic chat is listed (and selected) right after the Thread, so the sidebar always shows where the user is.
  if (isEmptyChatThreadId(activeThreadId)) {
    const home = chats[0]?.home ? [chats[0]] : [];
    chats = [...home, emptyChatThread(loupistId), ...chats.slice(home.length)];
  }
  // The home Thread is always first and never pushed out of the preview.
  const recent = chats.slice(0, RECENT_CHAT_COUNT + (chats[0]?.home ? 1 : 0) + (isEmptyChatThreadId(activeThreadId) ? 1 : 0));
  const activeIndex = chats.findIndex((thread) => thread.id === activeThreadId);
  // Keep an older active conversation visible in the collapsed preview.
  if (activeIndex >= recent.length) recent[recent.length - 1] = chats[activeIndex];
  return recent;
}
