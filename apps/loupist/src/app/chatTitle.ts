import i18n from "@/i18n";
import { scheduledTaskTitle } from "../components/briefings/scheduledPresentation";
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

/** A scheduled request opened this session, and the user has since continued it. */
export function scheduledSessionOrigin(thread: ChatThread) {
  const opening = thread.messages[0];
  if (thread.home || !opening?.scheduled || !thread.messages.some(message => message.role === "user" && !message.scheduled)) return null;
  return { turn: opening.scheduled, date: opening.created_at,
    shared: opening.scheduled.task_kind === "briefing" || thread.messages.slice(1).some(message => message.scheduled) };
}
