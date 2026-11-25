// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { AgentSkillSummary } from "./agent_harness";
import type { AppMenuContext } from "./app_menu";
import type { BriefingSettings, BriefingSettingsRequest, BriefingsLoupistRequest, InboxPage, InboxRequest, ReadRequest, UnreadCount } from "./briefings/commands";
import type { BotFocusRequest, ResidentRequest, TemplatesRequest } from "./briefings/desktop";
import type { DesktopSettings, LoginItemRequest } from "./briefings/login_item";
import type { ChatActivityDetail } from "./chat/activity";
import type { ChatArtifact, ChatArtifactVersion } from "./chat/artifacts";
import type { ModelVisibilityUpdateRequest } from "./commands";
import type { ApprovalDecideRequest } from "./commands/approvals";
import type { AssetEntitySetRequest, AssetVerdictUndoRequest } from "./commands/assets";
import type { AssetFollowNameRequest, AssetIdRequest, AssetLookupRequest, AssetLookupResult, AssetOpenRequest, AssetPricePoint, AssetPriceSeriesRequest, AssetRow, AssetsListRequest, StoredAssetQuote, ThinAssetPageView } from "./commands/assets/wire";
import type { AttentionItem, NotifyRequest } from "./commands/attention";
import type { BotMemoryForgetRequest, BotMemoryListEntry, BotMemoryListRequest, BotMemoryUpdateRequest } from "./commands/bot_memory";
import type { AskAnswerRequest, OfferRequest } from "./commands/cards";
import type { ChatRetryReceipt, ChatRetryRequest } from "./commands/chat_runtime";
import type { ClipboardImage } from "./commands/clipboard";
import type { PickFileRequest } from "./commands/dialog";
import type { ExportRequest } from "./commands/export_file";
import type { FxDisplayRequest } from "./commands/fx_display";
import type { ResearchHandoffReceipt, ResearchHandoffRequest } from "./commands/handoff";
import type { JournalSourceReadDto, KnowledgeCatalogDto, KnowledgeCatalogRequest, KnowledgeConnectionRequest, KnowledgeGraphJobRequest, KnowledgeGraphLoupistRequest, KnowledgeGraphPauseRequest, KnowledgeGraphRetrySourceRequest, KnowledgeGraphRetrySourcesRequest, KnowledgeGraphSettingsRequest, KnowledgeJournalReadRequest, KnowledgeJournalSourceReadRequest, KnowledgeLensPagesForNodeRequest, KnowledgeLensReadRequest, KnowledgeLensSearchRequest, KnowledgeResearchPageDto, KnowledgeResearchSearchRequest, KnowledgeSourceReadRequest, KnowledgeSourceRetainRequest, LensSearchHitDto, MemoryLearnedRequest, MemoryOpUndoRequest, MemorySectionForgetRequest, SourceDetailDto, WorldQueryDto } from "./commands/knowledge";
import type { FollowedStatusRequest } from "./commands/knowledge/followed";
import type { JournalArchiveRequest, JournalCreateRequest, JournalListRequest, JournalUpdateRequest } from "./commands/knowledge/journal";
import type { LensArchiveRequest } from "./commands/knowledge/lens_archive";
import type { PauseRequest, ResetRequest } from "./commands/knowledge/memory_controls";
import type { MemoryUsage, MemoryUsageRequest } from "./commands/knowledge/memory_usage";
import type { MemoryLearningSettingsRequest } from "./commands/knowledge/method_learning";
import type { PrincipleArchiveRequest, PrincipleCreateRequest, PrincipleListRequest, PrincipleUpdateRequest } from "./commands/knowledge/principles";
import type { SourceSelectionPin, SourceSelectionRequest } from "./commands/knowledge/source_selection";
import type { WorkingNoteReadRequest, WorkingNotesPage, WorkingNotesRequest } from "./commands/knowledge/working_notes";
import type { RemoveRequest, UndoRequest, WorldSchemaLoupistRequest } from "./commands/knowledge/world_schema";
import type { LegacyHoldingsStatus } from "./commands/legacy_holdings";
import type { WorldKeepAwakeRequest } from "./commands/power";
import type { QuestionAnswerRequest } from "./commands/questions";
import type { DeliveryRequest, FromPresetRequest, ProposalDeclineRequest, ProposalVisitRequest, RevisionRequest, RunRequest, ScheduledTasksLoupistRequest, StatusRequest, TaskRequest, TriggerGetRequest, TriggerRevertRequest, TriggerReviewRequest, TriggerView } from "./commands/scheduled_tasks";
import type { PythonReplay, PythonStepReplayRequest, ScriptGetRequest, ScriptSaveRequest, ScriptTestRequest, ScriptsListRequest } from "./commands/scripts";
import type { SkillCreateRequest, SkillDeleteRequest, SkillImportRequest, SkillReadRequest, SkillSaveRequest } from "./commands/skills";
import type { ChatArtifactListRequest, ChatArtifactReadRequest, ChatArtifactRestoreRequest, ChatArtifactViewDto, ChatAttachmentReadDto, ChatAttachmentReadRequest, ChatBranchRequest, ChatControlReceiptDto, ChatControlRequestDto, ChatCreateRequest, ChatDeleteRequest, ChatFeedbackRequest, ChatMemorySetAsideRequest, ChatRenameRequest, ChatSaveState, ChatSavesRequest, ChatSendRequest, ChatThreadDto, ChatVersionSwitchRequest, ChatWatchRequest, ChatWatchSnapshot, LoupistArtifactItemDto, LoupistArtifactListRequest, LoupistCreateRequest, LoupistDeleteRequest, LoupistImportRequest, LoupistProfileUpdateRequest, LoupistReconnectRequest, LoupistSummary, LoupistWorkspace, RunStarted } from "./commands/types";
import type { UsageSummary, UsageSummaryRequest } from "./commands/usage";
import type { ChatActivityRequest, WorkspacePathRequest } from "./commands/workspace";
import type { DraftAttachment, DraftIdentity, StoredChatDraft, WorkspaceUiState, WorkspaceUiWriteRequest } from "./commands/workspace_ui";
import type { ComposerModelRead, ComposerModelSelection, ComposerModelWrite } from "./commands/workspace_ui/composer_model";
import type { AddCommandRequest, AddJsonRequest, AddUrlRequest, ApproveRequest, AuthorizeRequest, ConfirmRequest, EnabledRequest, IdentityRequest, ListRequest, MemoryRequest } from "./custom_connections/commands";
import type { CustomConnection } from "./custom_connections/service";
import type { ChatFeedback } from "./domain/chat";
import type { JsonValue } from "./external";
import type { FxDisplay } from "./finance_data/fx";
import type { WorkspaceDocumentHandler, WorkspaceDocumentOpen, WorkspaceDocumentRequest } from "./fs/workspace_document";
import type { WorkspaceDirectory, WorkspaceFile } from "./fs/workspace_files";
import type { FollowedStatus } from "./knowledge/followed";
import type { GraphJobView, GraphProcessingView, SourcesRetried } from "./knowledge/graph_jobs";
import type { GrowthEventsPage, GrowthEventsQuery } from "./knowledge/growth";
import type { LearningSettings, Review, ReviewQuery } from "./knowledge/learning";
import type { LearnedDigest, PageRead, PageSummary } from "./knowledge/lens";
import type { KnowledgeMap, KnowledgeMapQuery } from "./knowledge/map";
import type { MemoryControls } from "./knowledge/memory_controls";
import type { ResetCounts } from "./knowledge/memory_reset";
import type { JournalRecord, RetainedSource, WorkingNote, WorldGraphQuery } from "./knowledge/model";
import type { Principle } from "./knowledge/principles";
import type { ConnectionView } from "./knowledge/world_followups";
import type { WorldSchemaView } from "./knowledge/world_schema";
import type { LoupistCapabilityBindingDto, LoupistCapabilityRequest, MarketplaceConnectorConfigureRequest, MarketplaceConnectorStatusDto, MarketplaceCredentialRequest, MarketplaceCredentialSaveRequest, MarketplaceCredentialStatusDto, MarketplaceSnapshotDto } from "./marketplace/catalog";
import type { KeepAwakeStatus } from "./power";
import type { ModelCatalogView } from "./providers/catalog";
import type { ModelNoticeAckRequest, ProviderConfigureRequest, ProviderConnectionRequest, ProviderModelsRequest, ProviderPageRequest, ProviderSubmitCodeRequest } from "./providers/connection";
import type { CustomProviderRequest } from "./providers/custom";
import type { LocalDiscovery, ProviderCustomTestRequest, ProviderCustomTestResult, ProviderPresetRequest } from "./providers/local";
import type { PythonStep } from "./python_run/ledger";
import type { WorkbookRead, WorkbookRequest } from "./python_run/workbook";
import type { RunActivity } from "./runs/coordinator";
import type { DiagnosticsRequest } from "./runs/log";
import type { WatchSnapshot } from "./scheduled_tasks";
import type { Run, Task, TaskListItem, TriggerEvaluation } from "./scheduled_tasks/types";
import type { ConfigReadRequest, ConfigSnapshot, ConfigUpdateRequest } from "./scoped_config";
import type { SharingLoupistRequest } from "./sharing/commands";
import type { PackAddRequest } from "./sharing/commands/add";
import type { PackImportRequest, PackImportView, PackOriginRequest, PackRemoveRequest } from "./sharing/commands/added";
import type { GalleryImageRequest, GalleryIndexRequest, GalleryListingRequest, GalleryView } from "./sharing/commands/gallery";
import type { GithubAccount } from "./sharing/commands/github_account";
import type { PackOpenRequest } from "./sharing/commands/open";
import type { SharePublishRequest } from "./sharing/commands/publish";
import type { PackDecideRequest, PackItemRequest, SuggestionGroup } from "./sharing/commands/review";
import type { ShareExportRequest } from "./sharing/commands/save";
import type { ShareRequest, ShareSummary, ShareSummaryRequest } from "./sharing/commands/share";
import type { PackApplyUpdateRequest, PackCheckRequest, PackChooseNewerRequest, UpdateCheck } from "./sharing/commands/updates";
import type { DeepLinkEvent } from "./sharing/deep_link";
import type { ItemView, OpenResult } from "./sharing/import";
import type { SkillCatalog, SkillDocument } from "./skills";
import type { AppSettings } from "./store/app_settings";
import type { ScriptSummary, ScriptVersion } from "./store/scripts";
import type { UpdateInstallRequest, UpdateInstallResult, UpdateStateDto } from "./updater";
import type { BrowserTabBoundsRequest, BrowserTabHistoryRequest, BrowserTabNavigateRequest, BrowserTabOpenRequest, BrowserTabRequest, BrowserTabState } from "./web/browser";

/**
 * Each registered command's `request` argument and response (`commands!` in
 * `src-tauri/src/bridge_contract.rs`). `never`: the command takes other arguments.
 */
export type BridgeCommands = {
  app_menu_update: { request: AppMenuContext, response: null },
  bot_offer_accept: { request: OfferRequest, response: Task },
  bot_offer_dismiss: { request: OfferRequest, response: null },
  bot_ask_answer: { request: AskAnswerRequest, response: null },
  briefing_settings_get: { request: BriefingsLoupistRequest, response: BriefingSettings },
  briefing_settings_set: { request: BriefingSettingsRequest, response: BriefingSettings },
  inbox_list: { request: InboxRequest, response: InboxPage },
  inbox_mark_read: { request: ReadRequest, response: null },
  inbox_unread_counts: { request: never, response: Array<UnreadCount> },
  briefing_resident_settings_get: { request: never, response: DesktopSettings },
  briefing_resident_settings_set: { request: ResidentRequest, response: DesktopSettings },
  login_item_set: { request: LoginItemRequest, response: DesktopSettings },
  login_item_hint_dismiss: { request: never, response: DesktopSettings },
  login_item_open_settings: { request: never, response: null },
  model_catalog_get: { request: never, response: ModelCatalogView },
  model_visibility_update: { request: ModelVisibilityUpdateRequest, response: ModelCatalogView },
  open_external_url: { request: never, response: null },
  run_activity_list: { request: never, response: Array<RunActivity> },
  notice_acknowledge: { request: never, response: AppSettings },
  notice_settings_get: { request: never, response: AppSettings },
  chat_approval_decide: { request: ApprovalDecideRequest, response: null },
  assets_list: { request: AssetsListRequest, response: Array<AssetRow> },
  asset_follow_name: { request: AssetFollowNameRequest, response: AssetRow },
  asset_unfollow: { request: AssetIdRequest, response: null },
  asset_lookup: { request: AssetLookupRequest, response: AssetLookupResult },
  asset_price_check: { request: AssetIdRequest, response: ThinAssetPageView },
  asset_follow: { request: AssetIdRequest, response: AssetRow },
  asset_entity_set: { request: AssetEntitySetRequest, response: ThinAssetPageView },
  asset_page_get: { request: AssetOpenRequest, response: ThinAssetPageView },
  asset_page_seen: { request: AssetIdRequest, response: null },
  watchlist_quotes: { request: AssetsListRequest, response: Array<StoredAssetQuote> },
  asset_price_series: { request: AssetPriceSeriesRequest, response: Array<AssetPricePoint> },
  asset_verdict_undo: { request: AssetVerdictUndoRequest, response: ThinAssetPageView },
  attention_list: { request: never, response: Array<AttentionItem> },
  attention_notify: { request: NotifyRequest, response: boolean },
  chat_abort: { request: never, response: null },
  chat_retry_last: { request: ChatRetryRequest, response: ChatRetryReceipt },
  chat_saves: { request: ChatSavesRequest, response: Array<ChatSaveState> },
  chat_send: { request: ChatSendRequest, response: RunStarted },
  chat_steer: { request: ChatControlRequestDto, response: ChatControlReceiptDto },
  chat_unwatch: { request: ChatWatchRequest, response: null },
  chat_watch: { request: ChatWatchRequest, response: ChatWatchSnapshot | null },
  config_read: { request: ConfigReadRequest, response: ConfigSnapshot },
  config_reveal: { request: ConfigReadRequest, response: null },
  config_update: { request: ConfigUpdateRequest, response: ConfigSnapshot },
  pick_file: { request: PickFileRequest, response: string | null },
  clipboard_read_image: { request: never, response: ClipboardImage },
  export_save: { request: ExportRequest, response: string | null },
  scripts_list: { request: ScriptsListRequest, response: Array<ScriptSummary> },
  script_get: { request: ScriptGetRequest, response: ScriptVersion },
  script_save: { request: ScriptSaveRequest, response: ScriptVersion },
  script_delete: { request: ScriptGetRequest, response: null },
  script_test: { request: ScriptTestRequest, response: PythonStep },
  python_step_replay: { request: PythonStepReplayRequest, response: PythonReplay },
  research_handoff_start: { request: ResearchHandoffRequest, response: ResearchHandoffReceipt },
  knowledge_catalog: { request: KnowledgeCatalogRequest, response: KnowledgeCatalogDto },
  knowledge_connection_dismiss: { request: KnowledgeConnectionRequest, response: ConnectionView },
  knowledge_graph_job: { request: KnowledgeGraphJobRequest, response: GraphJobView },
  knowledge_graph_pause: { request: KnowledgeGraphPauseRequest, response: GraphProcessingView },
  knowledge_graph_retry_source: {
    request: KnowledgeGraphRetrySourceRequest,
    response: GraphJobView,
  },
  knowledge_graph_retry_sources: {
    request: KnowledgeGraphRetrySourcesRequest,
    response: SourcesRetried,
  },
  knowledge_graph_recover: { request: KnowledgeGraphLoupistRequest, response: GraphProcessingView },
  knowledge_graph_settings_get: {
    request: KnowledgeGraphLoupistRequest,
    response: GraphProcessingView,
  },
  knowledge_graph_settings_set: {
    request: KnowledgeGraphSettingsRequest,
    response: GraphProcessingView,
  },
  knowledge_growth_events: { request: GrowthEventsQuery, response: GrowthEventsPage },
  knowledge_journal_read: { request: KnowledgeJournalReadRequest, response: JournalRecord },
  knowledge_journal_source_read: {
    request: KnowledgeJournalSourceReadRequest,
    response: JournalSourceReadDto,
  },
  knowledge_lens_pages_for_node: {
    request: KnowledgeLensPagesForNodeRequest,
    response: Array<PageSummary>,
  },
  knowledge_lens_read: { request: KnowledgeLensReadRequest, response: PageRead },
  knowledge_lens_search: { request: KnowledgeLensSearchRequest, response: Array<LensSearchHitDto> },
  knowledge_map: { request: KnowledgeMapQuery, response: KnowledgeMap },
  knowledge_research_search: {
    request: KnowledgeResearchSearchRequest,
    response: KnowledgeResearchPageDto,
  },
  knowledge_source_read: { request: KnowledgeSourceReadRequest, response: SourceDetailDto },
  knowledge_source_retain: { request: KnowledgeSourceRetainRequest, response: RetainedSource },
  knowledge_world_query: { request: WorldGraphQuery, response: WorldQueryDto },
  memory_learned: { request: MemoryLearnedRequest, response: Array<LearnedDigest> },
  memory_op_undo: { request: MemoryOpUndoRequest, response: PageRead },
  memory_section_forget: { request: MemorySectionForgetRequest, response: PageRead },
  knowledge_followed_status: { request: FollowedStatusRequest, response: FollowedStatus },
  journal_archive: { request: JournalArchiveRequest, response: JournalRecord },
  journal_create: { request: JournalCreateRequest, response: JournalRecord },
  journal_list: { request: JournalListRequest, response: Array<JournalRecord> },
  journal_update: { request: JournalUpdateRequest, response: JournalRecord },
  knowledge_lens_archive: { request: LensArchiveRequest, response: null },
  knowledge_lens_pause: { request: PauseRequest, response: MemoryControls },
  knowledge_memory_controls: { request: KnowledgeCatalogRequest, response: MemoryControls },
  knowledge_memory_reset: { request: ResetRequest, response: ResetCounts },
  knowledge_memory_reset_preview: { request: KnowledgeCatalogRequest, response: ResetCounts },
  knowledge_memory_usage: { request: MemoryUsageRequest, response: Array<MemoryUsage> },
  learning_review_list: { request: ReviewQuery, response: Array<Review> },
  memory_learning_settings_get: { request: KnowledgeCatalogRequest, response: LearningSettings },
  memory_learning_settings_set: {
    request: MemoryLearningSettingsRequest,
    response: LearningSettings,
  },
  principle_archive: { request: PrincipleArchiveRequest, response: Principle },
  principle_create: { request: PrincipleCreateRequest, response: Principle },
  principle_list: { request: PrincipleListRequest, response: Array<Principle> },
  principle_update: { request: PrincipleUpdateRequest, response: Principle },
  knowledge_source_export: { request: KnowledgeSourceReadRequest, response: string | null },
  knowledge_source_selection_resolve: {
    request: SourceSelectionRequest,
    response: SourceSelectionPin,
  },
  knowledge_working_note_read: { request: WorkingNoteReadRequest, response: WorkingNote },
  knowledge_working_notes: { request: WorkingNotesRequest, response: WorkingNotesPage },
  world_schema_get: { request: WorldSchemaLoupistRequest, response: WorldSchemaView },
  world_schema_remove: { request: RemoveRequest, response: WorldSchemaView },
  world_schema_undo: { request: UndoRequest, response: WorldSchemaView },
  agent_skill_catalog: { request: never, response: Array<AgentSkillSummary> },
  loupist_choose_folder: { request: never, response: string | null },
  loupist_create: { request: LoupistCreateRequest, response: LoupistSummary },
  loupist_delete: { request: LoupistDeleteRequest, response: null },
  loupist_import: { request: LoupistImportRequest, response: LoupistSummary },
  loupist_list: { request: never, response: Array<LoupistSummary> },
  loupist_profile_update: { request: LoupistProfileUpdateRequest, response: LoupistSummary },
  loupist_reconnect: { request: LoupistReconnectRequest, response: LoupistSummary },
  loupist_select: { request: never, response: LoupistWorkspace },
  chat_question_answer: { request: QuestionAnswerRequest, response: null },
  artifact_restore: { request: ChatArtifactRestoreRequest, response: ChatArtifactViewDto },
  artifact_versions: { request: ChatArtifactReadRequest, response: Array<ChatArtifactVersion> },
  chat_artifact_list: { request: ChatArtifactListRequest, response: Array<ChatArtifact> },
  chat_artifact_read: { request: ChatArtifactReadRequest, response: ChatArtifactViewDto },
  chat_attachment_read: { request: ChatAttachmentReadRequest, response: ChatAttachmentReadDto },
  chat_branch: { request: ChatBranchRequest, response: ChatThreadDto },
  chat_create: { request: ChatCreateRequest, response: ChatThreadDto },
  chat_delete: { request: ChatDeleteRequest, response: null },
  chat_feedback_set: { request: ChatFeedbackRequest, response: ChatFeedback },
  chat_memory_set_aside: { request: ChatMemorySetAsideRequest, response: ChatThreadDto },
  chat_rename: { request: ChatRenameRequest, response: ChatThreadDto },
  chat_version_switch: { request: ChatVersionSwitchRequest, response: ChatThreadDto },
  loupist_artifact_list: {
    request: LoupistArtifactListRequest,
    response: Array<LoupistArtifactItemDto>,
  },
  scheduled_task_confirm: { request: RevisionRequest, response: Task },
  scheduled_task_delete: { request: RevisionRequest, response: null },
  scheduled_proposal_set_declined: { request: ProposalDeclineRequest, response: null },
  scheduled_proposal_visit_end: { request: ProposalVisitRequest, response: null },
  scheduled_task_discard: { request: TaskRequest, response: null },
  notification_permission_settings_open: { request: never, response: null },
  notification_bot_focus_set: { request: BotFocusRequest, response: null },
  notification_templates_set: { request: TemplatesRequest, response: null },
  scheduled_task_from_preset: { request: FromPresetRequest, response: Task },
  scheduled_trigger_get: { request: TriggerGetRequest, response: TriggerView },
  scheduled_trigger_approve: { request: TriggerReviewRequest, response: Task },
  scheduled_trigger_revert: { request: TriggerRevertRequest, response: Task },
  scheduled_trigger_test: { request: TriggerReviewRequest, response: TriggerEvaluation },
  scheduled_trigger_evaluations: { request: TaskRequest, response: Array<TriggerEvaluation> },
  scheduled_task_set_delivery: { request: DeliveryRequest, response: Task },
  scheduled_task_get: { request: TaskRequest, response: Task | null },
  scheduled_task_run_now: { request: TaskRequest, response: Run },
  scheduled_task_runs: { request: TaskRequest, response: Array<Run> },
  scheduled_task_set_status: { request: StatusRequest, response: Task },
  scheduled_task_watch: { request: RunRequest, response: WatchSnapshot | null },
  scheduled_tasks_activity: { request: ScheduledTasksLoupistRequest, response: Array<Run> },
  scheduled_tasks_list: { request: ScheduledTasksLoupistRequest, response: Array<TaskListItem> },
  skills_create: { request: SkillCreateRequest, response: SkillCatalog },
  skills_delete: { request: SkillDeleteRequest, response: SkillCatalog },
  skills_import: { request: SkillImportRequest, response: SkillCatalog | null },
  skills_list: { request: ConfigReadRequest, response: SkillCatalog },
  skills_read: { request: SkillReadRequest, response: SkillDocument },
  skills_reveal: { request: SkillReadRequest, response: null },
  skills_save: { request: SkillSaveRequest, response: SkillCatalog },
  fx_display: { request: FxDisplayRequest, response: FxDisplay },
  legacy_holdings_status: { request: never, response: LegacyHoldingsStatus },
  legacy_holdings_retry: { request: never, response: LegacyHoldingsStatus },
  legacy_holdings_reveal: { request: never, response: null },
  legacy_holdings_save_copy: { request: never, response: boolean },
  legacy_holdings_dismiss: { request: never, response: LegacyHoldingsStatus },
  chat_activity_read: { request: ChatActivityRequest, response: ChatActivityDetail },
  workspace_list: { request: WorkspacePathRequest, response: WorkspaceDirectory },
  workspace_workbook_read: { request: WorkbookRequest, response: WorkbookRead },
  workspace_read: { request: WorkspacePathRequest, response: WorkspaceFile },
  workspace_document_handler: {
    request: WorkspaceDocumentRequest,
    response: WorkspaceDocumentHandler,
  },
  workspace_document_open: { request: WorkspaceDocumentRequest, response: WorkspaceDocumentOpen },
  workspace_reveal: { request: WorkspacePathRequest, response: null },
  chat_draft_files_read: { request: DraftIdentity, response: Array<DraftAttachment> },
  chat_draft_files_write: { request: DraftIdentity, response: null },
  chat_draft_read: { request: DraftIdentity, response: StoredChatDraft | null },
  chat_draft_write: { request: DraftIdentity, response: null },
  workspace_ui_read: { request: never, response: WorkspaceUiState | null },
  chat_composer_model_read: { request: ComposerModelRead, response: ComposerModelSelection | null },
  chat_composer_model_write: { request: ComposerModelWrite, response: null },
  workspace_ui_write: { request: WorkspaceUiWriteRequest, response: WorkspaceUiState },
  custom_connection_add_command: { request: AddCommandRequest, response: CustomConnection },
  custom_connection_add_json: { request: AddJsonRequest, response: Array<CustomConnection> },
  custom_connection_add_url: { request: AddUrlRequest, response: CustomConnection },
  custom_connection_authorize: { request: AuthorizeRequest, response: CustomConnection },
  custom_connection_confirm: { request: ConfirmRequest, response: CustomConnection },
  custom_connection_list: { request: ListRequest, response: Array<CustomConnection> },
  custom_connection_remove: { request: IdentityRequest, response: null },
  custom_connection_set_enabled: { request: EnabledRequest, response: CustomConnection },
  custom_connection_set_memory: { request: MemoryRequest, response: CustomConnection },
  custom_connection_sign_in: { request: IdentityRequest, response: CustomConnection },
  custom_connection_sign_in_cancel: { request: IdentityRequest, response: null },
  custom_tool_approve: { request: ApproveRequest, response: CustomConnection },
  loupist_capability_disable: {
    request: LoupistCapabilityRequest,
    response: LoupistCapabilityBindingDto,
  },
  loupist_capability_enable: {
    request: LoupistCapabilityRequest,
    response: LoupistCapabilityBindingDto,
  },
  loupist_capability_list: { request: never, response: Array<LoupistCapabilityBindingDto> },
  marketplace_snapshot: { request: never, response: MarketplaceSnapshotDto },
  marketplace_connector_configure: {
    request: MarketplaceConnectorConfigureRequest,
    response: MarketplaceConnectorStatusDto,
  },
  marketplace_credential_delete: {
    request: MarketplaceCredentialRequest,
    response: Array<MarketplaceCredentialStatusDto>,
  },
  marketplace_credential_save: {
    request: MarketplaceCredentialSaveRequest,
    response: Array<MarketplaceCredentialStatusDto>,
  },
  marketplace_credential_unlock: {
    request: MarketplaceCredentialRequest,
    response: Array<MarketplaceCredentialStatusDto>,
  },
  marketplace_credentials_allow_access: { request: string, response: null },
  marketplace_credentials_cancel_access: { request: string, response: null },
  marketplace_credential_verify: {
    request: MarketplaceCredentialRequest,
    response: Array<MarketplaceCredentialStatusDto>,
  },
  model_notice_ack: { request: ModelNoticeAckRequest, response: ModelCatalogView },
  provider_configure: { request: ProviderConfigureRequest, response: ModelCatalogView },
  provider_connect: { request: ProviderConnectionRequest, response: ModelCatalogView },
  provider_connect_cancel: { request: never, response: null },
  provider_connect_open_browser: { request: never, response: null },
  provider_connect_submit_code: { request: ProviderSubmitCodeRequest, response: null },
  provider_custom_remove: { request: ProviderConnectionRequest, response: ModelCatalogView },
  provider_custom_save: { request: CustomProviderRequest, response: ModelCatalogView },
  provider_disconnect: { request: ProviderConnectionRequest, response: ModelCatalogView },
  provider_models: { request: ProviderModelsRequest, response: ModelCatalogView },
  provider_open_console: { request: ProviderPageRequest, response: null },
  provider_open_usage: { request: ProviderPageRequest, response: null },
  usage_summary: { request: UsageSummaryRequest, response: UsageSummary },
  bot_memory_list: { request: BotMemoryListRequest, response: Array<BotMemoryListEntry> },
  bot_memory_forget: { request: BotMemoryForgetRequest, response: Array<BotMemoryListEntry> },
  bot_memory_update: { request: BotMemoryUpdateRequest, response: Array<BotMemoryListEntry> },
  provider_custom_test: { request: ProviderCustomTestRequest, response: ProviderCustomTestResult },
  provider_local_discover: { request: never, response: LocalDiscovery },
  provider_preset_save: { request: ProviderPresetRequest, response: ModelCatalogView },
  run_logs_last_error: { request: DiagnosticsRequest, response: string | null },
  run_logs_reveal: { request: DiagnosticsRequest, response: null },
  deep_link_dismiss: { request: never, response: null },
  deep_link_pending: { request: never, response: DeepLinkEvent | null },
  gallery_image: { request: GalleryImageRequest, response: string | null },
  gallery_index: { request: GalleryIndexRequest, response: GalleryView },
  gallery_listing_request: { request: GalleryListingRequest, response: JsonValue },
  gallery_listing_status: { request: SharingLoupistRequest, response: string | null },
  github_account: { request: never, response: GithubAccount },
  github_sign_in: { request: never, response: GithubAccount },
  github_sign_in_cancel: { request: never, response: null },
  github_sign_in_open: { request: never, response: null },
  github_sign_out: { request: never, response: GithubAccount },
  pack_add: { request: PackAddRequest, response: JsonValue },
  pack_apply_update: { request: PackApplyUpdateRequest, response: Array<PackImportView> },
  pack_check_update: { request: PackCheckRequest, response: UpdateCheck },
  pack_choose_newer: { request: PackChooseNewerRequest, response: UpdateCheck | null },
  pack_decide: { request: PackDecideRequest, response: Array<SuggestionGroup> },
  pack_item_read: { request: PackItemRequest, response: ItemView },
  pack_list: { request: SharingLoupistRequest, response: Array<PackImportView> },
  pack_open: { request: PackOpenRequest, response: OpenResult | null },
  pack_origin: { request: PackOriginRequest, response: JsonValue | null },
  pack_remove: { request: PackRemoveRequest, response: Array<PackImportView> },
  pack_report: { request: PackImportRequest, response: null },
  pack_suggestions: { request: SharingLoupistRequest, response: Array<SuggestionGroup> },
  share_copy_text: { request: ShareRequest, response: JsonValue },
  share_export: { request: ShareExportRequest, response: JsonValue | null },
  share_publish: { request: SharePublishRequest, response: JsonValue },
  share_reveal: { request: SharingLoupistRequest, response: null },
  share_summary: { request: ShareSummaryRequest, response: ShareSummary },
  update_check: { request: never, response: UpdateStateDto },
  update_install: { request: UpdateInstallRequest, response: UpdateInstallResult },
  update_status: { request: never, response: UpdateStateDto },
  browser_tab_close: { request: BrowserTabRequest, response: null },
  browser_tab_history: { request: BrowserTabHistoryRequest, response: null },
  browser_tab_navigate: { request: BrowserTabNavigateRequest, response: null },
  browser_tab_open: { request: BrowserTabOpenRequest, response: BrowserTabState },
  browser_tab_reload: { request: BrowserTabRequest, response: null },
  browser_tab_set_bounds: { request: BrowserTabBoundsRequest, response: null },
  browser_tabs_reset: { request: never, response: null },
  site_favicon: { request: never, response: string | null },
  world_keep_awake_get: { request: never, response: KeepAwakeStatus },
  world_keep_awake_set: { request: WorldKeepAwakeRequest, response: KeepAwakeStatus },
};
