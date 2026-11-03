import i18n, { useTranslation } from "@/i18n";
import { WorkspaceEvidence } from "./WorkspaceEvidence";
import { WorkspaceFiles } from "./WorkspaceFiles";
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
} from "react";
import { ArrowLeftIcon, ChevronLeftIcon, ChevronRightIcon, Maximize2Icon, MoreHorizontalIcon, PanelBottomIcon, PanelRightIcon, XIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Spinner } from "@/components/ui/spinner";
import { formatDate, formatNumber } from "@/i18n/format";
import { citationPageUrl } from "../../chat/evidence";
import { parseCredentialFreeHttpUrl } from "../../lib/credentialFreeUrl";
import type {
  ChatArtifactRef,
  ChatArtifactView,
  LoupistBridge,
  KnowledgeLocation,
  KnowledgePin,
  WorkspaceFile,
} from "../../types";
import {
  CHAT_MAP_TAB_ID,
  EVIDENCE_TAB_ID,
  WORK_TAB_ID,
  currentEvidenceEntry,
  updateCurrentEvidenceEntry,
  workTab,
  workWorkspaceTab,
  workspaceTabButtonId,
  workspaceTabPanelId,
  type ChatWorkspaceSession,
  type ChatWorkspaceTab,
  type EvidenceEntry,
  type EvidenceWorkspaceTab,
  type WorkspacePlacement,
} from "../../chat/workspace";
import type { WorkspacePanelKind } from "../../app/useWorkspacePanel";
import type { DockKind } from "../../app/workspaceUiState";
import { WorkspaceWorkList, type BuildingWork } from "./WorkspaceWorkList";
import { WorkVersionBar, WorkVersionMenu } from "./WorkspaceWorkVersions";
import { sourceIdentityLabel } from "../../chat/evidence";
import { errorMessage } from "../../lib/errors";
import { focusForKeyboard } from "../../lib/inputModality";
import { ArtifactContent, ArtifactStatus, chatArtifactRef } from "../artifacts/ArtifactContent";
import { BrowserWorkspaceView } from "./BrowserWorkspaceView";
import { useNativeBrowserTabs } from "./useNativeBrowserTabs";
import {
  useWorkspacePanelResize,
  useWorkspaceOverlay,
  workspacePanelBounds,
} from "./useWorkspacePanelResize";
import { WorkspaceKnowledgeReader } from "./WorkspaceKnowledgeReader";
import { ChatMiniMap } from "./ChatMiniMap";
import { useChatMap } from "./ChatMapContext";
import { WorkspaceTabBar } from "./WorkspaceTabBar";

const t = i18n.getFixedT(null, "chat");

// Remember a keyboard header activation until the panel opens. The header remains visible;
// its menu closes before focus settles on Close. A chat switch can detach any earlier opener.
let focusCloseAfterOpen = false;
export function notePanelOpenedFromHeader(keyboard: boolean) {
  focusCloseAfterOpen = keyboard;
}
function takeFocusCloseAfterOpen() {
  const keyboard = focusCloseAfterOpen;
  focusCloseAfterOpen = false;
  return keyboard;
}

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  threadId: string;
  /** Shown in the Evidence status line for this Loupist's saved Memory. */
  loupistName?: string;
  workspacePath?: string;
  onOpenFiles?: (filePath?: string) => void;
  onAskFile?: (file: WorkspaceFile) => void;
  open: boolean;
  kind?: WorkspacePanelKind;
  children?: ReactNode;
  /** Charts the visible chat is still building, shown on top of the Work list. */
  buildingWork?: readonly BuildingWork[];
  /** Changes when the visible chat publishes work, so the Work list re-reads. */
  workRefreshKey?: string | number;
  session: ChatWorkspaceSession;
  theme: "light" | "dark";
  width: number;
  height: number;
  placement: WorkspacePlacement;
  onWidthChange: (width: number) => void;
  onHeightChange: (height: number) => void;
  onPlacementToggle: (placement: WorkspacePlacement, kind: DockKind) => void;
  /** The content kind on screen changed (side-panel.md P7): re-dock for it. */
  onDockKind?: (kind: DockKind) => void;
  /** Every content kind docks at the current placement ("Always dock here"). */
  placementPinned?: boolean;
  onPinPlacement?: (pinned: boolean) => void;
  onSelectTab: (tabId: string) => void;
  onUpdateTab: (
    tabId: string,
    update: (tab: ChatWorkspaceTab) => ChatWorkspaceTab,
  ) => void;
  onCloseTab: (tab: ChatWorkspaceTab) => void;
  /** Opens a chart or document in the Work tab; `threadId` is the chat that made it. */
  onOpenArtifact: (artifact: ChatArtifactRef, threadId: string) => void;
  onAskArtifact: (artifact: ChatArtifactRef) => void;
  askArtifactDisabled?: boolean;
  onNewBrowser: () => void;
  onOpenKnowledge: (location: KnowledgeLocation) => void;
  onOpenKnowledgePage: (location: KnowledgeLocation) => void;
  onAskKnowledge: (pin: KnowledgePin, threadId?: string) => void;
  onOpenLink: (url: string) => void;
  onRevealProgress?: (itemId: string) => void;
  /** Moves the Evidence reader through its history. */
  onMoveEvidence?: (delta: -1 | 1) => void;
  /** Pointer or key activity inside the panel; recent activity turns new content into a badge. */
  onInteract?: () => void;
  onClose: () => void;
};

const knowledgeKindLabel = (location: KnowledgeLocation) => {
  if (location.surface === "source") return t("workspaceEvidence.kind.savedSource");
  if (location.surface === "lens") return t("workspaceEvidence.kind.lens");
  if (location.surface === "note") return location.selection?.kind === "journal" ? t("workspaceEvidence.kind.journal") : t("workspaceEvidence.kind.note");
  return location.surface === "world" ? "World" : t("workspaceEvidence.kind.source");
};

/** "{kind} · {hostname or Loupist} · {retrieved date}" for the Evidence reader. */
const evidenceStatus = (entry: EvidenceEntry, loupistName?: string) => {
  if (entry.kind === "knowledge") return [knowledgeKindLabel(entry.location), loupistName].filter(Boolean).join(" · ");
  const evidence = entry.evidence;
  const kind = evidence.kind === "local_file" ? t("workspaceEvidence.kind.file") : t("workspaceEvidence.kind.source");
  const retrieved = new Date(evidence.retrieved_at);
  return [kind, sourceIdentityLabel(evidence), Number.isNaN(retrieved.getTime()) ? null : formatDate(retrieved)].filter(Boolean).join(" · ");
};

const isTextEntry = (target: EventTarget | null) =>
  target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));

export function ChatWorkspacePanel(props: Props) {
  useTranslation("chat");
  return <ScopedChatWorkspacePanel key={JSON.stringify([props.loupistId, props.threadId])} {...props} />;
}

function ScopedChatWorkspacePanel({
  bridge,
  loupistId,
  threadId,
  loupistName,
  workspacePath,
  onOpenFiles,
  onAskFile,
  open,
  kind = "materials",
  children,
  buildingWork = [],
  workRefreshKey = 0,
  session,
  theme,
  width,
  height,
  placement,
  onWidthChange,
  onHeightChange,
  onPlacementToggle,
  onDockKind,
  placementPinned = false,
  onPinPlacement,
  onSelectTab,
  onUpdateTab,
  onCloseTab,
  onOpenArtifact,
  onAskArtifact,
  askArtifactDisabled,
  onNewBrowser,
  onOpenKnowledge,
  onOpenKnowledgePage,
  onAskKnowledge,
  onOpenLink,
  onRevealProgress,
  onMoveEvidence,
  onInteract,
  onClose,
}: Props) {
  useTranslation("chat");
  const chatMap = useChatMap();
  useEffect(() => { chatMap?.deselect(); }, [session.active_tab_id]);
  const [view, setView] = useState<ChatArtifactView | null>(null);
  const [loadingArtifact, setLoadingArtifact] = useState(false);
  const [readError, setReadError] = useState<string | null>(null);
  const overlay = useWorkspaceOverlay();
  const [menuOpen, setMenuOpen] = useState(false);
  const [actionsMenuOpen, setActionsMenuOpen] = useState(false);
  const readRequest = useRef(0);
  // The panel is keyed by Loupist and chat; a read that finishes after the scope changed must not open anything.
  const scopeLive = useRef(true);
  useEffect(() => {
    scopeLive.current = true;
    return () => { scopeLive.current = false; };
  }, []);
  const activeLayoutButtonRef = useRef<HTMLButtonElement>(null);
  // Expand (side-panel.md §2.4) shows the open tab as a reading column over the main area; it resets when the
  // panel closes, turns into the narrow sheet or re-docks.
  const [expanded, setExpanded] = useState(false);
  useEffect(() => { if (!open || overlay) setExpanded(false); }, [open, overlay, placement]);
  const expandButtonRef = useRef<HTMLButtonElement>(null);
  // Expand closes the menu; skip the menu's focus return so the reading column can take it.
  const expandOnClose = useRef(false);
  const backToChatRef = useRef<HTMLButtonElement>(null);
  const wasExpanded = useRef(false);
  // An earlier version of the open Work item, shown read-only; `null` is the current one.
  const [shownRevision, setShownRevision] = useState<number | null>(null);
  const [restored, setRestored] = useState<{ from: number; to: number } | null>(null);
  const versionTriggerRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const [stageSize, setStageSize] = useState({ width: 0, height: 0 });
  const sideBounds = workspacePanelBounds("side", stageSize.width);
  const bottomBounds = workspacePanelBounds("bottom", stageSize.height);
  const renderedWidth = overlay ? width : Math.min(width, sideBounds.max);
  const renderedHeight = overlay ? height : Math.min(height, bottomBounds.max);
  const resizeBounds = placement === "side" ? sideBounds : bottomBounds;
  const openerRef = useRef<HTMLElement | null>(null);
  const previousOpenRef = useRef(false);

  const activeTab = useMemo(
    () => session.tabs.find((tab) => tab.id === session.active_tab_id),
    [session.active_tab_id, session.tabs],
  );
  useLayoutEffect(() => {
    const focusClose = takeFocusCloseAfterOpen();
    const opened = open && !previousOpenRef.current;
    if (!open && openerRef.current && !isWorkspaceFocusable(openerRef.current, document.body)) openerRef.current = null;
    if (opened) {
      const current = document.activeElement;
      const live = current instanceof HTMLElement && current !== document.body && !panelRef.current?.contains(current) ? current : null;
      // Only an opener that is still in this chat. A detached header button is not stored.
      openerRef.current = live && isWorkspaceFocusable(live, document.body) ? live : null;
      if (focusClose) focusForKeyboard(activeLayoutButtonRef.current);
    } else if (!open && previousOpenRef.current) {
      const opener = openerRef.current;
      openerRef.current = null;
      const current = document.activeElement;
      const focusAlreadyOutside = current instanceof HTMLElement && current !== document.body && current.isConnected && !panelRef.current?.contains(current) && isWorkspaceFocusable(current, document.body);
      if (!focusAlreadyOutside) restoreWorkspaceOpener(opener);
    } else if (open && openerRef.current && !isWorkspaceFocusable(openerRef.current, document.body)) {
      // The chat that held the opener was hidden. The next close uses the Panel button on screen.
      openerRef.current = null;
    }
    previousOpenRef.current = open;
  }, [open, overlay, activeTab?.id, threadId]);

  const activeBrowserTab =
    kind === "materials" && !chatMap?.active && activeTab?.kind === "browser" ? activeTab : undefined;
  const savedPassageIndex = useMemo(() => {
    const evidenceTab = session.tabs.find((tab): tab is EvidenceWorkspaceTab => tab.kind === "evidence");
    if (!activeBrowserTab || !evidenceTab) return -1;
    // Each browser tab keeps the citation that opened it, including after the page redirects.
    const bound = activeBrowserTab.evidence_key;
    if (bound) {
      const index = evidenceTab.entries.findIndex((entry) => entry.key === bound);
      if (index >= 0) return index;
    }
    if (!activeBrowserTab.url) return -1;
    const href = parseCredentialFreeHttpUrl(activeBrowserTab.url)?.href;
    if (!href) return -1;
    const current = evidenceTab.entries[evidenceTab.index];
    if (current?.kind === "evidence" && citationPageUrl(current.evidence) === href) return evidenceTab.index;
    for (let index = evidenceTab.entries.length - 1; index >= 0; index -= 1) {
      const entry = evidenceTab.entries[index];
      if (entry?.kind === "evidence" && citationPageUrl(entry.evidence) === href) return index;
    }
    return -1;
  }, [activeBrowserTab, session.tabs]);
  const viewSavedPassage = savedPassageIndex < 0 ? undefined : () => {
    onUpdateTab(EVIDENCE_TAB_ID, (tab) => tab.kind === "evidence" ? { ...tab, index: savedPassageIndex } : tab);
    onSelectTab(EVIDENCE_TAB_ID);
  };
  // The Work tab is fixed: it may be chosen before it holds an open item.
  const workChosen = session.active_tab_id === WORK_TAB_ID;
  // With nothing selected the panel shows the chat map, or the Evidence reader's empty state.
  const showMap = kind === "materials" && Boolean(chatMap && (chatMap.active || (!activeTab && !workChosen)));
  const workSelected = !showMap && workChosen;
  const work = workTab(session);
  const openWork = workSelected && work?.artifact && work.artifactThreadId ? { artifact: work.artifact, threadId: work.artifactThreadId } : undefined;
  const activeEvidenceTab: EvidenceWorkspaceTab | undefined = !showMap && activeTab?.kind === "evidence" ? activeTab : undefined;
  const evidenceSelected = !showMap && !workSelected && (!activeTab || activeTab.kind === "evidence");
  // Charts read best below the chat, everything else beside it (side-panel.md P7).
  const dockKind: DockKind = kind === "materials" && openWork?.artifact.kind === "chart" ? "chart" : "reading";
  // Only a change of kind while the panel is in use re-docks: the placement restored at start-up
  // and the user's own Dock choice are not overridden by the first render.
  const dockKindSeen = useRef<DockKind | null>(null);
  useEffect(() => {
    if (!open || overlay) return;
    const previous = dockKindSeen.current;
    dockKindSeen.current = dockKind;
    if (previous !== null && previous !== dockKind) onDockKind?.(dockKind);
  }, [dockKind, open, overlay]);
  const evidenceEntryShown = currentEvidenceEntry(activeEvidenceTab);
  const moveEvidence = (delta: -1 | 1) => {
    if (!activeEvidenceTab) return;
    const target = activeEvidenceTab.index + delta;
    if (target >= 0 && target < activeEvidenceTab.entries.length) onMoveEvidence?.(delta);
  };

  useEffect(() => {
    if (!open) { setMenuOpen(false); setActionsMenuOpen(false); }
  }, [open]);

  const { resizing, resizeWithKeyboard, startResize } = useWorkspacePanelResize(
    {
      placement,
      width: renderedWidth,
      height: renderedHeight,
      onWidthChange,
      onHeightChange,
    },
  );
  const browser = useNativeBrowserTabs({
    bridge,
    loupistId,
    threadId,
    open,
    session,
    activeTab: activeBrowserTab,
    menuOpen,
    placement,
    width: renderedWidth,
    height: renderedHeight,
    onUpdateTab,
    onOpenLink,
  });

  useLayoutEffect(() => {
    if (!open || overlay) return;
    const stage = panelRef.current?.closest<HTMLElement>(".app-stage");
    if (!stage) return;
    const fitPanel = () => {
      const rect = stage.getBoundingClientRect();
      // Fitting a smaller window is temporary; only a deliberate resize changes
      // the remembered preference, which returns when the window has room again.
      setStageSize(previous => previous.width === rect.width && previous.height === rect.height
        ? previous : { width: rect.width, height: rect.height });
    };
    fitPanel();
    const observer = new ResizeObserver(fitPanel);
    observer.observe(stage);
    return () => observer.disconnect();
  }, [open, overlay]);

  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented || menuOpen || actionsMenuOpen) return;
      // Esc leaves the expanded reading column first; the next one closes the panel.
      if (expanded) setExpanded(false);
      else onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [actionsMenuOpen, expanded, menuOpen, onClose, open]);

  // The expanded column covers the chat: the chat leaves the focus order, and focus moves between
  // "Back to chat" and Expand as the column opens and closes.
  useEffect(() => {
    if (!expanded) {
      if (wasExpanded.current) focusForKeyboard(expandButtonRef.current);
      wasExpanded.current = false;
      return;
    }
    wasExpanded.current = true;
    const background = panelRef.current?.closest(".app-stage")?.querySelector<HTMLElement>(".app-stage-main");
    const previousInert = Boolean(background?.inert);
    if (background) background.inert = true;
    // The menu closes after this effect and would drop focus. Move it once the menu is gone.
    focusForKeyboard(backToChatRef.current);
    queueMicrotask(() => focusForKeyboard(backToChatRef.current));
    return () => { if (background) background.inert = previousInert; };
  }, [expanded]);

  useEffect(() => {
    if (!open || !overlay) return;
    const panel = panelRef.current;
    const background = panel
      ?.closest(".app-stage")
      ?.querySelector<HTMLElement>(".app-stage-main");
    if (!panel || !background) return;

    const previousFocus = openerRef.current;
    const previousInert = Boolean(background.inert);
    background.inert = true;

    const focusableElements = () =>
      Array.from(
        panel.querySelectorAll<HTMLElement>(
          'button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), summary, [href], [tabindex]:not([tabindex="-1"])',
        ),
      ).filter((element) => isWorkspaceFocusable(element, panel));
    const trapFocus = (event: KeyboardEvent) => {
      if (event.key !== "Tab") return;
      const focusable = focusableElements();
      const first = focusable[0];
      const last = focusable.at(-1);
      if (!first || !last) {
        event.preventDefault();
        activeLayoutButtonRef.current?.focus();
        return;
      }
      const current = document.activeElement;
      if (event.shiftKey && (current === first || !panel.contains(current))) {
        event.preventDefault();
        last.focus();
      } else if (
        !event.shiftKey &&
        (current === last || !panel.contains(current))
      ) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", trapFocus);
    activeLayoutButtonRef.current?.focus();
    return () => {
      document.removeEventListener("keydown", trapFocus);
      background.inert = previousInert;
      const restoreFocus = () => {
        const current = document.activeElement;
        if (current instanceof HTMLElement && current !== document.body && current.isConnected && !panel.contains(current) && isWorkspaceFocusable(current, document.body)) return;
        restoreWorkspaceOpener(previousFocus);
      };
      restoreFocus();
      queueMicrotask(() => {
        if (document.activeElement === document.body) restoreFocus();
      });
      window.setTimeout(() => {
        if (document.activeElement === document.body) restoreFocus();
      }, 0);
    };
  }, [open, overlay]);

  const readArtifact = useCallback(
    async (artifactThreadId: string, artifactId: string, revision: number | null) => {
      const request = ++readRequest.current;
      setLoadingArtifact(true);
      setReadError(null);
      try {
        const next = await bridge.chatArtifactRead(loupistId, artifactThreadId, artifactId, revision ?? undefined);
        if (request === readRequest.current) setView(next);
      } catch (cause) {
        if (request === readRequest.current) {
          setView(null);
          setReadError(errorMessage(cause, t("chatWorkspacePanel.couldNotOpenThisItem")));
        }
      } finally {
        if (request === readRequest.current) setLoadingArtifact(false);
      }
    },
    [bridge, loupistId],
  );

  const openArtifactId = openWork?.artifact.artifact_id;
  const openArtifactThread = openWork?.threadId;
  // A newly published revision of the open item re-reads it in place.
  const openArtifactRevision = openWork?.artifact.revision;
  // Another item starts on its current version.
  useEffect(() => { setShownRevision(null); setRestored(null); }, [openArtifactId, openArtifactThread]);
  useEffect(() => {
    if (!openArtifactId || !openArtifactThread) {
      ++readRequest.current;
      setLoadingArtifact(false);
      setView(null);
      setReadError(null);
      return;
    }
    void readArtifact(openArtifactThread, openArtifactId, shownRevision);
    return () => { ++readRequest.current; };
  }, [openArtifactId, openArtifactThread, openArtifactRevision, shownRevision, readArtifact]);
  const showRevision = (revision: number | null) => { setRestored(null); setShownRevision(revision); };

  const openFullMap = (location: KnowledgeLocation) => { onClose(); onOpenKnowledgePage(location); };
  // Expand on the Map opens the full Map on the name selected here; any other tab becomes a reading column.
  const expand = () => {
    if (kind === "materials" && showMap && chatMap) {
      const [focal] = chatMap.highlightIds.size === 1 ? [...chatMap.highlightIds] : [];
      openFullMap({ loupistId: chatMap.loupistId, surface: "world", selection: focal ? { kind: "node", id: focal } : undefined });
      return;
    }
    expandOnClose.current = true;
    setExpanded(true);
  };
  useEffect(() => { if (showMap) setExpanded(false); }, [showMap]);
  const backToCurrent = () => {
    showRevision(null);
    requestAnimationFrame(() => focusForKeyboard(versionTriggerRef.current));
  };

  if (!open) return null;

  const evidencePanel = evidenceSelected ? (
    <div
      className="workspace-tabpanel workspace-evidence-tab"
      role="tabpanel"
      id={workspaceTabPanelId(EVIDENCE_TAB_ID)}
      aria-labelledby={workspaceTabButtonId(EVIDENCE_TAB_ID)}
    >
      {evidenceEntryShown && activeEvidenceTab ? <div className="workspace-status">
        <Button type="button" variant="ghost" size="icon-sm" aria-label={t("workspaceEvidence.back")} title={t("workspaceEvidence.back")} disabled={activeEvidenceTab.index <= 0} onClick={() => moveEvidence(-1)}><ChevronLeftIcon /></Button>
        <Button type="button" variant="ghost" size="icon-sm" aria-label={t("workspaceEvidence.forward")} title={t("workspaceEvidence.forward")} disabled={activeEvidenceTab.index >= activeEvidenceTab.entries.length - 1} onClick={() => moveEvidence(1)}><ChevronRightIcon /></Button>
        <span className="workspace-status-text">{evidenceStatus(evidenceEntryShown, loupistName)}</span>
      </div> : null}
      {!evidenceEntryShown ? <div className="artifact-empty workspace-empty"><p>{t("chatWorkspacePanel.evidenceEmpty")}</p></div>
      : evidenceEntryShown.kind === "evidence" ? <WorkspaceEvidence key={evidenceEntryShown.key} evidence={evidenceEntryShown.evidence} step={evidenceEntryShown.step} onRevealStep={onRevealProgress} onOpenLink={onOpenLink} />
      : <WorkspaceKnowledgeReader key={`${loupistId}:${evidenceEntryShown.key}`} bridge={bridge} location={evidenceEntryShown.location} onOpen={onOpenKnowledge} onOpenPage={onOpenKnowledgePage} onAsk={onAskKnowledge} onOpenLink={onOpenLink}
        onPatch={(location) => onUpdateTab(EVIDENCE_TAB_ID, (tab) => tab.kind === "evidence" ? updateCurrentEvidenceEntry(tab, (entry) => entry.kind === "knowledge" && entry.key === evidenceEntryShown.key ? { ...entry, location } : entry) : tab)} />}
    </div>
  ) : null;

  const workPanel = workSelected ? (
    <div
      className="workspace-tabpanel workspace-work-tab"
      role="tabpanel"
      id={workspaceTabPanelId(WORK_TAB_ID)}
      aria-labelledby={workspaceTabButtonId(WORK_TAB_ID)}
    >
      {openWork ? <>
        <div className="workspace-status">
          <Button type="button" variant="ghost" size="sm" onClick={() => onUpdateTab(WORK_TAB_ID, () => workWorkspaceTab())}><ChevronLeftIcon />{t("workList.allWork")}</Button>
          {/* The row stays while another version loads, so the version control keeps focus. */}
          {view && view.artifact.id === openWork.artifact.artifact_id && !readError ? <ArtifactStatus view={view} version={
            <WorkVersionMenu key={view.artifact.id} bridge={bridge} loupistId={loupistId} threadId={openWork.threadId} view={view} triggerRef={versionTriggerRef} onChoose={showRevision} />
          } /> : null}
        </div>
        {view && shownRevision !== null && !loadingArtifact && !readError ? <WorkVersionBar key={`${view.artifact.id}:${view.revision.revision}`} bridge={bridge} loupistId={loupistId} threadId={openWork.threadId} view={view}
          onBack={backToCurrent}
          onRestored={(next, from) => {
            setView(next);
            setShownRevision(null);
            setRestored({ from, to: next.revision.revision });
            // The panel's reference follows the new current revision; answers keep the ones they showed.
            onUpdateTab(WORK_TAB_ID, (tab) => tab.kind === "work" && tab.artifactThreadId ? workWorkspaceTab(chatArtifactRef(next), tab.artifactThreadId) : tab);
            requestAnimationFrame(() => focusForKeyboard(versionTriggerRef.current));
          }} /> : null}
        {restored && shownRevision === null ? <p className="work-version-restored" role="status" data-work-version-restored>
          {t("workVersions.restored", { from: formatNumber(restored.from), to: formatNumber(restored.to) })}
        </p> : null}
        {loadingArtifact ? (
          <div className="artifact-panel-state" role="status">
            <Spinner />
            <span>{t("chatWorkspacePanel.openingDocument")}</span>
          </div>
        ) : readError ? (
          <div className="artifact-panel-state" role="alert">
            <strong>{t(shownRevision === null ? "chatWorkspacePanel.couldNotOpenWorkspaceItem" : "workVersions.unavailable")}</strong>
            {shownRevision === null ? <p>{readError}</p> : null}
            <Button type="button" variant="outline" size="sm" onClick={() => void readArtifact(openWork.threadId, openWork.artifact.artifact_id, shownRevision)}>{t("chatWorkspacePanel.tryAgain")}</Button>
            {shownRevision !== null ? <Button type="button" variant="ghost" size="sm" onClick={() => showRevision(null)}>{t("workVersions.backToCurrent")}</Button> : null}
          </div>
        ) : view ? (
          <ArtifactContent view={view} theme={theme} onOpenLink={onOpenLink} onAsk={onAskArtifact} askDisabled={askArtifactDisabled} onExport={(request) => bridge.exportSave(request)} />
        ) : null}
      </> : <WorkspaceWorkList bridge={bridge} loupistId={loupistId} refreshKey={workRefreshKey} building={buildingWork}
        onOpen={async (item) => {
          // Read first: the reference the panel keeps carries the revision's digest.
          const opened = await bridge.chatArtifactRead(loupistId, item.artifact.chat_session_id, item.artifact.id);
          if (scopeLive.current) onOpenArtifact(chatArtifactRef(opened), item.artifact.chat_session_id);
        }} />}
    </div>
  ) : null;

  const tabPanel = evidencePanel ?? workPanel ?? (activeTab ? (
    <div
      className="workspace-tabpanel"
      role="tabpanel"
      id={workspaceTabPanelId(activeTab.id)}
      aria-labelledby={workspaceTabButtonId(activeTab.id)}
    >
      {activeTab.kind === "files" ? <WorkspaceFiles bridge={bridge} loupistId={loupistId} rootPath={workspacePath} path={activeTab.path} filePath={activeTab.filePath}
        onNavigate={(path, filePath) => onUpdateTab(activeTab.id, (tab) => tab.kind === "files" ? { ...tab, path, filePath } : tab)}
        onAsk={(file) => onAskFile?.(file)} onOpenLink={onOpenLink} />
      : activeBrowserTab ? (
        <BrowserWorkspaceView
          tab={activeBrowserTab}
          address={browser.address}
          viewportRef={browser.viewportRef}
          onAddressChange={browser.setAddress}
          onNavigate={(event) => void browser.navigate(event)}
          onAction={(action) => void browser.performAction(action)}
          onRetry={() => void browser.retry()}
          onOpenExternal={(url) => void browser.openExternal(url)}
          onViewSaved={viewSavedPassage}
        />
      ) : null}
    </div>
  ) : null);

  return (
    <>
      {open && overlay && <button
        type="button"
        className="artifact-panel-backdrop"
        aria-label={t("chatWorkspacePanel.closeChatWorkspace")}
        aria-hidden="true"
        tabIndex={-1}
        onClick={onClose}
      />}
      <aside
        ref={panelRef}
        id="workspace-panel"
        className="artifact-panel workspace-panel"
        data-placement={placement}
        data-panel-kind={kind}
        data-expanded={expanded || undefined}
        data-resizing={resizing || undefined}
        hidden={!open}
        inert={!open}
        aria-hidden={!open || undefined}
        style={
          {
            "--artifact-panel-width": `${renderedWidth}px`,
            "--artifact-panel-height": `${renderedHeight}px`,
            display: open ? undefined : "none",
          } as CSSProperties
        }
        aria-label={t("chatWorkspacePanel.chatWorkspacePanel")}
        aria-modal={overlay || undefined}
        role={overlay ? "dialog" : "complementary"}
        onPointerDownCapture={onInteract}
        onKeyDownCapture={(event) => {
          onInteract?.();
          if (!event.altKey || event.metaKey || event.ctrlKey || !evidenceEntryShown || isTextEntry(event.target)) return;
          if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
          event.preventDefault();
          moveEvidence(event.key === "ArrowLeft" ? -1 : 1);
        }}
      >
        <div
          className="artifact-resize-handle"
          role="separator"
          aria-label={t("chatWorkspacePanel.resizeChatWorkspace")}
          aria-orientation={placement === "side" ? "vertical" : "horizontal"}
          aria-valuemin={resizeBounds.min}
          aria-valuemax={resizeBounds.max}
          aria-valuenow={placement === "side" ? renderedWidth : renderedHeight}
          tabIndex={0}
          onPointerDown={startResize}
          onKeyDown={resizeWithKeyboard}
        />
        {kind !== "materials" ? <div className="workspace-kind-heading"><strong>{kind === "lens" ? "Lens" : "World"}</strong><div className="workspace-tab-controls">{overlay ? null : <Button ref={expandButtonRef} type="button" variant="ghost" size="icon-sm" aria-label={t("chatWorkspacePanel.expand")} title={t("chatWorkspacePanel.expand")} onClick={expand}><Maximize2Icon /></Button>}<Button ref={activeLayoutButtonRef} type="button" variant="ghost" size="icon-sm" data-workspace-close aria-label={t("chatWorkspacePanel.closeWorkspace")} title={t("chatWorkspacePanel.closeWorkspace")} onClick={onClose}><XIcon /></Button></div></div> : <WorkspaceTabBar
          session={session}
          activeTab={activeTab}
          menuOpen={menuOpen}
          onMenuOpenChange={setMenuOpen}
          onSelectTab={onSelectTab}
          onCloseTab={onCloseTab}
          onNewBrowser={onNewBrowser}
          onOpenFiles={onOpenFiles}
          layoutControls={<>{overlay ? null : <DropdownMenu open={actionsMenuOpen} onOpenChange={setActionsMenuOpen}>
            <DropdownMenuTrigger asChild><Button ref={expandButtonRef} type="button" variant="ghost" size="icon-sm" className="workspace-dock-options" aria-label={t("chatWorkspacePanel.panelActions")} title={t("chatWorkspacePanel.panelActions")}><MoreHorizontalIcon aria-hidden="true" /></Button></DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="workspace-dock-menu" onCloseAutoFocus={(event) => { if (expandOnClose.current) { expandOnClose.current = false; event.preventDefault(); } }}>
              <DropdownMenuItem data-workspace-expand onSelect={expand}><Maximize2Icon aria-hidden="true" />{t("chatWorkspacePanel.expand")}</DropdownMenuItem>
              <DropdownMenuItem onSelect={() => onPlacementToggle(placement === "side" ? "bottom" : "side", dockKind)}>{placement === "side" ? <PanelBottomIcon aria-hidden="true" /> : <PanelRightIcon aria-hidden="true" />}{t(placement === "side" ? "chatWorkspacePanel.dockBottom" : "chatWorkspacePanel.dockSide")}</DropdownMenuItem>
              {onPinPlacement ? <><DropdownMenuSeparator /><DropdownMenuCheckboxItem checked={placementPinned} onCheckedChange={(checked) => onPinPlacement(checked === true)}>{t("chatWorkspacePanel.alwaysDockHere")}</DropdownMenuCheckboxItem></> : null}
            </DropdownMenuContent>
          </DropdownMenu>}<Button ref={activeLayoutButtonRef} type="button" variant="ghost" size="icon-sm" data-workspace-close aria-label={t("chatWorkspacePanel.closeWorkspace")} title={t("chatWorkspacePanel.closeWorkspace")} onClick={onClose}><XIcon aria-hidden="true" /></Button></>}
        />}
        {expanded ? <div className="workspace-expanded-bar">
          <Button ref={backToChatRef} type="button" variant="ghost" onClick={() => setExpanded(false)}><ArrowLeftIcon aria-hidden="true" />{t("chatWorkspacePanel.backToChat")}</Button>
        </div> : null}
        <div className="workspace-kind-content" hidden={kind === "materials"} inert={kind === "materials" || undefined}>{children}</div>
        <div className="workspace-materials-content" hidden={kind !== "materials"} inert={kind !== "materials" || undefined}>
        {open ? showMap ? <div className="workspace-tabpanel" role="tabpanel" id={workspaceTabPanelId(CHAT_MAP_TAB_ID)} aria-labelledby={workspaceTabButtonId(CHAT_MAP_TAB_ID)}><ChatMiniMap onOpenFull={openFullMap} /></div>
          // One wrapper in both modes, so expanding keeps the reader's state and scroll position.
          : <div className="workspace-reading-column">{tabPanel}</div> : null}
        </div>
      </aside>
    </>
  );
}

/** Close returns to the opener when it is still in this chat, otherwise the Panel button now on screen. */
function restoreWorkspaceOpener(opener: HTMLElement | null) {
  if (opener?.isConnected && isWorkspaceFocusable(opener, document.body)) {
    opener.focus({ preventScroll: true });
    return;
  }
  const panelButton = document.querySelector<HTMLElement>(".header-panel-toggle");
  if (panelButton && isWorkspaceFocusable(panelButton, document.body)) panelButton.focus({ preventScroll: true });
}

/** Closed disclosures and retained inactive tabs must not enter the overlay's focus loop. */
function isWorkspaceFocusable(element: HTMLElement, panel: HTMLElement) {
  if (element.matches('.artifact-resize-handle, [tabindex="-1"], :disabled')) return false;
  for (let current: HTMLElement | null = element; current; current = current.parentElement) {
    if (current.hidden || current.inert || current.hasAttribute("inert") || current.getAttribute("aria-hidden") === "true") return false;
    const style = window.getComputedStyle(current);
    if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse") return false;
    if (current instanceof HTMLDetailsElement && !current.open) {
      const summary = [...current.children].find((child) => child.tagName === "SUMMARY");
      if (!summary?.contains(element)) return false;
    }
    if (current === panel) break;
  }
  return true;
}
