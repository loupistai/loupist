import type { NoticeSettings } from "../bridge/notice";
import type { AttentionItem, AttentionNotifyRequest } from "./attention";
import type { AssetBridge } from "./assets";
import type { EarningsBridge } from "./earnings";
import type { WatchBridge } from "./watch";
import type { JournalListRequest, JournalCreateRequest, JournalUpdateRequest, JournalArchiveRequest } from "./knowledge";
import type { BriefingSettings, BriefingSettingsRequest, InboxRequest, InboxPage, InboxUnreadCount, InboxChangedEvent, InboxOpenEvent, BriefingResidentSettings } from "./scheduled";
import type { Principle, PrincipleListRequest, PrincipleCreateRequest, PrincipleUpdateRequest, PrincipleArchiveRequest } from "./knowledge";
import type { UserContextBridge } from "./userContext";
import type { CustomConnectionsBridge } from "./customConnections";
import type { SharingBridge } from "./sharing";
import type { SkillCatalog, SkillDocument, SkillReadRequest, SkillCreateRequest, SkillSaveRequest, SkillDeleteRequest, SkillImportRequest } from "./skills";
import type { DraftAttachment, StoredChatDraft, WorkspacePathRequest, WorkspaceDirectory, WorkspaceFile, ChatActivityRequest, ChatActivityDetail, WorkspaceUiState } from "./workspace";
import type { ScheduledRun, ScheduledTask, ScheduledTaskIdentity, ScheduledRunIdentity, ScheduledTaskWatch } from "./scheduled";
import type {
  LoupistCapabilityBinding,
  MarketplaceCredentialStatus,
  MarketplaceSnapshot,
} from "../marketplace/types";
import type {
  BrowserHistoryDirection,
  BrowserTabBoundsRequest,
  BrowserTabEvent,
  BrowserTabOpenRequest,
  BrowserTabState,
} from "./browser";
import type {
  ChatArtifact,
  ChatArtifactView,
  ChatArtifactVersion,
  ChatArtifactRestoreRequest,
  LoupistArtifactItem,
  ChatControlReceipt,
  ChatControlRequest,
  ChatCreateRequest,
  ChatDeleteRequest,
  ChatRenameRequest,
  ChatFeedbackRequest,
  ChatFeedback,
  ChatRetryRequest,
  ChatSendRequest,
  ChatStreamEvent,
  ChatThread,
  LoupistWorkspace,
  ResearchHandoffReceipt,
  ResearchHandoffRequest,
} from "./chat";
import type {
  AgentSkillSummary,
  LoupistCapabilityRequest,
  LoupistCreateRequest,
  LoupistImportRequest,
  LoupistReconnectRequest,
  LoupistDeleteRequest,
  LoupistRecoverRequest,
  LoupistSummary,
  LoupistProfileUpdateRequest,
  MarketplaceCredentialRequest,
  MarketplaceCredentialSaveRequest,
  MarketplaceConnectorConfigureRequest,
} from "./loupist";
import type {
  JournalRecord,
  JournalSourceRead,
  GraphJobView,
  GraphProcessingSettings,
  GraphProcessingView,
  KeepAwakeStatus,
  KnowledgeCatalog,
  KnowledgePin,
  KnowledgeCatalogRequest,
  KnowledgeJournalReadRequest,
  KnowledgeJournalSourceReadRequest,
  KnowledgeLensReadRequest,
  KnowledgeResearchPage,
  KnowledgeResearchSearchRequest,
  WorkingNote, WorkingNoteReadRequest, WorkingNotesRequest, WorkingNotesPage,
  KnowledgeSourceReadRequest,
  KnowledgeSourceSelectionRequest,
  KnowledgeSourceRetainRequest,
  RetainedSource,
  SourceDetail,
  WorldGraphQuery,
  WorldGraphView,
} from "./knowledge";
import type {
  ModelCatalog,
  ModelVisibilityUpdateRequest,
  ProviderConfigureRequest,
  CustomProviderRequest,
  ProviderConnectionEvent,
  LocalDiscovery,
  ProviderCustomTestRequest,
  ProviderCustomTestResult,
  ProviderPresetRequest,
  ProviderStatusEvent,
} from "./model";
import type {
  RunActivity,
  StreamObserver,
  UpdateInstallRequest,
  UpdateInstallResult,
  UpdateState,
} from "./runtime";

import type { ConfigScope, ConfigSnapshot, ConfigUpdateRequest } from "./config";

export interface LoupistBridge extends UserContextBridge, CustomConnectionsBridge, AssetBridge, SharingBridge, EarningsBridge, WatchBridge {
  noticeSettingsGet(): Promise<NoticeSettings>;
  noticeAcknowledge(): Promise<NoticeSettings>;
  learningReviewList(request: import("./knowledge").LearningReviewQuery): Promise<import("./knowledge").LearningReview[]>;
  memoryLearningSettingsGet(request: { loupistId: string }): Promise<import("./knowledge").MemoryLearningSettings>;
  memoryLearningSettingsSet(request: { loupistId: string; learnMethods: boolean }): Promise<import("./knowledge").MemoryLearningSettings>;
  onLearningReviewFinished(handler: (event: import("./knowledge").LearningReviewFinishedEvent) => void): () => void;
  worldSchemaGet(request: { loupistId: string }): Promise<import("./knowledge").WorldSchemaView>;
  worldSchemaUndo(request: { loupistId: string; changeId: string }): Promise<import("./knowledge").WorldSchemaView>;
  worldSchemaRemove(request: { loupistId: string; kind: import("./knowledge").SchemaRemovalKind; name: string }): Promise<import("./knowledge").WorldSchemaView>;
  onWorldSchemaChanged(handler: (event: { loupistId: string }) => void): () => void;
  principleList(request: PrincipleListRequest): Promise<Principle[]>;
  principleCreate(request: PrincipleCreateRequest): Promise<Principle>;
  principleUpdate(request: PrincipleUpdateRequest): Promise<Principle>;
  principleArchive(request: PrincipleArchiveRequest): Promise<Principle>;
  /** Saves prepared bytes through the native save dialog; Rust checks the size and that the extension matches the kind. */
  exportSave(request: { suggestedName: string; bytes: number[]; kind: "png" | "mp4" | "webm" | "md" | "csv" }): Promise<string | null>;
  pickFile(request: { title: string; filters: { name: string; extensions: string[] }[] }): Promise<string | null>;
  onMenuAction(handler: (action: string) => void): () => void;

  skillsList(request: { scope: ConfigScope }): Promise<SkillCatalog>;
  skillsRead(request: SkillReadRequest): Promise<SkillDocument>;
  skillsCreate(request: SkillCreateRequest): Promise<SkillCatalog>;
  skillsSave(request: SkillSaveRequest): Promise<SkillCatalog>;
  skillsDelete(request: SkillDeleteRequest): Promise<SkillCatalog>;
  skillsImport(request: SkillImportRequest): Promise<SkillCatalog | null>;
  skillsReveal(request: SkillReadRequest): Promise<void>;
  configRead(request: { scope: ConfigScope }): Promise<ConfigSnapshot>;
  configUpdate(request: ConfigUpdateRequest): Promise<ConfigSnapshot>;
  configReveal(request: { scope: ConfigScope }): Promise<void>;
  runLogsReveal(request: { loupist_id: string | null }): Promise<void>;
  runLogsLastError(request: { loupist_id: string | null }): Promise<string | null>;
  briefingSettingsGet(request: { loupistId: string }): Promise<BriefingSettings>;
  briefingSettingsSet(request: BriefingSettingsRequest): Promise<BriefingSettings>;
  briefingResidentSettingsGet(): Promise<BriefingResidentSettings>;
  briefingResidentSettingsSet(request: { keepRunning: boolean }): Promise<BriefingResidentSettings>;
  inboxList(request: InboxRequest): Promise<InboxPage>;
  inboxMarkRead(request: { loupistId: string; runIds: string[] }): Promise<void>;
  inboxUnreadCounts(): Promise<InboxUnreadCount[]>;
  onInboxChanged(handler: (event: InboxChangedEvent) => void): () => void;
  onInboxOpen(handler: (event: InboxOpenEvent) => void): () => void;
  /** Approvals still pending, unread scheduled results and checks coming due, across Loupists. */
  attentionList(): Promise<AttentionItem[]>;
  /** An OS notification for a chat the user is not watching; false when not sent. */
  attentionNotify(request: AttentionNotifyRequest): Promise<boolean>;
  scheduledTaskRunNow(request: ScheduledTaskIdentity): Promise<ScheduledRun>;
  /** Every scheduled task of the Loupist with its latest run, for the Schedule tab. */
  scheduledTasksList(request: { loupist_id: string }): Promise<ScheduledTask[]>;
  /** Pause or resume; the revision guards against a concurrent edit. */
  scheduledTaskSetStatus(request: ScheduledTaskIdentity & { expected_revision: number; status: "active" | "paused" }): Promise<ScheduledTask>;
  /** Deletes a custom task; the briefing is turned off from its settings instead. */
  scheduledTaskDelete(request: ScheduledTaskIdentity & { expected_revision: number }): Promise<void>;
  /** The task's run history, newest first. */
  scheduledTaskRuns(request: ScheduledTaskIdentity): Promise<ScheduledRun[]>;
  /** The current task behind a Chat card, or null once it is gone. */
  scheduledTaskGet(request: ScheduledTaskIdentity): Promise<ScheduledTask | null>;
  /** The user's one-tap OK for a task the Loupist proposed in conversation. */
  scheduledTaskConfirm(request: ScheduledTaskIdentity): Promise<ScheduledTask>;
  /** Declines a proposed task that was never confirmed. */
  scheduledTaskDiscard(request: ScheduledTaskIdentity): Promise<void>;
  scheduledTasksActivity(request: { loupist_id: string }): Promise<ScheduledRun[]>;
  scheduledTaskWatch(request: ScheduledRunIdentity, observer: StreamObserver<ChatStreamEvent>): Promise<ScheduledTaskWatch | null>;

  modelCatalogGet(): Promise<ModelCatalog>;
  modelVisibilityUpdate(
    request: ModelVisibilityUpdateRequest,
  ): Promise<ModelCatalog>;
  providerModels(provider: string): Promise<ModelCatalog>;
  providerConfigure(request: ProviderConfigureRequest): Promise<ModelCatalog>;
  providerConnect(
    provider: string,
    observer: StreamObserver<ProviderConnectionEvent>,
  ): Promise<ModelCatalog>;
  providerConnectCancel(): Promise<void>;
  providerConnectOpenBrowser(): Promise<void>;
  /**
   * Answers the active sign-in's paste prompt with the address the browser
   * shows after signing in. Rust accepts only that provider's exact loopback
   * callback; anything else rejects with code `paste_invalid` and nothing is
   * forwarded.
   */
  providerConnectSubmitCode(request: { value: string }): Promise<void>;
  /** Loopback-only probes for Ollama, LM Studio and llama.cpp (600 ms each). */
  providerLocalDiscover(): Promise<LocalDiscovery>;
  /** One bounded `models` call against a custom endpoint; a key is used for this call only. */
  providerCustomTest(request: ProviderCustomTestRequest): Promise<ProviderCustomTestResult>;
  /** Rust opens the provider's fixed key page; the renderer never sends a URL. */
  providerOpenConsole(request: { provider: string }): Promise<void>;
  /** Rust opens the provider's fixed usage page (ChatGPT plan). */
  providerOpenUsage(request: { provider: string }): Promise<void>;
  /** Saves a preset endpoint (Upstage Solar); rejects with code `key_rejected` when the key is refused. */
  providerPresetSave(request: ProviderPresetRequest): Promise<ModelCatalog>;
  providerDisconnect(provider: string): Promise<ModelCatalog>;
  providerCustomSave(request: CustomProviderRequest): Promise<ModelCatalog>;
  providerCustomRemove(provider: string): Promise<ModelCatalog>;
  /** Clears the pending "Now using …" line once it was shown (on send or ×). */
  modelNoticeAck(request: { id: string }): Promise<ModelCatalog>;
  /** Rust emits `provider-status` on every state or notice change. */
  onProviderStatus(handler: (event: ProviderStatusEvent) => void): () => void;
  marketplaceSnapshot(): Promise<MarketplaceSnapshot>;
  loupistCapabilityList(loupist_id: string): Promise<LoupistCapabilityBinding[]>;
  agentSkillCatalog(loupist_id: string): Promise<AgentSkillSummary[]>;
  loupistCapabilityEnable(
    request: LoupistCapabilityRequest,
  ): Promise<LoupistCapabilityBinding>;
  marketplaceConnectorConfigure(
    request: MarketplaceConnectorConfigureRequest,
  ): Promise<void>;
  loupistCapabilityDisable(
    request: LoupistCapabilityRequest,
  ): Promise<LoupistCapabilityBinding>;
  marketplaceCredentialSave(
    request: MarketplaceCredentialSaveRequest,
  ): Promise<MarketplaceCredentialStatus[]>;
  marketplaceCredentialVerify(
    request: MarketplaceCredentialRequest,
  ): Promise<MarketplaceCredentialStatus[]>;
  marketplaceCredentialDelete(
    request: MarketplaceCredentialRequest,
  ): Promise<MarketplaceCredentialStatus[]>;
  openExternalUrl(url: string): Promise<void>;
  siteFavicon(url: string): Promise<string | null>;
  browserTabOpen(
    request: BrowserTabOpenRequest,
    observer: StreamObserver<BrowserTabEvent>,
  ): Promise<BrowserTabState>;
  browserTabNavigate(tab_id: string, url: string): Promise<void>;
  browserTabHistory(
    tab_id: string,
    direction: BrowserHistoryDirection,
  ): Promise<void>;
  browserTabReload(tab_id: string): Promise<void>;
  browserTabSetBounds(request: BrowserTabBoundsRequest): Promise<void>;
  browserTabClose(tab_id: string): Promise<void>;
  browserTabsReset(): Promise<void>;
  updateStatus(): Promise<UpdateState>;
  updateCheck(): Promise<UpdateState>;
  updateInstall(request: UpdateInstallRequest): Promise<UpdateInstallResult>;
  loupistChooseFolder(): Promise<string | null>;
  loupistReconnect(request: LoupistReconnectRequest): Promise<LoupistSummary>;
  workspaceList(request: WorkspacePathRequest): Promise<WorkspaceDirectory>;
  workspaceRead(request: WorkspacePathRequest & { path: string }): Promise<WorkspaceFile>;
  workspaceReveal(request: WorkspacePathRequest): Promise<void>;
  chatActivityRead(request: ChatActivityRequest): Promise<ChatActivityDetail>;
  chatDraftFilesRead(request: { loupist_id: string; thread_id: string }): Promise<DraftAttachment[]>;
  chatDraftFilesWrite(request: { loupist_id: string; thread_id: string }, files: DraftAttachment[]): Promise<void>;
  chatDraftRead(request: { loupist_id: string; thread_id: string }): Promise<StoredChatDraft | null>;
  chatDraftWrite(request: { loupist_id: string; thread_id: string }, draft: StoredChatDraft): Promise<void>;
  workspaceUiRead(): Promise<WorkspaceUiState | null>;
  workspaceUiWrite(state: WorkspaceUiState): Promise<WorkspaceUiState>;
  loupistList(): Promise<LoupistSummary[]>;
  loupistSelect(loupist_id: string): Promise<LoupistWorkspace>;
  loupistRecover(request: LoupistRecoverRequest): Promise<LoupistSummary>;
  loupistProfileUpdate(request: LoupistProfileUpdateRequest): Promise<LoupistSummary>;
  loupistImport(request: LoupistImportRequest): Promise<LoupistSummary>;
  loupistCreate(request: LoupistCreateRequest): Promise<LoupistSummary>;
  loupistDelete(request: LoupistDeleteRequest): Promise<void>;
  chatCreate(request: ChatCreateRequest): Promise<ChatThread>;
  chatFeedbackSet(request: ChatFeedbackRequest): Promise<ChatFeedback>;
  chatRename(request: ChatRenameRequest): Promise<ChatThread>;
  /** "Not this time": the host leaves the record out of this chat's context until it is brought back. */
  chatMemorySetAside(request: import("./chat").ChatMemorySetAsideRequest): Promise<ChatThread>;
  /** Edit forks (X18): sets the tail from this user message aside as an earlier version, before the edited text is sent. */
  chatBranch(request: import("./chat").ChatBranchRequest): Promise<ChatThread>;
  /** Shows another version of the chat's tail; the shown one becomes a version. */
  chatVersionSwitch(request: import("./chat").ChatVersionSwitchRequest): Promise<ChatThread>;
  /** The user's answer to an approval card; the waiting tool call resumes or is skipped. */
  chatApprovalDecide(request: { loupist_id: string; request_id: string; decision: import("./chat").ChatApprovalDecision }): Promise<void>;
  /** The user's answer to a question card: exactly one of `choice` (an option index) and `text`. */
  chatQuestionAnswer(request: { loupist_id: string; request_id: string; choice?: number; text?: string }): Promise<void>;
  chatDelete(request: ChatDeleteRequest): Promise<void>;
  chatAttachmentRead(
    loupist_id: string,
    thread_id: string,
    message_id: string,
    attachment_id: string,
  ): Promise<{ data_url: string }>;
  chatArtifactList(loupist_id: string, thread_id: string): Promise<ChatArtifact[]>;
  /** The Loupist's charts and documents across its chats, newest first (the panel's Work tab). */
  loupistArtifactList(request: { loupist_id: string; limit?: number }): Promise<LoupistArtifactItem[]>;
  chatArtifactRead(
    loupist_id: string,
    thread_id: string,
    artifact_id: string,
    revision_id?: number,
  ): Promise<ChatArtifactView>;
  chatArtifactVersions(loupist_id: string, thread_id: string, artifact_id: string): Promise<ChatArtifactVersion[]>;
  chatArtifactRestore(request: ChatArtifactRestoreRequest): Promise<ChatArtifactView>;
  chatSend(
    request: ChatSendRequest,
    observer: StreamObserver<ChatStreamEvent>,
  ): Promise<{ run_id: string }>;
  chatSteer(request: ChatControlRequest): Promise<ChatControlReceipt>;
  /** `as_of` is the date the failed request was sent with; Retry repeats it. */
  chatRetryLast(request: ChatRetryRequest): Promise<{ prompt: string; as_of?: string }>;
  /** Opens a research session that runs on its own and reports into the Bot; a repeated key returns the same one. */
  researchHandoffStart(request: ResearchHandoffRequest): Promise<ResearchHandoffReceipt>;
  /** Links discovery found after World publications, newest first. */
  knowledgeConnections(request: { loupistId: string }): Promise<import("./knowledge").ConnectionView[]>;
  /** Not useful: the link stays recorded so it is never found again. */
  knowledgeConnectionDismiss(request: { loupistId: string; connectionId: string }): Promise<import("./knowledge").ConnectionView>;
  /** Look into it: records that a research hand-off was started for the link. */
  knowledgeConnectionOpen(request: { loupistId: string; connectionId: string }): Promise<import("./knowledge").ConnectionView>;
  chatAbort(run_id: string): Promise<void>;
  runActivityList(): Promise<RunActivity[]>;
  knowledgeGrowthEvents(request: import("./knowledge").GrowthEventsQuery): Promise<import("./knowledge").GrowthEventsPage>;
  knowledgeCatalog(request: KnowledgeCatalogRequest): Promise<KnowledgeCatalog>;
  knowledgeWorldQuery(request: WorldGraphQuery): Promise<WorldGraphView>;
  knowledgeMap(request: import("./knowledge").KnowledgeMapQuery): Promise<import("./knowledge").KnowledgeMap>;
  knowledgeSourceSelectionResolve(request: KnowledgeSourceSelectionRequest): Promise<KnowledgePin>;
  knowledgeSourceRead(request: KnowledgeSourceReadRequest): Promise<SourceDetail>;
  knowledgeSourceExport(request: KnowledgeSourceReadRequest): Promise<string | null>;
  knowledgeLensArchive(request: { loupistId: string; recordId: string; archived: boolean }): Promise<void>;
  knowledgeMemoryControls(request: { loupistId: string }): Promise<import("./knowledge").MemoryControls>;
  knowledgeLensPause(request: { loupistId: string; recordId: string; paused: boolean }): Promise<import("./knowledge").MemoryControls>;
  knowledgeMemoryResetPreview(request: { loupistId: string }): Promise<import("./knowledge").MemoryResetCounts>;
  knowledgeMemoryReset(request: { loupistId: string; confirm: boolean; expected: import("./knowledge").MemoryResetCounts }): Promise<import("./knowledge").MemoryResetCounts>;
  knowledgeMemoryUsage(request: { loupistId: string }): Promise<import("./knowledge").MemoryUsage[]>;
  knowledgeLensRead(request: KnowledgeLensReadRequest): Promise<import("./lens").PageView>;
  knowledgeLensPagesForNode(request: { loupistId: string; nodeId: string }): Promise<import("./lens").PageSummary[]>;
  /** Retires one section as the user; the newest history entry of the returned page is the op Undo reverses. */
  memorySectionForget(request: { loupistId: string; recordId: string; key: string }): Promise<import("./lens").PageView>;
  memoryOpUndo(request: { loupistId: string; opId: string }): Promise<import("./lens").PageView>;
  memoryLearned(request: { loupistId: string; messageIds: string[] }): Promise<import("./lens").LearnedDigest[]>;
  knowledgeWorkingNoteRead(request: WorkingNoteReadRequest): Promise<WorkingNote>;
  knowledgeWorkingNotes(request: WorkingNotesRequest): Promise<WorkingNotesPage>;
  knowledgeLensSearch(request: { loupistId: string; query: string; kinds?: import("./lens").LensKind[] }): Promise<import("./lens").LensSearchHit[]>;
  knowledgeResearchSearch(
    request: KnowledgeResearchSearchRequest,
  ): Promise<KnowledgeResearchPage>;
  knowledgeSourceRetain(request: KnowledgeSourceRetainRequest): Promise<RetainedSource>;
  knowledgeGraphJob(request: { loupistId: string; jobId: string }): Promise<GraphJobView>;
  knowledgeFollowedStatus(request: { loupistId: string }): Promise<import("./knowledge").FollowedStatus>;
  knowledgeGraphRecover(request: { loupistId: string }): Promise<GraphProcessingView>;
  knowledgeGraphPause(request: {
    loupistId: string;
    paused: boolean;
  }): Promise<GraphProcessingView>;
  knowledgeGraphSettingsGet(request: { loupistId: string }): Promise<GraphProcessingView>;
  knowledgeGraphSettingsSet(request: {
    loupistId: string;
    settings: GraphProcessingSettings;
  }): Promise<GraphProcessingView>;
  worldKeepAwakeGet(): Promise<KeepAwakeStatus>;
  worldKeepAwakeSet(request: { enabled: boolean }): Promise<KeepAwakeStatus>;
  journalList(request: JournalListRequest): Promise<JournalRecord[]>;
  journalCreate(request: JournalCreateRequest): Promise<JournalRecord>;
  journalUpdate(request: JournalUpdateRequest): Promise<JournalRecord>;
  journalArchive(request: JournalArchiveRequest): Promise<JournalRecord>;
  knowledgeJournalRead(request: KnowledgeJournalReadRequest): Promise<JournalRecord>;
  knowledgeJournalSourceRead(
    request: KnowledgeJournalSourceReadRequest,
  ): Promise<JournalSourceRead>;
}
