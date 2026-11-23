import { chatTitle, scheduledSessionOrigin } from "@/app/chatTitle";
import i18n, { useTranslation } from "@/i18n";
import { useId, useRef } from "react";
import { tabShortcut } from "../../app/shortcuts";
import { CalendarClockIcon, MoreHorizontalIcon, DownloadIcon, EyeOffIcon, MessageCircleIcon, PencilIcon, PrinterIcon, Trash2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { SidebarMenuButton, SidebarMenuItem } from "@/components/ui/sidebar";
import {
  DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ChatThread } from "../../types";
import { isEmptyChatThreadId } from "../../app/emptyChat";

type Props = {
  iconOnly?: boolean;
  thread: ChatThread;
  active: boolean;
  /** A reply is running in this chat; the row's status slot shows the spinner. */
  running: boolean;
  saving?: boolean;
  /** The run ended while the user was looking elsewhere. */
  finished?: boolean;
  /** The composer holds unsent text for this chat. */
  draft?: boolean;
  onSelect: (thread: ChatThread) => void;
  onSchedule?: () => void;
  onRename: (thread: ChatThread) => void;
  onDelete: (thread: ChatThread) => void;
  /** Saves the chat as Markdown through the native dialog, or opens the print dialog (Save as PDF). */
  onExport?: (thread: ChatThread, format: "md" | "pdf") => void;
};

export function LoupistChatItem({ iconOnly = false, thread, active, running, saving, finished = false, draft = false, onSelect, onSchedule, onRename, onDelete, onExport }: Props) {
  useTranslation();
  const homeDescId = useId();
  const originId = useId();
  const scheduled = Boolean(scheduledSessionOrigin(thread));
  const draftId = useId();
  const statusId = useId();
  const title = chatTitle(thread);
  const homeDescriptor = thread.home ? i18n.t("app:loupistChatItem.homeDescriptor") : "";
  // A chat that neither reads nor saves Memory is a private chat (composer spec X12).
  const privateChat = thread.use_memory === false && thread.update_memory === false;
  // Scheduled tasks opens another page, so focus moves to that page's heading instead of
  // back to "…": no ring stays on a sidebar control the user has left. Escape still returns to "…".
  const openedSchedule = useRef(false);
  // One status at the row's end, first match: running, finished out of sight, private. A
  // button's own marks are not read inside it, so hidden words describe the row with them.
  const status = running ? "running" : finished ? "finished" : privateChat ? "private" : null;
  const statusLabel = status === "running" ? saving ? i18n.t("chat:memorySaving.label") : i18n.t("app:loupistChatItem.running")
    : status === "finished" ? i18n.t("app:attention.finishedHidden")
      : status === "private" ? i18n.t("app:loupistChatItem.privateChat") : "";
  const draftLabel = i18n.t("app:loupistChatItem.unsentDraft");
  const describedBy = [scheduled ? originId : null, thread.home && !iconOnly ? homeDescId : null, draft ? draftId : null, statusLabel ? statusId : null].filter(Boolean).join(" ") || undefined;
  return (
    <SidebarMenuItem className="loupist-chat-row" data-testid="loupist-chat-row" data-home={thread.home || undefined}>
      <SidebarMenuButton
        type="button"
        className="loupist-chat-button" data-testid="loupist-chat-button" data-thread-id={thread.id}
        aria-label={title}
        aria-describedby={describedBy}
        title={saving && running ? statusLabel : thread.home ? `${title}. ${homeDescriptor}` : title}
        isActive={active}
        data-selected={active || undefined}
        aria-current={active ? "page" : undefined}
        onClick={() => onSelect(thread)}
      >
        {scheduled ? <><CalendarClockIcon aria-hidden="true" /><span hidden id={originId}>{i18n.t("app:loupistChatItem.startedBySchedule")}</span></> : thread.home ? <MessageCircleIcon className="loupist-home-icon" aria-hidden="true" /> : null}
        {iconOnly ? null : thread.home ? <><span className="loupist-chat-title" data-testid="loupist-chat-title">{title}</span><span id={homeDescId} hidden>{homeDescriptor}</span><kbd className="loupist-place-key" aria-hidden="true">{tabShortcut(1)}</kbd></>
          : <span className="loupist-chat-title" data-testid="loupist-chat-title">{title}</span>}
        {draft ? <><span className="loupist-draft-dot" role="img" aria-label={draftLabel} title={draftLabel} /><span id={draftId} hidden>{draftLabel}</span></> : null}
        {/* The app's spinner, with a fixed name and no live region, so its turning is never announced.
            Reduced motion stops it (tokens.css), leaving its open ring still. */}
        {status === "running" ? <Spinner className="thread-running" role="img" aria-label={statusLabel} />
          : status === "finished" ? <span className="loupist-status-dot" data-state="finished" role="img" aria-label={statusLabel} />
            : status === "private" ? <span className="loupist-private-mark" role="img" aria-label={statusLabel} title={statusLabel}><EyeOffIcon aria-hidden="true" /></span> : null}
        {statusLabel ? <span id={statusId} hidden>{statusLabel}</span> : null}
      </SidebarMenuButton>
      {/* The Thread is permanent and an unsent chat has nothing to rename or delete yet.
          The rail is the icon alone, so the options stay on the expanded row. */}
      {iconOnly ? null : thread.home && onSchedule ? <DropdownMenu><DropdownMenuTrigger asChild><Button size="icon-sm" variant="ghost" className="loupist-chat-menu" data-testid="loupist-chat-menu" data-bot-menu aria-label={i18n.t("app:loupistChatItem.optionsFor", { title })}><MoreHorizontalIcon /></Button></DropdownMenuTrigger><DropdownMenuContent side="right" align="start" onCloseAutoFocus={event => { if (!openedSchedule.current) return; openedSchedule.current = false; event.preventDefault(); document.getElementById("current-location")?.focus({ preventScroll: true }); }}><DropdownMenuItem data-bot-schedule onSelect={() => { openedSchedule.current = true; onSchedule(); }}>{i18n.t("app:shell.taskHistory")}</DropdownMenuItem></DropdownMenuContent></DropdownMenu> : thread.home || isEmptyChatThreadId(thread.id) ? null : <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button size="icon-sm" variant="ghost" className="loupist-chat-menu" data-testid="loupist-chat-menu" aria-label={i18n.t("app:loupistChatItem.optionsFor", { title: chatTitle(thread) })}>
            <MoreHorizontalIcon />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent className="w-56" side="right" align="start">
          <DropdownMenuItem onSelect={() => onRename(thread)}><PencilIcon />{i18n.t("app:loupistChatItem.rename")}</DropdownMenuItem>
          {onExport ? <DropdownMenuItem onSelect={() => onExport(thread, "md")}><DownloadIcon />{i18n.t("app:loupistChatItem.export")}</DropdownMenuItem> : null}
          {onExport ? <DropdownMenuItem data-chat-print onSelect={() => onExport(thread, "pdf")}><PrinterIcon />{i18n.t("app:loupistChatItem.print")}</DropdownMenuItem> : null}
          <DropdownMenuItem variant="destructive" onSelect={() => onDelete(thread)}><Trash2Icon />{i18n.t("app:loupistChatItem.delete")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>}
    </SidebarMenuItem>
  );
}
