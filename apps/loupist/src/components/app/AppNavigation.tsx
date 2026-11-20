import i18n, { useTranslation } from "@/i18n";
import { chatSessionKey } from "../../chat/sessionRegistry";
import { runningHandoffChats } from "../../chat/useHostedRun";
import { useCallback, useLayoutEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import {
  ApertureIcon, ArrowLeftIcon, CalendarClockIcon, NetworkIcon, PlusIcon, SearchIcon, SettingsIcon, StarIcon, XIcon, type LucideIcon,
} from "lucide-react";
import {
  Sidebar, SidebarContent, SidebarFooter, SidebarGroup, SidebarGroupContent,
  SidebarGroupLabel, SidebarHeader, SidebarMenu,
  SidebarMenuButton, SidebarMenuItem, useSidebar,
} from "@/components/ui/sidebar";
import { Button } from "@/components/ui/button";
import { SETTINGS_SECTIONS, type AppDestinationId, type AppTab, type SettingsSection } from "../../app/navigation";
import { LoupistChatItem } from "./LoupistChatItem";
import { LoupistSwitcher } from "./LoupistSwitcher";
import { SidebarResizeHandle } from "./SidebarResizeHandle";
import { NeedsAttention } from "./NeedsAttention";
import type { AttentionItem } from "../../contracts/attention";
import { getLoupistChatList, recentLoupistChats } from "./loupistNavigation";
import { shortcutFor, tabShortcut } from "../../app/shortcuts";
import { APP_DESTINATIONS } from "../../app/navigation";
import type { ChatThread, LoupistSummary } from "../../types";
import "../../styles/navigation.css";

type Props = {
  /** Cross-Loupist items that wait for the user; approvals also turn the Loupist's dot amber. */
  attention?: readonly AttentionItem[];
  /** Chats whose run ended while the user was not looking at them. */
  finishedThreadKeys?: ReadonlySet<string>;
  onOpenAttention?: (item: AttentionItem) => void;
  onMoreAttention?: () => void;
  tab: AppTab;
  selectedLoupist: LoupistSummary | null;
  /** Every Loupist, for the switcher at the top of the sidebar. */
  loupists: LoupistSummary[];
  /** Loupists with a reply or a scheduled run in progress. */
  runningLoupists: ReadonlySet<string>;
  /** Unread scheduled results per Loupist. */
  unreadCounts: Record<string, number>;
  onSelectLoupist: (id: string) => void;
  onCreateLoupist: () => void;
  onLoupistSettings: () => void;
  loading: boolean;
  threadsByLoupist: Record<string, ChatThread[]>;
  /** The chat on screen, or null while another page covers the conversation. */
  activeThreadId: string | null;
  /** Chats with a reply running that this window streams or the host reported on reload (keys from `chatSessionKey`). */
  runningThreadKeys: ReadonlySet<string>;
  /** Chats with unsent composer text (keys from `chatSessionKey`). */
  draftThreadKeys?: ReadonlySet<string>;
  settingsSection: SettingsSection;
  updateAvailable: boolean;
  onTabChange: (tab: AppTab) => void;
  onSettingsSectionChange: (section: SettingsSection) => void;
  onExitSettings: () => void;
  onCreateThread: (loupistId: string) => void;
  onSelectThread: (thread: ChatThread) => void;
  onRenameThread: (thread: ChatThread) => void;
  onSearch: () => void;
  onDeleteThread: (thread: ChatThread) => void;
  onExportThread?: (thread: ChatThread, format: "md" | "pdf") => void;
  /** Unread scheduled results for the selected Loupist, shown on the Schedule row. */
  scheduleUnread?: number;
  /** Opens a ⌘ destination: Bot, or the Map, Lens or Schedule page. */
  onDestination?: (id: AppDestinationId) => void;
};

export function AppNavigation({
  attention = [], finishedThreadKeys = new Set(), onOpenAttention, onMoreAttention, tab, selectedLoupist, loupists, runningLoupists, unreadCounts,
  onSelectLoupist, onCreateLoupist, onLoupistSettings, loading, threadsByLoupist, activeThreadId,
  runningThreadKeys, draftThreadKeys = new Set(), settingsSection, updateAvailable, onTabChange, onSettingsSectionChange, onExitSettings,
  onCreateThread, onSelectThread, onRenameThread, onDeleteThread, onExportThread, onSearch, scheduleUnread = 0, onDestination,
}: Props) {
  useTranslation();
  const { isMobile, setOpenMobile, state } = useSidebar();
  const searchShortcut = shortcutFor("palette");
  const newChatShortcut = shortcutFor("newChat");
  const canStartChat = selectedLoupist != null && !loading && selectedLoupist.availability.status === "available";
  const [settingsSearch, setSettingsSearch] = useState("");
  const recent = selectedLoupist ? recentLoupistChats(getLoupistChatList(threadsByLoupist[selectedLoupist.id], activeThreadId), activeThreadId, selectedLoupist.id) : [];
  const iconOnly = !isMobile && state === "collapsed";
  // On a short window the section list scrolls above Back: the open section's row stays in view there.
  const settingsList = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const list = settingsList.current;
    const row = list?.querySelector<HTMLElement>(`#settings-section-${settingsSection}`);
    if (list && row) keepInView(list, row);
  }, [tab, settingsSection]);
  const openDestination = (id: AppDestinationId) => { onDestination?.(id); closeMobile(); };
  /**
   * Bot, Following, Map, Lens and Schedule: one row each, with the ⌘ key where the destination has one.
   * Each is a destination: choosing it again keeps it open, and the row of the place on screen is raised.
   * Map and Lens open beside the conversation; while one is open its row is the current place.
   */
  const placeRow = ({ id, icon: Icon, active, onClick, unread = 0 }: { id: "following" | AppDestinationId; icon: LucideIcon; active: boolean; onClick: () => void; unread?: number }) => {
    const label = i18n.t(`app:shell.tabs.${id}`);
    const index = APP_DESTINATIONS.findIndex(item => item.id === id);
    const unreadId = `sidebar-${id}-unread`;
    return <SidebarMenuItem key={id} className="loupist-chat-row loupist-place-row" data-sidebar-place={id}>
      <SidebarMenuButton type="button" className="loupist-chat-button" data-sidebar-schedule={id === "schedule" || undefined}
        isActive={active} aria-current={active ? "page" : undefined}
        aria-label={iconOnly ? label : undefined} aria-describedby={unread > 0 ? unreadId : undefined}
        title={index >= 0 ? `${label} ${tabShortcut(index + 1)}` : label} onClick={onClick}>
        <Icon className="loupist-home-icon" aria-hidden="true" />
        {iconOnly ? null : <span className="loupist-chat-title">{label}</span>}
        {unread > 0 ? <><span className="shell-unread" aria-hidden="true">{unread}</span><span className="sr-only" id={unreadId}>{i18n.t("app:shell.unreadBriefings", { count: unread })}</span></>
          : !iconOnly && index >= 0 ? <kbd className="loupist-place-key" aria-hidden="true">{tabShortcut(index + 1)}</kbd> : null}
      </SidebarMenuButton>
    </SidebarMenuItem>;
  };
  const sessions = recent.filter(thread => !thread.home);
  // Research sessions running a hand-off, as the host reports on each, whether or not this window has joined it.
  const selectedThreads = selectedLoupist ? threadsByLoupist[selectedLoupist.id] : undefined;
  const handoffs = useMemo(() => runningHandoffChats(selectedThreads ?? []), [selectedThreads]);
  const row = (thread: ChatThread) => {
    const key = chatSessionKey(thread.loupist_id, thread.id);
    const active = thread.id === activeThreadId;
    // A session spins while a reply runs there; Bot, for its own reply or a hand-off it started,
    // only while it is not the open view: its header shows its own reply, the session's row the hand-off.
    const running = thread.home ? !active && (runningThreadKeys.has(key) || handoffs.size > 0) : runningThreadKeys.has(key) || handoffs.has(thread.id);
    return <LoupistChatItem iconOnly={iconOnly} key={key} thread={thread} active={active} running={running} finished={finishedThreadKeys.has(key)} draft={draftThreadKeys.has(key)}
      onSelect={item => { onSelectThread(item); closeMobile(); }} onSchedule={thread.home ? () => navigate("schedule") : undefined} onRename={onRenameThread} onDelete={onDeleteThread} onExport={onExportThread} />;
  };
  const closeMobile = () => { if (isMobile) setOpenMobile(false); };
  const navigate = (next: AppTab) => { onTabChange(next); closeMobile(); };

  if (tab === "settings") {
    return (
      <Sidebar className="app-navigation settings-navigation" collapsible={isMobile ? "offcanvas" : "none"} aria-label={i18n.t("app:appNavigation.settingsNavigation")}>
        <SidebarHeader className="p-3" data-tauri-drag-region="deep">
          <div className="navigation-toolbar" data-tauri-drag-region="deep" />
          <div className="app-brand"><strong>{i18n.t("app:appNavigation.settings")}</strong></div>
          <div className="settings-search"><SearchIcon aria-hidden="true" />
            <input type="search" aria-label={i18n.t("app:appNavigation.searchSettings")} placeholder={i18n.t("app:appNavigation.findASetting")} value={settingsSearch} onChange={(event) => setSettingsSearch(event.target.value)} />
            {settingsSearch ? <button type="button" aria-label={i18n.t("app:appNavigation.clearSettingsSearch")} onClick={() => setSettingsSearch("")}><XIcon /></button> : null}
          </div>
        </SidebarHeader>
        <SidebarContent ref={settingsList}>
          <nav aria-label={i18n.t("app:appNavigation.settingsSections")}>
            {(["research", "application"] as const).map((group) => {
              const items = SETTINGS_SECTIONS.filter((item) => item.group === group && `${item.label} ${item.keywords} ${item.caption ?? ""}`.toLowerCase().includes(settingsSearch.trim().toLowerCase()));
              return items.length ? <SidebarGroup key={group}>
                <SidebarGroupLabel>{i18n.t(`app:appNavigation.${group}`)}</SidebarGroupLabel>
                <SidebarGroupContent><SidebarMenu className="gap-1">
                  {items.map((item) => <SidebarMenuItem key={item.id}>
                    <SidebarMenuButton id={`settings-section-${item.id}`} type="button"
                      isActive={settingsSection === item.id}
                      aria-current={settingsSection === item.id ? "page" : undefined}
                      onClick={() => { onSettingsSectionChange(item.id); closeMobile(); }}>
                      <span className="settings-nav-label"><span>{item.label}</span>{item.caption ? <small className="settings-nav-caption" aria-hidden="true">{item.caption}</small> : null}</span>
                      {item.id === "updates" && updateAvailable ? <span className="shell-update-dot" aria-label={i18n.t("app:shell.updateAvailable")}><span className="sr-only">{i18n.t("app:shell.updateAvailable")}</span></span> : null}
                    </SidebarMenuButton>
                  </SidebarMenuItem>)}
                </SidebarMenu></SidebarGroupContent>
              </SidebarGroup> : null;
            })}
            {!SETTINGS_SECTIONS.some((item) => `${item.label} ${item.keywords} ${item.caption ?? ""}`.toLowerCase().includes(settingsSearch.trim().toLowerCase())) ? <p className="settings-search-empty" role="status">{i18n.t("app:appNavigation.noSettingsFound")}</p> : null}
          </nav>
        </SidebarContent>
        <SidebarFooter className="navigation-footer">
          <SidebarMenu><SidebarMenuItem><SidebarMenuButton type="button" onClick={() => { onExitSettings(); closeMobile(); }}>
            <ArrowLeftIcon /><span>{i18n.t("app:appNavigation.backToApp")}</span>
          </SidebarMenuButton></SidebarMenuItem></SidebarMenu>
        </SidebarFooter>
      </Sidebar>
    );
  }

  return (
    <Sidebar className="app-navigation" collapsible="icon" aria-label={i18n.t("app:appNavigation.applicationNavigation")}>
      <SidebarHeader data-tauri-drag-region="deep">
        <div className="navigation-toolbar" data-tauri-drag-region="deep" />
        {selectedLoupist ? <LoupistSwitcher loupist={selectedLoupist} loupists={loupists} runningLoupists={runningLoupists} unreadCounts={unreadCounts} iconOnly={iconOnly}
          onSelect={id => { onSelectLoupist(id); closeMobile(); }} onCreate={() => { onCreateLoupist(); closeMobile(); }} onSettings={() => { onLoupistSettings(); closeMobile(); }} /> : null}
        <Button variant="ghost" className="navigation-command navigation-search" aria-label={iconOnly ? i18n.t("app:shell.search") : undefined}
          title={i18n.t("app:shell.searchShortcut", { shortcut: searchShortcut })} onClick={() => { onSearch(); closeMobile(); }}>
          <SearchIcon /><span>{i18n.t("app:shell.search")}</span><kbd aria-hidden="true">{searchShortcut}</kbd>
        </Button>
      </SidebarHeader>
      <SidebarContent>{onOpenAttention ? <NeedsAttention threads={threadsByLoupist} items={attention} onOpen={(item) => { onOpenAttention(item); closeMobile(); }} onMore={onMoreAttention ? () => { onMoreAttention(); closeMobile(); } : undefined} /> : null}<SidebarGroup className="loupist-navigation" data-testid="loupist-navigation">
        <SidebarMenu className="loupist-places">{recent.filter(thread => thread.home).map(row)}
          {selectedLoupist ? <>
            {placeRow({ id: "following", icon: StarIcon, active: tab === "following", onClick: () => navigate("following") })}
            {placeRow({ id: "map", icon: NetworkIcon, active: tab === "map", onClick: () => openDestination("map") })}
            {placeRow({ id: "lens", icon: ApertureIcon, active: tab === "lens", onClick: () => openDestination("lens") })}
            {placeRow({ id: "schedule", icon: CalendarClockIcon, active: tab === "schedule", onClick: () => navigate("schedule"), unread: scheduleUnread })}
          </> : null}
        </SidebarMenu>
        <div className="loupist-list-heading">
          <SidebarGroupLabel>{i18n.t("app:shell.sessions")}</SidebarGroupLabel>
          <Button variant="ghost" size="icon-sm" className="navigation-new" data-testid="navigation-new" disabled={!canStartChat} aria-label={i18n.t("app:shell.newResearch")}
            title={`${i18n.t("app:shell.newResearchHint")} ${newChatShortcut}`} onClick={() => { if (selectedLoupist) onCreateThread(selectedLoupist.id); closeMobile(); }}>
            <PlusIcon aria-hidden="true" />
          </Button>
        </div>
        {iconOnly ? null : sessions.length ? <SidebarMenu className="loupist-sessions">{sessions.map(row)}</SidebarMenu>
          : <p className="loupist-sessions-empty" data-sessions-empty>{i18n.t("app:shell.sessionsEmpty")}</p>}
      </SidebarGroup></SidebarContent>
      <SidebarFooter className="navigation-footer">
        <SidebarMenu><SidebarMenuItem><SidebarMenuButton id="main-tab-settings" aria-label={i18n.t("app:shell.settings")} title={i18n.t("app:shell.settings")} onClick={() => navigate("settings")}><SettingsIcon /><span>{i18n.t("app:shell.settings")}</span>{updateAvailable ? <span className="shell-update-dot" aria-label={i18n.t("app:shell.updateAvailable")}><span className="sr-only">{i18n.t("app:shell.updateAvailable")}</span></span> : null}</SidebarMenuButton></SidebarMenuItem></SidebarMenu></SidebarFooter>
      <SidebarResizeHandle />
    </Sidebar>
  );
}

/** Scrolls `list` only as far as it takes to show all of `row`; the window never moves. */
function keepInView(list: HTMLElement, row: HTMLElement) {
  const bounds = list.getBoundingClientRect();
  const box = row.getBoundingClientRect();
  if (box.bottom > bounds.bottom) list.scrollTop += box.bottom - bounds.bottom;
  else if (box.top < bounds.top) list.scrollTop -= bounds.top - box.top;
}

/** Below this width the sidebar starts as an icon rail; above it, expanded. */
const RAIL_QUERY = "(max-width: 939px)";
const subscribeRail = (onChange: () => void) => {
  const query = window.matchMedia(RAIL_QUERY);
  query.addEventListener("change", onChange);
  return () => query.removeEventListener("change", onChange);
};

/** Whether the window is in the width range where the sidebar starts as an icon rail. */
export function useRailWidth() {
  return useSyncExternalStore(subscribeRail, () => window.matchMedia(RAIL_QUERY).matches, () => false);
}

/**
 * Whether the sidebar is expanded. A narrow window keeps its own choice, so expanding the
 * rail at 800 px does not carry into the wide layout, and the reverse.
 */
export function useSidebarOpen() {
  const narrow = useRailWidth();
  const [wideOpen, setWideOpen] = useState(true);
  const [narrowOpen, setNarrowOpen] = useState(false);
  const setOpen = useCallback((next: boolean) => (narrow ? setNarrowOpen : setWideOpen)(next), [narrow]);
  return { open: narrow ? narrowOpen : wideOpen, setOpen };
}
