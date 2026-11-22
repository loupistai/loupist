import { useCallback, type ComponentProps, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { Chat } from "@ai-sdk/react";
import i18n from "@/i18n";
import type { ChatView } from "../components/ChatView";
import type { ConnectSheetTarget } from "../components/settings/ConnectSheet";
import type { LoupistUIMessage } from "../chat/ai-sdk";
import type { RecoveredChatRun } from "../chat/loupistRunRegistry";
import { chatSessionKey, type HostedWatchPhase } from "../chat/sessionRegistry";
import { modelRunSelection, resolveExplicitSelection } from "../lib/modelSelection";
import type { ChatArtifactRef, ChatThread, LoupistBridge, LoupistSummary, ModelRunSelection } from "../types";
import type { ChatDraft, useChatDrafts } from "./useChatDrafts";
import type { useChatSessions } from "./useChatSessions";
import type { useConfiguration } from "./useConfiguration";
import type { useLoupistDirectory } from "./useLoupistDirectory";
import type { useModelCatalog } from "./useModelCatalog";
import type { useThreadActions } from "./useThreadActions";
import type { useWorkspacePanel } from "./useWorkspacePanel";
import { EMPTY_CHAT_THREAD_ID } from "./emptyChat";

type ChatViewModelInput = {
  bridge: LoupistBridge;
  /** The Chat is on screen and not paused behind Loupist settings. */
  active: boolean;
  theme: "light" | "dark";
  selectedLoupist: LoupistSummary | null;
  activeChat: Chat<LoupistUIMessage> | undefined;
  recoveredChat: RecoveredChatRun | undefined;
  threads: ChatThread[];
  activeThreadId: string | null;
  effectiveThreadId: string | null;
  persistedThreadId: string | null;
  effectiveChatKey: string | null;
  activeDraft: ChatDraft | null;
  progressFocus: string | null;
  chat: ReturnType<typeof useChatSessions>;
  model: ReturnType<typeof useModelCatalog>;
  configuration: ReturnType<typeof useConfiguration>;
  loupist: Pick<ReturnType<typeof useLoupistDirectory>, "recordLastModel">;
  workspace: Pick<ReturnType<typeof useWorkspacePanel>, "selectedArtifacts" | "selectArtifactForChat" | "consumeArtifactForChat">;
  drafts: ReturnType<typeof useChatDrafts>;
  threadActions: Pick<ReturnType<typeof useThreadActions>, "createThreadForLoupist" | "renameThread">;
  /** Draft text and file writes queue behind these, so an adopted draft is cleared after its last write. */
  draftWrites: MutableRefObject<Promise<void>>;
  fileWrites: MutableRefObject<Promise<void>>;
  /** A turn or a World job changed what the Loupist knows. */
  refreshKnowledge: () => void;
  onOpenModelSettings: (target?: ConnectSheetTarget) => void;
  onOpenChatPreferences: (loupistId: string) => void;
  /** Opens the Following page with its search field focused (Bot's "Add assets to follow" card). */
  onOpenFollowing: () => void;
};

/**
 * The props of the visible Chat: its thread, model and draft wiring. Returns null
 * until a Loupist and its Chat session exist.
 */
export function useChatViewModel({
  bridge, active, theme, selectedLoupist, activeChat, recoveredChat, threads, activeThreadId,
  effectiveThreadId, persistedThreadId, effectiveChatKey, activeDraft, progressFocus,
  chat, model, configuration, loupist, workspace, drafts, threadActions, draftWrites, fileWrites,
  refreshKnowledge, onOpenModelSettings, onOpenChatPreferences, onOpenFollowing,
}: ChatViewModelInput): ComponentProps<typeof ChatView> | null {
  const selectedLoupistId = selectedLoupist?.id ?? null;
  const chosen = selectedLoupistId ? model.selections[selectedLoupistId] : undefined;
  const resolved = configuration.snapshot?.modelSelection ?? model.catalog?.default_selection;
  const modelSelection = chosen ? resolveExplicitSelection(model.catalog, chosen) : modelRunSelection(resolved);
  const { createThreadForLoupist } = threadActions;
  const { adopt } = drafts;
  const createThread = useCallback(
    async () => {
      if (!selectedLoupist) return null;
      const created = await createThreadForLoupist(selectedLoupist.id);
      if (created) {
        fileWrites.current = fileWrites.current.then(() => bridge.chatDraftFilesWrite({ loupist_id: selectedLoupist.id, thread_id: EMPTY_CHAT_THREAD_ID }, [])).catch(() => {});
        adopt(chatSessionKey(selectedLoupist.id, EMPTY_CHAT_THREAD_ID), chatSessionKey(selectedLoupist.id, created.id));
        draftWrites.current = draftWrites.current.then(() => bridge.chatDraftWrite({ loupist_id: selectedLoupist.id, thread_id: EMPTY_CHAT_THREAD_ID }, { prompt: "" })).catch(() => {});
      }
      return created;
    },
    [createThreadForLoupist, selectedLoupist, adopt, bridge, draftWrites, fileWrites],
  );
  const { changeSelection } = model;
  const changeSelectedModel = useCallback(
    (selection: ModelRunSelection) => {
      if (selectedLoupistId) changeSelection(selectedLoupistId, selection);
    },
    [changeSelection, selectedLoupistId],
  );
  const { recordLastModel } = loupist;
  const recordSelectedModel = useCallback(
    (selection: ModelRunSelection) => {
      // Rust records the model it resolved for an unset request.
      if (!selectedLoupistId || !selection.model_profile_id) return;
      recordLastModel(selectedLoupistId, selection.model_profile_id);
    },
    [recordLastModel, selectedLoupistId],
  );
  const { setThreadsForLoupist, setActiveThreadForLoupist, runRegistry, chatRegistry } = chat;
  const setVisibleThreads = useCallback<Dispatch<SetStateAction<ChatThread[]>>>(
    (action) => {
      if (selectedLoupistId) setThreadsForLoupist(selectedLoupistId, action);
    },
    [selectedLoupistId, setThreadsForLoupist],
  );
  const setVisibleActiveThreadId = useCallback(
    (threadId: string) => {
      if (selectedLoupistId) setActiveThreadForLoupist(selectedLoupistId, threadId);
    },
    [selectedLoupistId, setActiveThreadForLoupist],
  );
  const watchHostedRun = useCallback(
    (threadId: string, onPhase: (phase: HostedWatchPhase) => void) =>
      selectedLoupistId ? chatRegistry.watchHosted(selectedLoupistId, threadId, onPhase) : () => {},
    [chatRegistry, selectedLoupistId],
  );
  const abortVisibleRecoveredChat = useCallback(() => {
    if (selectedLoupistId && persistedThreadId) {
      runRegistry.abortRecoveredChat(selectedLoupistId, persistedThreadId);
    }
  }, [persistedThreadId, runRegistry, selectedLoupistId]);

  // Every answer card receives these; a new function on each app render would redraw every card while a reply streams.
  const { refresh: refreshDrafts } = drafts;
  const { selectArtifactForChat } = workspace;
  const readWorldJob = useCallback(
    (jobId: string) => bridge.knowledgeGraphJob({ loupistId: selectedLoupistId ?? "", jobId }),
    [bridge, selectedLoupistId],
  );
  const selectArtifact = useCallback(
    (artifact: ChatArtifactRef) => { if (effectiveChatKey) selectArtifactForChat(effectiveChatKey, artifact); },
    [effectiveChatKey, selectArtifactForChat],
  );
  const changeDraft = useCallback((prompt: string) => {
    if (!activeDraft || !effectiveChatKey || !effectiveThreadId) return;
    activeDraft.prompt = prompt;
    refreshDrafts();
  }, [activeDraft, effectiveChatKey, effectiveThreadId, refreshDrafts]);
  const changeAsOf = useCallback((asOf: string | undefined) => {
    if (activeDraft) { activeDraft.asOf = asOf; refreshDrafts(); }
  }, [activeDraft, refreshDrafts]);

  if (!selectedLoupist || !activeChat) return null;
  const loupistId = selectedLoupist.id;
  const { snapshot } = configuration;
  return {
    active,
    theme,
    bridge,
    chat: activeChat,
    recoveredRun: recoveredChat,
    onWaitForMemory: runRegistry?.waitForMemory,
    loupist: selectedLoupist,
    threads,
    setThreads: setVisibleThreads,
    activeThreadId,
    setActiveThreadId: setVisibleActiveThreadId,
    onCreateThread: createThread,
    modelCatalog: model.catalog,
    modelSelection,
    skillPreferences: snapshot?.effective.skills,
    memoryDefaults: snapshot?.effective.chat,
    configurationReady: Boolean(snapshot) && !configuration.error && !snapshot?.diagnostics.length,
    configurationError: configuration.error ?? (snapshot?.diagnostics.length ? i18n.t("app:app.correctTheConfigurationFileInSettingsChat") : snapshot && resolved?.unavailable_reason && resolved.unavailable_reason !== "no_connected_account" && !chosen ? i18n.t("app:app.theConfiguredChatDefaultsAreUnavailableFor") : undefined),
    onModelSelectionChange: changeSelectedModel,
    modelName: chosen ? undefined : resolved?.model?.name,
    modelIsDefault: !chosen && Boolean(resolved?.is_default),
    noAccount: resolved && resolved.unavailable_reason === "no_connected_account",
    onChooseDefault: () => model.resetSelection(loupistId),
    onModelUsed: recordSelectedModel,
    onResolveModelSelection: async () => {
      const next = await configuration.reload();
      if (!next || next.diagnostics.length) throw new Error(i18n.t("app:app.correctTheConfigurationFileBeforeSending"));
      const catalog = model.catalog ? { ...model.catalog, hidden_model_profile_ids: next.effective.models.hidden_model_profile_ids } : null;
      const override = model.selections[loupistId];
      if (override) return { ...resolveExplicitSelection(catalog, override), memoryDefaults: next.effective.chat };
      const selection = next.modelSelection;
      if (selection?.is_default && selection.model) {
        return { model_profile_id: "", thinking_level: next.effective.chat.thinking_level ?? "", run_options: { ...next.effective.chat.run_options }, auto: true, memoryDefaults: next.effective.chat };
      }
      return { ...modelRunSelection(selection), memoryDefaults: next.effective.chat };
    },
    selectedArtifact: effectiveChatKey ? workspace.selectedArtifacts[effectiveChatKey] : undefined,
    onClearSelectedArtifact: () => { if (effectiveChatKey) workspace.selectArtifactForChat(effectiveChatKey); },
    onConsumeSelectedArtifact: (artifact, threadId) => { const key = threadId ? chatSessionKey(loupistId, threadId) : effectiveChatKey; if (key) workspace.consumeArtifactForChat(key, artifact); },
    knowledgePins: activeDraft?.pins ?? [],
    onRemoveKnowledgePin: (pin) => { if (activeDraft) drafts.removePin(activeDraft, pin); },
    onAddKnowledgePin: (pin) => { if (activeDraft) drafts.addPin(activeDraft, pin); },
    onSelectArtifact: selectArtifact,
    onConsumeKnowledgePins: (pins) => { if (activeDraft) pins.forEach((pin) => drafts.removePin(activeDraft, pin)); },
    onReadWorldJob: readWorldJob,
    onWorldChanged: refreshKnowledge,
    progressFocus,
    onAbortRecoveredRun: abortVisibleRecoveredChat,
    onWatchHostedRun: watchHostedRun,
    draft: activeDraft ?? undefined,
    sessionDrafts: drafts.getSession(loupistId),
    initialPrompt: activeDraft?.prompt ?? "",
    onDraftChange: changeDraft,
    onRenameThread: threadActions.renameThread,
    onChipsChange: (chips) => { if (activeDraft) { activeDraft.chips = chips; drafts.refresh(); } },
    onAsOfChange: changeAsOf,
    onTurnComplete: refreshKnowledge,
    modelSetupMessage: model.needsVisibleModels ? i18n.t("app:app.showOrLoadAModelInSettings") : undefined,
    onOpenModelSettings,
    modelNotice: chosen ? null : snapshot?.defaultModelChange,
    onAcknowledgeModelNotice: model.acknowledgeModelNotice,
    hostAuto: !chosen && Boolean(resolved?.is_default),
    onOpenChatPreferences: () => onOpenChatPreferences(loupistId),
    onOpenFollowing,
  };
}
