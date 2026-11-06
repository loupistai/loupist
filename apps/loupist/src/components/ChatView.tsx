import i18n, { useTranslation } from "@/i18n";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type Dispatch,
  type SetStateAction,
} from "react";
import type { Chat } from "@ai-sdk/react";
import type { FileUIPart } from "ai";
import { InfoIcon, Settings2Icon } from "lucide-react";
import { Button } from "./ui/button";
import { errorMessage } from "../lib/errors";
import { useLoupistChat } from "../chat/useLoupistChat";
import { useChatMap } from "./workspace/ChatMapContext";
import { stepEntityIds } from "./workspace/chatMapData";
import {
  withVisibleSteers,
  type VisibleSteer,
} from "../chat/visibleSteers";
import type { LoupistUIMessage } from "../chat/ai-sdk";
import { createChatDraftSession } from "../chat/draftSession";
import type {
  ChatArtifactRef,
  ChatMessage,
  ChatThread,
  LoupistBridge,
  LoupistSummary,
  KnowledgeLocation,
  KnowledgePin,
  ModelCatalog,
  ModelNotice,
  GraphJobView,
  ModelRunSelection,
} from "../types";
import { visibleCatalogModels } from "../lib/modelSelection";
import { quoteChip, type ComposerChip } from "../chat/composerChips";
import { retainedEvidenceFor, type OpenLinkOptions } from "../chat/links";

const NO_CHIPS: ComposerChip[] = [];
import { searchComposerContext } from "../chat/searchComposerContext";
import type { RecoveredChatRun } from "../chat/loupistRunRegistry";
import { ChatComposer } from "./chat/ChatComposer";
import type { ComposerOption } from "./chat/ComposerAssist";
import { ChatConversation } from "./chat/ChatConversation";
import { ChatSetAsideProvider, declinedSensitiveMemory } from "../chat/chatSetAside";
import { ChatExecutionProvider } from "./chat/ChatExecutionContext";
import { ChatWorkingContext } from "./chat/ChatWorkingContext";
import { ChatRail, type ChatRailActions } from "./chat/ChatRail";
import { railData, railMode, type RailUsing } from "../chat/chatRail";
import { knowledgeLocationFromReference } from "../knowledge/location";
import { composeMapExport } from "./map/export";
import { formatDate, formatNumber } from "@/i18n/format";
import type { EvidenceSourceOption, IncomingFiles, MapScreenshot } from "./chat/ChatComposer";
import { ChatLightboxProvider } from "./chat/ChatLightbox";
import {
  ChatPendingQueue,
  type QueuedChatMessage,
} from "./chat/ChatPendingQueue";

import { dismissLensProposal } from "../chat/dismissLensProposal";
import { emptyChatThread, isEmptyChatThreadId } from "../app/emptyChat";
import { useOnline } from "../app/useOnline";
import { providerShortName } from "../chat/providerNames";
import type { ConnectSheetTarget } from "./settings/ConnectSheet";
import { reconnectTarget } from "./settings/ProviderCard";
import type { ProviderErrorAction } from "./chat/ChatErrorState";
import { ModelNoticeLine } from "./chat/ModelNoticeLine";
import type { ChatDraft, ChatSessionDrafts } from "../app/useChatDrafts";

const t = i18n.getFixedT(null, "chat");

const chatPins = (pins: readonly KnowledgePin[]) =>
  pins.map((pin) => ({
    kind: pin.kind,
    recordId: pin.recordId,
    revisionId: pin.revisionId,
    objectId: pin.objectId,
    spanId: pin.spanId,
    ...(pin.selection ? { selection: pin.selection } : {}),
    label: pin.label,
  }));
/** Workspace-launch callbacks already bound to the visible loupist and thread. */
export type ChatWorkspaceActions = {
  openEvidence?: (evidence: import("../types").ChatEvidenceRef, step?: { label: string; itemId?: string }) => void;
  openFile?: (path?: string) => void;
  readWorldJob?: (jobId: string) => Promise<GraphJobView>;
  onWorldChanged?: () => void;
  openMemory: (location: KnowledgeLocation, title: string) => void;
  openMemoryPage: (location: KnowledgeLocation) => void;
  openArtifact: (threadId: string, artifact: ChatArtifactRef) => void;
  openLink: (url: string) => void;
};

type Props = {
  active?: boolean;
  theme?: "light" | "dark";
  bridge: LoupistBridge;
  chat: Chat<LoupistUIMessage>;
  recoveredRun?: RecoveredChatRun;
  loupist: LoupistSummary;
  threads: ChatThread[];
  setThreads: Dispatch<SetStateAction<ChatThread[]>>;
  activeThreadId: string | null;
  setActiveThreadId: (id: string) => void;
  onCreateThread: () => Promise<ChatThread | null>;
  knowledgePins: KnowledgePin[];
  selectedArtifact?: ChatArtifactRef;
  onClearSelectedArtifact?: () => void;
  onConsumeSelectedArtifact?: (artifact: ChatArtifactRef, threadId?: string) => void;
  onRemoveKnowledgePin: (pin: KnowledgePin) => void;
  onAddKnowledgePin?: (pin: KnowledgePin) => void;
  onSelectArtifact?: (artifact: ChatArtifactRef) => void;
  workspaceActions: ChatWorkspaceActions;
  modelCatalog: ModelCatalog | null;
  modelSelection: ModelRunSelection;
  modelSetupMessage?: string;
  skillPreferences?: Record<string, { enabled: boolean }>;
  memoryDefaults?: { use_memory: boolean; update_memory: boolean };
  configurationReady?: boolean;
  configurationError?: string;
  /** Opens Settings › Models & providers, optionally on a connect sheet (a Chat error's account action). */
  onOpenModelSettings?: (target?: ConnectSheetTarget) => void;
  /** The pending "Now using …" line Rust recorded (W17.5). */
  modelNotice?: ModelNotice | null;
  onAcknowledgeModelNotice?: (id: string) => void;
  /** Nothing chosen and no Chat default: sends name no model, and Rust resolves Auto at admission. */
  hostAuto?: boolean;
  onOpenSchedule?: (taskId: string) => void;
  onOpenChatPreferences?: () => void;
  /** Opens Settings › Connections (the morning message's "Works best with" line). */
  onOpenConnections?: () => void;
  /** Opens a briefing run in the Briefings tab (morning message). */
  onOpenBriefing?: (runId: string) => void;
  initialPrompt?: string;
  draft?: ChatDraft;
  sessionDrafts?: ChatSessionDrafts;
  onConsumeKnowledgePins?: (pins: KnowledgePin[]) => void;
  onDraftChange?: (prompt: string) => void;
  /** Saves an inline title edit through the ordinary rename. */
  onRenameThread?: (thread: ChatThread, title: string) => Promise<boolean>;
  onChipsChange?: (chips: ComposerChip[]) => void;
  /** Sets or clears the draft's as-of date. */
  onAsOfChange?: (asOf: string | undefined) => void;
  onModelSelectionChange: (selection: ModelRunSelection) => void;
  modelAuto: boolean;
  onModelResetToAuto: () => void;
  onModelUsed: (selection: ModelRunSelection) => void;
  onResolveModelSelection?: () => Promise<ModelRunSelection & { auto?: boolean; memoryDefaults?: { use_memory: boolean; update_memory: boolean } }>;
  onAbortRecoveredRun: () => void;
  onTurnComplete?: (message: ChatMessage, threadId: string) => void;
  onRunningChange?: (running: boolean) => void;
  progressFocus?: string | null;
};

export function ChatView({
  onOpenBriefing,
  active = true,
  theme = "dark",
  bridge,
  chat,
  recoveredRun,
  loupist,
  threads,
  setThreads,
  activeThreadId,
  setActiveThreadId,
  onCreateThread,
  knowledgePins,
  selectedArtifact,
  onClearSelectedArtifact,
  onConsumeSelectedArtifact,
  onRemoveKnowledgePin,
  onAddKnowledgePin,
  onSelectArtifact,
  workspaceActions,
  modelCatalog,
  modelSelection,
  modelSetupMessage,
  skillPreferences,
  memoryDefaults,
  configurationReady = true,
  configurationError,
  onOpenModelSettings,
  modelNotice = null,
  onAcknowledgeModelNotice,
  hostAuto = false,
  onOpenSchedule,
  onOpenChatPreferences,
  onOpenConnections,
  initialPrompt,
  draft,
  sessionDrafts,
  onConsumeKnowledgePins,
  onDraftChange,
  onChipsChange,
  onAsOfChange,
  onModelSelectionChange,
  modelAuto,
  onModelResetToAuto,
  onModelUsed,
  onResolveModelSelection,
  onAbortRecoveredRun,
  onTurnComplete,
  onRunningChange,
  progressFocus,
}: Props) {
  useTranslation("chat");
  const visibleDraft = useRef(draft);
  visibleDraft.current = draft;
  const [prompt, setLocalPrompt] = useState(() => initialPrompt ?? "");
  const taughtFeedback = useRef("");
  const setPrompt = useCallback((nextPrompt: string) => {
    if (!nextPrompt.trim()) taughtFeedback.current = "";
    setLocalPrompt(nextPrompt);
    onDraftChange?.(nextPrompt);
  }, [onDraftChange]);
  const promptRef = useRef(prompt);
  promptRef.current = prompt;
  const teachFeedback = useCallback((text: string) => {
    const current = promptRef.current;
    const previous = taughtFeedback.current;
    if (current.includes(text)) {
      taughtFeedback.current = text;
      return;
    }
    if (previous && current.includes(previous)) {
      taughtFeedback.current = text;
      setPrompt(current.replace(previous, text));
      return;
    }
    // Preserve edits to this teaching draft, while allowing a different correction.
    if (previous === text && current.trim()) return;
    taughtFeedback.current = text;
    setPrompt(current.trim() ? `${current.trimEnd()}\n\n${text}` : text);
  }, [setPrompt]);
  const memoryChoices = useRef<{ loupistId?: string; threadId?: string; privateChat?: boolean }>({});
  const [privateChat, setPrivateChat] = useState(false);
  const [composerSkills, setComposerSkills] = useState<ComposerOption[]>(
    [],
  );
  const [composerPlugins, setComposerPlugins] = useState<
    ComposerOption[]
  >([]);
  const draftSession = useMemo(() => sessionDrafts ?? createChatDraftSession(), [sessionDrafts]);
  const { queued: queuedByThread, steers: steersByThread, holds: queueHoldByThread } = useSyncExternalStore(draftSession.subscribe, draftSession.getSnapshot);
  const setSteersByThread = useCallback((updater: (current: Record<string, VisibleSteer[]>) => Record<string, VisibleSteer[]>) => {
    draftSession.update((current) => ({ ...current, steers: updater(current.steers) }));
  }, [draftSession]);
  const setQueueHoldByThread = useCallback((updater: (current: Record<string, string | null>) => Record<string, string | null>) => {
    draftSession.update((current) => ({ ...current, holds: updater(current.holds) }));
  }, [draftSession]);
  const messagesRef = useRef<ChatMessage[]>([]);
  const creatingThread = useRef(false);
  const [isCreatingThread, setIsCreatingThread] = useState(false);
  const [configurationReadError, setConfigurationReadError] = useState<string | null>(null);
  // Edit forks (X18): the sent message being edited; sending sets the old version aside first.
  const [editing, setEditing] = useState<{ threadId: string; messageId: string; position: number; content: string } | null>(null);
  const [pendingSubmission, setPendingSubmission] = useState<{ threadId: string; item: QueuedChatMessage } | null>(null);
  const admitting = useRef(new Set<string>());
  const selectedModelProfileId = modelSelection.model_profile_id;
  const selectedModelAvailable = configurationReady && visibleCatalogModels(modelCatalog).some(
    (model) => model.id === selectedModelProfileId && model.credential_source !== "missing",
  );
  const workspaceAvailable = loupist.availability.status === "available";
  const selectedThinkingLevel = modelSelection.thinking_level;
  const selectedRunOptions = modelSelection.run_options;
  const activeThread = useMemo<ChatThread | undefined>(() => {
    if (isEmptyChatThreadId(activeThreadId)) {
      return emptyChatThread(loupist.id);
    }
    return threads.find((thread) => thread.id === activeThreadId) ?? threads[0];
  }, [activeThreadId, loupist.id, threads]);
  const readAttachment = useCallback(
    async (messageId: string, attachmentId: string) => {
      if (!activeThread?.id) throw new Error(t("chatView.chatThreadIsUnavailable"));
      const result = await bridge.chatAttachmentRead(
        loupist.id,
        activeThread.id,
        messageId,
        attachmentId,
      );
      return result.data_url;
    },
    [activeThread?.id, bridge, loupist.id],
  );
  const {
    messages,
    messageKeys,
    isRunning: localIsRunning,
    isStopping: localIsStopping,
    announcement: localStreamAnnouncement,
    error: localError,
    submit,
    steer,
    commitSteers,
    abort,
    retryLast,
    saveFeedback,
    replaceThread,
  } = useLoupistChat({
    bridge,
    chat,
    loupistId: loupist.id,
    activeThread,
    setThreads,
  });
  messagesRef.current = messages;
  // The running reply's step names light the chat's mini map (chat-run.md A4).
  const chatMap = useChatMap();
  const reportLive = chatMap?.reportLive;
  const lastMessage = messages.at(-1);
  const liveNames = localIsRunning && lastMessage ? stepEntityIds(lastMessage).join("\n") : "";
  useEffect(() => {
    const threadId = activeThread?.id;
    if (!reportLive || !threadId) return;
    reportLive(loupist.id, threadId, liveNames ? liveNames.split("\n") : []);
    return () => reportLive(loupist.id, threadId, []);
  }, [reportLive, loupist.id, activeThread?.id, liveNames]);
  const activeRecoveredRun = localIsRunning ? undefined : recoveredRun;
  const isRunning = localIsRunning || Boolean(activeRecoveredRun);

  // "In this chat" (context-rail.md R1): placed by the chat column's own width, so the panel and sidebar count.
  const chatWorkspaceRef = useRef<HTMLDivElement>(null);
  const [chatWidth, setChatWidth] = useState(0);
  useEffect(() => {
    const element = chatWorkspaceRef.current;
    if (!element) return undefined;
    const update = () => setChatWidth(element.clientWidth);
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const railChatId = activeThread?.id ?? "";
  const railMap = chatMap?.loupistId === loupist.id && chatMap.chatId === railChatId ? chatMap : undefined;
  const rail = useMemo(() => railData({
    messages, pins: draft?.pins ?? [], chatId: railChatId, language: i18n.language, workspacePath: loupist.workspace_path,
    map: railMap?.map, events: railMap?.events, learningPending: railMap?.learningPending,
  }), [messages, draft?.pins, railChatId, loupist.workspace_path, railMap?.map, railMap?.events, railMap?.learningPending, i18n.language]);
  const chatRailMode = !railChatId || isEmptyChatThreadId(railChatId) ? "none" : railMode(chatWidth, rail.hasCards);
  const openRailUsing = useCallback((item: RailUsing) => {
    if (item.recordId) { workspaceActions.openMemory({ loupistId: loupist.id, surface: "lens", recordId: item.recordId }, item.label); return; }
    const pin = item.pin;
    if (pin?.recordId && pin.revisionId) {
      workspaceActions.openMemory(knowledgeLocationFromReference(loupist.id, { kind: pin.kind, recordId: pin.recordId, revisionId: pin.revisionId, objectId: pin.objectId, spanId: pin.spanId }), pin.label);
    }
  }, [loupist.id, workspaceActions]);
  // "+ › Screenshot of the map" captures the map the user can see: the full Map tab or the panel's mini map.
  const mapScreenshot = useMemo<MapScreenshot>(() => {
    const canvas = () => document.querySelector<HTMLCanvasElement>(".map-view .map-canvas canvas, .chat-mini-map canvas");
    return {
      status: () => !chatMap?.map?.nodes.some((node) => node.kind === "entity") ? "empty" : canvas() ? "ready" : "closed",
      take: async () => {
        const source = canvas();
        if (!source) return null;
        const names = chatMap?.map?.nodes.filter((node) => node.kind === "entity").length ?? 0;
        const composed = composeMapExport(source, { name: loupist.name, stats: t("chatComposer.attach.mapStats", { count: names, formattedCount: formatNumber(names) }), date: formatDate(Date.now()), disclosure: i18n.t("knowledge:map.export.disclosure") });
        const blob = await new Promise<Blob | null>((resolve) => composed.toBlob(resolve, "image/png"));
        return blob ? new File([blob], `${loupist.name.replace(/[/\\:]/g, "-")}-map.png`, { type: "image/png" }) : null;
      },
    };
  }, [chatMap?.map, loupist.name]);
  // "+ › Add from Evidence": the Sources this chat saved, newest first, as whole-source pins.
  const evidenceSources = useMemo<EvidenceSourceOption[]>(() => {
    const seen = new Set<string>();
    const options: EvidenceSourceOption[] = [];
    for (const message of [...messages].reverse()) {
      const update = message.memory_update;
      if (update?.status !== "applied") continue;
      for (const change of update.changes) {
        if (change.kind !== "Source" || change.operation === "proposed" || !change.knowledge || seen.has(change.knowledge.recordId)) continue;
        seen.add(change.knowledge.recordId);
        options.push({ key: change.knowledge.recordId, label: change.title, pin: { kind: "source", loupistId: loupist.id, recordId: change.knowledge.recordId, revisionId: change.knowledge.revisionId, objectId: change.knowledge.objectId, label: change.title } });
        if (options.length >= 8) return options;
      }
    }
    return options;
  }, [messages, loupist.id]);
  // The lightbox's "Add to question": the image goes into this chat's composer tray, never sent.
  const composerKey = activeThread?.id ?? "draft";
  const [incomingFiles, setIncomingFiles] = useState<IncomingFiles & { composerKey: string } | null>(null);
  const incomingSequence = useRef(0);
  const addToQuestion = useCallback((file: File) => {
    incomingSequence.current += 1;
    setIncomingFiles({ id: incomingSequence.current, composerKey, files: [file] });
    requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>("[data-chat-composer]")?.focus());
  }, [composerKey]);
  const takeIncomingFiles = useCallback(() => setIncomingFiles(null), []);
  const railActions: ChatRailActions = {
    bridge, loupistId: loupist.id,
    onOpenFile: workspaceActions.openFile ? (path) => workspaceActions.openFile?.(path) : undefined,
    onOpenUsing: openRailUsing,
    onShowLearned: (ids) => { chatMap?.select(ids); },
  };
  const isBusy = isRunning || isCreatingThread || Boolean(pendingSubmission);
  const online = useOnline();
  // Busy (rate-limited) answers in a row per Chat, for the X11 countdown; a finished answer resets it.
  const busyStreak = useRef(new Map<string, Set<string>>());
  const streakKey = activeThread?.id ?? "";
  if (lastMessage?.role === "assistant" && lastMessage.status === "complete") busyStreak.current.delete(streakKey);
  if (lastMessage?.role === "assistant" && lastMessage.status === "error" && lastMessage.failure_kind === "rate_limited") {
    const seen = busyStreak.current.get(streakKey) ?? new Set<string>();
    seen.add(lastMessage.id);
    busyStreak.current.set(streakKey, seen);
  }
  const retryAttempt = busyStreak.current.get(streakKey)?.size ?? 1;
  const providerLabel = useCallback((providerId?: string) => {
    if (!providerId) return undefined;
    const option = modelCatalog?.providers.find((provider) => provider.id === providerId);
    return providerShortName(providerId, option?.status?.credential_kind) ?? option?.label
      ?? modelCatalog?.custom_providers?.find((provider) => provider.id === providerId)?.name;
  }, [modelCatalog]);
  /** A failed answer's account action: Rust opens usage pages; the rest open the connect sheet in Settings. */
  const providerAction = useCallback((action: ProviderErrorAction, providerId?: string) => {
    if (action === "manage_usage" || action === "open_claude") {
      void bridge.providerOpenUsage({ provider: action === "open_claude" ? "anthropic" : providerId ?? "openai" }).catch((cause) => setConfigurationReadError(errorMessage(cause, t("chatError.manageUsage"))));
      return;
    }
    if (!onOpenModelSettings) return;
    if (action === "use_another") return onOpenModelSettings({ kind: "picker" });
    if (action === "use_api_key") return onOpenModelSettings({ kind: "provider", provider: providerId === "anthropic" ? "anthropic" : "openai", method: "api_key" });
    const option = modelCatalog?.providers.find((provider) => provider.id === providerId);
    const custom = modelCatalog?.custom_providers?.find((provider) => provider.id === providerId);
    onOpenModelSettings(option ? reconnectTarget(option) : custom ? { kind: "custom", edit: custom } : undefined);
  }, [bridge, modelCatalog, onOpenModelSettings]);
  const completedTurnRef = useRef<string | null>(null);
  useEffect(() => {
    onRunningChange?.(isBusy);
  }, [isBusy, onRunningChange]);
  useEffect(() => {
    if (isRunning) {
      completedTurnRef.current = activeThread?.id ?? null;
      return;
    }
    if (!completedTurnRef.current || completedTurnRef.current !== activeThread?.id) return;
    const last = messages.at(-1);
    if (!last || last.role !== "assistant" || last.status === "streaming") return;
    completedTurnRef.current = null;
    if (last.status === "complete") onTurnComplete?.(last, activeThread.id);
  }, [activeThread?.id, isRunning, messages, onTurnComplete]);
  const openChatArtifact = useCallback(
    (artifact: ChatArtifactRef) => {
      if (activeThread?.id) {
        workspaceActions.openArtifact(activeThread.id, artifact);
      }
    },
    [activeThread?.id, workspaceActions],
  );
  // Read through a ref so the opener stays stable while answers stream (earlier answers do not rerender).
  const linkMessages = useRef(activeThread?.messages ?? []);
  linkMessages.current = activeThread?.messages ?? [];
  const openChatLink = useCallback(
    async (url: string, options?: OpenLinkOptions) => {
      if (options?.external) { await bridge.openExternalUrl(url); return; }
      // A page the Loupist already kept opens its retained copy in Evidence.
      const retained = workspaceActions.openEvidence ? retainedEvidenceFor(linkMessages.current, url) : undefined;
      if (retained && workspaceActions.openEvidence) { workspaceActions.openEvidence(retained); return; }
      workspaceActions.openLink(url);
    },
    [bridge, workspaceActions],
  );
  const streamAnnouncement = activeRecoveredRun
    ? activeRecoveredRun.abort_requested
      ? t("chatView.stoppingTheRecoveredResponse")
      : activeRecoveredRun.status === "reconciling"
        ? t("chatView.refreshingTheCompletedResponse")
        : t("chatView.loupistIsContinuingAResponseStartedBeforeReload")
    : localStreamAnnouncement;
  const error = activeRecoveredRun?.error ?? localError;
  const abortActiveRun = useCallback(() => {
    if (localIsRunning) void abort();
    else onAbortRecoveredRun();
  }, [abort, localIsRunning, onAbortRecoveredRun]);

  useEffect(() => {
    if (!activeThreadId && threads[0]) setActiveThreadId(threads[0].id);
  }, [activeThreadId, setActiveThreadId, threads]);

  useEffect(() => {
    setLocalPrompt(initialPrompt ?? "");
    if (!initialPrompt?.trim()) taughtFeedback.current = "";
  }, [activeThreadId, initialPrompt]);

  useEffect(() => {
    taughtFeedback.current = "";
  }, [loupist.id, activeThreadId]);

  useEffect(() => {
    if (memoryChoices.current.loupistId !== loupist.id || memoryChoices.current.threadId !== activeThread?.id) memoryChoices.current = { loupistId: loupist.id, threadId: activeThread?.id };
    const saved = activeThread && !isEmptyChatThreadId(activeThread.id);
    const remembered = saved ? activeThread.use_memory && activeThread.update_memory : (memoryDefaults?.use_memory ?? true) && (memoryDefaults?.update_memory ?? true);
    setPrivateChat(memoryChoices.current.privateChat ?? !remembered);
  }, [loupist.id, activeThread?.id, activeThread?.update_memory, activeThread?.use_memory, memoryDefaults?.use_memory, memoryDefaults?.update_memory]);

  useEffect(() => {
    let cancelled = false;
    void Promise.all([
      bridge.agentSkillCatalog(loupist.id),
      bridge.loupistCapabilityList(loupist.id),
      bridge.marketplaceSnapshot(),
    ])
      .then(([skills, bindings, marketplace]) => {
        if (cancelled) return;
        setComposerSkills(
          skills
            .filter((skill) => skill.enabled)
            .map((skill) => ({
              id: skill.id,
              name: skill.name,
              description: skill.description,
            })),
        );
        const entriesById = new Map(
          marketplace.catalog.entries.map((entry) => [entry.id, entry]),
        );
        setComposerPlugins(
          bindings.flatMap((binding) => {
            if (!binding.enabled) return [];
            const entry = entriesById.get(binding.entry_id);
            const connector = marketplace.connectors.find((item) => item.entry_id === binding.entry_id);
            if (entry?.kind !== "data_source" || connector?.readiness !== "ready") return [];
            return [
              {
                id: binding.entry_id,
                name: entry.name,
                description: entry.summary,
              },
            ];
          }),
        );
      })
      .catch(() => {
        if (cancelled) return;
        setComposerSkills([]);
        setComposerPlugins([]);
      });
    return () => {
      cancelled = true;
    };
  }, [bridge, loupist.enabled_skill_ids, loupist.id, skillPreferences]);

  const updateQueued = useCallback((
    threadId: string,
    updater: (items: QueuedChatMessage[]) => QueuedChatMessage[],
  ) => {
    draftSession.update((current) => ({ ...current, queued: { ...current.queued, [threadId]: updater(current.queued[threadId] ?? []) } }));
  }, [draftSession]);

  const dispatchSubmission = useCallback(async (threadId: string, item: QueuedChatMessage) => {
    const modelAvailable = visibleCatalogModels(modelCatalog).some(
      (model) => (!item.modelProfileId || model.id === item.modelProfileId) && model.credential_source !== "missing",
    );
    if (!workspaceAvailable || !modelAvailable) {
      setQueueHoldByThread((current) => ({ ...current, [threadId]: workspaceAvailable
        ? t("chatView.connectTheQueuedMessageSModelProviderBeforeSending")
        : t("chatView.reconnectTheWorkspaceFolderBeforeSendingQueuedMessagesWere") }));
      return;
    }
    if (admitting.current.has(item.id)) return;
    admitting.current.add(item.id);
    setQueueHoldByThread((current) => ({ ...current, [threadId]: null }));
    if (item.modelProfileId) onModelUsed({ model_profile_id: item.modelProfileId, thinking_level: item.thinkingLevel, run_options: item.runOptions });
    const receipt = await submit(threadId, item.text, item.privateChat,
      item.modelProfileId, item.thinkingLevel, item.runOptions, item.files, item.pins, item.selectedArtifact, item.asOf);
    admitting.current.delete(item.id);
    if (receipt.accepted) {
      updateQueued(threadId, (items) => items.filter((entry) => entry.id !== item.id));
    } else {
      // Keep the complete request for explicit retry, without replacing newer input.
      const reason = t("chatView.retainedRequestError", { error: receipt.error });
      setQueueHoldByThread((current) => ({ ...current, [threadId]: reason }));
    }
  }, [modelCatalog, workspaceAvailable, onModelUsed, setQueueHoldByThread, submit, updateQueued]);

  useEffect(() => {
    if (!pendingSubmission || activeThread?.id !== pendingSubmission.threadId || isRunning) return;
    const { threadId, item } = pendingSubmission;
    setPendingSubmission(null);
    void dispatchSubmission(threadId, item);
  }, [activeThread?.id, isRunning, pendingSubmission, dispatchSubmission]);

  const captureSubmission = (text: string, files: FileUIPart[], asOf: string | null | undefined = draft?.asOf): QueuedChatMessage => ({
    id: crypto.randomUUID(), text, createdAt: new Date().toISOString(),
    ...(asOf ? { asOf } : {}),
    privateChat, modelProfileId: hostAuto ? "" : selectedModelProfileId,
    thinkingLevel: hostAuto ? "" : selectedThinkingLevel, runOptions: hostAuto ? {} : { ...selectedRunOptions },
    files: files.map((file) => ({ ...file })), pins: chatPins(knowledgePins),
    selectedArtifact: selectedArtifact && { artifact_id: selectedArtifact.artifact_id, revision: selectedArtifact.revision },
  });
  const withArtifactRange = (raw: string) => {
    const text = raw.trim();
    return text && selectedArtifact?.selected_range && !text.includes(t("chatView.chartRange", selectedArtifact.selected_range))
      ? t("chatView.withChartRange", { text, ...selectedArtifact.selected_range })
      : text;
  };
  const consumeComposer = (submittedText: string, threadId = activeThread?.id) => {
    // PromptInput owns consuming just the submitted file IDs after conversion.
    if (selectedArtifact) onConsumeSelectedArtifact?.(selectedArtifact, threadId);
    onConsumeKnowledgePins?.(knowledgePins);
    if (!draft || draft.prompt.trim() === submittedText.trim()) {
      if (visibleDraft.current === draft) setPrompt("");
      else onDraftChange?.("");
    }
  };
  const enqueueFollowUp = (submittedText = prompt, files: FileUIPart[] = [], draftText?: string) => {
    const text = withArtifactRange(submittedText);
    if (!isRunning || (!text && !files.length) || !activeThread?.id || !selectedModelAvailable || !workspaceAvailable) return false;
    const threadId = activeThread.id;
    const item = captureSubmission(text, files);
    // File conversion can finish after this response's completion was observed.
    draftSession.update((current) => ({
      ...current,
      queued: { ...current.queued, [threadId]: [...(current.queued[threadId] ?? []), item] },
      holds: { ...current.holds, [threadId]: null },
      consumeThreads: { ...current.consumeThreads, [threadId]: true },
    }));
    consumeComposer(draftText ?? submittedText);
    return true;
  };

  const send = async (submittedPrompt?: string, files: FileUIPart[] = [], options?: { draftText?: string; preserveDraft?: boolean; asOf?: string | null }) => {
    const text = withArtifactRange(submittedPrompt ?? prompt);
    if ((!text && files.length === 0) || !selectedModelAvailable || !workspaceAvailable) return false;
    // The "Now using …" line was seen once this message goes out.
    if (modelNotice && !isRunning) onAcknowledgeModelNotice?.(modelNotice.id);
    if (isRunning) {
      if (selectedArtifact || knowledgePins.length > 0 || files.length > 0) return enqueueFollowUp(text, files, options?.draftText);
      if (!text || !activeThread?.id || isEmptyChatThreadId(activeThread.id)) return false;
      const receipt = await steer(text);
      if (receipt) {
        const threadId = activeThread.id;
        commitSteers([receipt]);
        setSteersByThread((current) => ({ ...current, [threadId]: [...(current[threadId] ?? []), receipt] }));
        if (!options?.preserveDraft) consumeComposer(options?.draftText ?? submittedPrompt ?? prompt);
      }
      return Boolean(receipt);
    }
    if (editing && activeThread?.id === editing.threadId) {
      try {
        replaceThread(await bridge.chatBranch({ loupist_id: loupist.id, thread_id: editing.threadId, message_id: editing.messageId, position: editing.position, content: editing.content }));
      } catch (cause) {
        setConfigurationReadError(errorMessage(cause, t("chatBranch.couldNotEdit")));
        return false;
      } finally {
        setEditing(null);
      }
    }
    // Freeze settings and evidence before the asynchronous Chat creation.
    // Retry repeats the failed request's own as-of date (null: none).
    const item = captureSubmission(text, files, options?.asOf === undefined ? draft?.asOf : options.asOf);
    const memoryChoice = { ...memoryChoices.current };
    if (onResolveModelSelection) {
      try {
        const selection = await onResolveModelSelection();
        if (!selection.auto && !selection.model_profile_id) throw new Error(t("chatView.chooseAnAvailableModelInChatOrSettingsChat"));
        item.modelProfileId = selection.model_profile_id;
        item.thinkingLevel = selection.thinking_level;
        item.runOptions = { ...selection.run_options };
        if ((!activeThread || isEmptyChatThreadId(activeThread.id)) && selection.memoryDefaults) {
          item.privateChat = memoryChoice.privateChat ?? !(selection.memoryDefaults.use_memory && selection.memoryDefaults.update_memory);
        }
        setConfigurationReadError(null);
      } catch (cause) {
        setConfigurationReadError(errorMessage(cause, t("chatView.couldNotReadConfigurationBeforeSending")));
        return false;
      }
    }
    let targetThread = activeThread && !isEmptyChatThreadId(activeThread.id) ? activeThread : undefined;
    if (!targetThread) {
      if (creatingThread.current) return false;
      creatingThread.current = true;
      setIsCreatingThread(true);
      try {
        targetThread = (await onCreateThread()) ?? undefined;
        if (targetThread) {
          memoryChoices.current = { loupistId: loupist.id, threadId: targetThread.id, privateChat: item.privateChat };
          setPrivateChat(item.privateChat);
        }
      }
      finally { creatingThread.current = false; setIsCreatingThread(false); }
    }
    if (!targetThread) return false;
    updateQueued(targetThread.id, (items) => [...items, item]);
    if (!options?.preserveDraft) consumeComposer(options?.draftText ?? submittedPrompt ?? prompt, targetThread.id);
    if (!online) {
      // Offline (X11): the message waits in the queue and is sent when the connection returns.
      const threadId = targetThread.id;
      setQueueHoldByThread((current) => ({ ...current, [threadId]: t("chatError.waitingOffline") }));
      return true;
    }
    if (activeThread?.id !== targetThread.id) setPendingSubmission({ threadId: targetThread.id, item });
    else void dispatchSubmission(targetThread.id, item);
    return true;
  };

  /** Try again: Rust clears the failed tail, then the request is sent again with current settings. */
  const retryFailed = () => {
    if (!selectedModelAvailable || !workspaceAvailable || isRunning) return;
    const originThread = activeThread?.id;
    const originDraft = draft;
    void (async () => {
      const replay = await retryLast();
      if (!replay || !originThread) return;
      const restore = () => {
        if (originDraft) {
          originDraft.prompt = replay.prompt;
          originDraft.files = replay.files.map((file, index) => ({ ...file, id: `retry-${index}` }));
        }
        if (activeThread?.id === originThread) setPrompt(replay.prompt);
      };
      if (replay.detached || activeThread?.id !== originThread) {
        restore();
        return;
      }
      const started = await send(replay.prompt, replay.files, { preserveDraft: true, asOf: replay.asOf ?? null });
      if (!started) restore();
    })();
  };
  // W17.5: a retired model (model_gone) is retried once on Auto by Rust within the same
  // turn; the answer then names the model it replaced. The renderer never resends.
  /** "Change" on the default-changed line opens the run-profile menu. */
  const [modelMenuRequest, setModelMenuRequest] = useState(0);

  const sendQueuedNow = (id: string) => {
    if (!activeThread?.id || isRunning) return;
    const item = (draftSession.getSnapshot().queued[activeThread.id] ?? []).find((entry) => entry.id === id);
    if (item) void dispatchSubmission(activeThread.id, item);
  };

  useEffect(() => {
    if (isRunning && activeThread?.id) {
      draftSession.update((current) => current.consumeThreads[activeThread.id] ? current : { ...current, consumeThreads: { ...current.consumeThreads, [activeThread.id]: true } });
    }
  }, [activeThread?.id, isRunning, draftSession]);

  useEffect(() => {
    if (isRunning) return;
    const threadId = activeThread?.id;
    if (!threadId || !draftSession.getSnapshot().consumeThreads[threadId]) {
      return;
    }
    const lastStatus = messagesRef.current.at(-1)?.status;
    if (
      lastStatus !== "complete" &&
      lastStatus !== "aborted" &&
      lastStatus !== "error"
    ) {
      return;
    }
    draftSession.update((current) => ({ ...current, consumeThreads: { ...current.consumeThreads, [threadId]: false } }));
    if (draftSession.getSnapshot().holds[threadId]) return;
    const delivered = draftSession.getSnapshot().steers[threadId] ?? [];
    commitSteers(delivered);
    if (lastStatus === "aborted") {
      setQueueHoldByThread((current) => ({
        ...current,
        [threadId]: t("chatView.responseStoppedQueuedMessagesWereKept"),
      }));
      return;
    }
    if (lastStatus === "error") {
      setQueueHoldByThread((current) => ({
        ...current,
        [threadId]: t("chatView.responseFailedQueuedMessagesWereKept"),
      }));
      return;
    }
    if (lastStatus !== "complete") return;
    if (!online) {
      setQueueHoldByThread((current) => ({ ...current, [threadId]: t("chatError.waitingOffline") }));
      return;
    }
    setQueueHoldByThread((current) => ({ ...current, [threadId]: null }));
    const [next] = draftSession.getSnapshot().queued[threadId] ?? [];
    if (next) setPendingSubmission({ threadId, item: next });
  }, [
    activeThread?.id,
    draftSession,
    setQueueHoldByThread,
    commitSteers,
    isRunning,
    selectedModelProfileId,
    selectedRunOptions,
    selectedThinkingLevel,
    privateChat,
    queuedByThread,
    messages,
  ]);

  useEffect(() => {
    if (!online || isRunning || !activeThread?.id) return;
    const threadId = activeThread.id;
    if (draftSession.getSnapshot().holds[threadId] !== t("chatError.waitingOffline")) return;
    setQueueHoldByThread((current) => ({ ...current, [threadId]: null }));
    const [next] = draftSession.getSnapshot().queued[threadId] ?? [];
    if (next) setPendingSubmission({ threadId, item: next });
  }, [online, isRunning, activeThread?.id, draftSession, setQueueHoldByThread]);

  useEffect(() => {
    if (isRunning || !activeThread?.id) return;
    const threadId = activeThread.id;
    const delivered = steersByThread[threadId] ?? [];
    if (delivered.length === 0) return;
    const present = new Set(messages.map((message) => message.id));
    const remaining = delivered.filter((item) => !present.has(item.id));
    if (remaining.length === delivered.length) return;
    setSteersByThread((current) => ({ ...current, [threadId]: remaining }));
  }, [activeThread?.id, isRunning, messages, steersByThread]);

  const activeSteers = activeThread
    ? steersByThread[activeThread.id]
    : undefined;
  const visibleConversation = useMemo(() => {
    const steers = activeSteers ?? [];
    if (!isRunning || !steers.length) return withVisibleSteers(messages, messageKeys, steers);
    // While the reply runs, guidance shows inside its timeline (S4) instead of as a message.
    const guided = new Set(steers.map((item) => item.id));
    const keep = messages.map((message) => !guided.has(message.id));
    return { messages: messages.filter((_, index) => keep[index]), messageKeys: messageKeys.filter((_, index) => keep[index]) };
  }, [activeSteers, isRunning, messageKeys, messages]);
  const guidingTexts = useMemo(() => (isRunning ? (activeSteers ?? []).map((item) => item.text) : []), [activeSteers, isRunning]);
  const executionScope = useMemo(() => activeThread ? ({
    bridge, loupistId: loupist.id, threadId: activeThread.id, surface: activeThread.home ? "bot" as const : "session" as const, theme, active,
  }) : null, [active, activeThread?.id, activeThread?.home, bridge, loupist.id, theme]);
  const dismissProposal = useCallback((target: { recordId: string; messageId: string }) => {
    const chatId = activeThread?.id;
    return dismissLensProposal(bridge, {
      loupistId: loupist.id,
      recordId: target.recordId,
      messageId: target.messageId,
      chatId: chatId && !isEmptyChatThreadId(chatId) ? chatId : undefined,
    });
  }, [bridge, loupist.id, activeThread?.id]);
  /** A question from the morning message goes into the composer; it never sends. */
  const quoteToComposer = useCallback((messageId: string, text: string) => {
    const chip = quoteChip(messageId, text);
    onChipsChange?.([...(draft?.chips ?? []).filter((existing) => existing.key !== chip.key), chip]);
    requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>("[data-chat-composer]")?.focus());
  }, [draft, onChipsChange]);
  const askFromMorningMessage = useCallback((question: string) => {
    setPrompt(question);
    requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>("[data-chat-composer]")?.focus());
  }, [setPrompt]);
  const setAsideIds = useMemo(() => new Set(activeThread?.set_aside_memory ?? []), [activeThread?.set_aside_memory]);
  const firstDeclinedMessageId = useMemo(() => activeThread?.messages.find(declinedSensitiveMemory)?.id, [activeThread?.messages]);
  const saveOff = activeThread ? activeThread.update_memory === false : false;
  const setAside = useMemo(() => ({
    ids: setAsideIds,
    firstDeclinedMessageId,
    saveOff,
    toggle: async (recordId: string, value: boolean) => {
      const threadId = activeThread?.id;
      if (!threadId || isEmptyChatThreadId(threadId)) return;
      const updated = await bridge.chatMemorySetAside({ loupist_id: loupist.id, thread_id: threadId, record_id: recordId, set_aside: value });
      setThreads(current => current.map(thread => thread.id === updated.id ? { ...thread, set_aside_memory: updated.set_aside_memory ?? [] } : thread));
    },
  }), [setAsideIds, firstDeclinedMessageId, saveOff, activeThread?.id, bridge, loupist.id, setThreads]);
  const conversation = <ChatExecutionProvider key={`${loupist.id}:${activeThread?.id ?? "draft"}`} value={executionScope}><ChatSetAsideProvider value={setAside}><ChatConversation
    active={active}
    messages={visibleConversation.messages}
    messageKeys={visibleConversation.messageKeys}
    loupistName={loupist.name}
    workspacePath={loupist.workspace_path}
    loupistId={loupist.id}
    chatId={activeThread?.id}
    onOpenFile={workspaceActions.openFile}
    onOpenEvidence={workspaceActions.openEvidence}
    progressFocus={progressFocus}
    onReadWorldJob={workspaceActions.readWorldJob}
    onWorldChanged={workspaceActions.onWorldChanged}
    onSuggestion={askFromMorningMessage}
    onQuote={onChipsChange ? quoteToComposer : undefined}
    loupistFocus={loupist.philosophy}
    loupistTemplate={loupist.template}
    onOpenConnections={onOpenConnections}
    guiding={guidingTexts}
    onOpenBriefing={onOpenBriefing}
    home={activeThread?.home}
    onOpenRunChat={setActiveThreadId}
    // A Look back draft already holds its question; generic starters would compete with it.
    offerQuestions={!(draft?.asOf && knowledgePins.some((pin) => pin.kind === "journal"))}
    asOf={draft?.asOf}
    onSaveFeedback={saveFeedback}
    onTeachFeedback={teachFeedback}
    feedbackBusy={isRunning}
    run={{ running: isRunning }}
    onOpenMemory={workspaceActions.openMemory}
    onOpenMemoryPage={workspaceActions.openMemoryPage}
    onOpenArtifact={openChatArtifact}
    onAskArtifact={onSelectArtifact}
    onOpenSchedule={onOpenSchedule}
    onOpenLink={openChatLink}
    onReadAttachment={readAttachment}
    mentionSkills={composerSkills}
    mentionPlugins={composerPlugins}
    bridge={bridge}
    onDismissLensProposal={dismissProposal}
    onEditUser={(message, index) => {
      if (!activeThread?.id || isEmptyChatThreadId(activeThread.id)) return;
      setPrompt(message.content);
      // Editing reopens the message with its own as-of date (or none).
      onAsOfChange?.(message.as_of);
      setEditing({ threadId: activeThread.id, messageId: message.id, position: index, content: message.content });
      requestAnimationFrame(() => document.querySelector<HTMLTextAreaElement>("[data-chat-composer]")?.focus());
    }}
    versions={activeThread?.versions ?? []}
    onSwitchVersion={isRunning || !activeThread?.id ? undefined : (versionId) => {
      const threadId = activeThread.id;
      setEditing(null);
      void bridge.chatVersionSwitch({ loupist_id: loupist.id, thread_id: threadId, version_id: versionId })
        .then(replaceThread)
        .catch((cause) => setConfigurationReadError(errorMessage(cause, t("chatBranch.couldNotSwitch"))));
    }}
    providerLabel={providerLabel}
    retryAttempt={retryAttempt}
    onProviderAction={providerAction}
    onRetry={() => retryFailed()}
  /></ChatSetAsideProvider></ChatExecutionProvider>;

  return (
    <section className="chat-layout" aria-label={t("chatView.chatWithLoupist")}>
      <div className="chat-workspace" ref={chatWorkspaceRef} data-rail={chatRailMode}>
        <ChatLightboxProvider messages={visibleConversation.messages} onRead={readAttachment} onAddToQuestion={addToQuestion}>
        {localIsStopping || activeRecoveredRun ? <header className="workspace-heading is-status">
          <div className="workspace-chat-title">
            {localIsStopping || activeRecoveredRun ? (
              <div className="thread-statuses">
                {localIsStopping ? (
                  <div className="memory-state" role="status">
                    <span className="status-dot on" />
                    {t("chatView.stopping")}</div>
                ) : null}
                {activeRecoveredRun ? (
                  <div className="memory-state" role="status">
                    <span className="status-dot on" />
                    {activeRecoveredRun.abort_requested
                      ? t("chatView.stopping")
                      : activeRecoveredRun.status === "reconciling"
                        ? t("chatView.updatingTheAnswer")
                        : t("chatView.stillWorking")}
                  </div>
                ) : null}
              </div>
            ) : null}
          </div>
        </header> : null}
        <div className="chat-body">
        <ChatRail mode={chatRailMode} data={rail} actions={railActions} />
        <div className="chat-reading-pane">
        <ChatWorkingContext
          key={`${loupist.id}:${activeThread?.id ?? "draft"}`}
          bridge={bridge}
          loupistId={loupist.id}
          chatId={activeThread?.id ?? ""}
          isRunning={isRunning}
          updateKey={`${messages.at(-1)?.id ?? ""}:${messages.at(-1)?.status ?? ""}`}
          onAskForChange={() => {
            setPrompt(prompt.trim() ? t("chatView.appendWorkingNoteRequest", { prompt: prompt }) : t("chatView.iDLikeToChangeTheWorkingNote"));
            document.querySelector<HTMLTextAreaElement>('[data-chat-composer]' )?.focus();
          }}
          onOpenLink={openChatLink}
          onOpenKnowledge={workspaceActions.openMemory}
        >
        {!online ? <p className="chat-offline-banner" role="status">{t("chatError.offline")}</p>
          : lastMessage?.role === "assistant" && lastMessage.offline_reads ? <p className="chat-offline-banner" role="status">{t("chatError.offlineReads")}</p> : null}
        {conversation}
        </ChatWorkingContext>

        {activeThread && !isEmptyChatThreadId(activeThread.id) && (
          <ChatPendingQueue
            models={modelCatalog?.models ?? []}
            queued={queuedByThread[activeThread.id] ?? []}
            holdReason={queueHoldByThread[activeThread.id]}
            onRemove={(id) =>
              updateQueued(activeThread.id, (items) =>
                items.filter((item) => item.id !== id),
              )
            }
            onEdit={(id, text) =>
              updateQueued(activeThread.id, (items) =>
                items.map((item) => (item.id === id ? { ...item, text } : item)),
              )
            }
            onMove={(id, offset) =>
              updateQueued(activeThread.id, (items) => {
                const index = items.findIndex((item) => item.id === id);
                const nextIndex = index + offset;
                if (index < 0 || nextIndex < 0 || nextIndex >= items.length) {
                  return items;
                }
                const next = [...items];
                const [moved] = next.splice(index, 1);
                next.splice(nextIndex, 0, moved!);
                return next;
              })
            }
            onSendNow={sendQueuedNow}
            canSendNow={!isRunning && workspaceAvailable && selectedModelAvailable}
            offline={!online}
          />
        )}

        {workspaceAvailable && (configurationError || configurationReadError) ? <aside className="chat-model-setup-notice" role="alert"><InfoIcon aria-hidden="true" /><p>{configurationError ?? configurationReadError}</p>{onOpenChatPreferences ? <Button variant="ghost" size="sm" onClick={onOpenChatPreferences}><Settings2Icon />{t("chatView.chatPreferencesForThisLoupist")}</Button> : null}</aside> : null}
        {modelSetupMessage ? (
          <aside className="chat-model-setup-notice" role="note" aria-label={t("chatView.modelSetup")}>
            <InfoIcon aria-hidden="true" />
            <p>{modelSetupMessage}</p>
            {onOpenModelSettings ? (
                <Button type="button" size="sm" variant="outline" onClick={() => onOpenModelSettings()}>
                {t("chatView.openSettings")}</Button>
            ) : null}
          </aside>
        ) : null}

        {modelNotice ? <ModelNoticeLine notice={modelNotice} onChange={() => setModelMenuRequest((value) => value + 1)} onDismiss={() => onAcknowledgeModelNotice?.(modelNotice.id)} /> : null}
        {editing && editing.threadId === activeThread?.id ? <p className="chat-editing-note" role="status">
          <span>{t("chatBranch.editing")}</span>
          <button type="button" onClick={() => { setEditing(null); setPrompt(""); }}>{t("chatBranch.cancel")}</button>
        </p> : null}
        <ChatComposer
          key={activeThread?.id ?? "draft"}
          attachmentDraft={draft}
          placeholder={t("chatComposer.askLoupistToAddContextForCommands", { loupistName: loupist.name })}
          prompt={prompt}
          privateChat={privateChat}
          isRunning={isRunning}
          isStopping={
            localIsStopping || Boolean(activeRecoveredRun?.abort_requested)
          }
          error={error}
          sendDisabledReason={workspaceAvailable ? undefined : t("chatView.reconnectTheWorkspaceFolderToSendMessages")}
          streamAnnouncement={streamAnnouncement}
          models={visibleCatalogModels(modelCatalog)}
          providers={modelCatalog?.providers ?? []}
          modelSelection={modelSelection}
          skills={composerSkills}
          plugins={composerPlugins}
          selectedArtifact={selectedArtifact}
          onClearSelectedArtifact={onClearSelectedArtifact}
          knowledgePins={knowledgePins}
          chips={draft?.chips ?? NO_CHIPS}
          onChipsChange={(chips: ComposerChip[]) => onChipsChange?.(chips)}
          onSearchContext={(query) => searchComposerContext(
            bridge,
            loupist.id,
            activeThread && !isEmptyChatThreadId(activeThread.id) ? activeThread.id : null,
            query,
            composerPlugins,
          )}
          onAddKnowledgePin={onAddKnowledgePin}
          mapScreenshot={mapScreenshot}
          evidenceSources={evidenceSources}
          onSelectArtifact={onSelectArtifact}
          onNewChat={() => void onCreateThread()}
          onRemoveKnowledgePin={onRemoveKnowledgePin}
          onPromptChange={setPrompt}
          onPrivateChatChange={(value) => { memoryChoices.current.privateChat = value; setPrivateChat(value); }}
          onModelSelectionChange={onModelSelectionChange}
          modelAuto={modelAuto}
          onModelResetToAuto={onModelResetToAuto}
          onSend={(next, files, draftText) => send(next, files, { draftText })}
          onFollowUp={enqueueFollowUp}
          onAbort={abortActiveRun}
          onOpenModelSettings={onOpenModelSettings ? () => onOpenModelSettings() : undefined}
          onOpenUsage={(provider) => { void bridge.providerOpenUsage({ provider }).catch((cause) => setConfigurationReadError(errorMessage(cause, t("chatError.manageUsage")))); }}
          openModelMenuSignal={modelMenuRequest}
          asOf={draft?.asOf}
          onAsOfChange={onAsOfChange}
          incomingFiles={incomingFiles?.composerKey === composerKey ? incomingFiles : undefined}
          onIncomingFilesTaken={takeIncomingFiles}
        />
        </div>
        </div>
        </ChatLightboxProvider>
      </div>
    </section>
  );
}
