import i18n from "@/i18n";
import { scheduledTaskTitle } from "../components/briefings/scheduledPresentation";
import { requestTitle } from "../lib/requestTitle";
import type { ChatThread } from "../types";

/**
 * The home Thread is always "Thread" in the interface language; its stored title is not shown.
 * The briefing's session carries the host's English task title, so it reads as the briefing does.
 */
export const chatTitle = (thread: Pick<ChatThread, "title" | "home"> & Partial<Pick<ChatThread, "messages">>): string => {
  if (thread.home) return i18n.t("app:thread.title");
  if (thread.messages?.find((message) => message.scheduled)?.scheduled?.task_kind === "briefing") return scheduledTaskTitle("briefing", thread.title);
  return thread.title.trim() ? thread.title : i18n.t("app:emptyChat.newChat");
};

/**
 * A session another reply's save waits on. While its first reply runs it has no title yet, so it
 * is named from its request, as the host titles it if that reply stops or fails: the last request
 * that is not a steer, or its first file's name when it has no text.
 */
export const waitedOnChatTitle = (thread: Pick<ChatThread, "title" | "home"> & Partial<Pick<ChatThread, "messages">>): string => {
  if (thread.home || thread.title.trim()) return chatTitle(thread);
  const request = [...(thread.messages ?? [])].reverse().find((message) => message.role === "user" && !message.steer);
  const file = request?.attachments?.[0];
  const title = request ? requestTitle(file && !request.content ? file.filename : request.content) : "";
  return title || chatTitle(thread);
};

/** A scheduled request opened this session, and the user has since continued it. */
export function scheduledSessionOrigin(thread: ChatThread) {
  const opening = thread.messages[0];
  if (thread.home || !opening?.scheduled || !thread.messages.some(message => message.role === "user" && !message.scheduled)) return null;
  return { turn: opening.scheduled, date: opening.created_at,
    shared: opening.scheduled.task_kind === "briefing" || thread.messages.slice(1).some(message => message.scheduled) };
}
