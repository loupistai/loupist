import { useCallback, type ComponentProps, type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { Chat } from "@ai-sdk/react";
import i18n from "@/i18n";
import type { ChatView } from "../components/ChatView";
import type { ConnectSheetTarget } from "../components/settings/ConnectSheet";
import type { LoupistUIMessage } from "../chat/ai-sdk";
import type { RecoveredChatRun } from "../chat/loupistRunRegistry";
import { chatSessionKey } from "../chat/sessionRegistry";
import { resolveConfiguredModelRunSelection } from "../lib/modelSelection";
import type { ChatThread, LoupistBridge, LoupistSummary, ModelRunSelection } from "../types";
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
};

/**
 * The props of the visible Chat: its thread, model and draft wiring. Returns null
 * until a Loupist and its Chat session exist.
 */
export function useChatViewModel({
  bridge, active, theme, selectedLoupist, activeChat, recoveredChat, threads, activeThreadId,
  effectiveThreadId, persistedThreadId, effectiveChatKey, activeDraft, progressFocus,
  chat, model, configuration, loupist, workspace, drafts, threadActions, draftWrites, fileWrites,
  refreshKnowledge, onOpenModelSettings, onOpenChatPreferences,
}: ChatViewModelInput): ComponentProps<typeof ChatView> | null {
  const selectedLoupistId = selectedLoupist?.id ?? null;
  const modelSelection = resolveConfiguredModelRunSelection(
    model.catalog,
    selectedLoupist ? model.selections[selectedLoupist.id] : undefined,
    configuration.snapshot?.effective.chat,
    selectedLoupist?.last_model_profile_id,
  );
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
      // An Auto send names no model; Rust records what it resolved in the answer.
      if (!selectedLoupistId || !selection.model_profile_id) return;
      recordLastModel(selectedLoupistId, selection.model_profile_id);
    },
    [recordLastModel, selectedLoupistId],
  );
  const { setThreadsForLoupist, setActiveThreadForLoupist, runRegistry } = chat;
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
  const abortVisibleRecoveredChat = useCallback(() => {
    if (selectedLoupistId && persistedThreadId) {
      runRegistry.abortRecoveredChat(selectedLoupistId, persistedThreadId);
    }
  }, [persistedThreadId, runRegistry, selectedLoupistId]);

  if (!selectedLoupist || !activeChat) return null;
  const loupistId = selectedLoupist.id;
  const { snapshot } = configuration;
  const chosen = model.selections[loupistId];
  return {
    active,
    theme,
    bridge,
    chat: activeChat,
    recoveredRun: recoveredChat,
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
    configurationError: configuration.error ?? (snapshot?.diagnostics.length ? i18n.t("app:app.correctTheConfigurationFileInSettingsChat") : snapshot && model.catalog?.models.length && !modelSelection.model_profile_id && !chosen ? i18n.t("app:app.theConfiguredChatDefaultsAreUnavailableFor") : undefined),
    onModelSelectionChange: changeSelectedModel,
    modelAuto: !chosen,
    onModelResetToAuto: () => model.resetSelection(loupistId),
    onModelUsed: recordSelectedModel,
    onResolveModelSelection: async () => {
      const next = await configuration.reload();
      if (!next || next.diagnostics.length) throw new Error(i18n.t("app:app.correctTheConfigurationFileBeforeSending"));
      const catalog = model.catalog ? { ...model.catalog, hidden_model_profile_ids: next.effective.models.hidden_model_profile_ids } : null;
      // Nothing chosen here and no Chat default: Rust resolves Auto at admission (W17.5) and applies
      // the configured reasoning and run options where the model it picks supports them.
      if (!model.selections[loupistId] && !next.effective.chat.model_profile_id) return { model_profile_id: "", thinking_level: next.effective.chat.thinking_level ?? "", run_options: { ...next.effective.chat.run_options }, auto: true, memoryDefaults: next.effective.chat };
      return { ...resolveConfiguredModelRunSelection(catalog, model.selections[loupistId], next.effective.chat, selectedLoupist.last_model_profile_id), memoryDefaults: next.effective.chat };
    },
    selectedArtifact: effectiveChatKey ? workspace.selectedArtifacts[effectiveChatKey] : undefined,
    onClearSelectedArtifact: () => { if (effectiveChatKey) workspace.selectArtifactForChat(effectiveChatKey); },
    onConsumeSelectedArtifact: (artifact, threadId) => { const key = threadId ? chatSessionKey(loupistId, threadId) : effectiveChatKey; if (key) workspace.consumeArtifactForChat(key, artifact); },
    knowledgePins: activeDraft?.pins ?? [],
    onRemoveKnowledgePin: (pin) => { if (activeDraft) drafts.removePin(activeDraft, pin); },
    onAddKnowledgePin: (pin) => { if (activeDraft) drafts.addPin(activeDraft, pin); },
    onSelectArtifact: (artifact) => { if (effectiveChatKey) workspace.selectArtifactForChat(effectiveChatKey, artifact); },
    onConsumeKnowledgePins: (pins) => { if (activeDraft) pins.forEach((pin) => drafts.removePin(activeDraft, pin)); },
    onReadWorldJob: (jobId) => bridge.knowledgeGraphJob({ loupistId, jobId }),
    onWorldChanged: refreshKnowledge,
    progressFocus,
    onAbortRecoveredRun: abortVisibleRecoveredChat,
    draft: activeDraft ?? undefined,
    sessionDrafts: drafts.getSession(loupistId),
    initialPrompt: activeDraft?.prompt ?? "",
    onDraftChange: (prompt) => {
      if (!activeDraft || !effectiveChatKey || !effectiveThreadId) return;
      activeDraft.prompt = prompt;
      drafts.refresh();
    },
    onRenameThread: threadActions.renameThread,
    onChipsChange: (chips) => { if (activeDraft) { activeDraft.chips = chips; drafts.refresh(); } },
    onAsOfChange: (asOf) => { if (activeDraft) { activeDraft.asOf = asOf; drafts.refresh(); } },
    onTurnComplete: refreshKnowledge,
    modelSetupMessage: model.needsVisibleModels ? i18n.t("app:app.showOrLoadAModelInSettings") : undefined,
    onOpenModelSettings,
    modelNotice: model.catalog?.model_notice ?? null,
    onAcknowledgeModelNotice: model.acknowledgeModelNotice,
    hostAuto: !chosen && !snapshot?.effective.chat.model_profile_id,
    onOpenChatPreferences: () => onOpenChatPreferences(loupistId),
  };
}
