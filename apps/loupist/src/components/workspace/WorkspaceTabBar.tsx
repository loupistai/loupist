import i18n, { useTranslation } from "@/i18n";
import { useEffect, useLayoutEffect, useRef, type KeyboardEvent, type ReactNode } from "react";
import {
  BookOpenIcon,
  ChartNoAxesCombinedIcon,
  FileTextIcon,
  FolderIcon,
  GlobeIcon,
  NetworkIcon,
  PlusIcon,
  XIcon,
} from "lucide-react";
import { Spinner } from "@/components/ui/spinner";
import type {
  ChatWorkspaceSession,
  ChatWorkspaceTab,
} from "../../chat/workspace";
import {
  CHAT_MAP_TAB_ID,
  EVIDENCE_TAB_ID,
  WORK_TAB_ID,
  workspaceTabButtonId,
  workspaceTabPanelId,
  workspaceTabTitle,
} from "../../chat/workspace";

import { useChatMap } from "./ChatMapContext";

const t = i18n.getFixedT(null, "chat");


type Props = {
  session: ChatWorkspaceSession;
  activeTab?: ChatWorkspaceTab;
  menuOpen: boolean;
  layoutControls?: ReactNode;
  onMenuOpenChange: (open: boolean) => void;
  onSelectTab: (tabId: string) => void;
  onCloseTab: (tab: ChatWorkspaceTab) => void;
  onNewBrowser: () => void;
  onOpenFiles?: () => void;
};

const tabIcon = (tab: ChatWorkspaceTab) => {
  if (tab.kind === "files") return <FolderIcon />;
  if (tab.kind === "browser") return <GlobeIcon />;
  return <FileTextIcon />;
};

/** Content arrived while the user was reading another tab. */
function NewContentBadge({ title }: { title: string }) {
  return <><span className="workspace-tab-badge" aria-hidden="true" /><span className="sr-only">{t("workspaceTabBar.newContent", { tab: title })}</span></>;
}

export function WorkspaceTabBar({
  session,
  activeTab,
  menuOpen,
  layoutControls,
  onMenuOpenChange,
  onSelectTab,
  onCloseTab,
  onNewBrowser,
  onOpenFiles,
}: Props) {
  useTranslation("chat");
  const chatMap = useChatMap();
  const addMenuRef = useRef<HTMLDetailsElement>(null);
  const tabsRef = useRef<HTMLDivElement>(null);
  const addTriggerRef = useRef<HTMLElement>(null);
  const menuFocusIndex = useRef<number | null>(null);
  useLayoutEffect(() => {
    if (!menuOpen || menuFocusIndex.current === null) return;
    const items = addMenuRef.current?.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]:not([disabled])');
    items?.[menuFocusIndex.current]?.focus();
    menuFocusIndex.current = null;
  }, [menuOpen]);
  useEffect(() => {
    if (!menuOpen) return;
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && !addMenuRef.current?.contains(event.target)) {
        onMenuOpenChange(false);
      }
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [menuOpen, onMenuOpenChange]);

  // Fixed tabs first (Map, Evidence, Work), then the closable readers in the order they opened.
  const closable = session.tabs.filter((tab) => tab.kind !== "evidence" && tab.kind !== "work");
  const tabIds = [...(chatMap ? [CHAT_MAP_TAB_ID] : []), EVIDENCE_TAB_ID, WORK_TAB_ID, ...closable.map((tab) => tab.id)];
  const workChosen = session.active_tab_id === WORK_TAB_ID;
  const mapSelected = Boolean(chatMap && (chatMap.active || (!activeTab && !workChosen)));
  const workSelected = !mapSelected && workChosen;
  const evidenceSelected = !mapSelected && !workSelected && (activeTab?.kind === "evidence" || !activeTab);
  const badged = (tabId: string) => session.badges?.includes(tabId) ?? false;

  const moveTabFocus = (event: KeyboardEvent, tabId: string) => {
    if (event.key === "Delete") {
      const index = closable.findIndex((tab) => tab.id === tabId);
      const tab = closable[index];
      if (!tab) return;
      event.preventDefault();
      const next = closable[index + 1] ?? closable[index - 1];
      onCloseTab(tab);
      document.getElementById(workspaceTabButtonId(next?.id ?? EVIDENCE_TAB_ID))?.focus();
      return;
    }
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const current = tabIds.indexOf(tabId);
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? tabIds.length - 1
          : (current +
              (event.key === "ArrowRight" ? 1 : -1) +
              tabIds.length) %
            tabIds.length;
    const nextId = tabIds[next];
    if (nextId) {
      if (nextId === CHAT_MAP_TAB_ID) chatMap?.select();
      else { chatMap?.deselect(); onSelectTab(nextId); }
      document.getElementById(workspaceTabButtonId(nextId))?.focus();
    }
  };
  useLayoutEffect(() => {
    const list = tabsRef.current;
    const selected = list?.querySelector<HTMLElement>('[data-active="true"]');
    if (!list || !selected) return;
    const left = selected.offsetLeft;
    const right = left + selected.offsetWidth;
    if (left < list.scrollLeft) list.scrollLeft = left;
    else if (right > list.scrollLeft + list.clientWidth) list.scrollLeft = right - list.clientWidth;
  }, [activeTab?.id, mapSelected, evidenceSelected, workSelected]);
  const evidenceTitle = t("workspaceTabBar.evidence");
  const workTitle = t("workspaceTabBar.work");

  const moveMenuFocus = (event: KeyboardEvent<HTMLDetailsElement>) => {
    if (event.key === "Escape" && menuOpen) {
      event.preventDefault();
      event.stopPropagation();
      onMenuOpenChange(false);
      addTriggerRef.current?.focus();
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]:not([disabled])'));
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = event.key === "Home" ? 0 : event.key === "End" ? items.length - 1
      : index < 0 ? event.key === "ArrowUp" ? items.length - 1 : 0
      : (index + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
    if (!menuOpen) {
      menuFocusIndex.current = next;
      onMenuOpenChange(true);
    } else {
      items[next]?.focus();
    }
  };

  return (
    <div className="workspace-tabbar">
      <div
        ref={tabsRef}
        className="workspace-tabs"
        role="tablist"
        aria-label={t("workspaceTabBar.openWorkspaceTabs")}
      >
        {chatMap ? <div className="workspace-tab workspace-tab-fixed" data-active={mapSelected || undefined}><button id={workspaceTabButtonId(CHAT_MAP_TAB_ID)} type="button" role="tab" aria-selected={mapSelected} aria-controls={workspaceTabPanelId(CHAT_MAP_TAB_ID)} tabIndex={mapSelected ? 0 : -1} onClick={() => chatMap.select()} onKeyDown={event => moveTabFocus(event, CHAT_MAP_TAB_ID)}><NetworkIcon /><span>{t("workspaceTabBar.map")}</span>{chatMap.badged ? <NewContentBadge title={t("workspaceTabBar.map")} /> : null}</button></div> : null}
        <div className="workspace-tab workspace-tab-fixed" data-active={evidenceSelected || undefined}>
          <button id={workspaceTabButtonId(EVIDENCE_TAB_ID)} type="button" role="tab" aria-selected={evidenceSelected} aria-controls={workspaceTabPanelId(EVIDENCE_TAB_ID)} tabIndex={evidenceSelected ? 0 : -1}
            onClick={() => { chatMap?.deselect(); onSelectTab(EVIDENCE_TAB_ID); }} onKeyDown={event => moveTabFocus(event, EVIDENCE_TAB_ID)}>
            <BookOpenIcon /><span>{evidenceTitle}</span>
            {badged(EVIDENCE_TAB_ID) && !evidenceSelected ? <NewContentBadge title={evidenceTitle} /> : null}
          </button>
        </div>
        <div className="workspace-tab workspace-tab-fixed" data-active={workSelected || undefined}>
          <button id={workspaceTabButtonId(WORK_TAB_ID)} type="button" role="tab" aria-selected={workSelected} aria-controls={workspaceTabPanelId(WORK_TAB_ID)} tabIndex={workSelected ? 0 : -1}
            onClick={() => { chatMap?.deselect(); onSelectTab(WORK_TAB_ID); }} onKeyDown={event => moveTabFocus(event, WORK_TAB_ID)}>
            <ChartNoAxesCombinedIcon /><span>{workTitle}</span>
            {badged(WORK_TAB_ID) && !workSelected ? <NewContentBadge title={workTitle} /> : null}
          </button>
        </div>
        {closable.map((tab) => {
          const title = workspaceTabTitle(tab);
          const selected = !mapSelected && tab.id === activeTab?.id;
          return (
            <div
              className="workspace-tab"
              data-active={selected || undefined}
              key={tab.id}
            >
              <button
                id={workspaceTabButtonId(tab.id)}
                type="button"
                role="tab"
                aria-selected={selected}
                aria-controls={workspaceTabPanelId(tab.id)}
                title={title}
                tabIndex={selected ? 0 : -1}
                onClick={() => { chatMap?.deselect(); onSelectTab(tab.id); }}
                onKeyDown={(event) => moveTabFocus(event, tab.id)}
              >
                {tabIcon(tab)}
                <span>{title}</span>
                {badged(tab.id) && !selected ? <NewContentBadge title={title} /> : null}
                {tab.kind === "browser" && tab.loading && <Spinner />}
              </button>
              <button
                type="button"
                className="workspace-tab-close"
                aria-label={t("workspaceTabBar.closeTab", { title: title })}
                onClick={(event) => {
                  const shouldRestore = event.currentTarget === document.activeElement;
                  const index = closable.findIndex((item) => item.id === tab.id);
                  const next = closable[index + 1] ?? closable[index - 1];
                  onCloseTab(tab);
                  if (shouldRestore) document.getElementById(workspaceTabButtonId(next?.id ?? EVIDENCE_TAB_ID))?.focus();
                }}
              >
                <XIcon />
              </button>
            </div>
          );
        })}
      </div>
      <details
        ref={addMenuRef}
        className="workspace-add"
        open={menuOpen}
        onToggle={(event) => onMenuOpenChange(event.currentTarget.open)}
        onKeyDown={moveMenuFocus}
      >
        <summary ref={addTriggerRef} aria-label={t("workspaceTabBar.openWorkspaceItem")} title={t("workspaceTabBar.openWorkspaceItem")} aria-haspopup="menu" aria-expanded={menuOpen}>
          <PlusIcon />
        </summary>
        <div className="workspace-add-menu" role="menu">
          {onOpenFiles ? <button type="button" role="menuitem" onClick={() => { onOpenFiles(); onMenuOpenChange(false); }}><FolderIcon /> {t("workspaceTabBar.files")}</button> : null}
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onNewBrowser();
              onMenuOpenChange(false);
            }}
          >
            <GlobeIcon />
            <span>
              <strong>{t("workspaceTabBar.newBrowserTab")}</strong>
              <small>{t("workspaceTabBar.openAnHTTPOrHTTPSAddress")}</small>
            </span>
          </button>
        </div>
      </details>
      {layoutControls ? <div className="workspace-tab-controls">{layoutControls}</div> : null}
    </div>
  );
}
