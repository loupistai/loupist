import { chatTitle } from "@/app/chatTitle";
import i18n, { useTranslation } from "@/i18n";
import { useId } from "react";
import { tabShortcut } from "../../app/shortcuts";
import { MoreHorizontalIcon, DownloadIcon, EyeOffIcon, HouseIcon, PencilIcon, PrinterIcon, Trash2Icon } from "lucide-react";
import { Button } from "@/components/ui/button";
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
  running: boolean;
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

export function LoupistChatItem({ iconOnly = false, thread, active, running, finished = false, draft = false, onSelect, onSchedule, onRename, onDelete, onExport }: Props) {
  useTranslation();
  const homeDescId = useId();
  const title = chatTitle(thread);
  const homeDescriptor = thread.home ? i18n.t("app:loupistChatItem.homeDescriptor") : "";
  // A chat that neither reads nor saves Memory is a private chat (composer spec X12).
  const privateChat = thread.use_memory === false && thread.update_memory === false;
  return (
    <SidebarMenuItem className="loupist-chat-row" data-testid="loupist-chat-row" data-home={thread.home || undefined}>
      <SidebarMenuButton
        type="button"
        className="loupist-chat-button" data-testid="loupist-chat-button" data-thread-id={thread.id}
        aria-label={thread.home && !iconOnly ? undefined : title}
        aria-describedby={thread.home && !iconOnly ? homeDescId : undefined}
        title={thread.home ? `${title}. ${homeDescriptor}` : title}
        aria-busy={running || undefined}
        isActive={active}
        data-selected={active || undefined}
        aria-current={active ? "page" : undefined}
        onClick={() => onSelect(thread)}
      >
        {thread.home ? <HouseIcon className="loupist-home-icon" aria-hidden="true" /> : null}
        {iconOnly ? null : thread.home ? <><span className="loupist-chat-title" data-testid="loupist-chat-title">{title}</span><span id={homeDescId} hidden>{homeDescriptor}</span><kbd className="loupist-place-key" aria-hidden="true">{tabShortcut(1)}</kbd></>
          : <span className="loupist-chat-title" data-testid="loupist-chat-title">{title}</span>}
        {draft ? <span className="loupist-draft-dot" role="img" aria-label={i18n.t("app:loupistChatItem.unsentDraft")} title={i18n.t("app:loupistChatItem.unsentDraft")} /> : null}
        {/* A still mark: while a reply runs, its live line's sheen is the one thing that loops. */}
        {running ? <span className="thread-running" role="status" aria-label={i18n.t("app:loupistChatItem.isRunning", { title: chatTitle(thread) })} />
          : finished ? <span className="loupist-status-dot" data-state="finished" role="img" aria-label={i18n.t("app:attention.finishedHidden")} />
            : privateChat ? <span className="loupist-private-mark" role="img" aria-label={i18n.t("app:loupistChatItem.privateChat")} title={i18n.t("app:loupistChatItem.privateChat")}><EyeOffIcon aria-hidden="true" /></span> : null}
      </SidebarMenuButton>
      {/* The Thread is permanent and an unsent chat has nothing to rename or delete yet. */}
      {thread.home && onSchedule ? <DropdownMenu><DropdownMenuTrigger asChild><Button size="icon-sm" variant="ghost" className="loupist-chat-menu" data-testid="loupist-chat-menu" data-bot-menu aria-label={i18n.t("app:loupistChatItem.optionsFor", { title })}><MoreHorizontalIcon /></Button></DropdownMenuTrigger><DropdownMenuContent side="right" align="start"><DropdownMenuItem data-bot-schedule onSelect={onSchedule}>{i18n.t("app:shell.taskHistory")}</DropdownMenuItem></DropdownMenuContent></DropdownMenu> : thread.home || isEmptyChatThreadId(thread.id) ? null : <DropdownMenu>
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
