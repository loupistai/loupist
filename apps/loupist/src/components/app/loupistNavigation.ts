import { emptyChatThread, isEmptyChatThreadId } from "../../app/emptyChat";
import type { ChatThread } from "../../types";

const RECENT_CHAT_COUNT = 5;

/**
 * The home Thread leads; topic chats follow, newest first. A session the host keeps out of
 * the list (a scheduled run's, a price setup) is still listed while it is the one open.
 */
export function getLoupistChatList(threads: ChatThread[] = [], activeThreadId: string | null = null) {
  const saved = threads.filter((thread) => (!thread.hide_from_recent || thread.id === activeThreadId) && !isEmptyChatThreadId(thread.id));
  const chats = [
    ...saved.filter((thread) => thread.home),
    ...saved.filter((thread) => !thread.home).sort((a, b) => b.updated_at.localeCompare(a.updated_at)),
  ];
  return chats;
}

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
