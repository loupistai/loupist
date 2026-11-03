import type { ChatThread } from "../types";

export const EMPTY_CHAT_THREAD_ID = "__empty-chat__";

export const isEmptyChatThreadId = (threadId: string | null | undefined) =>
  threadId === EMPTY_CHAT_THREAD_ID;

export const emptyChatThread = (loupistId: string): ChatThread => ({
  id: EMPTY_CHAT_THREAD_ID,
  loupist_id: loupistId,
  title: "",
  updated_at: "1970-01-01T00:00:00.000Z",
  use_memory: true,
  update_memory: true,
  messages: [],
});
