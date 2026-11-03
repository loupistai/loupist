import i18n from "@/i18n";
import type { ChatThread } from "../types";

/** The home Thread is always "Thread" in the interface language; its stored title is not shown. */
export const chatTitle = (thread: Pick<ChatThread, "title" | "home">): string =>
  thread.home ? i18n.t("app:thread.title") : thread.title.trim() ? thread.title : i18n.t("app:emptyChat.newChat");
