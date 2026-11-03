import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import { EyeOffIcon } from "lucide-react";
import type { ChatThread } from "../../types";
import { isEmptyChatThreadId } from "../../app/emptyChat";

const t = i18n.getFixedT(null, "chat");
const MAX_TITLE_CHARS = 120;

/**
 * The chat title under the Loupist header (composer spec X15, X12). Click or
 * Enter turns it into a field in the same place; Enter or leaving the field
 * saves through the ordinary rename, Esc cancels. The home Thread and an
 * unsent chat have no title to change. A private chat shows a crossed eye.
 */
export function ChatTitle({ thread, title, privateChat, onRename }: {
  thread?: ChatThread;
  title: string;
  privateChat: boolean;
  onRename?: (thread: ChatThread, title: string) => Promise<boolean>;
}) {
  useTranslation("chat");
  const [draft, setDraft] = useState<string | null>(null);
  const editable = Boolean(onRename && thread && !thread.home && !isEmptyChatThreadId(thread.id));
  useEffect(() => { setDraft(null); }, [thread?.id]);

  const save = async () => {
    if (draft === null || !thread || !onRename) return;
    const next = draft.trim();
    setDraft(null);
    if (next && next !== title) await onRename(thread, next);
  };

  // The heading names the chat; the rename button and private mark keep their own names.
  return <h1 className="chat-title" aria-label={title}>
    {/* The field replaces the title in place and starts with it selected. */}
    {draft !== null ? <input autoFocus onFocus={(event) => {
      // Select everything but keep the start of the title in view (select() scrolls to the end).
      const field = event.currentTarget;
      field.setSelectionRange(0, field.value.length, "backward");
      field.scrollLeft = 0;
    }} className="chat-title-input" value={draft} maxLength={MAX_TITLE_CHARS} aria-label={t("chatTitle.field")}
      onChange={(event) => setDraft(event.target.value)}
      onBlur={() => void save()}
      onKeyDown={(event) => {
        if (event.nativeEvent.isComposing) return;
        if (event.key === "Enter") { event.preventDefault(); void save(); }
        if (event.key === "Escape") { event.preventDefault(); setDraft(null); }
      }} />
      : editable ? <button type="button" className="chat-title-button" aria-label={t("chatTitle.rename", { title })} title={t("chatTitle.rename", { title })} onClick={() => setDraft(title)}>{title}</button>
        : <span>{title}</span>}
    {privateChat ? <span className="chat-title-private" role="img" aria-label={t("chatTitle.private")} title={t("chatTitle.private")}><EyeOffIcon aria-hidden="true" /></span> : null}
  </h1>;
}
