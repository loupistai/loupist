import i18n, { useTranslation } from "@/i18n";
import { subjectName } from "@/i18n/format";
import { useEffect, useState } from "react";
import { BookOpenIcon, ChevronRightIcon, FolderIcon, FolderOpenIcon, MoreHorizontalIcon, SquarePenIcon, Trash2Icon, UserRoundCogIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { SidebarMenu, useSidebar } from "@/components/ui/sidebar";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { chatSessionKey } from "../../chat/sessionRegistry";
import type { ChatThread, LoupistSummary } from "../../types";
import { LoupistAvatar } from "./AppHeader";
import { LoupistChatItem } from "./LoupistChatItem";
import { recentLoupistChats, type LoupistChatList } from "./loupistNavigation";

type Props = {
  unreadCount?: number;
  /** A refused tool still needs the user's approval in one of this Loupist's chats. */
  waiting?: boolean;
  /** A run ended while the user was not looking. */
  finishedHidden?: boolean;
  finishedThreadKeys?: ReadonlySet<string>;
  loupist: LoupistSummary; parentPath?: string; selected: boolean; loading: boolean; chatList: LoupistChatList; loaded: boolean;
  activeThreadId: string | null; running: boolean; runningThreadKeys: ReadonlySet<string>; draftThreadKeys?: ReadonlySet<string>; loadThreads: (loupistId: string) => Promise<void>;
  allChatsOpen: boolean;
  onSelect: () => void; onManage: () => void; onReveal: () => void; onRemove: () => void; onAllChats: () => void; onCreateThread: () => void;
  /** Opens "What {name} knows" for the selected Loupist. */
  onWhatItKnows?: () => void;
  /** "What {name} knows" is the open page, so its link is the current item rather than a chat. */
  whatItKnowsOpen?: boolean;
  settingsOpen?: boolean;
  onSelectThread: (thread: ChatThread) => void; onRenameThread: (thread: ChatThread) => void; onDeleteThread: (thread: ChatThread) => void; onExportThread?: (thread: ChatThread, format: "md" | "pdf") => void;
};

export function LoupistNavigationGroup({ unreadCount = 0, waiting = false, finishedHidden = false, finishedThreadKeys = new Set(), loupist, parentPath, selected, loading, chatList, loaded, activeThreadId, allChatsOpen, running, runningThreadKeys, draftThreadKeys = new Set(), loadThreads, onSelect, onManage, onReveal, onRemove, onAllChats, onWhatItKnows, whatItKnowsOpen = false, settingsOpen = false, onCreateThread, onSelectThread, onRenameThread, onDeleteThread, onExportThread }: Props) {
  useTranslation();
  const { state, isMobile } = useSidebar();
  // The icon rail is only avatars and commands. Chat titles are not mounted there, so a narrow rail cannot wrap them one character per line.
  const iconRail = state === "collapsed" && !isMobile;
  const label = parentPath ? `${loupist.name} · ${parentPath}` : loupist.name;
  const cannotCreateChat = loading || loupist.availability.status !== "available";
  const [menuOpen, setMenuOpen] = useState(false);
  const [loadError, setLoadError] = useState(false);
  const [retry, setRetry] = useState(0);
  // Leaving a Loupist closes its folder. Opening another folder does not take selection or the shared unsent draft.
  const [expanded, setExpanded] = useState(selected);
  useEffect(() => { setExpanded(selected); }, [selected]);
  useEffect(() => {
    if (!expanded || loading || loaded || loupist.availability.status === "recovery_required") return;
    let live = true;
    void loadThreads(loupist.id).catch(() => { if (live) setLoadError(true); });
    return () => { live = false; };
  }, [expanded, loading, loaded, loupist.id, loupist.availability.status, loadThreads, retry]);
  const disclosureLabel = i18n.t(expanded ? "app:loupistNavigationGroup.hideChats" : "app:loupistNavigationGroup.showChats", { label });
  const listedThreadId = selected ? activeThreadId : null;
  const detail = [parentPath, loupist.philosophy].filter(Boolean).join(" · ");
  return <li className="loupist-navigation-group" data-loupist-id={loupist.id} data-expanded={expanded || undefined}>
    <div className="loupist-navigation-row" data-selected={selected || undefined} onContextMenu={event => { event.preventDefault(); setMenuOpen(true); }}>
      <button type="button" className="loupist-disclosure" aria-expanded={expanded} aria-label={disclosureLabel} title={disclosureLabel} onClick={() => setExpanded(value => !value)}>
        <ChevronRightIcon aria-hidden="true" />
      </button>
      <button type="button" className="loupist-navigation-button" aria-label={i18n.t("app:loupistNavigationGroup.selectLoupist", { label })} aria-pressed={selected} title={detail ? `${loupist.name} — ${detail}` : loupist.name} onClick={() => { setExpanded(true); onSelect(); }}>
        <FolderIcon className="loupist-folder-glyph" aria-hidden="true" />
        <LoupistAvatar loupist={loupist} />
        <span className="loupist-row-copy"><span>{loupist.name}</span></span>
      </button>
      {/* One status at a time: waiting for you, running, unread results, finished while away. */}
      {waiting ? <span className="loupist-status-dot" data-state="waiting" role="img" aria-label={i18n.t("app:attention.waitingFor", { label: loupist.name })} />
        : running ? <Spinner className="loupist-navigation-running" aria-label={i18n.t("app:loupistNavigationGroup.hasActiveSessions", { label: loupist.name })} role="status" />
          : unreadCount > 0 ? <span className="shell-unread" aria-label={i18n.t("app:shell.unreadBriefings", { count: unreadCount })}>{unreadCount > 99 ? "99+" : unreadCount}</span>
            : finishedHidden ? <span className="loupist-status-dot" data-state="finished" role="img" aria-label={i18n.t("app:attention.finishedHidden")} /> : null}
      <div className="loupist-row-actions">
        <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}><DropdownMenuTrigger asChild><Button size="icon-sm" variant="ghost" aria-label={i18n.t("app:loupistNavigationGroup.optionsForLoupist", { label: loupist.name })} title={i18n.t("app:loupistNavigationGroup.optionsForLoupist", { label: loupist.name })}><MoreHorizontalIcon /></Button></DropdownMenuTrigger>
          <DropdownMenuContent side="right" align="start"><DropdownMenuItem disabled={cannotCreateChat} onSelect={onCreateThread}><SquarePenIcon />{i18n.t("app:shell.newChat")}</DropdownMenuItem>{loupist.workspace_path ? <DropdownMenuItem onSelect={onReveal}><FolderOpenIcon />{i18n.t("app:shell.reveal")}</DropdownMenuItem> : null}<DropdownMenuItem variant="destructive" onSelect={onRemove}><Trash2Icon />{i18n.t("app:shell.removeLoupist")}</DropdownMenuItem></DropdownMenuContent>
        </DropdownMenu>
      </div>
    </div>
    {expanded && !iconRail && <div className="loupist-nav-children">
      <nav className="loupist-recents" aria-label={i18n.t("app:loupistNavigationGroup.chatsFor", { label: loupist.name })}><SidebarMenu className="loupist-chat-list">
      {recentLoupistChats(chatList.chats, listedThreadId, loupist.id).map(thread => <LoupistChatItem key={thread.id} thread={thread} active={selected && thread.id === activeThreadId} running={runningThreadKeys.has(chatSessionKey(loupist.id, thread.id))} finished={finishedThreadKeys.has(chatSessionKey(loupist.id, thread.id))} draft={selected && draftThreadKeys.has(chatSessionKey(loupist.id, thread.id))} onSelect={onSelectThread} onRename={onRenameThread} onDelete={onDeleteThread} onExport={onExportThread} />)}
      {loadError ? <li><button type="button" className="loupist-nav-action" onClick={() => { setLoadError(false); setRetry(value => value + 1); }}>{i18n.t("app:loupistNavigationGroup.couldnTLoadChatsRetry")}</button></li> : null}
      {selected && <li><button type="button" className="loupist-nav-action loupist-all-chats" aria-expanded={allChatsOpen} onClick={onAllChats}>{i18n.t("app:shell.allChats")}</button></li>}
      {selected && onWhatItKnows && <li><button type="button" className="loupist-nav-action loupist-knows" data-what-it-knows aria-current={whatItKnowsOpen ? "page" : undefined}
        title={i18n.t("app:palette.actions.whatItKnows", { loupistSubject: subjectName(loupist.name, i18n.language) })} onClick={onWhatItKnows}>
        <BookOpenIcon aria-hidden="true" />{i18n.t("app:shell.whatItKnows")}
      </button></li>}
      {selected && <li><button type="button" className="loupist-nav-action" data-loupist-settings aria-current={settingsOpen ? "page" : undefined} onClick={onManage}><UserRoundCogIcon aria-hidden="true" />{i18n.t("app:shell.loupistSettings")}</button></li>}
      </SidebarMenu></nav>
    </div>}
  </li>;
}
