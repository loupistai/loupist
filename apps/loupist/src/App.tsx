import { attachmentDataUrl } from "./components/ai-elements/prompt-input";
import { chatTitle as displayThreadTitle } from "./app/chatTitle";
import { LaunchNotice } from "./components/onboarding/Notice";
import { useInboxCounts } from "./components/briefings/useInboxCounts";
import { useFollowedMarkets } from "./app/useFollowedMarkets";
import { LearningToasts, useLearningNotifications } from "./components/map/LearningToast";
import { Onboarding } from "./components/onboarding/Onboarding";
import i18n, { Trans, useTranslation } from "@/i18n";
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type Dispatch,
  type SetStateAction,
} from "react";
import { createLoupistBridge } from "./bridge/tauriBridge";
import { ChatView, type ChatWorkspaceActions } from "./components/ChatView";
import { AppHeader } from "./components/app/AppHeader";
import { LoupistAvailabilityBoundary } from "./components/app/LoupistAvailabilityBoundary";
import { AppNavigation } from "./components/app/AppNavigation";
import { MacTitlebarControls } from "./components/app/MacTitlebarControls";
import { workspaceOverlayMedia } from "./components/workspace/useWorkspacePanelResize";
import { ResearchReader } from "./components/knowledge/ResearchReader";
import { Dialog, DialogContent, DialogTitle } from "./components/ui/dialog";
import { Button } from "./components/ui/button";
import { ChatMapProvider } from "./components/workspace/ChatMapContext";
import { OpenEvidenceProvider } from "./components/chat/OpenEvidence";
import { ChatWorkspacePanel } from "./components/workspace/ChatWorkspacePanel";
import { ViewBoundary } from "./components/app/ViewBoundary";
import { useAppUpdate } from "./components/settings/useAppUpdate";
import { SidebarInset, SidebarProvider } from "@/components/ui/sidebar";
import { Spinner } from "@/components/ui/spinner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SourceFaviconProvider } from "./components/chat/SourceFavicon";
import type {
  ChatThread,
  LoupistBridge,
  ModelRunSelection,
  KnowledgePin,
  KnowledgeSourceSelectionRequest,
  KnowledgeLocation,
  KnowledgeMapNode,
  GraphCamera,
  ConfigScope,
} from "./types";
import { resolveConfiguredModelRunSelection } from "./lib/modelSelection";
import { quoteChip } from "./chat/composerChips";
import { chartBlocks, gridBlocks } from "./chat/chartBlocks";
import { QuoteAsk } from "./components/chat/QuoteAsk";
import { useAttention, useFinishedWhileHidden, splitChatKey } from "./app/useAttention";
import type { AttentionItem } from "./contracts/attention";
import { notifyConfigurationChanged, useConfiguration } from "./app/useConfiguration";
import { useLanguage } from "./i18n/useLanguage";
import { useTheme } from "./app/useTheme";
import { APP_TAB_LABELS, type AppTab, type SettingsSection } from "./app/navigation";
import { chatSessionKey } from "./chat/sessionRegistry";
import { emptyWorkspaceSession } from "./chat/workspace";
import { EMPTY_CHAT_THREAD_ID, isEmptyChatThreadId } from "./app/emptyChat";
import { AppDialogs, useAppDialogs } from "./app/AppDialogs";
import { useChatSessions } from "./app/useChatSessions";
import { sameKnowledgeTarget } from "./knowledge/location";
import { useKnowledgeNavigation } from "./app/useKnowledgeNavigation";
import { useLoupistDirectory } from "./app/useLoupistDirectory";
import { useModelCatalog } from "./app/useModelCatalog";
import { useThreadActions } from "./app/useThreadActions";
import { useWorkspacePanel } from "./app/useWorkspacePanel";
import { useAppShortcuts } from "./app/useAppShortcuts";
import { shortcutFor, tabShortcut } from "./app/shortcuts";
import { chatMarkdown, exportFileName, utf8Bytes } from "./chat/exportChat";
import { printChat } from "./components/print/printDocuments";
import { errorMessage } from "./lib/errors";
import { CommandPalette, type PaletteAction, type PaletteGroup } from "./components/app/CommandPalette";
import { usePersistedChatDraft } from "./app/usePersistedChatDraft";
import { useChatDrafts } from "./app/useChatDrafts";
import { useFileDropGuard } from "./app/useFileDropGuard";
import { useScheduledRuns } from "./app/useScheduledRuns";
import { useLoupistVisit } from "./app/loupistVisit";
import type { ConnectSheetTarget } from "./components/settings/ConnectSheet";
import type { AssetPageTarget } from "./contracts/assets";
import { AssetPageOpenerProvider } from "./components/assets/assetOpener";
import { LookBackProvider, lookBackDraft, type LookBackEntry } from "./components/knowledge/lookBack";
import { FixMemoryProvider } from "./components/memory/fixMemory";
import { SHARED_SECTION_ITEM } from "./components/sharing/SharedPacks";
import { PackLinkDialog } from "./components/sharing/PackLinkDialog";
import { DeepLinkConfirm, useDeepLinks } from "./components/sharing/DeepLinkConfirm";
import type { PackAddResult, PackOpenResult } from "./contracts/sharing";

import type { LoupistSettingsSection } from "./components/agents/LoupistSettings";
const LoupistSettings = lazy(() => import("./components/agents/LoupistSettings").then(module => ({ default: module.LoupistSettings })));
const MemoryPage = lazy(() => import("./components/memory/MemoryPage").then(module => ({ default: module.MemoryPage })));
const AssetPage = lazy(() => import("./components/assets/AssetPage").then(module => ({ default: module.AssetPage })));
const MemoryWorkspace = lazy(() => import("./components/knowledge/MemoryWorkspace").then(module => ({ default: module.MemoryWorkspace })));
const SettingsView = lazy(() => import("./components/SettingsView").then(module => ({ default: module.SettingsView })));
const ScheduleView = lazy(() => import("./components/briefings/ScheduleView").then(module => ({ default: module.ScheduleView })));
const PackReview = lazy(() => import("./components/sharing/PackReview").then(module => ({ default: module.PackReview })));
const Gallery = lazy(() => import("./components/sharing/Gallery").then(module => ({ default: module.Gallery })));

export function App(props: { bridge?: LoupistBridge }) {
  return <ViewBoundary name="Loupist" root><AppContent {...props} /></ViewBoundary>;
}

function AppContent({ bridge: providedBridge }: { bridge?: LoupistBridge }) {
  useTranslation();
  useFileDropGuard();
  const [bridge] = useState(() => providedBridge ?? createLoupistBridge());
  const appUpdate = useAppUpdate(bridge);
  const workspace = useWorkspacePanel(bridge);
  const legacyPanel = useRef(localStorage.getItem("loupist.lastView"));
  useEffect(() => {
    if (!workspace.hydrated) return;
    const previous = legacyPanel.current;
    legacyPanel.current = null;
    if (previous === "map" || previous === "lens") {
      workspace.selectKind(previous === "map" ? "world" : "lens");
      localStorage.setItem("loupist.lastView", "chat");
    }
  }, [workspace.hydrated, workspace.selectKind]);
  const [loupistSettingsOpen, setLoupistSettingsOpen] = useState(false);
  const [tab, setTabState] = useState<AppTab>(() => {
    const last = localStorage.getItem("loupist.lastView");
    return last === "schedule" ? last : "chat";
  });
  const surfaceNavigationEpoch = useRef(0);
  const setTab = useCallback((nextTab: AppTab) => {
    setLoupistSettingsOpen(false);
    surfaceNavigationEpoch.current += 1;
    if (nextTab === "map" || nextTab === "lens") {
      workspace.selectKind(nextTab === "map" ? "world" : "lens");
      localStorage.setItem("loupist.lastView", "chat");
      setTabState("chat");
      return;
    }
    if (nextTab !== "settings") localStorage.setItem("loupist.lastView", nextTab);
    setTabState(nextTab);
  }, [workspace.selectKind]);
  const [noteTrail, setNoteTrail] = useState<KnowledgeLocation[]>([]);
  const [noteReaderOpen, setNoteReaderOpen] = useState(false);
  const [settingsSection, setSettingsSection] =
    useState<SettingsSection>("loupists");
  const [loupistSettingsTargetId, setLoupistSettingsTargetId] = useState<string | null>(null);
  const [loupistSettingsSection, setLoupistSettingsSection] = useState<LoupistSettingsSection | undefined>();
  const loupistSettingsReturnFocus = useRef<HTMLElement | null>(null);
  /** "What {Loupist} knows" over the current tab, optionally opened on one principle or method. */
  const [memoryPage, setMemoryPage] = useState<{ loupistId: string; item?: string; key: number } | null>(null);
  /** One followed asset's page over the current tab (W16.1). */
  const [assetPage, setAssetPage] = useState<(AssetPageTarget & { key: number }) | null>(null);
  const [removeLoupistOpen, setRemoveLoupistOpen] = useState(false);
  const [marketplaceEntryId, setMarketplaceEntryId] = useState<string | null>(null);
  /** A connect sheet a Chat error asked for; Settings › Models opens it once. */
  const [modelConnectTarget, setModelConnectTarget] = useState<ConnectSheetTarget | null>(null);
  const {
    preference: theme,
    resolved: resolvedTheme,
    setPreference: setTheme,
    configuration: appearanceConfiguration,
  } = useTheme(bridge);
  const { preference: language, setPreference: setLanguage } = useLanguage(appearanceConfiguration);
  const settingsReturnLoupistRef = useRef<string | null>(null);
  const settingsReturnTabRef = useRef<AppTab>("chat");
  const drafts = useChatDrafts();
  const pendingComposerFocus = useRef<string | null>(null);
  const pendingBriefingMessage = useRef<{ loupistId: string; threadId: string; messageId: string; epoch: number } | null>(null);
  const [knowledgeRefresh, setKnowledgeRefresh] = useState(0);
  const refreshKnowledge = useCallback(() => setKnowledgeRefresh((value) => value + 1), []);

  const dialogs = useAppDialogs();
  const model = useModelCatalog(bridge);
  const chat = useChatSessions(bridge);
  const loupist = useLoupistDirectory(bridge, {
    chat,
    removeLoupistSessions: workspace.removeLoupistSessions,
    closeWorkspace: workspace.close,
    onSelectionStarted: dialogs.closeThreadDialogs,
    onFolderReconnected: () => { refreshKnowledge(); notifyConfigurationChanged(); },
  });
  const threadActions = useThreadActions(bridge, { chat, loupist, workspace });
  chat.handlersRef.current = {
    publishArtifact: workspace.publishArtifact,
    refreshLoupistAvailability: loupist.refreshLoupistAvailability,
    applyRecoveredLoupist: loupist.applyRecoveredLoupist,
    onMemoryChanged: refreshKnowledge,
  };
  const { loupists, selectedLoupist } = loupist;
  const [onboardingActive, setOnboardingActive] = useState(false);
  const [noticeAcknowledgedVersion, setNoticeAcknowledgedVersion] = useState<number | null>(null);
  useEffect(() => {
    let active = true;
    void bridge.noticeSettingsGet().then(settings => {
      if (active) setNoticeAcknowledgedVersion(settings.notice_acknowledged_version);
    }).catch(() => { if (active) setNoticeAcknowledgedVersion(0); });
    return () => { active = false; };
  }, [bridge]);
  // A new profile has a default Loupist before onboarding finishes. After the
  // notice has been accepted, losing the last AI account must not hide saved research.
  const needsOnboarding = model.resolved && noticeAcknowledgedVersion !== null && !loupist.loading && !loupist.error
    && (loupists.length === 0 || (model.needsProviderSetup && noticeAcknowledgedVersion === 0));
  useEffect(() => { if (needsOnboarding) setOnboardingActive(true); }, [needsOnboarding]);
  const finishOnboarding = useCallback(async () => {
    // A failed save leaves the shell notice gate active, where the user can retry.
    await bridge.noticeAcknowledge().then(settings => setNoticeAcknowledgedVersion(settings.notice_acknowledged_version)).catch(() => {});
    setOnboardingActive(false);
    if (selectedLoupist) chat.setActiveThreadForLoupist(selectedLoupist.id, `home-${selectedLoupist.id}`);
    setTab("chat");
  }, [bridge, setTab, selectedLoupist, chat.setActiveThreadForLoupist]);
  useEffect(() => { if (tab !== "settings") settingsReturnLoupistRef.current = selectedLoupist?.id ?? null; }, [tab, selectedLoupist?.id]);
  useEffect(() => {
    if (loupistSettingsOpen && loupistSettingsTargetId && !loupists.some(item => item.id === loupistSettingsTargetId)) setLoupistSettingsOpen(false);
  }, [loupists, loupistSettingsOpen, loupistSettingsTargetId]);
  const openLoupistSettingsFor = async (id: string, remove = false, section?: LoupistSettingsSection) => {
    loupistSettingsReturnFocus.current = document.activeElement?.closest("[role=menu]") ? document.querySelector(".loupist-header-identity") : document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (id !== selectedLoupist?.id || loupist.loading) {
      const opened = await loupist.selectLoupist(id, loupists.find(item => item.id === id));
      if (!opened) return;
    }
    setLoupistSettingsTargetId(id);
    setLoupistSettingsSection(section);
    setRemoveLoupistOpen(remove);
    setLoupistSettingsOpen(true);
  };
  const skillDraftLoupist = selectedLoupist;
  const configuration = useConfiguration(bridge, selectedLoupist ? { kind: "project", loupistId: selectedLoupist.id } : { kind: "global" });

  const draftWrites = useRef(Promise.resolve());
  const fileWrites = useRef(Promise.resolve());
  const createThread = useCallback(
    async () => {
      if (!selectedLoupist) return null;
      const created = await threadActions.createThreadForLoupist(selectedLoupist.id);
      if (created) {
        fileWrites.current = fileWrites.current.then(() => bridge.chatDraftFilesWrite({ loupist_id: selectedLoupist.id, thread_id: EMPTY_CHAT_THREAD_ID }, [])).catch(() => {});
        drafts.adopt(chatSessionKey(selectedLoupist.id, EMPTY_CHAT_THREAD_ID), chatSessionKey(selectedLoupist.id, created.id));
        draftWrites.current = draftWrites.current.then(() => bridge.chatDraftWrite({ loupist_id: selectedLoupist.id, thread_id: EMPTY_CHAT_THREAD_ID }, { prompt: "" })).catch(() => {});
      }
      return created;
    },
    [threadActions.createThreadForLoupist, selectedLoupist, drafts.adopt, bridge],
  );

  /** Opens app-wide Settings and remembers where to return. */
  const openSettings = useCallback((section?: SettingsSection, target: { entryId?: string | null } = {}) => {
    if (section) setSettingsSection(section);
    if (target.entryId !== undefined) setMarketplaceEntryId(target.entryId);
    setLoupistSettingsOpen(false);
    if (tab !== "settings") settingsReturnTabRef.current = tab;
    setTab("settings");
  }, [tab, setTab]);
  const openModelSettings = useCallback((target?: ConnectSheetTarget) => {
    setModelConnectTarget(target ?? null);
    openSettings("model");
  }, [openSettings]);
  const consumeModelConnectTarget = useCallback(() => setModelConnectTarget(null), []);
  const [briefingToast, setBriefingToast] = useState<string | null>(null);
  useEffect(() => { if (briefingToast) { const timer = setTimeout(() => setBriefingToast(null), 8000); return () => clearTimeout(timer); } }, [briefingToast]);
  /** Sharing (W19.4, W19.8): the link/file dialog, the review page and held deep links. */
  const [packLinkOpen, setPackLinkOpen] = useState(false);
  const [packReview, setPackReview] = useState<PackOpenResult | null>(null);
  /** W19.6: the community Gallery page, and Use from a card (the review focuses Add). */
  const [galleryOpen, setGalleryOpen] = useState(false);
  const [packUse, setPackUse] = useState(false);
  const [indexConfigured, setIndexConfigured] = useState(false);
  useEffect(() => { void bridge.githubAccount().then(account => setIndexConfigured(account.indexConfigured)).catch(() => undefined); }, [bridge]);
  const deepLinks = useDeepLinks(bridge, model.resolved && !loupist.loading && !loupist.error && !onboardingActive && !needsOnboarding, setBriefingToast);
  // The morning message reads "last seen" per Loupist; leaving a Loupist (or Settings) records the visit.
  useLoupistVisit(selectedLoupist?.id ?? null, tab !== "settings" && !onboardingActive);
  const inboxCounts = useInboxCounts(bridge, loupists.map(loupist => loupist.id).join(","));
  const [inboxFocus, setInboxFocus] = useState<{ loupistId: string; runId: string | null } | null>(null);
  useEffect(() => bridge.onInboxOpen(event => {
    void (async () => {
      if (event.loupistId !== selectedLoupist?.id && !await loupist.selectLoupist(event.loupistId, loupists.find(item => item.id === event.loupistId))) return;
      setInboxFocus(event); setTab("schedule");
    })();
  }), [bridge, loupist.selectLoupist, loupists, selectedLoupist?.id]);
  const [scheduledFocus, setScheduledFocus] = useState<{ loupistId: string; taskId: string } | null>(null);
  const showChat = useCallback(() => {
    setMemoryPage(null);
    setAssetPage(null);
    setGalleryOpen(false);
    setPackReview(null);
    setNoteReaderOpen(false);
    setTab("chat");
  }, [setTab]);
  const startChatFor = useCallback((loupistId: string) => {
    pendingComposerFocus.current = chatSessionKey(loupistId, EMPTY_CHAT_THREAD_ID);
    showChat();
    void threadActions.openDraftChatForLoupist(loupistId);
  }, [showChat, threadActions.openDraftChatForLoupist]);
  const startNewChat = useCallback(() => { if (selectedLoupist) startChatFor(selectedLoupist.id); }, [selectedLoupist, startChatFor]);
  useEffect(() => bridge.onMenuAction(action => {
    if (action === "settings") openSettings();
    else if (action === "new-chat") startNewChat();
    else if (action === "new-loupist") dialogs.openCreateLoupist();
    else if (action === "chat" || action === "map" || action === "lens" || action === "schedule") {
      setMemoryPage(null); setAssetPage(null); setTab(action);
    }
    else if (action === "check-updates") openSettings("updates");
    else if (action === "toggle-sidebar") document.querySelector<HTMLButtonElement>("[data-sidebar='trigger']")?.click();
  }), [bridge, dialogs, openSettings, startNewChat, setTab]);
  // ⌘K and the sidebar search open the command palette (X7); "{n} more" opens it on what needs attention.
  const [palette, setPalette] = useState<{ open: boolean; focus?: PaletteGroup }>({ open: false });
  const openPalette = useCallback(() => setPalette({ open: true }), []);
  useAppShortcuts({ onSettings: openSettings, onNewChat: startNewChat, onPalette: openPalette,
    canCreateChat: !loupist.loading && selectedLoupist?.availability.status === "available" });

  const modelSelection = resolveConfiguredModelRunSelection(
    model.catalog,
    selectedLoupist ? model.selections[selectedLoupist.id] : undefined,
    configuration.snapshot?.effective.chat,
    selectedLoupist?.last_model_profile_id,
  );

  const selectedLoupistId = selectedLoupist?.id ?? null;
  const followedMarkets = useFollowedMarkets(bridge, selectedLoupistId);
  const scheduled = useScheduledRuns(bridge, selectedLoupistId, chat);
  const memory = useKnowledgeNavigation(selectedLoupistId, workspace.open && workspace.kind === "world");
  const lens = useKnowledgeNavigation(selectedLoupistId, workspace.open && workspace.kind === "lens");
  const noteLocation = noteReaderOpen && noteTrail.at(-1)?.loupistId === selectedLoupistId ? noteTrail.at(-1) : undefined;
  useEffect(() => { setNoteTrail([]); setNoteReaderOpen(false); }, [selectedLoupistId]);
  useEffect(() => { setAssetPage(current => current && current.loupistId !== selectedLoupistId ? null : current); }, [selectedLoupistId]);
  useEffect(() => { setMemoryPage(current => current && current.loupistId !== selectedLoupistId ? null : current); }, [selectedLoupistId]);
  const assetPageOpen = Boolean(assetPage && selectedLoupist && assetPage.loupistId === selectedLoupist.id && tab !== "settings");
  const chatVisible = tab === "chat" && !loupistSettingsOpen && !assetPageOpen && !galleryOpen && !packReview;
  const openAssetPage = useCallback((target: AssetPageTarget) => {
    if (target.loupistId !== selectedLoupistId) return;
    setLoupistSettingsOpen(false);
    setMemoryPage(null);
    setAssetPage({ ...target, key: Date.now() });
  }, [selectedLoupistId]);
  const openPackReview = useCallback((result: PackOpenResult, use = false) => {
    setPackLinkOpen(false);
    deepLinks.clear();
    setMemoryPage(null);
    setPackUse(use);
    setPackReview(result);
  }, [deepLinks.clear]);
  /** A shared Loupist was added: that Loupist opens on "What {Loupist} knows" with its suggestions on top. */
  const landSharedLoupist = useCallback(async ({ loupistId }: PackAddResult) => {
    setPackReview(null);
    setGalleryOpen(false);
    if (loupistId !== selectedLoupistId) {
      const added = (await bridge.loupistList().catch(() => [])).find(item => item.id === loupistId);
      const opened = added ? await loupist.adoptLoupist(added) : await loupist.selectLoupist(loupistId);
      if (!opened) return;
    }
    setTab("lens");
    setMemoryPage(current => ({ loupistId, key: (current?.key ?? 0) + 1 }));
    refreshKnowledge();
  }, [bridge, loupist.adoptLoupist, loupist.selectLoupist, selectedLoupistId, tab, setTab, refreshKnowledge]);
  const openWhatItKnows = useCallback((item?: string) => {
    if (!selectedLoupistId) return;
    setLoupistSettingsOpen(false);
    setAssetPage(null);
    setGalleryOpen(false);
    setPackReview(null);
    setMemoryPage(current => ({ loupistId: selectedLoupistId, item, key: (current?.key ?? 0) + 1 }));
    setTab("lens");
  }, [selectedLoupistId, setTab]);
  const changeSelectedModel = useCallback(
    (selection: ModelRunSelection) => {
      if (selectedLoupistId) model.changeSelection(selectedLoupistId, selection);
    },
    [model.changeSelection, selectedLoupistId],
  );
  const recordSelectedModel = useCallback(
    (selection: ModelRunSelection) => {
      // An Auto send names no model; Rust records what it resolved in the answer.
      if (!selectedLoupistId || !selection.model_profile_id) return;
      loupist.recordLastModel(selectedLoupistId, selection.model_profile_id);
    },
    [loupist.recordLastModel, selectedLoupistId],
  );
  const threads = (
    selectedLoupistId ? (chat.threadsByLoupist[selectedLoupistId] ?? []) : []
  ).filter((thread) => !isEmptyChatThreadId(thread.id));
  const activeThreadId = selectedLoupistId
    ? (chat.activeThreadIds[selectedLoupistId] ?? null)
    : null;
  const effectiveThreadId = isEmptyChatThreadId(activeThreadId)
    ? EMPTY_CHAT_THREAD_ID
    : (activeThreadId ?? threads.find((thread) => thread.home)?.id ?? threads[0]?.id ?? null);
  const persistedThreadId = isEmptyChatThreadId(effectiveThreadId)
    ? null
    : effectiveThreadId;
  const activeChat = selectedLoupistId
    ? chat.chatRegistry.get(
        selectedLoupistId,
        effectiveThreadId ?? EMPTY_CHAT_THREAD_ID,
      )
    : undefined;
  const [progressFocus, setProgressFocus] = useState<string | null>(null);
  const workspaceThreadId =
    effectiveThreadId ?? (activeChat ? EMPTY_CHAT_THREAD_ID : null);
  const effectiveChatKey =
    selectedLoupistId && workspaceThreadId
      ? chatSessionKey(selectedLoupistId, workspaceThreadId)
      : null;
  // A handoff is complete when its Chat is committed, even if the native
  // webview is not producing animation frames. Keep the request until it mounts.
  useLayoutEffect(() => {
    if (tab !== "chat" || !effectiveChatKey || pendingComposerFocus.current !== effectiveChatKey) return;
    const input = document.querySelector<HTMLTextAreaElement>('#main-panel-chat textarea[data-chat-composer]');
    if (!input || input.closest("[hidden], [inert]")) return;
    input.focus({ preventScroll: true });
    if (document.activeElement === input) pendingComposerFocus.current = null;
  });
  useLayoutEffect(() => {
    const target = pendingBriefingMessage.current;
    if (!target) return;
    if (target.epoch !== surfaceNavigationEpoch.current) { pendingBriefingMessage.current = null; return; }
    if (tab !== "chat" || selectedLoupistId !== target.loupistId || effectiveThreadId !== target.threadId) return;
    const message = document.querySelector<HTMLElement>(`#main-panel-chat [data-message-id="${CSS.escape(target.messageId)}"]`);
    if (message) { message.scrollIntoView({ block: "center" }); pendingBriefingMessage.current = null; }
  });
  const activeDraft = effectiveChatKey ? drafts.get(effectiveChatKey) : null;

  useEffect(() => {
    if (!activeDraft || !selectedLoupistId || !effectiveThreadId) return;
    const identity = { loupist_id: selectedLoupistId, thread_id: effectiveThreadId };
    const initialFiles = activeDraft.files;
    let current = true;
    if (!initialFiles.length) void fileWrites.current.then(() => bridge.chatDraftFilesRead(identity)).then(files => {
      if (current && activeDraft.files === initialFiles && files.length) {
        activeDraft.files = files.map(file => ({ ...file, type: "file" as const }));
        drafts.refresh();
      }
    }).catch(() => {});
    activeDraft.onFilesChanged = files => {
      // Start reading object URLs while the tray still owns them. Commit snapshots in edit order.
      const snapshot = Promise.all(files.map(async file => {
        const url = await attachmentDataUrl(file);
        if (!url) throw new Error("draft_attachment_unreadable");
        return { id: file.id, filename: file.filename ?? "attachment", mediaType: file.mediaType, url };
      }));
      const outcome = snapshot.then(value => ({ value }), () => ({ value: null }));
      fileWrites.current = fileWrites.current.then(async () => {
        const { value } = await outcome;
        if (!value) throw new Error("draft_attachment_unreadable");
        await bridge.chatDraftFilesWrite(identity, value);
      }).catch(() => { if (current) setBriefingToast(i18n.t("chat:messageSchedules.draftFilesFailed")); });
    };
    return () => { current = false; delete activeDraft.onFilesChanged; };
  }, [bridge, activeDraft, selectedLoupistId, effectiveThreadId, drafts.refresh]);

  usePersistedChatDraft(bridge, activeDraft, effectiveChatKey, selectedLoupistId, effectiveThreadId,
    effectiveChatKey ? workspace.selectedArtifacts[effectiveChatKey] : undefined, workspace.selectArtifactForChat,
    drafts.refresh, draftWrites);
  useEffect(() => {
    if (selectedLoupistId && effectiveThreadId && !loupist.loading) {
      localStorage.setItem("loupist.lastConversation", JSON.stringify([selectedLoupistId, effectiveThreadId]));
    }
  }, [selectedLoupistId, effectiveThreadId, loupist.loading]);

  const quoteReaderSelection = useCallback(async (sourceId: string, text: string, retained?: KnowledgeSourceSelectionRequest) => {
    if (!activeDraft) return;
    const epoch = surfaceNavigationEpoch.current;
    if (retained) {
      if (retained.source.loupistId !== selectedLoupistId) return;
      const pin = await bridge.knowledgeSourceSelectionResolve(retained);
      if (epoch !== surfaceNavigationEpoch.current) return;
      drafts.addPin(activeDraft, pin);
    } else {
      const chip = quoteChip(sourceId, text);
      activeDraft.chips = [...(activeDraft.chips ?? []).filter((existing) => existing.key !== chip.key), chip];
    }
    if (window.matchMedia(workspaceOverlayMedia).matches) workspace.close();
    pendingComposerFocus.current = effectiveChatKey;
    drafts.refresh();
    // The overlay restores its opener during cleanup, after layout effects.
    // Complete this explicit handoff once that cleanup has finished.
    requestAnimationFrame(() => {
      if (epoch === surfaceNavigationEpoch.current) document.querySelector<HTMLTextAreaElement>("#main-panel-chat [data-chat-composer]")?.focus({ preventScroll: true });
    });
  }, [activeDraft, drafts, bridge, selectedLoupistId, effectiveChatKey, workspace.close]);
  const [botReceipt, setBotReceipt] = useState<{ loupistId: string; taskId: string } | null>(null);
  const openScheduledTask = useCallback((taskId: string) => {
    if (!selectedLoupist) return;
    const homeId = `home-${selectedLoupist.id}`;
    if (taskId.startsWith("bot-handoff-") && activeThreadId !== homeId) {
      setBotReceipt({ loupistId: selectedLoupist.id, taskId });
      chat.setActiveThreadForLoupist(selectedLoupist.id, homeId);
      void scheduled.refreshThreads(selectedLoupist.id).catch(() => {});
      setTab("chat");
      return;
    }
    setScheduledFocus({ loupistId: selectedLoupist.id, taskId });
    setTab("schedule");
  }, [selectedLoupist, activeThreadId, chat, scheduled.refreshThreads, setTab]);
  useEffect(() => {
    if (!botReceipt || botReceipt.loupistId !== selectedLoupistId || activeThreadId !== `home-${selectedLoupistId}`) return;
    const receipt = document.querySelector(`[data-message-id="${CSS.escape(botReceipt.taskId)}"]`);
    if (receipt) { receipt.scrollIntoView({ block: "center" }); setBotReceipt(null); return; }
    if (tab !== "chat") return;
    const timer = setTimeout(() => void scheduled.refreshThreads(botReceipt.loupistId).catch(() => setBotReceipt(null)), 2000);
    return () => clearTimeout(timer);
  }, [botReceipt, selectedLoupistId, activeThreadId, threads, tab, scheduled.refreshThreads]);
  const openKnowledgePage = useCallback((next: KnowledgeLocation, camera?: GraphCamera) => {
    if (next.loupistId !== selectedLoupistId) return;
    if (next.surface === "lens") {
      if (next.recordId === SHARED_SECTION_ITEM) { openWhatItKnows(next.recordId); return; }
      setMemoryPage(null); setNoteReaderOpen(false); setAssetPage(null);
      lens.open(next); setTab("lens"); return;
    }
    if (next.surface === "note" && workspace.kind === "world" && workspace.open && !noteReaderOpen && !memoryPage) {
      setNoteTrail([]);
      memory.open(next, camera);
      return;
    }
    if (next.surface === "note") {
      setNoteTrail((current) => [...(noteReaderOpen ? current.filter((item) => item.loupistId === next.loupistId) : []), next]);
      setNoteReaderOpen(true);
      return;
    }
    if (noteLocation) memory.open(noteLocation, camera);
    setNoteReaderOpen(false);
    setMemoryPage(null);
    memory.open(next, camera);
    setTab("map");
  }, [memory.open, lens.open, setTab, selectedLoupistId, noteReaderOpen, noteLocation, tab, openWhatItKnows, memoryPage, workspace.kind, workspace.open]);
  // Receipt and feedback actions open the item's management controls.
  const openChatMemoryPage = openKnowledgePage;
  const learningNotifications = useLearningNotifications(bridge, workspace.open && workspace.kind === "world" ? selectedLoupistId ?? undefined : undefined, refreshKnowledge);
  const [pendingLearningLocation, setPendingLearningLocation] = useState<KnowledgeLocation>();
  useEffect(() => {
    if (!pendingLearningLocation || pendingLearningLocation.loupistId !== selectedLoupistId) return;
    if (pendingLearningLocation.surface === "lens") lens.open(pendingLearningLocation);
    else memory.open(pendingLearningLocation);
    setPendingLearningLocation(undefined);
  }, [pendingLearningLocation, selectedLoupistId, memory.open, lens.open]);
  const openLearnedMethod = async (loupistId: string, recordId: string) => {
    if (loupistId !== selectedLoupistId && !await loupist.selectLoupist(loupistId, loupists.find(item => item.id === loupistId))) throw new Error("Loupist unavailable");
    setLoupistSettingsOpen(false); setNoteReaderOpen(false); setMemoryPage(null); setAssetPage(null);
    setPendingLearningLocation({ loupistId, surface: "lens", recordId });
    setTab("lens");
  };
  const backKnowledgePage = useCallback(() => {
    const previous = memory.navigation?.past.at(-1);
    if (!previous) return;
    if (previous.surface === "note" && noteTrail.some((note) => sameKnowledgeTarget(note, previous))) {
      memory.back();
      memory.back();
      if (noteTrail.at(-1) !== previous) setNoteTrail([previous]);
      setNoteReaderOpen(true);
      const origin = memory.navigation?.past.at(-2);
      if (origin) setTab("map");
      return;
    }
    memory.back();
    setTab("map");
  }, [memory.back, memory.navigation, noteTrail]);
  const useKnowledgeInChat = useCallback((pin: KnowledgePin, threadId?: string, options?: { correct?: boolean }) => {
    if (!selectedLoupistId) return;
    setNoteTrail([]);
    setNoteReaderOpen(false);
    if (window.matchMedia(workspaceOverlayMedia).matches) workspace.close();
    const targetThreadId = threadId
      ? threads.find((thread) => thread.id === threadId)?.id ?? EMPTY_CHAT_THREAD_ID
      : pin.kind === "lens" ? `home-${selectedLoupistId}` : effectiveThreadId ?? EMPTY_CHAT_THREAD_ID;
    pendingComposerFocus.current = chatSessionKey(selectedLoupistId, targetThreadId);
    const draft = drafts.get(chatSessionKey(selectedLoupistId, targetThreadId));
    drafts.addPin(draft, pin);
    if (pin.kind === "lens") {
      const question = options?.correct ? i18n.t("app:app.correctThis") : i18n.t("app:app.discussThisLensWhatShouldWeRevise", { label: pin.label });
      if (!draft.prompt.includes(question)) draft.prompt = [draft.prompt.trimEnd(), question].filter(Boolean).join("\n\n");
    }
    if (isEmptyChatThreadId(targetThreadId)) void threadActions.openDraftChatForLoupist(selectedLoupistId);
    else chat.setActiveThreadForLoupist(selectedLoupistId, targetThreadId);
    setTab("chat");
  }, [selectedLoupistId, threads, effectiveThreadId, drafts.get, drafts.addPin, threadActions.openDraftChatForLoupist, chat.setActiveThreadForLoupist, workspace.close]);
  const fixMemory = useCallback((pin: KnowledgePin) => useKnowledgeInChat(pin, undefined, { correct: true }), [useKnowledgeInChat]);
  /**
   * Look back (finance-point-in-time.md §2.5): a new topic Chat whose composer
   * holds the entry's as-of date, its journal pin and the question. Never sends.
   */
  const lookBack = useCallback((entry: LookBackEntry) => {
    const loupistId = entry.loupistId;
    const draft = lookBackDraft(entry);
    void threadActions.createThreadForLoupist(loupistId, draft.title).then((created) => {
      if (!created) return;
      const key = chatSessionKey(loupistId, created.id);
      const target = drafts.get(key);
      target.asOf = draft.asOf;
      target.prompt = draft.prompt;
      drafts.addPin(target, draft.pin);
      pendingComposerFocus.current = key;
      setMemoryPage(null);
      setAssetPage(null);
      setNoteReaderOpen(false);
      if (window.matchMedia(workspaceOverlayMedia).matches) workspace.close();
      chat.setActiveThreadForLoupist(loupistId, created.id);
      setTab("chat");
    });
  }, [threadActions.createThreadForLoupist, drafts, chat.setActiveThreadForLoupist, workspace.close]);
  /**
   * Explain the disagreement (finance-korea-retail-disagreement.md §2.3): a new
   * topic Chat whose composer holds the entity and relation chips and the
   * question. Never sends.
   */
  const explainDisagreement = useCallback((pins: KnowledgePin[]) => {
    const loupistId = pins[0]?.loupistId;
    if (!loupistId) return;
    void threadActions.createThreadForLoupist(loupistId, pins[0]!.label).then((created) => {
      if (!created) return;
      const key = chatSessionKey(loupistId, created.id);
      const target = drafts.get(key);
      target.prompt = i18n.t("chat:conflict.askPrompt");
      for (const pin of pins) drafts.addPin(target, pin);
      pendingComposerFocus.current = key;
      setMemoryPage(null);
      setAssetPage(null);
      setNoteReaderOpen(false);
      if (window.matchMedia(workspaceOverlayMedia).matches) workspace.close();
      chat.setActiveThreadForLoupist(loupistId, created.id);
      setTab("chat");
    });
  }, [threadActions.createThreadForLoupist, drafts, chat.setActiveThreadForLoupist, workspace.close]);
  /** Puts text into the current Chat's composer (never sends), as the morning questions do. */
  const askInChat = useCallback((text: string) => {
    if (!selectedLoupistId) return;
    const targetThreadId = effectiveThreadId ?? EMPTY_CHAT_THREAD_ID;
    pendingComposerFocus.current = chatSessionKey(selectedLoupistId, targetThreadId);
    const draft = drafts.get(chatSessionKey(selectedLoupistId, targetThreadId));
    if (!draft.prompt.includes(text.trim())) draft.prompt = [draft.prompt.trimEnd(), text].filter(Boolean).join(draft.prompt.trim() ? "\n\n" : "");
    drafts.refresh();
    if (isEmptyChatThreadId(targetThreadId)) void threadActions.openDraftChatForLoupist(selectedLoupistId);
    else chat.setActiveThreadForLoupist(selectedLoupistId, targetThreadId);
    setTab("chat");
  }, [selectedLoupistId, effectiveThreadId, drafts, threadActions.openDraftChatForLoupist, chat.setActiveThreadForLoupist]);
  /** Puts text into a new topic Chat's composer (never sends): "Ask about these results". */
  const askInNewChat = useCallback((text: string) => {
    if (!selectedLoupistId) return;
    const key = chatSessionKey(selectedLoupistId, EMPTY_CHAT_THREAD_ID);
    pendingComposerFocus.current = key;
    drafts.get(key).prompt = text;
    drafts.refresh();
    void threadActions.openDraftChatForLoupist(selectedLoupistId);
    setTab("chat");
  }, [selectedLoupistId, drafts, threadActions.openDraftChatForLoupist]);
  // The Work tab lists charts the visible chat is still building, and re-reads when it publishes.
  const workspaceMessages = threads.find((thread) => thread.id === workspaceThreadId)?.messages;
  const buildingWork = useMemo(() => (workspaceMessages ?? []).flatMap((message) => message.role === "assistant" && message.status === "streaming"
    ? [...chartBlocks(message), ...gridBlocks(message)].flatMap((block) => block.kind === "building" ? [{ key: block.key, title: block.title }] : []) : []), [workspaceMessages]);
  const workRefreshKey = useMemo(() => (workspaceMessages ?? []).reduce((count, message) => count + (message.artifact_refs?.length ?? 0), 0), [workspaceMessages]);
  const workspaceSession = effectiveChatKey
    ? (workspace.sessions[effectiveChatKey] ?? emptyWorkspaceSession())
    : emptyWorkspaceSession();
  const recoveredChat =
    selectedLoupistId && persistedThreadId
      ? chat.runRegistry.getRecoveredChat(selectedLoupistId, persistedThreadId)
      : undefined;
  const setVisibleThreads = useCallback<Dispatch<SetStateAction<ChatThread[]>>>(
    (action) => {
      if (selectedLoupistId) chat.setThreadsForLoupist(selectedLoupistId, action);
    },
    [selectedLoupistId, chat.setThreadsForLoupist],
  );
  const setVisibleActiveThreadId = useCallback(
    (threadId: string) => {
      if (selectedLoupistId) chat.setActiveThreadForLoupist(selectedLoupistId, threadId);
    },
    [selectedLoupistId, chat.setActiveThreadForLoupist],
  );
  const abortVisibleRecoveredChat = useCallback(() => {
    if (selectedLoupistId && persistedThreadId) {
      chat.runRegistry.abortRecoveredChat(selectedLoupistId, persistedThreadId);
    }
  }, [persistedThreadId, chat.runRegistry, selectedLoupistId]);
  const runningThreadKeys = useMemo(() => {
    const active = new Set(Object.keys(chat.chatStatuses));
    for (const recovered of chat.runRegistrySnapshot.active_chat_threads) {
      active.add(chatSessionKey(recovered.loupist_id, recovered.thread_id));
    }
    return active;
  }, [chat.chatStatuses, chat.runRegistrySnapshot.active_chat_threads]);
  // Needs-you (S7, E1): what waits across Loupists, and runs that ended out of sight.
  const runEndKey = useMemo(() => [...runningThreadKeys].sort().join("|"), [runningThreadKeys]);
  const attention = useAttention(bridge, runEndKey);
  const visibleChatKey = tab === "chat" && effectiveChatKey && !isEmptyChatThreadId(workspaceThreadId) ? effectiveChatKey : null;
  const finishedRuns = useFinishedWhileHidden({ bridge, loupists, runningThreadKeys, visibleChatKey });
  const notifiedApprovals = useRef(new Set<string>());
  useEffect(() => {
    for (const item of attention) {
      if (item.kind !== "approval" || !item.chatId) continue;
      const key = `${item.loupistId}:${item.chatId}:${item.atMs}`;
      if (notifiedApprovals.current.has(key)) continue;
      notifiedApprovals.current.add(key);
      const watching = visibleChatKey !== null && splitChatKey(visibleChatKey).chatId === item.chatId && document.visibilityState === "visible" && document.hasFocus();
      if (!watching) void bridge.attentionNotify({ loupistId: item.loupistId, chatId: item.chatId, title: i18n.t("app:notifications.approval", { loupist: item.loupistName }) }).catch(() => undefined);
    }
  }, [attention, bridge, visibleChatKey]);
  // Export a chat (X9) from the loaded messages: Markdown through the Rust save dialog, or the
  // same transcript through the print dialog, where macOS offers Save as PDF.
  const exportThread = (thread: ChatThread, format: "md" | "pdf" = "md") => {
    const loupistName = loupists.find((item) => item.id === thread.loupist_id)?.name ?? "";
    if (format === "pdf") {
      void printChat(thread, loupistName).catch((cause) => threadActions.setMutationError(errorMessage(cause, i18n.t("app:app.couldNotPrint"))));
      return;
    }
    void bridge.exportSave({ suggestedName: `${exportFileName(thread.title)}.md`, kind: "md", bytes: utf8Bytes(chatMarkdown(thread, loupistName)) })
      .catch((cause) => threadActions.setMutationError(errorMessage(cause, i18n.t("app:app.couldNotExport"))));
  };
  const activeExportThread = threads.find((thread) => thread.id === activeThreadId && thread.messages.length > 0);
  const paletteActions: PaletteAction[] = [
    ...(selectedLoupist ? [{ id: "new-chat", label: i18n.t("app:palette.actions.newChat"), keys: shortcutFor("newChat"), run: startNewChat }] : []),
    { id: "chat", label: i18n.t("app:palette.actions.openChat"), keys: tabShortcut(1), run: () => setTab("chat") },
    { id: "map", label: i18n.t("app:palette.actions.openMap"), keys: tabShortcut(2), run: () => setTab("map") },
    { id: "lens", label: "Lens", keys: tabShortcut(3), run: () => { setMemoryPage(null); setAssetPage(null); setTab("lens"); } },
    { id: "schedule", label: i18n.t("app:shell.taskHistory"), keys: tabShortcut(4), run: () => setTab("schedule") },
    { id: "panel", label: i18n.t("app:palette.actions.togglePanel"), keys: shortcutFor("panel"), run: () => { if (workspace.open) workspace.dismiss(effectiveChatKey ?? undefined); else workspace.show(effectiveChatKey ?? undefined); } },
    ...(activeExportThread ? [
      { id: "export-chat", label: i18n.t("app:palette.actions.exportChat"), run: () => exportThread(activeExportThread) },
      { id: "print-chat", label: i18n.t("app:palette.actions.printChat"), run: () => exportThread(activeExportThread, "pdf") },
    ] : []),
    { id: "settings", label: i18n.t("app:palette.actions.settings"), keys: shortcutFor("settings"), run: () => openSettings() },
    { id: "keyboard", label: i18n.t("app:palette.actions.keyboard"), run: () => openSettings("keyboard") },
  ];
  const openAttention = async (item: AttentionItem) => {
    if (item.kind === "lineCrossed" && item.recordId) {
      if (item.loupistId !== selectedLoupistId && !await loupist.selectLoupist(item.loupistId, loupists.find(candidate => candidate.id === item.loupistId))) return;
      setLoupistSettingsOpen(false);
      setMemoryPage(null);
      setAssetPage({ loupistId: item.loupistId, assetId: item.recordId, name: item.title, key: Date.now() });
      return;
    }
    // An approval or a question is answered in its chat.
    if ((item.kind === "approval" || item.kind === "question") && item.chatId) {
      selectLoupistFromNavigation(item.loupistId, item.chatId);
      setTab("chat");
      return;
    }
    // Watcher alerts are read in the Loupist's Thread, where Rust posted them.
    if (item.kind === "watchAlerts") {
      setLoupistSettingsOpen(false);
      setMemoryPage(null);
      setAssetPage(null);
      selectLoupistFromNavigation(item.loupistId, item.loupistId === selectedLoupistId ? threads.find(thread => thread.home)?.id : undefined);
      setTab("chat");
      return;
    }
    if (item.kind === "checkDue" && item.recordId) {
      if (item.loupistId !== selectedLoupistId && !await loupist.selectLoupist(item.loupistId, loupists.find(candidate => candidate.id === item.loupistId))) return;
      setLoupistSettingsOpen(false);
      setPendingLearningLocation({ loupistId: item.loupistId, surface: "note", objectId: item.recordId, selection: { kind: "journal", journalId: item.recordId } });
      setTab("map");
      return;
    }
    selectLoupistFromNavigation(item.loupistId);
    setTab("schedule");
  };
  workspace.visibleRef.current = {
    loupistId: selectedLoupistId,
    threadId: effectiveThreadId,
    tab,
  };
  const chatWorkspaceActions = useMemo<ChatWorkspaceActions>(
    () => ({
      openEvidence: (evidence, step) => { if (selectedLoupistId && workspaceThreadId) workspace.openEvidence(selectedLoupistId, workspaceThreadId, evidence, step); },
      openFile: (path) => { if (selectedLoupistId && workspaceThreadId) workspace.openFiles(selectedLoupistId, workspaceThreadId, path); },
      readWorldJob: (jobId) => bridge.knowledgeGraphJob({ loupistId: selectedLoupistId!, jobId }),
      onWorldChanged: refreshKnowledge,
      openMemory: (location, title) => {
        if (location.surface === "world") { openKnowledgePage(location); return; }
        if (selectedLoupistId && workspaceThreadId) {
          workspace.openMemory(
            selectedLoupistId,
            workspaceThreadId,
            location,
            title,
          );
        }
      },
      openMemoryPage: openChatMemoryPage,
      openArtifact: (threadId, artifact) => {
        if (selectedLoupistId) {
          workspace.openArtifact(selectedLoupistId, threadId, artifact);
        }
      },
      openLink: (url) => {
        if (selectedLoupistId) {
          workspace.openBrowser(
            selectedLoupistId,
            workspaceThreadId ?? EMPTY_CHAT_THREAD_ID,
            url,
          );
        }
      },
    }),
    [selectedLoupistId, bridge, refreshKnowledge, workspaceThreadId, workspace.openArtifact, workspace.openEvidence, workspace.openFiles, openKnowledgePage, openChatMemoryPage, workspace.openBrowser, workspace.openMemory],
  );
  const selectLoupistFromNavigation = (loupistId: string, threadId?: string) => {
    setLoupistSettingsOpen(false);
    if (loupistId === selectedLoupistId && !loupist.loading) {
      loupist.keepCurrentLoupist();
      if (threadId) chat.setActiveThreadForLoupist(loupistId, threadId);
      return;
    }
    void loupist.selectLoupist(loupistId, loupists.find((item) => item.id === loupistId), threadId);
  };

  // The palette finds names and principles in every Loupist (shell spec X7); another Loupist's
  // result switches to it first and opens once it is selected.
  const openPaletteMapNode = (loupistId: string, node: KnowledgeMapNode) => {
    const location: KnowledgeLocation = { loupistId, surface: "world", recordId: node.recordId, objectId: node.id, selection: { kind: "node", id: node.id } };
    if (loupistId === selectedLoupistId) { openKnowledgePage(location); return; }
    selectLoupistFromNavigation(loupistId);
    setLoupistSettingsOpen(false); setNoteReaderOpen(false); setMemoryPage(null);
    setPendingLearningLocation(location);
    setTab("map");
  };
  const openPalettePrinciple = (loupistId: string, recordId: string) => {
    if (loupistId === selectedLoupistId) { openWhatItKnows(recordId); return; }
    selectLoupistFromNavigation(loupistId);
    setLoupistSettingsOpen(false);
    setMemoryPage({ loupistId, item: recordId, key: Date.now() });
    setTab("lens");
  };

  const draftSkill = skillDraftLoupist?.availability.status === "available" ? async (scope: ConfigScope, brief: string) => {
    const loupistId = scope.kind === "project" ? scope.loupistId : skillDraftLoupist.id;
    const config = await bridge.configRead({ scope });
    if (config.effective.skills["skill-creator"]?.enabled !== true) {
      await bridge.configUpdate({
        scope,
        expectedRevision: config.revision,
        set: { skills: { "skill-creator": { enabled: true } } },
        unset: [],
      });
      notifyConfigurationChanged();
      setBriefingToast(i18n.t(scope.kind === "global" ? "app:app.skillCreatorTurnedOnForAllLoupists" : "app:app.skillCreatorTurnedOnForThisLoupist"));
    }
    await threadActions.openDraftChatForLoupist(loupistId);
    const draft = drafts.get(chatSessionKey(loupistId, EMPTY_CHAT_THREAD_ID));
    const request = scope.kind === "global"
      ? brief ? i18n.t("app:app.skillDraftGlobal", { skill: "$skill-creator", brief }) : i18n.t("app:app.skillDraftGlobalEmpty", { skill: "$skill-creator" })
      : brief ? i18n.t("app:app.skillDraftLoupist", { skill: "$skill-creator", brief }) : i18n.t("app:app.skillDraftLoupistEmpty", { skill: "$skill-creator" });
    draft.prompt = [draft.prompt.trimEnd(), request].filter(Boolean).join("\n\n");
    drafts.refresh(); setLoupistSettingsOpen(false); setTab("chat");
  } : undefined;

  const appDialogs = <AppDialogs
          dialogs={dialogs}
          selectedLoupistName={selectedLoupist?.name}
          currentWorkspacePath={selectedLoupist?.workspace_path}
          loupistMutationBusy={loupist.mutationBusy}
          loupistMutationError={loupist.mutationError}
          threadMutationBusy={threadActions.mutationBusy}
          threadMutationError={threadActions.mutationError}
          onCreateLoupist={loupist.addLoupist}
          onImportLoupist={async request => {
            const imported = await loupist.importLoupist(request);
            if (imported) await finishOnboarding();
            return imported;
          }}
          onChooseFolder={() => bridge.loupistChooseFolder()}
          onUseLinkOrFile={() => { dialogs.setCreateLoupistOpen(false); setPackLinkOpen(true); }}
          onBrowseGallery={indexConfigured ? () => { dialogs.setCreateLoupistOpen(false); setMemoryPage(null); setGalleryOpen(true); } : undefined}
          onReconnectLoupist={loupist.reconnectLoupist}
          onRenameThread={threadActions.renameThread}
          onDeleteThread={threadActions.deleteThread}
        />;

  if (onboardingActive || needsOnboarding) return <TooltipProvider><Onboarding bridge={bridge} catalog={model.catalog}
    onImport={dialogs.openImportLoupist} onCatalogChange={model.setCatalog} onCreate={loupist.addLoupist} onComplete={finishOnboarding}
    busy={loupist.mutationBusy} error={loupist.mutationError} hasLoupist={loupists.length > 0} />{appDialogs}</TooltipProvider>;

  return (
    <TooltipProvider>
      <AssetPageOpenerProvider value={openAssetPage}><LookBackProvider value={lookBack}><FixMemoryProvider value={fixMemory}>
      <SourceFaviconProvider bridge={bridge}>
      <SidebarProvider
        className="app-shell"
        data-theme={resolvedTheme}
      >
        <LaunchNotice bridge={bridge} enabled={model.resolved && !loupist.loading && !loupist.error} />
        {briefingToast && <div className="briefings-toast" role="status">{briefingToast}</div>}
        <LearningToasts notifications={learningNotifications} onView={openLearnedMethod} />
        <div className="app-titlebar">
          <MacTitlebarControls />
          <AppHeader
            runningLoupists={new Set([...chat.runRegistrySnapshot.active_loupist_ids, ...loupists.filter(item => (chat.threadsByLoupist[item.id] ?? []).some(thread => runningThreadKeys.has(chatSessionKey(item.id,thread.id)))).map(item => item.id)])}
            unreadCounts={inboxCounts}
            tab={tab}
            loupists={loupists}
            onCreate={dialogs.openCreateLoupist}
            onSelect={id => { selectLoupistFromNavigation(id, chat.threadsByLoupist[id]?.find(thread => thread.home)?.id ?? `home-${id}`); showChat(); }}
            loupist={selectedLoupist}
            onTabChange={(next) => { setLoupistSettingsOpen(false); setMemoryPage(null); setAssetPage(null); setTab(next); }}
            onLoupistSettings={() => { if (selectedLoupistId) void openLoupistSettingsFor(selectedLoupistId); }}
            markets={followedMarkets}
            workspaceKind={workspace.kind}
            workspacePlacement={workspace.placement}
            onWorkspaceKind={kind => { setLoupistSettingsOpen(false); setAssetPage(null); setTab("chat"); workspace.selectKind(kind, effectiveChatKey ?? undefined); }}
            onWorkspaceSplit={placement => { setLoupistSettingsOpen(false); setTab("chat"); workspace.selectSplit(placement, effectiveChatKey ?? undefined); }}
            workspaceOpen={workspace.open}
            onWorkspaceToggle={() => { if (workspace.open) workspace.dismiss(effectiveChatKey ?? undefined); else workspace.show(effectiveChatKey ?? undefined); }}
          />
        </div>
        <div className="app-body">
        <AppNavigation
          tab={tab}
          selectedLoupist={selectedLoupist}
          loading={loupist.loading}
          threadsByLoupist={chat.threadsByLoupist}
          activeThreadId={chatVisible ? activeThreadId : null}
          attention={attention}
          finishedThreadKeys={finishedRuns.finishedKeys}
          onOpenAttention={(item) => void openAttention(item)}
          runningThreadKeys={runningThreadKeys}
          draftThreadKeys={drafts.unsentKeys()}
          settingsSection={settingsSection}
          updateAvailable={Boolean(appUpdate.status?.offer)}
          onTabChange={(nextTab) => { if (nextTab === "settings") openSettings(); else setTab(nextTab); }}
          onSearch={openPalette}
          onMoreAttention={() => setPalette({ open: true, focus: "attention" })}
          onSettingsSectionChange={section => { setLoupistSettingsOpen(false); setMarketplaceEntryId(null); setSettingsSection(section); }}
          onExitSettings={() => { if (settingsReturnLoupistRef.current) selectLoupistFromNavigation(settingsReturnLoupistRef.current); setTab(settingsReturnTabRef.current); }}
          onCreateThread={startChatFor}
          onRenameThread={(thread) => {
            threadActions.setMutationError(null);
            dialogs.openRenameThread(thread);
          }}
          onDeleteThread={(thread) => {
            threadActions.setMutationError(null);
            dialogs.setThreadToDelete(thread);
          }}
          onExportThread={exportThread}
          onSelectThread={(thread) => {
            selectLoupistFromNavigation(thread.loupist_id, thread.id);
            showChat();
          }}
        />

        <SidebarInset className="app-frame">
          <h1 id="current-location" className="sr-only">{tab === "chat" ? displayThreadTitle(threads.find(thread => thread.id === activeThreadId) ?? { title: "", home: false }) : tab === "lens" ? i18n.t("app:shell.principles") : tab === "schedule" ? i18n.t("app:shell.taskHistory") : APP_TAB_LABELS[tab]}</h1>
          {tab === "chat" && activeDraft ? <QuoteAsk scope="reader" onQuote={quoteReaderSelection} /> : null}

          {loupist.error && selectedLoupist && !loupist.loading ? (
            <section className="app-selection-error" role="alert" aria-label={loupist.failedSelectionId ? i18n.t("app:app.loupistSelectionFailed") : i18n.t("app:app.workspaceError")}>
              <p>{loupist.error}</p>
              {loupist.failedSelectionId ? <button type="button" className="ghost-button" onClick={() => void loupist.retrySelection()}>{i18n.t("app:app.tryAgain")}</button> : null}
              <button type="button" className="ghost-button" onClick={loupist.keepCurrentLoupist}>{loupist.failedSelectionId ? i18n.t("app:app.keepCurrentLoupist") : i18n.t("app:app.dismiss")}</button>
            </section>
          ) : null}

          {loupist.mutationError && !dialogs.createLoupistOpen && !dialogs.reconnectLoupistOpen && !loupistSettingsOpen ? <section className="app-selection-error" role="alert"><p>{loupist.mutationError}</p><button type="button" className="ghost-button" onClick={() => loupist.setMutationError(null)}>{i18n.t("app:app.dismiss")}</button></section> : null}

          {selectedLoupist?.availability.status === "workspace_unavailable" && (tab === "chat" || tab === "map" || tab === "lens") ? (
            <section className="app-selection-error" role="status" aria-label={i18n.t("app:app.workspaceFolderUnavailable")}>
              <p><Trans ns="app" i18nKey="app.workspaceUnavailableDescription" components={[<strong />]} /></p>
              <button type="button" className="ghost-button" onClick={dialogs.openReconnectLoupist}>{i18n.t("app:app.reconnectFolder")}</button>
            </section>
          ) : null}

          <ChatMapProvider bridge={bridge} loupistId={selectedLoupist?.id ?? ""} chatId={workspaceThreadId ?? ""} refreshKey={knowledgeRefresh} messages={threads.find(thread => thread.id === workspaceThreadId)?.messages ?? []} onOpen={() => workspace.selectKind("materials", effectiveChatKey ?? undefined)} onFirstNames={() => selectedLoupist && workspaceThreadId ? workspace.revealMap(selectedLoupist.id, workspaceThreadId) : "none"}>
          <OpenEvidenceProvider session={workspaceSession} open={tab === "chat" && workspace.open && workspace.kind === "materials"}>
          <div
            className={`app-stage artifact-placement-${workspace.placement}`}
          >
            <div
              className="app-stage-main"
            >
              {loupistSettingsOpen && selectedLoupist && selectedLoupist.id === loupistSettingsTargetId ? <div className="loupist-settings-page-layer"><Suspense fallback={<Spinner />}>
                    <LoupistSettings
                      key={`${selectedLoupist.id}:${loupistSettingsSection ?? "profile"}`}
                      catalog={model.catalog}
                      onChanged={refreshKnowledge}
                      initialSection={loupistSettingsSection}
                      onBack={() => { setLoupistSettingsOpen(false); requestAnimationFrame(() => loupistSettingsReturnFocus.current?.focus()); }}
                      onManageProviders={() => openSettings("model")}
                      onDraftSkill={draftSkill}
                      initialRemove={removeLoupistOpen}
                      bridge={bridge}
                      selectedAgent={selectedLoupist}
                      mutationBusy={loupist.mutationBusy}
                      mutationError={dialogs.createLoupistOpen || dialogs.reconnectLoupistOpen ? null : loupist.mutationError}
                      recoveryBusy={loupist.recoveryBusy}
                      recoveryError={loupist.recoveryError}
                      onRecover={() => void loupist.recoverSelectedLoupist()}
                      onReconnect={dialogs.openReconnectLoupist}
                      onOpenMarketplace={(entryId) => openSettings("connections", { entryId: entryId ?? null })}
                      onDelete={loupist.deleteLoupist}
                      onAgentUpdated={loupist.updateLoupist}
                    />
              </Suspense></div> : null}
              {assetPageOpen && assetPage && selectedLoupist ? <div className="asset-page-layer" inert={loupistSettingsOpen || undefined} aria-hidden={loupistSettingsOpen || undefined} onFocus={event => { if (event.target instanceof HTMLElement) event.target.scrollIntoView({ block: "nearest", inline: "nearest" }); }}><Suspense fallback={<Spinner />}>
                <AssetPage key={assetPage.key} bridge={bridge} loupist={selectedLoupist} target={assetPage} threads={threads}
                  onBack={() => setAssetPage(null)}
                  onAsk={text => { setAssetPage(null); askInChat(text); }}
                  onAskInNewChat={text => { setAssetPage(null); askInNewChat(text); }}
                  onOpenChat={threadId => { setAssetPage(null); chat.setActiveThreadForLoupist(selectedLoupist.id, threadId); setTab("chat"); }}
                  onOpenMap={entityId => { setAssetPage(null); openKnowledgePage({ loupistId: selectedLoupist.id, surface: "world", ...(entityId ? { selection: { kind: "node", id: entityId } } : { view: { followCurrent: true } }) }); }}
                  onOpenLink={chatWorkspaceActions.openLink}
                  onOpenSource={recordId => { setAssetPage(null); openKnowledgePage({ loupistId: selectedLoupist.id, surface: "source", recordId }); }}
                  onOpenConnections={() => { setAssetPage(null); openSettings("connections"); }}
                  onAddSource={() => { setAssetPage(null); openKnowledgePage({ loupistId: selectedLoupist.id, surface: "source" }); }}
                  onChanged={refreshKnowledge} />
              </Suspense></div> : null}
              {galleryOpen && !packReview ? <div className="pack-review-layer" onFocus={event => { if (event.target instanceof HTMLElement) event.target.scrollIntoView({ block: "nearest", inline: "nearest" }); }}><Suspense fallback={<Spinner />}>
                <Gallery bridge={bridge} onBack={() => setGalleryOpen(false)} onOpened={(result, use) => openPackReview(result, use)} onUseLinkOrFile={() => setPackLinkOpen(true)} />
              </Suspense></div> : null}
              {packReview ? <div className="pack-review-layer"><Suspense fallback={<Spinner />}>
                <PackReview bridge={bridge} result={packReview} loupists={loupists} currentLoupistId={selectedLoupistId} focusAdd={packUse} onBack={() => setPackReview(null)} onAdded={added => void landSharedLoupist(added)} />
              </Suspense></div> : null}
              <div className="app-stage-view" inert={loupistSettingsOpen || assetPageOpen || Boolean(packReview) || galleryOpen || undefined} aria-hidden={loupistSettingsOpen || assetPageOpen || Boolean(packReview) || galleryOpen || undefined}>
              <ViewBoundary name={APP_TAB_LABELS[tab]} resetKey={`${tab}:${selectedLoupistId ?? ""}:${settingsSection}`}>
              <Suspense fallback={<div className="app-loading" role="status" aria-label={i18n.t("app:app.opening2", { value1: APP_TAB_LABELS[tab] })}><Spinner className="size-6" /><strong>{i18n.t("app:app.openingView", { view: APP_TAB_LABELS[tab] })}</strong></div>}>
              {tab === "settings" ? (
                <div className="app-main">
                  <div
                    className="app-panel"
                    id="main-panel-settings"
                    aria-label={i18n.t("app:shell.settings")}
                  >
                    <SettingsView
                      bridge={bridge}
                      loupists={loupists}
                      connectionsEntryId={marketplaceEntryId}
                      onOpenLoupistSettings={id => { void openLoupistSettingsFor(id); }}
                      onCreateLoupist={() => { loupist.setMutationError(null); dialogs.openCreateLoupist(); }}
                      onImportLoupist={() => { loupist.setMutationError(null); dialogs.openImportLoupist(); }}
                      onRemoveLoupist={id => { void openLoupistSettingsFor(id, true); }}
                      onOpenLoupists={() => { setMarketplaceEntryId(null); setSettingsSection("loupists"); }}
                      onOpenLoupistConnections={id => { setMarketplaceEntryId(null); setSettingsSection("loupists"); void openLoupistSettingsFor(id, false, "connections"); }}
                      currentLoupistId={selectedLoupistId}
                      onManageProviders={() => openSettings("model")}
                      onDraftSkill={draftSkill}
                      appearanceConfiguration={appearanceConfiguration}
                      section={settingsSection}
                      catalog={model.catalog}
                      theme={theme}
                      loadError={model.settingsError}
                      language={language}
                      onLanguageChange={setLanguage}
                      onThemeChange={setTheme}
                      providerActions={model.providerActions}
                      connectTarget={modelConnectTarget}
                      onConnectTargetConsumed={consumeModelConnectTarget}
                      update={appUpdate}
                    />
                  </div>
                </div>
              ) : loupist.loading || (noticeAcknowledgedVersion === null && model.resolved) || (!model.resolved && !selectedLoupist) ? (
                <main className="app-loading" aria-label={i18n.t("app:app.openingLoupist")}>
                  <Spinner className="size-6" />
                  <strong>{i18n.t("app:app.opening")}</strong>
                </main>
              ) : loupist.error && !selectedLoupist ? (
                <main className="fatal-error" role="alert">
                  <h1>{i18n.t("app:app.couldNotOpenLoupist")}</h1>
                  <p>{loupist.error}</p>
                  <button
                    type="button"
                    className="primary-button"
                    onClick={() => window.location.reload()}
                  >
                    {i18n.t("app:app.tryAgain")}</button>
                </main>
              ) : selectedLoupist && (selectedLoupist.availability.status === "earlier_version" || (selectedLoupist.availability.status === "recovery_required" && tab !== "schedule")) ? (
                <LoupistAvailabilityBoundary
                  availability={selectedLoupist.availability}
                  busy={loupist.recoveryBusy}
                  error={loupist.recoveryError}
                  onRecover={() => void loupist.recoverSelectedLoupist()}
                      onReconnect={dialogs.openReconnectLoupist}
                  onRemove={() => void openLoupistSettingsFor(selectedLoupist.id, true)}
                />
              ) : selectedLoupist ? (
                <main className="app-main">
                  <div
                    id="main-panel-chat"
                    className="app-panel chat-tab-panel"
                    aria-labelledby="current-location"
                    aria-hidden={tab !== "chat"}
                    hidden={tab !== "chat"}
                    inert={tab !== "chat"}
                  >
                    {activeChat ? (
                      <ChatView
                        onOpenBriefing={(runId) => { setInboxFocus({ loupistId: selectedLoupist.id, runId }); setTab("schedule"); }}
                        onOpenSchedule={openScheduledTask}
                        key={selectedLoupist.id}
                        active={tab === "chat" && !loupistSettingsOpen}
                        theme={resolvedTheme}
                        bridge={bridge}
                        chat={activeChat}
                        recoveredRun={recoveredChat}
                        loupist={selectedLoupist}
                        threads={threads}
                        setThreads={setVisibleThreads}
                        activeThreadId={activeThreadId}
                        setActiveThreadId={setVisibleActiveThreadId}
                        onCreateThread={createThread}
                        modelCatalog={model.catalog}
                        modelSelection={modelSelection}
                        skillPreferences={configuration.snapshot?.effective.skills}
                        memoryDefaults={configuration.snapshot?.effective.chat}
                        configurationReady={Boolean(configuration.snapshot) && !configuration.error && !configuration.snapshot?.diagnostics.length}
                        configurationError={configuration.error ?? (configuration.snapshot?.diagnostics.length ? i18n.t("app:app.correctTheConfigurationFileInSettingsChat") : configuration.snapshot && model.catalog?.models.length && !modelSelection.model_profile_id && !model.selections[selectedLoupist.id] ? i18n.t("app:app.theConfiguredChatDefaultsAreUnavailableFor") : undefined)}
                        onModelSelectionChange={changeSelectedModel}
                        modelAuto={!model.selections[selectedLoupist.id]}
                        onModelResetToAuto={() => model.resetSelection(selectedLoupist.id)}
                        onModelUsed={recordSelectedModel}
                        onResolveModelSelection={async () => {
                          const next = await configuration.reload();
                          if (!next || next.diagnostics.length) throw new Error(i18n.t("app:app.correctTheConfigurationFileInSettingsChat2"));
                          const catalog = model.catalog ? { ...model.catalog, hidden_model_profile_ids: next.effective.models.hidden_model_profile_ids } : null;
                          // Nothing chosen here and no Chat default: Rust resolves Auto at admission (W17.5) and applies
                          // the configured reasoning and run options where the model it picks supports them.
                          if (!model.selections[selectedLoupist.id] && !next.effective.chat.model_profile_id) return { model_profile_id: "", thinking_level: next.effective.chat.thinking_level ?? "", run_options: { ...next.effective.chat.run_options }, auto: true, memoryDefaults: next.effective.chat };
                          return { ...resolveConfiguredModelRunSelection(catalog, model.selections[selectedLoupist.id], next.effective.chat, selectedLoupist.last_model_profile_id), memoryDefaults: next.effective.chat };
                        }}
                        selectedArtifact={effectiveChatKey ? workspace.selectedArtifacts[effectiveChatKey] : undefined}
                        onClearSelectedArtifact={() => { if (effectiveChatKey) workspace.selectArtifactForChat(effectiveChatKey); }}
                        onConsumeSelectedArtifact={(artifact, threadId) => { const key = threadId ? chatSessionKey(selectedLoupist.id, threadId) : effectiveChatKey; if (key) workspace.consumeArtifactForChat(key, artifact); }}
                        knowledgePins={activeDraft?.pins ?? []}
                        onRemoveKnowledgePin={(pin) => { if (activeDraft) drafts.removePin(activeDraft, pin); }}
                        onAddKnowledgePin={(pin) => { if (activeDraft) drafts.addPin(activeDraft, pin); }}
                        onSelectArtifact={(artifact) => { if (effectiveChatKey) workspace.selectArtifactForChat(effectiveChatKey, artifact); }}
                        onConsumeKnowledgePins={(pins) => { if (activeDraft) pins.forEach((pin) => drafts.removePin(activeDraft, pin)); }}
                        workspaceActions={chatWorkspaceActions}
                        progressFocus={progressFocus}
                        onAbortRecoveredRun={abortVisibleRecoveredChat}
                        draft={activeDraft ?? undefined}
                        sessionDrafts={drafts.getSession(selectedLoupist.id)}
                        initialPrompt={activeDraft?.prompt ?? ""}
                        onDraftChange={(prompt) => {
                          if (!activeDraft || !effectiveChatKey || !effectiveThreadId) return;
                          activeDraft.prompt = prompt;
                          drafts.refresh();

                        }}
                        onRenameThread={threadActions.renameThread}
                        onChipsChange={(chips) => { if (activeDraft) { activeDraft.chips = chips; drafts.refresh(); } }}
                        onAsOfChange={(asOf) => { if (activeDraft) { activeDraft.asOf = asOf; drafts.refresh(); } }}
                        onTurnComplete={refreshKnowledge}
                        modelSetupMessage={model.needsVisibleModels
                            ? i18n.t("app:app.showOrLoadAModelInSettings")
                            : undefined}
                        onOpenModelSettings={openModelSettings}
                        modelNotice={model.catalog?.model_notice ?? null}
                        onAcknowledgeModelNotice={model.acknowledgeModelNotice}
                        hostAuto={!model.selections[selectedLoupist.id] && !configuration.snapshot?.effective.chat.model_profile_id}
                        onOpenChatPreferences={() => { void openLoupistSettingsFor(selectedLoupist.id, false, "chat"); }}
                        onOpenConnections={() => openSettings("connections")}
                      />
                    ) : null}
                  </div>
                  {tab === "schedule" ? <div className="app-panel" id="main-panel-schedule" aria-labelledby="current-location">
                    <ScheduleView onOpenModelSettings={() => openSettings("model")} modelCatalog={model.catalog} initialRunId={inboxFocus?.loupistId === selectedLoupist.id ? inboxFocus.runId ?? undefined : undefined} initialTaskId={scheduledFocus?.loupistId === selectedLoupist.id ? scheduledFocus.taskId : undefined} onInitialConsumed={() => { setInboxFocus(null); setScheduledFocus(null); }} key={`${selectedLoupist.id}:${selectedLoupist.workspace_path}`} bridge={bridge} loupist={selectedLoupist}
                      onOpenEvidence={(item, evidence, step) => {
                        if (!item.threadId) return;
                        chat.setActiveThreadForLoupist(selectedLoupist.id, item.threadId);
                        setTab("chat");
                        workspace.openEvidence(selectedLoupist.id, item.threadId, evidence, step);
                      }}
                      onOpenMemory={(item, location, title) => {
                        if (location.surface === "world") { openKnowledgePage(location); return; }
                        if (!item.threadId) return;
                        chat.setActiveThreadForLoupist(selectedLoupist.id, item.threadId);
                        setTab("chat");
                        workspace.openMemory(selectedLoupist.id, item.threadId, location, title);
                      }}
                      onOpenMemoryPage={openChatMemoryPage}
                      onOpenArtifact={(item, artifact) => {
                        if (!item.threadId) return;
                        chat.setActiveThreadForLoupist(selectedLoupist.id, item.threadId);
                        setTab("chat");
                        workspace.openArtifact(selectedLoupist.id, item.threadId, artifact);
                      }}
                      onOpenMap={disputedEntityIds => {
                        if (disputedEntityIds?.length) {
                          openKnowledgePage({ loupistId: selectedLoupist.id, surface: "world", view: { onlyDisputed: true, highlightEntityIds: disputedEntityIds } });
                        } else setTab("map");
                      }}
                      onOpenChat={async item => {
                        if (!item.threadId) return;
                        const loupistId = selectedLoupist.id;
                        const surfaceEpoch = surfaceNavigationEpoch.current;
                        const threadEpoch = chat.activeThreadEpochsRef.current[loupistId] ?? 0;
                        await scheduled.refreshThreads(loupistId);
                        if (loupist.desiredLoupistIdRef.current !== loupistId || surfaceNavigationEpoch.current !== surfaceEpoch || (chat.activeThreadEpochsRef.current[loupistId] ?? 0) !== threadEpoch) return;
                        chat.setActiveThreadForLoupist(loupistId, item.threadId);
                        setTab("chat");
                        if (item.messageId) pendingBriefingMessage.current = { loupistId, threadId: item.threadId, messageId: item.messageId, epoch: surfaceNavigationEpoch.current };
                      }} />
                  </div> : null}

                </main>
              ) : null}
              </Suspense>
              </ViewBoundary>
              </div>
            </div>
            {selectedLoupist && selectedLoupist.availability.status !== "recovery_required" &&
              workspaceThreadId && (
                <ChatWorkspacePanel
                  bridge={bridge}
                  loupistId={selectedLoupist.id}
                  threadId={workspaceThreadId}
                  loupistName={selectedLoupist.name}
                  workspacePath={selectedLoupist.workspace_path}
                  onRevealProgress={setProgressFocus}
                  onOpenFiles={(path) => workspace.openFiles(selectedLoupist.id, workspaceThreadId, path)}
                  onAskFile={(file) => {
                    if (!effectiveChatKey) return;
                    setTab("chat");
                    const draft = drafts.get(effectiveChatKey);
                    const chips = draft.chips ?? [];
                    if (!chips.some(chip => chip.kind === "file" && chip.id === file.path)) {
                      draft.chips = [...chips, { key: `file:${file.path}`, kind: "file", id: file.path, label: file.path.split("/").at(-1) || file.path }];
                    }
                    drafts.refresh();
                    if (window.matchMedia(workspaceOverlayMedia).matches) workspace.close();
                    pendingComposerFocus.current = effectiveChatKey;
                    requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>('textarea[data-chat-composer]')?.focus());
                  }}
                  buildingWork={buildingWork}
                  workRefreshKey={workRefreshKey}
                  open={!loupist.loading && !loupistSettingsOpen && !assetPageOpen && !galleryOpen && !packReview && tab === "chat" && workspace.open}
                  session={workspaceSession}
                  theme={resolvedTheme}
                  width={workspace.panelWidth}
                  height={workspace.panelHeight}
                  placement={workspace.placement}
                  onWidthChange={workspace.setPanelWidth}
                  onHeightChange={workspace.setPanelHeight}
                  onPlacementToggle={(next, kind) => workspace.togglePlacement(next, effectiveChatKey ?? undefined, kind)}
                  onDockKind={workspace.applyDockKind}
                  placementPinned={workspace.placementPinned}
                  onPinPlacement={workspace.pinPlacement}
                  onMoveEvidence={(delta) => { if (effectiveChatKey) workspace.moveEvidence(effectiveChatKey, delta); }}
                  onInteract={workspace.noteInteraction}
                  onSelectTab={(tabId) => {
                    if (effectiveChatKey) workspace.selectTab(effectiveChatKey, tabId);
                  }}
                  onUpdateTab={(tabId, update) => {
                    if (effectiveChatKey) {
                      workspace.updateTab(effectiveChatKey, tabId, update);
                    }
                  }}
                  onCloseTab={(workspaceTab) => {
                    if (effectiveChatKey) {
                      workspace.closeTab(effectiveChatKey, workspaceTab);
                    }
                  }}
                  onOpenArtifact={(artifact, artifactThreadId) =>
                    workspace.openArtifact(
                      selectedLoupist.id,
                      workspaceThreadId,
                      artifact,
                      artifactThreadId,
                    )
                  }
                  onAskArtifact={(artifact) => {
                    if (!effectiveChatKey) return;
                    setTab("chat");
                    pendingComposerFocus.current = effectiveChatKey;
                    workspace.selectArtifactForChat(effectiveChatKey, artifact);
                    if (window.matchMedia(workspaceOverlayMedia).matches) workspace.close();
                  }}
                  askArtifactDisabled={activeChat?.status === "streaming" || activeChat?.status === "submitted" || Boolean(recoveredChat)}
                  onOpenKnowledge={(location) => {
                    if (location.surface === "world") openKnowledgePage(location);
                    else workspace.openMemory(selectedLoupist.id, workspaceThreadId, location, location.surface === "note" ? i18n.t("app:app.savedNote") : location.surface === "lens" ? "Lens" : i18n.t("app:app.source"));
                  }}
                  onOpenKnowledgePage={openKnowledgePage}
                  onAskKnowledge={useKnowledgeInChat}
                  onNewBrowser={() =>
                    workspace.openBrowser(selectedLoupist.id, workspaceThreadId)
                  }
                  onOpenLink={(url) =>
                    workspace.openBrowser(selectedLoupist.id, workspaceThreadId, url)
                  }
                  onClose={() => workspace.dismiss(effectiveChatKey ?? undefined)}
                  kind={workspace.kind}
                >
                  <div className="workspace-lens-content" hidden={workspace.kind !== "lens"} inert={workspace.kind !== "lens" || undefined}>
                    {lens.visited && <Suspense fallback={<Spinner />}><MemoryPage key={`${selectedLoupist.id}:${memoryPage?.loupistId === selectedLoupist.id ? memoryPage.key : lens.navigation?.current.recordId ?? ""}`} bridge={bridge} loupist={selectedLoupist}
                      threads={threads} initialItem={memoryPage?.loupistId === selectedLoupist.id ? memoryPage.item : lens.navigation?.current.recordId}
                      onBack={() => workspace.dismiss(effectiveChatKey ?? undefined)} onOpenChat={id => { chat.setActiveThreadForLoupist(selectedLoupist.id, id); setTab("chat"); }}
                      onOpenMap={() => setTab("map")} onChanged={refreshKnowledge} onOpenLink={chatWorkspaceActions.openLink}
                      onOpenKnowledge={openKnowledgePage} onAsk={useKnowledgeInChat} /></Suspense>}
                  </div>
                  <div className="workspace-world-content" hidden={workspace.kind !== "world"} inert={workspace.kind !== "world" || undefined}>
                    <Suspense fallback={<Spinner />}>
                      {memory.navigation ? (
                        <MemoryWorkspace
                          key={selectedLoupist.id}
                          bridge={bridge}
                          loupist={selectedLoupist}
                          navigation={memory.navigation}
                          active={workspace.open && workspace.kind === "world"}
                          refreshKey={knowledgeRefresh}
                          onPatch={memory.patch}
                          onOpen={openKnowledgePage}
                          onBack={backKnowledgePage}
                          onOpenChat={threadId => { chat.setActiveThreadForLoupist(selectedLoupist.id, threadId); setTab("chat"); }}
                          onAsk={useKnowledgeInChat}
                          onExplainDisagreement={explainDisagreement}
                          onAskInChat={() => { setTab("chat"); requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>('textarea[data-chat-composer]')?.focus()); }}
                          onOpenLink={chatWorkspaceActions.openLink}
                          onManageProviders={() => openSettings("model")}
                          onManageProcessing={() => { void openLoupistSettingsFor(selectedLoupist.id, false, "world"); }}
                        />
                      ) : null}
                    </Suspense>
                  </div>
                </ChatWorkspacePanel>
              )}
          </div>
          </OpenEvidenceProvider>
          </ChatMapProvider>
        </SidebarInset>
        </div>
        <Dialog open={Boolean(noteLocation)} onOpenChange={(open) => { if (!open) { setNoteTrail([]); setNoteReaderOpen(false); } }}>
          <DialogContent className="knowledge-note-dialog sm:max-w-2xl">
            <DialogTitle className="sr-only">{i18n.t("app:app.savedNote")}</DialogTitle>
            {noteTrail.length > 1 ? <Button variant="ghost" onClick={() => setNoteTrail((current) => current.slice(0, -1))}>{i18n.t("app:app.backToNote")}</Button> : null}
            {noteLocation ? <ResearchReader bridge={bridge} location={noteLocation} onOpen={openKnowledgePage} onAsk={useKnowledgeInChat} onOpenLink={(url) => { setNoteReaderOpen(false); chatWorkspaceActions.openLink(url); }} refreshKey={knowledgeRefresh} /> : null}
          </DialogContent>
        </Dialog>

        {appDialogs}
        <PackLinkDialog bridge={bridge} open={packLinkOpen} onOpenChange={setPackLinkOpen} onOpened={result => openPackReview(result)} />
        <DeepLinkConfirm bridge={bridge} source={packReview ? null : deepLinks.link} onClose={deepLinks.clear} onOpened={result => openPackReview(result)} />
        <CommandPalette
          open={palette.open}
          focusGroup={palette.focus}
          onOpenChange={(next) => setPalette((current) => ({ ...current, open: next }))}
          bridge={bridge}
          loupists={loupists}
          selectedLoupistId={selectedLoupistId}
          threadsByLoupist={chat.threadsByLoupist}
          attention={attention}
          actions={paletteActions}
          onOpenAttention={(item) => void openAttention(item)}
          onOpenThread={(loupistId, threadId) => { selectLoupistFromNavigation(loupistId, threadId); setTab("chat"); }}
          onOpenMapNode={(loupistId, node) => openPaletteMapNode(loupistId, node)}
          onOpenPrinciple={(loupistId, recordId) => openPalettePrinciple(loupistId, recordId)}
          onOpenBriefing={(loupistId, runId) => { if (loupistId !== selectedLoupistId) selectLoupistFromNavigation(loupistId); setInboxFocus({ loupistId, runId }); setTab("schedule"); }}
        />
      </SidebarProvider>
      </SourceFaviconProvider>
      </FixMemoryProvider></LookBackProvider></AssetPageOpenerProvider>
    </TooltipProvider>
  );
}
