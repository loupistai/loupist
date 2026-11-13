//! Generated renderer wire types (test only).
//!
//! `apps/loupist/src/contracts/generated/` holds the TypeScript for the Rust
//! types the bridge sends and receives: command requests and responses,
//! stream events and emitted event payloads. Each Rust type derives
//! `ts_rs::TS` under `cfg(test)`; this module walks the roots below, writes
//! one TypeScript module per Rust module and fails when the committed files
//! differ. Regenerate with [`REGENERATE`].

use std::any::TypeId;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use ts_rs::{Config, TypeVisitor, TS};

const REGENERATE: &str = "LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract";
const HEADER: &str = "// Generated from Rust by ts-rs; do not edit. Regenerate with:\n// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract\n";
const WIDTH: usize = 100;

/// Every bridge request, response, stream event and emitted payload. Their
/// field types follow automatically.
fn roots(collector: &mut Collector<'_>) {
    macro_rules! visit {
        ($($ty:ty),* $(,)?) => {$( collector.visit::<$ty>(); )*};
    }
    visit! {
        // Named only inside `ChartView::Analytic`'s TypeScript override.
        crate::chat::chart::AnalyticChartType,
        crate::app::CommandError,
        crate::events::LearningReviewFinished,
        crate::events::WatchChanged,
        crate::events::WorldSchemaChanged,
        crate::agent_harness::AgentSkillSummary,
        crate::briefings::commands::BriefingSettings,
        crate::briefings::commands::InboxPage,
        crate::briefings::commands::InboxRequest,
        crate::briefings::commands::LoupistRequest,
        crate::briefings::commands::ReadRequest,
        crate::briefings::commands::SettingsRequest,
        crate::briefings::commands::UnreadCount,
        crate::briefings::desktop::InboxChanged,
        crate::briefings::desktop::InboxOpen,
        crate::briefings::desktop::ResidentRequest,
        crate::briefings::desktop::ResidentSettings,
        crate::chat::activity::ChatActivityDetail,
        crate::chat::artifacts::ChatArtifact,
        crate::chat::artifacts::ChatArtifactVersion,
        crate::commands::ModelVisibilityUpdateRequest,
        crate::commands::approvals::ApprovalDecideRequest,
        crate::commands::assets::AssetClassSetRequest,
        crate::commands::assets::AssetDeltaRequest,
        crate::commands::assets::AssetPageGetRequest,
        crate::commands::assets::AssetPageSeenRequest,
        crate::commands::assets::AssetRefreshRequest,
        crate::commands::assets::AssetVerdictUndoRequest,
        crate::commands::assets::UpcomingEventsRequest,
        crate::commands::attention::AttentionItem,
        crate::commands::attention::NotifyRequest,
        crate::commands::chat_runtime::ChatRetryReceipt,
        crate::commands::chat_runtime::ChatRetryRequest,
        crate::commands::dialog::PickFileRequest,
        crate::commands::earnings::EarningsEventsListRequest,
        crate::commands::earnings::EarningsPrepareRequest,
        crate::commands::earnings::EarningsSnapshotGetRequest,
        crate::commands::export_file::ExportRequest,
        crate::commands::handoff::ResearchHandoffReceipt,
        crate::commands::handoff::ResearchHandoffRequest,
        crate::commands::knowledge::JournalSourceReadDto,
        crate::commands::knowledge::KnowledgeCatalogDto,
        crate::commands::knowledge::KnowledgeCatalogRequest,
        crate::commands::knowledge::KnowledgeConnectionRequest,
        crate::commands::knowledge::KnowledgeGraphJobRequest,
        crate::commands::knowledge::KnowledgeGraphLoupistRequest,
        crate::commands::knowledge::KnowledgeGraphPauseRequest,
        crate::commands::knowledge::KnowledgeGraphRetrySourceRequest,
        crate::commands::knowledge::KnowledgeGraphSettingsRequest,
        crate::commands::knowledge::KnowledgeJournalReadRequest,
        crate::commands::knowledge::KnowledgeJournalSourceReadRequest,
        crate::commands::knowledge::KnowledgeLensPagesForNodeRequest,
        crate::commands::knowledge::KnowledgeLensReadRequest,
        crate::commands::knowledge::KnowledgeLensSearchRequest,
        crate::commands::knowledge::KnowledgeResearchPageDto,
        crate::commands::knowledge::KnowledgeResearchSearchRequest,
        crate::commands::knowledge::KnowledgeSourceReadRequest,
        crate::commands::knowledge::KnowledgeSourceRetainRequest,
        crate::commands::knowledge::LensSearchHitDto,
        crate::commands::knowledge::MemoryLearnedRequest,
        crate::commands::knowledge::MemoryOpUndoRequest,
        crate::commands::knowledge::MemorySectionForgetRequest,
        crate::commands::knowledge::SourceDetailDto,
        crate::commands::knowledge::WorldQueryDto,
        crate::commands::knowledge::followed::FollowedStatusRequest,
        crate::commands::knowledge::journal::JournalArchiveRequest,
        crate::commands::knowledge::journal::JournalCreateRequest,
        crate::commands::knowledge::journal::JournalListRequest,
        crate::commands::knowledge::journal::JournalUpdateRequest,
        crate::commands::knowledge::lens_archive::LensArchiveRequest,
        crate::commands::knowledge::memory_controls::PauseRequest,
        crate::commands::knowledge::memory_controls::ResetRequest,
        crate::commands::knowledge::memory_usage::MemoryUsage,
        crate::commands::knowledge::memory_usage::MemoryUsageRequest,
        crate::commands::knowledge::method_learning::SettingsRequest,
        crate::commands::knowledge::principles::PrincipleArchiveRequest,
        crate::commands::knowledge::principles::PrincipleCreateRequest,
        crate::commands::knowledge::principles::PrincipleListRequest,
        crate::commands::knowledge::principles::PrincipleUpdateRequest,
        crate::commands::knowledge::source_selection::SourceSelectionPin,
        crate::commands::knowledge::source_selection::SourceSelectionRequest,
        crate::commands::knowledge::working_notes::WorkingNoteReadRequest,
        crate::commands::knowledge::working_notes::WorkingNotesPage,
        crate::commands::knowledge::working_notes::WorkingNotesRequest,
        crate::commands::knowledge::world_schema::LoupistRequest,
        crate::commands::knowledge::world_schema::RemoveRequest,
        crate::commands::knowledge::world_schema::UndoRequest,
        crate::commands::questions::QuestionAnswerRequest,
        crate::commands::scheduled_tasks::LoupistRequest,
        crate::commands::scheduled_tasks::RevisionRequest,
        crate::commands::scheduled_tasks::RunRequest,
        crate::commands::scheduled_tasks::StatusRequest,
        crate::commands::scheduled_tasks::TaskRequest,
        crate::commands::skills::SkillCreateRequest,
        crate::commands::skills::SkillDeleteRequest,
        crate::commands::skills::SkillImportRequest,
        crate::commands::skills::SkillReadRequest,
        crate::commands::skills::SkillSaveRequest,
        crate::commands::types::ChatArtifactListRequest,
        crate::commands::types::ChatArtifactReadRequest,
        crate::commands::types::ChatArtifactRestoreRequest,
        crate::commands::types::ChatArtifactViewDto,
        crate::commands::types::ChatAttachmentReadDto,
        crate::commands::types::ChatAttachmentReadRequest,
        crate::commands::types::ChatBranchRequest,
        crate::commands::types::ChatControlReceiptDto,
        crate::commands::types::ChatControlRequestDto,
        crate::commands::types::ChatCreateRequest,
        crate::commands::types::ChatDeleteRequest,
        crate::commands::types::ChatFeedbackRequest,
        crate::commands::types::ChatMemorySetAsideRequest,
        crate::commands::types::ChatRenameRequest,
        crate::commands::types::ChatSendRequest,
        crate::commands::types::ChatStreamEvent,
        crate::commands::types::ChatThreadDto,
        crate::commands::types::ChatVersionSwitchRequest,
        crate::commands::types::LoupistArtifactItemDto,
        crate::commands::types::LoupistArtifactListRequest,
        crate::commands::types::LoupistCreateRequest,
        crate::commands::types::LoupistDeleteRequest,
        crate::commands::types::LoupistImportRequest,
        crate::commands::types::LoupistProfileUpdateRequest,
        crate::commands::types::LoupistReconnectRequest,
        crate::commands::types::LoupistSummary,
        crate::commands::types::LoupistWorkspace,
        crate::commands::types::RunStarted,
        crate::commands::user_context::CsvCommitRequest,
        crate::commands::user_context::CsvImportRequest,
        crate::commands::user_context::MapColumnsRequest,
        crate::commands::user_context::WalletAddRequest,
        crate::commands::user_context::WalletIdRequest,
        crate::commands::user_context::WalletRefreshRequest,
        crate::commands::user_context::WalletRenameRequest,
        crate::commands::user_context::WatchlistAddRequest,
        crate::commands::user_context::WatchlistRemoveRequest,
        crate::commands::watch::WatchCheckNowRequest,
        crate::commands::watch::WatchCheckResult,
        crate::commands::watch::WatchDefaultsApplyRequest,
        crate::commands::watch::WatchDefaultsApplyResult,
        crate::commands::watch::WatchEventsListRequest,
        crate::commands::watch::WatchEventsSeenRequest,
        crate::commands::watch::WatchPauseSetRequest,
        crate::commands::watch::WatchSettingSetRequest,
        crate::commands::watch::WatchSettingsListRequest,
        crate::commands::watch::WatchSettingsView,
        crate::commands::workspace::ChatActivityRequest,
        crate::commands::workspace::WorkspacePathRequest,
        crate::commands::workspace_ui::DraftAttachment,
        crate::commands::workspace_ui::DraftIdentity,
        crate::commands::workspace_ui::StoredChatDraft,
        crate::commands::workspace_ui::WorkspaceUiState,
        crate::commands::workspace_ui::WorkspaceUiWriteRequest,
        crate::custom_connections::commands::AddCommandRequest,
        crate::custom_connections::commands::AddJsonRequest,
        crate::custom_connections::commands::AddUrlRequest,
        crate::custom_connections::commands::ApproveRequest,
        crate::custom_connections::commands::AuthorizeRequest,
        crate::custom_connections::commands::ConfirmRequest,
        crate::custom_connections::commands::EnabledRequest,
        crate::custom_connections::commands::IdentityRequest,
        crate::custom_connections::commands::ListRequest,
        crate::custom_connections::commands::MemoryRequest,
        crate::custom_connections::service::ConnectionView,
        crate::domain::ChatFeedback,
        crate::fs::workspace_files::WorkspaceDirectory,
        crate::fs::workspace_files::WorkspaceFile,
        crate::knowledge::assets::AssetDelta,
        crate::knowledge::assets::AssetPageView,
        crate::knowledge::assets::upcoming::UpcomingEvent,
        crate::knowledge::earnings::EventSummary,
        crate::knowledge::earnings::SnapshotView,
        crate::knowledge::FollowedStatus,
        crate::knowledge::GraphJobView,
        crate::knowledge::GraphProcessingView,
        crate::knowledge::GrowthEventsPage,
        crate::knowledge::GrowthEventsQuery,
        crate::knowledge::learning::LearningSettings,
        crate::knowledge::learning::Review,
        crate::knowledge::learning::ReviewQuery,
        crate::knowledge::LearnedDigest,
        crate::knowledge::PageRead,
        crate::knowledge::PageSummary,
        crate::knowledge::KnowledgeMap,
        crate::knowledge::KnowledgeMapQuery,
        crate::knowledge::memory_controls::MemoryControls,
        crate::knowledge::memory_reset::ResetCounts,
        crate::knowledge::JournalRecord,
        crate::knowledge::RetainedSource,
        crate::knowledge::WorkingNote,
        crate::knowledge::WorldGraphQuery,
        crate::knowledge::principles::Principle,
        crate::knowledge::watch::WatchEventView,
        crate::knowledge::ConnectionView,
        crate::knowledge::WorldSchemaView,
        crate::marketplace::LoupistCapabilityBindingDto,
        crate::marketplace::LoupistCapabilityRequest,
        crate::marketplace::MarketplaceConnectorConfigureRequest,
        crate::marketplace::MarketplaceConnectorStatusDto,
        crate::marketplace::MarketplaceCredentialRequest,
        crate::marketplace::MarketplaceCredentialSaveRequest,
        crate::marketplace::MarketplaceCredentialStatusDto,
        crate::marketplace::MarketplaceSnapshotDto,
        crate::providers::catalog::ModelCatalogView,
        crate::providers::connection::ModelNoticeAckRequest,
        crate::providers::connection::ProviderConfigureRequest,
        crate::providers::connection::ProviderConnectionEvent,
        crate::providers::connection::ProviderConnectionRequest,
        crate::providers::connection::ProviderModelsRequest,
        crate::providers::connection::ProviderPageRequest,
        crate::providers::connection::ProviderSubmitCodeRequest,
        crate::providers::custom::CustomProviderRequest,
        crate::providers::local::LocalDiscovery,
        crate::providers::local::ProviderCustomTestRequest,
        crate::providers::local::ProviderCustomTestResult,
        crate::providers::local::ProviderPresetRequest,
        crate::providers::status::StatusEvent<'static>,
        crate::runs::coordinator::RunActivity,
        crate::runs::log::DiagnosticsRequest,
        crate::scheduled_tasks::WatchSnapshot,
        crate::scheduled_tasks::types::Run,
        crate::scheduled_tasks::types::Task,
        crate::scheduled_tasks::types::TaskListItem,
        crate::scoped_config::ConfigReadRequest,
        crate::scoped_config::ConfigSnapshot,
        crate::scoped_config::ConfigUpdateRequest,
        crate::sharing::commands::add::AddProgress,
        crate::sharing::commands::gallery::GalleryImageRequest,
        crate::sharing::commands::gallery::GalleryIndexRequest,
        crate::sharing::commands::gallery::GalleryListingRequest,
        crate::sharing::commands::gallery::GalleryView,
        crate::sharing::commands::github_account::GithubAccount,
        crate::sharing::commands::LoupistRequest,
        crate::sharing::commands::add::PackAddRequest,
        crate::sharing::commands::updates::PackApplyUpdateRequest,
        crate::sharing::commands::updates::PackCheckRequest,
        crate::sharing::commands::updates::PackChooseNewerRequest,
        crate::sharing::commands::review::PackDecideRequest,
        crate::sharing::commands::added::PackImportRequest,
        crate::sharing::commands::added::PackImportView,
        crate::sharing::commands::review::PackItemRequest,
        crate::sharing::commands::open::PackOpenRequest,
        crate::sharing::commands::added::PackOriginRequest,
        crate::sharing::commands::added::PackRemoveRequest,
        crate::sharing::commands::publish::PublishEvent,
        crate::sharing::commands::save::ShareExportRequest,
        crate::sharing::commands::publish::SharePublishRequest,
        crate::sharing::commands::share::ShareRequest,
        crate::sharing::commands::share::ShareSummary,
        crate::sharing::commands::share::ShareSummaryRequest,
        crate::sharing::commands::github_account::SignInEvent,
        crate::sharing::commands::review::SuggestionGroup,
        crate::sharing::commands::updates::UpdateCheck,
        crate::sharing::deep_link::DeepLinkEvent,
        crate::sharing::import::ItemView,
        crate::sharing::import::OpenResult,
        crate::skills::SkillCatalog,
        crate::skills::SkillDocument,
        crate::store::app_settings::AppSettings,
        crate::store::user_context::HoldingsCsvPreview,
        crate::store::user_context::HoldingsOverview,
        crate::store::user_context::HoldingsSnapshot,
        crate::store::user_context::WatchlistEntry,
        crate::store::user_context::crypto_file::CryptoFilePreview,
        crate::store::user_context::wallets::WalletView,
        crate::updater::UpdateInstallRequest,
        crate::updater::UpdateInstallResult,
        crate::updater::UpdateStateDto,
        crate::web::browser::BrowserTabBoundsRequest,
        crate::web::browser::BrowserTabEvent,
        crate::web::browser::BrowserTabHistoryRequest,
        crate::web::browser::BrowserTabNavigateRequest,
        crate::web::browser::BrowserTabOpenRequest,
        crate::web::browser::BrowserTabRequest,
        crate::web::browser::BrowserTabState

    }
}

/// Each registered command's request and response, as `BridgeCommands` in
/// `bridge_commands.ts`. `command_signatures` keeps every row true.
macro_rules! commands {
    ($($command:ident: $request:ty => $response:ty;)*) => {
        fn commands(collector: &mut Collector<'_>) -> Vec<Command> {
            vec![$(command::<$request, $response>(collector, stringify!($command)),)*]
        }
    };
}

commands! {
        briefing_settings_get: crate::briefings::commands::LoupistRequest => crate::briefings::commands::BriefingSettings;
        briefing_settings_set: crate::briefings::commands::SettingsRequest => crate::briefings::commands::BriefingSettings;
        inbox_list: crate::briefings::commands::InboxRequest => crate::briefings::commands::InboxPage;
        inbox_mark_read: crate::briefings::commands::ReadRequest => ();
        inbox_unread_counts: () => Vec<crate::briefings::commands::UnreadCount>;
        briefing_resident_settings_get: () => crate::briefings::desktop::ResidentSettings;
        briefing_resident_settings_set: crate::briefings::desktop::ResidentRequest => crate::briefings::desktop::ResidentSettings;
        model_catalog_get: () => crate::providers::catalog::ModelCatalogView;
        model_visibility_update: crate::commands::ModelVisibilityUpdateRequest => crate::providers::catalog::ModelCatalogView;
        open_external_url: () => ();
        run_activity_list: () => Vec<crate::runs::coordinator::RunActivity>;
        notice_acknowledge: () => crate::store::app_settings::AppSettings;
        notice_settings_get: () => crate::store::app_settings::AppSettings;
        chat_approval_decide: crate::commands::approvals::ApprovalDecideRequest => ();
        asset_class_set: crate::commands::assets::AssetClassSetRequest => crate::knowledge::assets::AssetPageView;
        asset_delta: crate::commands::assets::AssetDeltaRequest => Vec<crate::knowledge::assets::AssetDelta>;
        asset_page_get: crate::commands::assets::AssetPageGetRequest => crate::knowledge::assets::AssetPageView;
        asset_page_seen: crate::commands::assets::AssetPageSeenRequest => ();
        asset_refresh: crate::commands::assets::AssetRefreshRequest => crate::knowledge::assets::AssetPageView;
        asset_verdict_undo: crate::commands::assets::AssetVerdictUndoRequest => crate::knowledge::assets::AssetPageView;
        upcoming_events: crate::commands::assets::UpcomingEventsRequest => Vec<crate::knowledge::assets::upcoming::UpcomingEvent>;
        attention_list: () => Vec<crate::commands::attention::AttentionItem>;
        attention_notify: crate::commands::attention::NotifyRequest => bool;
        chat_abort: () => ();
        chat_retry_last: crate::commands::chat_runtime::ChatRetryRequest => crate::commands::chat_runtime::ChatRetryReceipt;
        chat_send: crate::commands::types::ChatSendRequest => crate::commands::types::RunStarted;
        chat_steer: crate::commands::types::ChatControlRequestDto => crate::commands::types::ChatControlReceiptDto;
        config_read: crate::scoped_config::ConfigReadRequest => crate::scoped_config::ConfigSnapshot;
        config_reveal: crate::scoped_config::ConfigReadRequest => ();
        config_update: crate::scoped_config::ConfigUpdateRequest => crate::scoped_config::ConfigSnapshot;
        pick_file: crate::commands::dialog::PickFileRequest => Option<String>;
        earnings_events_list: crate::commands::earnings::EarningsEventsListRequest => Vec<crate::knowledge::earnings::EventSummary>;
        earnings_prepare: crate::commands::earnings::EarningsPrepareRequest => crate::knowledge::earnings::SnapshotView;
        earnings_snapshot_get: crate::commands::earnings::EarningsSnapshotGetRequest => Option<crate::knowledge::earnings::SnapshotView>;
        export_save: crate::commands::export_file::ExportRequest => Option<String>;
        research_handoff_start: crate::commands::handoff::ResearchHandoffRequest => crate::commands::handoff::ResearchHandoffReceipt;
        knowledge_catalog: crate::commands::knowledge::KnowledgeCatalogRequest => crate::commands::knowledge::KnowledgeCatalogDto;
        knowledge_connection_dismiss: crate::commands::knowledge::KnowledgeConnectionRequest => crate::knowledge::ConnectionView;
        knowledge_connection_open: crate::commands::knowledge::KnowledgeConnectionRequest => crate::knowledge::ConnectionView;
        knowledge_graph_job: crate::commands::knowledge::KnowledgeGraphJobRequest => crate::knowledge::GraphJobView;
        knowledge_graph_pause: crate::commands::knowledge::KnowledgeGraphPauseRequest => crate::knowledge::GraphProcessingView;
        knowledge_graph_retry_source: crate::commands::knowledge::KnowledgeGraphRetrySourceRequest => crate::knowledge::GraphJobView;
        knowledge_graph_recover: crate::commands::knowledge::KnowledgeGraphLoupistRequest => crate::knowledge::GraphProcessingView;
        knowledge_graph_settings_get: crate::commands::knowledge::KnowledgeGraphLoupistRequest => crate::knowledge::GraphProcessingView;
        knowledge_graph_settings_set: crate::commands::knowledge::KnowledgeGraphSettingsRequest => crate::knowledge::GraphProcessingView;
        knowledge_growth_events: crate::knowledge::GrowthEventsQuery => crate::knowledge::GrowthEventsPage;
        knowledge_journal_read: crate::commands::knowledge::KnowledgeJournalReadRequest => crate::knowledge::JournalRecord;
        knowledge_journal_source_read: crate::commands::knowledge::KnowledgeJournalSourceReadRequest => crate::commands::knowledge::JournalSourceReadDto;
        knowledge_lens_pages_for_node: crate::commands::knowledge::KnowledgeLensPagesForNodeRequest => Vec<crate::knowledge::PageSummary>;
        knowledge_lens_read: crate::commands::knowledge::KnowledgeLensReadRequest => crate::knowledge::PageRead;
        knowledge_lens_search: crate::commands::knowledge::KnowledgeLensSearchRequest => Vec<crate::commands::knowledge::LensSearchHitDto>;
        knowledge_map: crate::knowledge::KnowledgeMapQuery => crate::knowledge::KnowledgeMap;
        knowledge_research_search: crate::commands::knowledge::KnowledgeResearchSearchRequest => crate::commands::knowledge::KnowledgeResearchPageDto;
        knowledge_source_read: crate::commands::knowledge::KnowledgeSourceReadRequest => crate::commands::knowledge::SourceDetailDto;
        knowledge_source_retain: crate::commands::knowledge::KnowledgeSourceRetainRequest => crate::knowledge::RetainedSource;
        knowledge_world_query: crate::knowledge::WorldGraphQuery => crate::commands::knowledge::WorldQueryDto;
        memory_learned: crate::commands::knowledge::MemoryLearnedRequest => Vec<crate::knowledge::LearnedDigest>;
        memory_op_undo: crate::commands::knowledge::MemoryOpUndoRequest => crate::knowledge::PageRead;
        memory_section_forget: crate::commands::knowledge::MemorySectionForgetRequest => crate::knowledge::PageRead;
        knowledge_followed_status: crate::commands::knowledge::followed::FollowedStatusRequest => crate::knowledge::FollowedStatus;
        journal_archive: crate::commands::knowledge::journal::JournalArchiveRequest => crate::knowledge::JournalRecord;
        journal_create: crate::commands::knowledge::journal::JournalCreateRequest => crate::knowledge::JournalRecord;
        journal_list: crate::commands::knowledge::journal::JournalListRequest => Vec<crate::knowledge::JournalRecord>;
        journal_update: crate::commands::knowledge::journal::JournalUpdateRequest => crate::knowledge::JournalRecord;
        knowledge_lens_archive: crate::commands::knowledge::lens_archive::LensArchiveRequest => ();
        knowledge_lens_pause: crate::commands::knowledge::memory_controls::PauseRequest => crate::knowledge::memory_controls::MemoryControls;
        knowledge_memory_controls: crate::commands::knowledge::KnowledgeCatalogRequest => crate::knowledge::memory_controls::MemoryControls;
        knowledge_memory_reset: crate::commands::knowledge::memory_controls::ResetRequest => crate::knowledge::memory_reset::ResetCounts;
        knowledge_memory_reset_preview: crate::commands::knowledge::KnowledgeCatalogRequest => crate::knowledge::memory_reset::ResetCounts;
        knowledge_memory_usage: crate::commands::knowledge::memory_usage::MemoryUsageRequest => Vec<crate::commands::knowledge::memory_usage::MemoryUsage>;
        learning_review_list: crate::knowledge::learning::ReviewQuery => Vec<crate::knowledge::learning::Review>;
        memory_learning_settings_get: crate::commands::knowledge::KnowledgeCatalogRequest => crate::knowledge::learning::LearningSettings;
        memory_learning_settings_set: crate::commands::knowledge::method_learning::SettingsRequest => crate::knowledge::learning::LearningSettings;
        principle_archive: crate::commands::knowledge::principles::PrincipleArchiveRequest => crate::knowledge::principles::Principle;
        principle_create: crate::commands::knowledge::principles::PrincipleCreateRequest => crate::knowledge::principles::Principle;
        principle_list: crate::commands::knowledge::principles::PrincipleListRequest => Vec<crate::knowledge::principles::Principle>;
        principle_update: crate::commands::knowledge::principles::PrincipleUpdateRequest => crate::knowledge::principles::Principle;
        knowledge_source_export: crate::commands::knowledge::KnowledgeSourceReadRequest => Option<String>;
        knowledge_source_selection_resolve: crate::commands::knowledge::source_selection::SourceSelectionRequest => crate::commands::knowledge::source_selection::SourceSelectionPin;
        knowledge_working_note_read: crate::commands::knowledge::working_notes::WorkingNoteReadRequest => crate::knowledge::WorkingNote;
        knowledge_working_notes: crate::commands::knowledge::working_notes::WorkingNotesRequest => crate::commands::knowledge::working_notes::WorkingNotesPage;
        world_schema_get: crate::commands::knowledge::world_schema::LoupistRequest => crate::knowledge::WorldSchemaView;
        world_schema_remove: crate::commands::knowledge::world_schema::RemoveRequest => crate::knowledge::WorldSchemaView;
        world_schema_undo: crate::commands::knowledge::world_schema::UndoRequest => crate::knowledge::WorldSchemaView;
        agent_skill_catalog: () => Vec<crate::agent_harness::AgentSkillSummary>;
        loupist_choose_folder: () => Option<String>;
        loupist_create: crate::commands::types::LoupistCreateRequest => crate::commands::types::LoupistSummary;
        loupist_delete: crate::commands::types::LoupistDeleteRequest => ();
        loupist_import: crate::commands::types::LoupistImportRequest => crate::commands::types::LoupistSummary;
        loupist_list: () => Vec<crate::commands::types::LoupistSummary>;
        loupist_profile_update: crate::commands::types::LoupistProfileUpdateRequest => crate::commands::types::LoupistSummary;
        loupist_reconnect: crate::commands::types::LoupistReconnectRequest => crate::commands::types::LoupistSummary;
        loupist_select: () => crate::commands::types::LoupistWorkspace;
        chat_question_answer: crate::commands::questions::QuestionAnswerRequest => ();
        artifact_restore: crate::commands::types::ChatArtifactRestoreRequest => crate::commands::types::ChatArtifactViewDto;
        artifact_versions: crate::commands::types::ChatArtifactReadRequest => Vec<crate::chat::artifacts::ChatArtifactVersion>;
        chat_artifact_list: crate::commands::types::ChatArtifactListRequest => Vec<crate::chat::artifacts::ChatArtifact>;
        chat_artifact_read: crate::commands::types::ChatArtifactReadRequest => crate::commands::types::ChatArtifactViewDto;
        chat_attachment_read: crate::commands::types::ChatAttachmentReadRequest => crate::commands::types::ChatAttachmentReadDto;
        chat_branch: crate::commands::types::ChatBranchRequest => crate::commands::types::ChatThreadDto;
        chat_create: crate::commands::types::ChatCreateRequest => crate::commands::types::ChatThreadDto;
        chat_delete: crate::commands::types::ChatDeleteRequest => ();
        chat_feedback_set: crate::commands::types::ChatFeedbackRequest => crate::domain::ChatFeedback;
        chat_memory_set_aside: crate::commands::types::ChatMemorySetAsideRequest => crate::commands::types::ChatThreadDto;
        chat_rename: crate::commands::types::ChatRenameRequest => crate::commands::types::ChatThreadDto;
        chat_version_switch: crate::commands::types::ChatVersionSwitchRequest => crate::commands::types::ChatThreadDto;
        loupist_artifact_list: crate::commands::types::LoupistArtifactListRequest => Vec<crate::commands::types::LoupistArtifactItemDto>;
        scheduled_task_confirm: crate::commands::scheduled_tasks::TaskRequest => crate::scheduled_tasks::types::Task;
        scheduled_task_delete: crate::commands::scheduled_tasks::RevisionRequest => ();
        scheduled_task_discard: crate::commands::scheduled_tasks::TaskRequest => ();
        scheduled_task_get: crate::commands::scheduled_tasks::TaskRequest => Option<crate::scheduled_tasks::types::Task>;
        scheduled_task_run_now: crate::commands::scheduled_tasks::TaskRequest => crate::scheduled_tasks::types::Run;
        scheduled_task_runs: crate::commands::scheduled_tasks::TaskRequest => Vec<crate::scheduled_tasks::types::Run>;
        scheduled_task_set_status: crate::commands::scheduled_tasks::StatusRequest => crate::scheduled_tasks::types::Task;
        scheduled_task_watch: crate::commands::scheduled_tasks::RunRequest => Option<crate::scheduled_tasks::WatchSnapshot>;
        scheduled_tasks_activity: crate::commands::scheduled_tasks::LoupistRequest => Vec<crate::scheduled_tasks::types::Run>;
        scheduled_tasks_list: crate::commands::scheduled_tasks::LoupistRequest => Vec<crate::scheduled_tasks::types::TaskListItem>;
        skills_create: crate::commands::skills::SkillCreateRequest => crate::skills::SkillCatalog;
        skills_delete: crate::commands::skills::SkillDeleteRequest => crate::skills::SkillCatalog;
        skills_import: crate::commands::skills::SkillImportRequest => Option<crate::skills::SkillCatalog>;
        skills_list: crate::scoped_config::ConfigReadRequest => crate::skills::SkillCatalog;
        skills_read: crate::commands::skills::SkillReadRequest => crate::skills::SkillDocument;
        skills_reveal: crate::commands::skills::SkillReadRequest => ();
        skills_save: crate::commands::skills::SkillSaveRequest => crate::skills::SkillCatalog;
        holdings_commit_csv: crate::commands::user_context::CsvCommitRequest => crate::store::user_context::HoldingsSnapshot;
        holdings_import_crypto_file: crate::commands::user_context::CsvImportRequest => crate::store::user_context::crypto_file::CryptoFilePreview;
        holdings_import_csv: crate::commands::user_context::CsvImportRequest => crate::store::user_context::HoldingsCsvPreview;
        holdings_import_kis: () => crate::store::user_context::HoldingsSnapshot;
        holdings_map_columns: crate::commands::user_context::MapColumnsRequest => crate::store::user_context::crypto_file::CryptoFilePreview;
        holdings_summary: () => crate::store::user_context::HoldingsOverview;
        wallet_add: crate::commands::user_context::WalletAddRequest => Vec<crate::store::user_context::wallets::WalletView>;
        wallet_refresh: Option<crate::commands::user_context::WalletRefreshRequest> => Vec<crate::store::user_context::wallets::WalletView>;
        wallet_remove: crate::commands::user_context::WalletIdRequest => ();
        wallet_rename: crate::commands::user_context::WalletRenameRequest => crate::store::user_context::wallets::WalletView;
        wallets_list: () => Vec<crate::store::user_context::wallets::WalletView>;
        watchlist_add: crate::commands::user_context::WatchlistAddRequest => crate::store::user_context::WatchlistEntry;
        watchlist_list: () => Vec<crate::store::user_context::WatchlistEntry>;
        watchlist_remove: crate::commands::user_context::WatchlistRemoveRequest => ();
        watch_defaults_apply: crate::commands::watch::WatchDefaultsApplyRequest => crate::commands::watch::WatchDefaultsApplyResult;
        watch_check_now: crate::commands::watch::WatchCheckNowRequest => crate::commands::watch::WatchCheckResult;
        watch_events_list: crate::commands::watch::WatchEventsListRequest => Vec<crate::knowledge::watch::WatchEventView>;
        watch_events_seen: crate::commands::watch::WatchEventsSeenRequest => ();
        watch_pause_set: crate::commands::watch::WatchPauseSetRequest => crate::commands::watch::WatchSettingsView;
        watch_setting_set: crate::commands::watch::WatchSettingSetRequest => crate::commands::watch::WatchSettingsView;
        watch_settings_list: crate::commands::watch::WatchSettingsListRequest => crate::commands::watch::WatchSettingsView;
        chat_activity_read: crate::commands::workspace::ChatActivityRequest => crate::chat::activity::ChatActivityDetail;
        workspace_list: crate::commands::workspace::WorkspacePathRequest => crate::fs::workspace_files::WorkspaceDirectory;
        workspace_read: crate::commands::workspace::WorkspacePathRequest => crate::fs::workspace_files::WorkspaceFile;
        workspace_reveal: crate::commands::workspace::WorkspacePathRequest => ();
        chat_draft_files_read: crate::commands::workspace_ui::DraftIdentity => Vec<crate::commands::workspace_ui::DraftAttachment>;
        chat_draft_files_write: crate::commands::workspace_ui::DraftIdentity => ();
        chat_draft_read: crate::commands::workspace_ui::DraftIdentity => Option<crate::commands::workspace_ui::StoredChatDraft>;
        chat_draft_write: crate::commands::workspace_ui::DraftIdentity => ();
        workspace_ui_read: () => Option<crate::commands::workspace_ui::WorkspaceUiState>;
        workspace_ui_write: crate::commands::workspace_ui::WorkspaceUiWriteRequest => crate::commands::workspace_ui::WorkspaceUiState;
        custom_connection_add_command: crate::custom_connections::commands::AddCommandRequest => crate::custom_connections::service::ConnectionView;
        custom_connection_add_json: crate::custom_connections::commands::AddJsonRequest => Vec<crate::custom_connections::service::ConnectionView>;
        custom_connection_add_url: crate::custom_connections::commands::AddUrlRequest => crate::custom_connections::service::ConnectionView;
        custom_connection_authorize: crate::custom_connections::commands::AuthorizeRequest => crate::custom_connections::service::ConnectionView;
        custom_connection_confirm: crate::custom_connections::commands::ConfirmRequest => crate::custom_connections::service::ConnectionView;
        custom_connection_list: crate::custom_connections::commands::ListRequest => Vec<crate::custom_connections::service::ConnectionView>;
        custom_connection_remove: crate::custom_connections::commands::IdentityRequest => ();
        custom_connection_set_enabled: crate::custom_connections::commands::EnabledRequest => crate::custom_connections::service::ConnectionView;
        custom_connection_set_memory: crate::custom_connections::commands::MemoryRequest => crate::custom_connections::service::ConnectionView;
        custom_connection_sign_in: crate::custom_connections::commands::IdentityRequest => crate::custom_connections::service::ConnectionView;
        custom_connection_sign_in_cancel: crate::custom_connections::commands::IdentityRequest => ();
        custom_tool_approve: crate::custom_connections::commands::ApproveRequest => crate::custom_connections::service::ConnectionView;
        loupist_capability_disable: crate::marketplace::LoupistCapabilityRequest => crate::marketplace::LoupistCapabilityBindingDto;
        loupist_capability_enable: crate::marketplace::LoupistCapabilityRequest => crate::marketplace::LoupistCapabilityBindingDto;
        loupist_capability_list: () => Vec<crate::marketplace::LoupistCapabilityBindingDto>;
        marketplace_snapshot: () => crate::marketplace::MarketplaceSnapshotDto;
        marketplace_connector_configure: crate::marketplace::MarketplaceConnectorConfigureRequest => crate::marketplace::MarketplaceConnectorStatusDto;
        marketplace_credential_delete: crate::marketplace::MarketplaceCredentialRequest => Vec<crate::marketplace::MarketplaceCredentialStatusDto>;
        marketplace_credential_save: crate::marketplace::MarketplaceCredentialSaveRequest => Vec<crate::marketplace::MarketplaceCredentialStatusDto>;
        marketplace_credential_verify: crate::marketplace::MarketplaceCredentialRequest => Vec<crate::marketplace::MarketplaceCredentialStatusDto>;
        model_notice_ack: crate::providers::connection::ModelNoticeAckRequest => crate::providers::catalog::ModelCatalogView;
        provider_configure: crate::providers::connection::ProviderConfigureRequest => crate::providers::catalog::ModelCatalogView;
        provider_connect: crate::providers::connection::ProviderConnectionRequest => crate::providers::catalog::ModelCatalogView;
        provider_connect_cancel: () => ();
        provider_connect_open_browser: () => ();
        provider_connect_submit_code: crate::providers::connection::ProviderSubmitCodeRequest => ();
        provider_custom_remove: crate::providers::connection::ProviderConnectionRequest => crate::providers::catalog::ModelCatalogView;
        provider_custom_save: crate::providers::custom::CustomProviderRequest => crate::providers::catalog::ModelCatalogView;
        provider_disconnect: crate::providers::connection::ProviderConnectionRequest => crate::providers::catalog::ModelCatalogView;
        provider_models: crate::providers::connection::ProviderModelsRequest => crate::providers::catalog::ModelCatalogView;
        provider_open_console: crate::providers::connection::ProviderPageRequest => ();
        provider_open_usage: crate::providers::connection::ProviderPageRequest => ();
        provider_custom_test: crate::providers::local::ProviderCustomTestRequest => crate::providers::local::ProviderCustomTestResult;
        provider_local_discover: () => crate::providers::local::LocalDiscovery;
        provider_preset_save: crate::providers::local::ProviderPresetRequest => crate::providers::catalog::ModelCatalogView;
        run_logs_last_error: crate::runs::log::DiagnosticsRequest => Option<String>;
        run_logs_reveal: crate::runs::log::DiagnosticsRequest => ();
        deep_link_dismiss: () => ();
        deep_link_pending: () => Option<crate::sharing::deep_link::DeepLinkEvent>;
        gallery_image: crate::sharing::commands::gallery::GalleryImageRequest => Option<String>;
        gallery_index: crate::sharing::commands::gallery::GalleryIndexRequest => crate::sharing::commands::gallery::GalleryView;
        gallery_listing_request: crate::sharing::commands::gallery::GalleryListingRequest => serde_json::Value;
        gallery_listing_status: crate::sharing::commands::LoupistRequest => Option<&'static str>;
        github_account: () => crate::sharing::commands::github_account::GithubAccount;
        github_sign_in: () => crate::sharing::commands::github_account::GithubAccount;
        github_sign_in_cancel: () => ();
        github_sign_in_open: () => ();
        github_sign_out: () => crate::sharing::commands::github_account::GithubAccount;
        pack_add: crate::sharing::commands::add::PackAddRequest => serde_json::Value;
        pack_apply_update: crate::sharing::commands::updates::PackApplyUpdateRequest => Vec<crate::sharing::commands::added::PackImportView>;
        pack_check_update: crate::sharing::commands::updates::PackCheckRequest => crate::sharing::commands::updates::UpdateCheck;
        pack_choose_newer: crate::sharing::commands::updates::PackChooseNewerRequest => Option<crate::sharing::commands::updates::UpdateCheck>;
        pack_decide: crate::sharing::commands::review::PackDecideRequest => Vec<crate::sharing::commands::review::SuggestionGroup>;
        pack_item_read: crate::sharing::commands::review::PackItemRequest => crate::sharing::import::ItemView;
        pack_list: crate::sharing::commands::LoupistRequest => Vec<crate::sharing::commands::added::PackImportView>;
        pack_open: crate::sharing::commands::open::PackOpenRequest => Option<crate::sharing::import::OpenResult>;
        pack_origin: crate::sharing::commands::added::PackOriginRequest => Option<serde_json::Value>;
        pack_remove: crate::sharing::commands::added::PackRemoveRequest => Vec<crate::sharing::commands::added::PackImportView>;
        pack_report: crate::sharing::commands::added::PackImportRequest => ();
        pack_suggestions: crate::sharing::commands::LoupistRequest => Vec<crate::sharing::commands::review::SuggestionGroup>;
        share_copy_text: crate::sharing::commands::share::ShareRequest => serde_json::Value;
        share_export: crate::sharing::commands::save::ShareExportRequest => Option<serde_json::Value>;
        share_publish: crate::sharing::commands::publish::SharePublishRequest => serde_json::Value;
        share_reveal: crate::sharing::commands::LoupistRequest => ();
        share_summary: crate::sharing::commands::share::ShareSummaryRequest => crate::sharing::commands::share::ShareSummary;
        update_check: () => crate::updater::UpdateStateDto;
        update_install: crate::updater::UpdateInstallRequest => crate::updater::UpdateInstallResult;
        update_status: () => crate::updater::UpdateStateDto;
        browser_tab_close: crate::web::browser::BrowserTabRequest => ();
        browser_tab_history: crate::web::browser::BrowserTabHistoryRequest => ();
        browser_tab_navigate: crate::web::browser::BrowserTabNavigateRequest => ();
        browser_tab_open: crate::web::browser::BrowserTabOpenRequest => crate::web::browser::BrowserTabState;
        browser_tab_reload: crate::web::browser::BrowserTabRequest => ();
        browser_tab_set_bounds: crate::web::browser::BrowserTabBoundsRequest => ();
        browser_tabs_reset: () => ();
        site_favicon: () => Option<String>;
        world_keep_awake_get: () => crate::power::KeepAwakeStatus;
        world_keep_awake_set: crate::commands::power::WorldKeepAwakeRequest => crate::power::KeepAwakeStatus;
}

/// Never called: it compiles only while each command takes the request and
/// returns the response its `commands!` row names.
#[allow(dead_code, clippy::diverging_sub_expression)]
async fn command_signatures() {
    use crate::app::CommandError;
    fn any<T>() -> T {
        unreachable!()
    }
    let _: Result<crate::briefings::commands::BriefingSettings, CommandError> =
        crate::briefings::commands::briefing_settings_get(
            any::<crate::briefings::commands::LoupistRequest>(),
            any(),
        );
    let _: Result<crate::briefings::commands::BriefingSettings, CommandError> =
        crate::briefings::commands::briefing_settings_set(
            any::<crate::briefings::commands::SettingsRequest>(),
            any(),
        );
    let _: Result<crate::briefings::commands::InboxPage, CommandError> =
        crate::briefings::commands::inbox_list(
            any::<crate::briefings::commands::InboxRequest>(),
            any(),
        );
    let _: Result<(), CommandError> = crate::briefings::commands::inbox_mark_read(
        any::<crate::briefings::commands::ReadRequest>(),
        any(),
    );
    let _: Result<Vec<crate::briefings::commands::UnreadCount>, CommandError> =
        crate::briefings::commands::inbox_unread_counts(any());
    let _: Result<crate::briefings::desktop::ResidentSettings, CommandError> =
        crate::briefings::desktop::briefing_resident_settings_get(any());
    let _: Result<crate::briefings::desktop::ResidentSettings, CommandError> =
        crate::briefings::desktop::briefing_resident_settings_set(
            any::<crate::briefings::desktop::ResidentRequest>(),
            any(),
        );
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::commands::model_catalog_get(any());
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::commands::model_visibility_update(
            any::<crate::commands::ModelVisibilityUpdateRequest>(),
            any(),
        );
    let _: Result<(), CommandError> = crate::commands::open_external_url(any());
    let _: Result<Vec<crate::runs::coordinator::RunActivity>, CommandError> =
        crate::commands::run_activity_list(any());
    let _: Result<crate::store::app_settings::AppSettings, CommandError> =
        crate::commands::app_settings::notice_acknowledge(any());
    let _: Result<crate::store::app_settings::AppSettings, CommandError> =
        crate::commands::app_settings::notice_settings_get(any());
    let _: Result<(), CommandError> = crate::commands::approvals::chat_approval_decide(
        any::<crate::commands::approvals::ApprovalDecideRequest>(),
        any(),
    );
    let _: Result<crate::knowledge::assets::AssetPageView, CommandError> =
        crate::commands::assets::asset_class_set(
            any(),
            any::<crate::commands::assets::AssetClassSetRequest>(),
        );
    let _: Result<Vec<crate::knowledge::assets::AssetDelta>, CommandError> =
        crate::commands::assets::asset_delta(
            any(),
            any::<crate::commands::assets::AssetDeltaRequest>(),
        );
    let _: Result<crate::knowledge::assets::AssetPageView, CommandError> =
        crate::commands::assets::asset_page_get(
            any(),
            any::<crate::commands::assets::AssetPageGetRequest>(),
        );
    let _: Result<(), CommandError> = crate::commands::assets::asset_page_seen(
        any(),
        any::<crate::commands::assets::AssetPageSeenRequest>(),
    );
    let _: Result<crate::knowledge::assets::AssetPageView, CommandError> =
        crate::commands::assets::asset_refresh(
            any(),
            any::<crate::commands::assets::AssetRefreshRequest>(),
        )
        .await;
    let _: Result<crate::knowledge::assets::AssetPageView, CommandError> =
        crate::commands::assets::asset_verdict_undo(
            any(),
            any::<crate::commands::assets::AssetVerdictUndoRequest>(),
        );
    let _: Result<Vec<crate::knowledge::assets::upcoming::UpcomingEvent>, CommandError> =
        crate::commands::assets::upcoming_events(
            any(),
            any::<crate::commands::assets::UpcomingEventsRequest>(),
        );
    let _: Result<Vec<crate::commands::attention::AttentionItem>, CommandError> =
        crate::commands::attention::attention_list(any(), any()).await;
    let _: Result<bool, CommandError> = crate::commands::attention::attention_notify(
        any::<crate::commands::attention::NotifyRequest>(),
        any(),
        any(),
    );
    let _: Result<(), CommandError> = crate::commands::chat_runtime::chat_abort(any(), any()).await;
    let _: Result<crate::commands::chat_runtime::ChatRetryReceipt, CommandError> =
        crate::commands::chat_runtime::chat_retry_last(
            any::<crate::commands::chat_runtime::ChatRetryRequest>(),
            any(),
        );
    let _: Result<crate::commands::types::RunStarted, CommandError> =
        crate::commands::chat_runtime::chat_send(
            any::<crate::commands::types::ChatSendRequest>(),
            any(),
            any(),
        )
        .await;
    let _: Result<crate::commands::types::ChatControlReceiptDto, CommandError> =
        crate::commands::chat_runtime::chat_steer(
            any::<crate::commands::types::ChatControlRequestDto>(),
            any(),
        )
        .await;
    let _: Result<crate::scoped_config::ConfigSnapshot, CommandError> =
        crate::commands::configuration::config_read(
            any::<crate::scoped_config::ConfigReadRequest>(),
            any(),
        );
    let _: Result<(), CommandError> = crate::commands::configuration::config_reveal(
        any::<crate::scoped_config::ConfigReadRequest>(),
        any(),
    );
    let _: Result<crate::scoped_config::ConfigSnapshot, CommandError> =
        crate::commands::configuration::config_update(
            any::<crate::scoped_config::ConfigUpdateRequest>(),
            any(),
        );
    let _: Result<Option<String>, CommandError> =
        crate::commands::dialog::pick_file(any::<crate::commands::dialog::PickFileRequest>()).await;
    let _: Result<Vec<crate::knowledge::earnings::EventSummary>, CommandError> =
        crate::commands::earnings::earnings_events_list(
            any(),
            any::<crate::commands::earnings::EarningsEventsListRequest>(),
        );
    let _: Result<crate::knowledge::earnings::SnapshotView, CommandError> =
        crate::commands::earnings::earnings_prepare(
            any(),
            any::<crate::commands::earnings::EarningsPrepareRequest>(),
        )
        .await;
    let _: Result<Option<crate::knowledge::earnings::SnapshotView>, CommandError> =
        crate::commands::earnings::earnings_snapshot_get(
            any(),
            any::<crate::commands::earnings::EarningsSnapshotGetRequest>(),
        );
    let _: Result<Option<String>, CommandError> = crate::commands::export_file::export_save(
        any(),
        any::<crate::commands::export_file::ExportRequest>(),
    )
    .await;
    let _: Result<crate::commands::handoff::ResearchHandoffReceipt, CommandError> =
        crate::commands::handoff::research_handoff_start(
            any::<crate::commands::handoff::ResearchHandoffRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::commands::knowledge::KnowledgeCatalogDto, CommandError> =
        crate::commands::knowledge::knowledge_catalog(
            any::<crate::commands::knowledge::KnowledgeCatalogRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::ConnectionView, CommandError> =
        crate::commands::knowledge::knowledge_connection_dismiss(
            any::<crate::commands::knowledge::KnowledgeConnectionRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::ConnectionView, CommandError> =
        crate::commands::knowledge::knowledge_connection_open(
            any::<crate::commands::knowledge::KnowledgeConnectionRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::GraphJobView, CommandError> =
        crate::commands::knowledge::knowledge_graph_job(
            any::<crate::commands::knowledge::KnowledgeGraphJobRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::GraphJobView, CommandError> =
        crate::commands::knowledge::knowledge_graph_retry_source(
            any::<crate::commands::knowledge::KnowledgeGraphRetrySourceRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::GraphProcessingView, CommandError> =
        crate::commands::knowledge::knowledge_graph_pause(
            any::<crate::commands::knowledge::KnowledgeGraphPauseRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::GraphProcessingView, CommandError> =
        crate::commands::knowledge::knowledge_graph_recover(
            any::<crate::commands::knowledge::KnowledgeGraphLoupistRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::GraphProcessingView, CommandError> =
        crate::commands::knowledge::knowledge_graph_settings_get(
            any::<crate::commands::knowledge::KnowledgeGraphLoupistRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::GraphProcessingView, CommandError> =
        crate::commands::knowledge::knowledge_graph_settings_set(
            any::<crate::commands::knowledge::KnowledgeGraphSettingsRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::GrowthEventsPage, CommandError> =
        crate::commands::knowledge::knowledge_growth_events(
            any::<crate::knowledge::GrowthEventsQuery>(),
            any(),
        );
    let _: Result<crate::knowledge::JournalRecord, CommandError> =
        crate::commands::knowledge::knowledge_journal_read(
            any::<crate::commands::knowledge::KnowledgeJournalReadRequest>(),
            any(),
        );
    let _: Result<crate::commands::knowledge::JournalSourceReadDto, CommandError> =
        crate::commands::knowledge::knowledge_journal_source_read(
            any::<crate::commands::knowledge::KnowledgeJournalSourceReadRequest>(),
            any(),
        );
    let _: Result<Vec<crate::knowledge::PageSummary>, CommandError> =
        crate::commands::knowledge::knowledge_lens_pages_for_node(
            any::<crate::commands::knowledge::KnowledgeLensPagesForNodeRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::PageRead, CommandError> =
        crate::commands::knowledge::knowledge_lens_read(
            any::<crate::commands::knowledge::KnowledgeLensReadRequest>(),
            any(),
        );
    let _: Result<Vec<crate::commands::knowledge::LensSearchHitDto>, CommandError> =
        crate::commands::knowledge::knowledge_lens_search(
            any::<crate::commands::knowledge::KnowledgeLensSearchRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::KnowledgeMap, CommandError> =
        crate::commands::knowledge::knowledge_map(
            any::<crate::knowledge::KnowledgeMapQuery>(),
            any(),
        );
    let _: Result<crate::commands::knowledge::KnowledgeResearchPageDto, CommandError> =
        crate::commands::knowledge::knowledge_research_search(
            any::<crate::commands::knowledge::KnowledgeResearchSearchRequest>(),
            any(),
        );
    let _: Result<crate::commands::knowledge::SourceDetailDto, CommandError> =
        crate::commands::knowledge::knowledge_source_read(
            any::<crate::commands::knowledge::KnowledgeSourceReadRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::RetainedSource, CommandError> =
        crate::commands::knowledge::knowledge_source_retain(
            any::<crate::commands::knowledge::KnowledgeSourceRetainRequest>(),
            any(),
        );
    let _: Result<crate::commands::knowledge::WorldQueryDto, CommandError> =
        crate::commands::knowledge::knowledge_world_query(
            any::<crate::knowledge::WorldGraphQuery>(),
            any(),
        );
    let _: Result<Vec<crate::knowledge::LearnedDigest>, CommandError> =
        crate::commands::knowledge::memory_learned(
            any::<crate::commands::knowledge::MemoryLearnedRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::PageRead, CommandError> =
        crate::commands::knowledge::memory_op_undo(
            any::<crate::commands::knowledge::MemoryOpUndoRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::PageRead, CommandError> =
        crate::commands::knowledge::memory_section_forget(
            any::<crate::commands::knowledge::MemorySectionForgetRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::FollowedStatus, CommandError> =
        crate::commands::knowledge::followed::knowledge_followed_status(
            any::<crate::commands::knowledge::followed::FollowedStatusRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::JournalRecord, CommandError> =
        crate::commands::knowledge::journal::journal_archive(
            any(),
            any::<crate::commands::knowledge::journal::JournalArchiveRequest>(),
        );
    let _: Result<crate::knowledge::JournalRecord, CommandError> =
        crate::commands::knowledge::journal::journal_create(
            any(),
            any::<crate::commands::knowledge::journal::JournalCreateRequest>(),
        );
    let _: Result<Vec<crate::knowledge::JournalRecord>, CommandError> =
        crate::commands::knowledge::journal::journal_list(
            any(),
            any::<crate::commands::knowledge::journal::JournalListRequest>(),
        );
    let _: Result<crate::knowledge::JournalRecord, CommandError> =
        crate::commands::knowledge::journal::journal_update(
            any(),
            any::<crate::commands::knowledge::journal::JournalUpdateRequest>(),
        );
    let _: Result<(), CommandError> =
        crate::commands::knowledge::lens_archive::knowledge_lens_archive(
            any(),
            any::<crate::commands::knowledge::lens_archive::LensArchiveRequest>(),
        );
    let _: Result<crate::knowledge::memory_controls::MemoryControls, CommandError> =
        crate::commands::knowledge::memory_controls::knowledge_lens_pause(
            any(),
            any::<crate::commands::knowledge::memory_controls::PauseRequest>(),
        );
    let _: Result<crate::knowledge::memory_controls::MemoryControls, CommandError> =
        crate::commands::knowledge::memory_controls::knowledge_memory_controls(
            any(),
            any::<crate::commands::knowledge::KnowledgeCatalogRequest>(),
        );
    let _: Result<crate::knowledge::memory_reset::ResetCounts, CommandError> =
        crate::commands::knowledge::memory_controls::knowledge_memory_reset(
            any(),
            any::<crate::commands::knowledge::memory_controls::ResetRequest>(),
        );
    let _: Result<crate::knowledge::memory_reset::ResetCounts, CommandError> =
        crate::commands::knowledge::memory_controls::knowledge_memory_reset_preview(
            any(),
            any::<crate::commands::knowledge::KnowledgeCatalogRequest>(),
        );
    let _: Result<Vec<crate::commands::knowledge::memory_usage::MemoryUsage>, CommandError> =
        crate::commands::knowledge::memory_usage::knowledge_memory_usage(
            any(),
            any::<crate::commands::knowledge::memory_usage::MemoryUsageRequest>(),
        );
    let _: Result<Vec<crate::knowledge::learning::Review>, CommandError> =
        crate::commands::knowledge::method_learning::learning_review_list(
            any(),
            any::<crate::knowledge::learning::ReviewQuery>(),
        );
    let _: Result<crate::knowledge::learning::LearningSettings, CommandError> =
        crate::commands::knowledge::method_learning::memory_learning_settings_get(
            any(),
            any::<crate::commands::knowledge::KnowledgeCatalogRequest>(),
        );
    let _: Result<crate::knowledge::learning::LearningSettings, CommandError> =
        crate::commands::knowledge::method_learning::memory_learning_settings_set(
            any(),
            any::<crate::commands::knowledge::method_learning::SettingsRequest>(),
        );
    let _: Result<crate::knowledge::principles::Principle, CommandError> =
        crate::commands::knowledge::principles::principle_archive(
            any(),
            any::<crate::commands::knowledge::principles::PrincipleArchiveRequest>(),
        );
    let _: Result<crate::knowledge::principles::Principle, CommandError> =
        crate::commands::knowledge::principles::principle_create(
            any(),
            any::<crate::commands::knowledge::principles::PrincipleCreateRequest>(),
        );
    let _: Result<Vec<crate::knowledge::principles::Principle>, CommandError> =
        crate::commands::knowledge::principles::principle_list(
            any(),
            any::<crate::commands::knowledge::principles::PrincipleListRequest>(),
        );
    let _: Result<crate::knowledge::principles::Principle, CommandError> =
        crate::commands::knowledge::principles::principle_update(
            any(),
            any::<crate::commands::knowledge::principles::PrincipleUpdateRequest>(),
        );
    let _: Result<Option<String>, CommandError> =
        crate::commands::knowledge::source_export::knowledge_source_export(
            any(),
            any::<crate::commands::knowledge::KnowledgeSourceReadRequest>(),
        )
        .await;
    let _: Result<crate::commands::knowledge::source_selection::SourceSelectionPin, CommandError> =
        crate::commands::knowledge::source_selection::knowledge_source_selection_resolve(
            any::<crate::commands::knowledge::source_selection::SourceSelectionRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::WorkingNote, CommandError> =
        crate::commands::knowledge::working_notes::knowledge_working_note_read(
            any::<crate::commands::knowledge::working_notes::WorkingNoteReadRequest>(),
            any(),
        );
    let _: Result<crate::commands::knowledge::working_notes::WorkingNotesPage, CommandError> =
        crate::commands::knowledge::working_notes::knowledge_working_notes(
            any::<crate::commands::knowledge::working_notes::WorkingNotesRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::WorldSchemaView, CommandError> =
        crate::commands::knowledge::world_schema::world_schema_get(
            any::<crate::commands::knowledge::world_schema::LoupistRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::WorldSchemaView, CommandError> =
        crate::commands::knowledge::world_schema::world_schema_remove(
            any(),
            any::<crate::commands::knowledge::world_schema::RemoveRequest>(),
            any(),
        );
    let _: Result<crate::knowledge::WorldSchemaView, CommandError> =
        crate::commands::knowledge::world_schema::world_schema_undo(
            any(),
            any::<crate::commands::knowledge::world_schema::UndoRequest>(),
            any(),
        );
    let _: Result<Vec<crate::agent_harness::AgentSkillSummary>, CommandError> =
        crate::commands::loupist::agent_skill_catalog(any(), any());
    let _: Result<Option<String>, CommandError> =
        crate::commands::loupist::loupist_choose_folder().await;
    let _: Result<crate::commands::types::LoupistSummary, CommandError> =
        crate::commands::loupist::loupist_create(
            any::<crate::commands::types::LoupistCreateRequest>(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> = crate::commands::loupist::loupist_delete(
        any::<crate::commands::types::LoupistDeleteRequest>(),
        any(),
    )
    .await;
    let _: Result<crate::commands::types::LoupistSummary, CommandError> =
        crate::commands::loupist::loupist_import(
            any::<crate::commands::types::LoupistImportRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::commands::types::LoupistSummary>, CommandError> =
        crate::commands::loupist::loupist_list(any()).await;
    let _: Result<crate::commands::types::LoupistSummary, CommandError> =
        crate::commands::loupist::loupist_profile_update(
            any::<crate::commands::types::LoupistProfileUpdateRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::commands::types::LoupistSummary, CommandError> =
        crate::commands::loupist::loupist_reconnect(
            any::<crate::commands::types::LoupistReconnectRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::commands::types::LoupistWorkspace, CommandError> =
        crate::commands::loupist::loupist_select(any(), any()).await;
    let _: Result<(), CommandError> = crate::commands::questions::chat_question_answer(
        any::<crate::commands::questions::QuestionAnswerRequest>(),
        any(),
    );
    let _: Result<crate::commands::types::ChatArtifactViewDto, CommandError> =
        crate::commands::records::artifact_restore(
            any::<crate::commands::types::ChatArtifactRestoreRequest>(),
            any(),
        );
    let _: Result<Vec<crate::chat::artifacts::ChatArtifactVersion>, CommandError> =
        crate::commands::records::artifact_versions(
            any::<crate::commands::types::ChatArtifactReadRequest>(),
            any(),
        );
    let _: Result<Vec<crate::chat::artifacts::ChatArtifact>, CommandError> =
        crate::commands::records::chat_artifact_list(
            any::<crate::commands::types::ChatArtifactListRequest>(),
            any(),
        );
    let _: Result<crate::commands::types::ChatArtifactViewDto, CommandError> =
        crate::commands::records::chat_artifact_read(
            any::<crate::commands::types::ChatArtifactReadRequest>(),
            any(),
        );
    let _: Result<crate::commands::types::ChatAttachmentReadDto, CommandError> =
        crate::commands::records::chat_attachment_read(
            any::<crate::commands::types::ChatAttachmentReadRequest>(),
            any(),
        );
    let _: Result<crate::commands::types::ChatThreadDto, CommandError> =
        crate::commands::records::chat_branch(
            any::<crate::commands::types::ChatBranchRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::commands::types::ChatThreadDto, CommandError> =
        crate::commands::records::chat_create(
            any::<crate::commands::types::ChatCreateRequest>(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> = crate::commands::records::chat_delete(
        any::<crate::commands::types::ChatDeleteRequest>(),
        any(),
    )
    .await;
    let _: Result<crate::domain::ChatFeedback, CommandError> =
        crate::commands::records::chat_feedback_set(
            any::<crate::commands::types::ChatFeedbackRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::commands::types::ChatThreadDto, CommandError> =
        crate::commands::records::chat_memory_set_aside(
            any::<crate::commands::types::ChatMemorySetAsideRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::commands::types::ChatThreadDto, CommandError> =
        crate::commands::records::chat_rename(
            any::<crate::commands::types::ChatRenameRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::commands::types::ChatThreadDto, CommandError> =
        crate::commands::records::chat_version_switch(
            any::<crate::commands::types::ChatVersionSwitchRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::commands::types::LoupistArtifactItemDto>, CommandError> =
        crate::commands::records::loupist_artifact_list(
            any::<crate::commands::types::LoupistArtifactListRequest>(),
            any(),
        );
    let _: Result<crate::scheduled_tasks::types::Task, CommandError> =
        crate::commands::scheduled_tasks::scheduled_task_confirm(
            any::<crate::commands::scheduled_tasks::TaskRequest>(),
            any(),
        );
    let _: Result<(), CommandError> = crate::commands::scheduled_tasks::scheduled_task_delete(
        any::<crate::commands::scheduled_tasks::RevisionRequest>(),
        any(),
    );
    let _: Result<(), CommandError> = crate::commands::scheduled_tasks::scheduled_task_discard(
        any::<crate::commands::scheduled_tasks::TaskRequest>(),
        any(),
    );
    let _: Result<Option<crate::scheduled_tasks::types::Task>, CommandError> =
        crate::commands::scheduled_tasks::scheduled_task_get(
            any::<crate::commands::scheduled_tasks::TaskRequest>(),
            any(),
        );
    let _: Result<crate::scheduled_tasks::types::Run, CommandError> =
        crate::commands::scheduled_tasks::scheduled_task_run_now(
            any::<crate::commands::scheduled_tasks::TaskRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::scheduled_tasks::types::Run>, CommandError> =
        crate::commands::scheduled_tasks::scheduled_task_runs(
            any::<crate::commands::scheduled_tasks::TaskRequest>(),
            any(),
        );
    let _: Result<crate::scheduled_tasks::types::Task, CommandError> =
        crate::commands::scheduled_tasks::scheduled_task_set_status(
            any::<crate::commands::scheduled_tasks::StatusRequest>(),
            any(),
        );
    let _: Result<Option<crate::scheduled_tasks::WatchSnapshot>, CommandError> =
        crate::commands::scheduled_tasks::scheduled_task_watch(
            any::<crate::commands::scheduled_tasks::RunRequest>(),
            any(),
            any(),
        );
    let _: Result<Vec<crate::scheduled_tasks::types::Run>, CommandError> =
        crate::commands::scheduled_tasks::scheduled_tasks_activity(
            any::<crate::commands::scheduled_tasks::LoupistRequest>(),
            any(),
        );
    let _: Result<Vec<crate::scheduled_tasks::types::TaskListItem>, CommandError> =
        crate::commands::scheduled_tasks::scheduled_tasks_list(
            any::<crate::commands::scheduled_tasks::LoupistRequest>(),
            any(),
        );
    let _: Result<crate::skills::SkillCatalog, CommandError> =
        crate::commands::skills::skills_create(
            any::<crate::commands::skills::SkillCreateRequest>(),
            any(),
        );
    let _: Result<crate::skills::SkillCatalog, CommandError> =
        crate::commands::skills::skills_delete(
            any::<crate::commands::skills::SkillDeleteRequest>(),
            any(),
        );
    let _: Result<Option<crate::skills::SkillCatalog>, CommandError> =
        crate::commands::skills::skills_import(
            any::<crate::commands::skills::SkillImportRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::skills::SkillCatalog, CommandError> = crate::commands::skills::skills_list(
        any::<crate::scoped_config::ConfigReadRequest>(),
        any(),
    );
    let _: Result<crate::skills::SkillDocument, CommandError> =
        crate::commands::skills::skills_read(
            any::<crate::commands::skills::SkillReadRequest>(),
            any(),
        );
    let _: Result<(), CommandError> = crate::commands::skills::skills_reveal(
        any::<crate::commands::skills::SkillReadRequest>(),
        any(),
    );
    let _: Result<crate::skills::SkillCatalog, CommandError> = crate::commands::skills::skills_save(
        any::<crate::commands::skills::SkillSaveRequest>(),
        any(),
    );
    let _: Result<crate::store::user_context::HoldingsSnapshot, CommandError> =
        crate::commands::user_context::holdings_commit_csv(
            any::<crate::commands::user_context::CsvCommitRequest>(),
            any(),
        );
    let _: Result<crate::store::user_context::crypto_file::CryptoFilePreview, CommandError> =
        crate::commands::user_context::holdings_import_crypto_file(
            any::<crate::commands::user_context::CsvImportRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::store::user_context::HoldingsCsvPreview, CommandError> =
        crate::commands::user_context::holdings_import_csv(
            any::<crate::commands::user_context::CsvImportRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::store::user_context::HoldingsSnapshot, CommandError> =
        crate::commands::user_context::holdings_import_kis(any()).await;
    let _: Result<crate::store::user_context::crypto_file::CryptoFilePreview, CommandError> =
        crate::commands::user_context::holdings_map_columns(
            any::<crate::commands::user_context::MapColumnsRequest>(),
            any(),
        );
    let _: Result<crate::store::user_context::HoldingsOverview, CommandError> =
        crate::commands::user_context::holdings_summary(any());
    let _: Result<Vec<crate::store::user_context::wallets::WalletView>, CommandError> =
        crate::commands::user_context::wallet_add(
            any::<crate::commands::user_context::WalletAddRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::store::user_context::wallets::WalletView>, CommandError> =
        crate::commands::user_context::wallet_refresh(
            any::<Option<crate::commands::user_context::WalletRefreshRequest>>(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> = crate::commands::user_context::wallet_remove(
        any::<crate::commands::user_context::WalletIdRequest>(),
        any(),
    );
    let _: Result<crate::store::user_context::wallets::WalletView, CommandError> =
        crate::commands::user_context::wallet_rename(
            any::<crate::commands::user_context::WalletRenameRequest>(),
            any(),
        );
    let _: Result<Vec<crate::store::user_context::wallets::WalletView>, CommandError> =
        crate::commands::user_context::wallets_list(any());
    let _: Result<crate::store::user_context::WatchlistEntry, CommandError> =
        crate::commands::user_context::watchlist_add(
            any::<crate::commands::user_context::WatchlistAddRequest>(),
            any(),
        );
    let _: Result<Vec<crate::store::user_context::WatchlistEntry>, CommandError> =
        crate::commands::user_context::watchlist_list(any());
    let _: Result<(), CommandError> = crate::commands::user_context::watchlist_remove(
        any::<crate::commands::user_context::WatchlistRemoveRequest>(),
        any(),
    );
    let _: Result<crate::commands::watch::WatchDefaultsApplyResult, CommandError> =
        crate::commands::watch::watch_defaults_apply(
            any(),
            any::<crate::commands::watch::WatchDefaultsApplyRequest>(),
        );
    let _: Result<crate::commands::watch::WatchCheckResult, CommandError> =
        crate::commands::watch::watch_check_now(
            any(),
            any::<crate::commands::watch::WatchCheckNowRequest>(),
        )
        .await;
    let _: Result<Vec<crate::knowledge::watch::WatchEventView>, CommandError> =
        crate::commands::watch::watch_events_list(
            any(),
            any::<crate::commands::watch::WatchEventsListRequest>(),
        );
    let _: Result<(), CommandError> = crate::commands::watch::watch_events_seen(
        any(),
        any::<crate::commands::watch::WatchEventsSeenRequest>(),
    );
    let _: Result<crate::commands::watch::WatchSettingsView, CommandError> =
        crate::commands::watch::watch_pause_set(
            any(),
            any::<crate::commands::watch::WatchPauseSetRequest>(),
        );
    let _: Result<crate::commands::watch::WatchSettingsView, CommandError> =
        crate::commands::watch::watch_setting_set(
            any(),
            any::<crate::commands::watch::WatchSettingSetRequest>(),
        );
    let _: Result<crate::commands::watch::WatchSettingsView, CommandError> =
        crate::commands::watch::watch_settings_list(
            any(),
            any::<crate::commands::watch::WatchSettingsListRequest>(),
        );
    let _: Result<crate::chat::activity::ChatActivityDetail, CommandError> =
        crate::commands::workspace::chat_activity_read(
            any::<crate::commands::workspace::ChatActivityRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::fs::workspace_files::WorkspaceDirectory, CommandError> =
        crate::commands::workspace::workspace_list(
            any::<crate::commands::workspace::WorkspacePathRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::fs::workspace_files::WorkspaceFile, CommandError> =
        crate::commands::workspace::workspace_read(
            any::<crate::commands::workspace::WorkspacePathRequest>(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> = crate::commands::workspace::workspace_reveal(
        any::<crate::commands::workspace::WorkspacePathRequest>(),
        any(),
    )
    .await;
    let _: Result<Vec<crate::commands::workspace_ui::DraftAttachment>, CommandError> =
        crate::commands::workspace_ui::chat_draft_files_read(
            any(),
            any::<crate::commands::workspace_ui::DraftIdentity>(),
        );
    let _: Result<(), CommandError> = crate::commands::workspace_ui::chat_draft_files_write(
        any(),
        any::<crate::commands::workspace_ui::DraftIdentity>(),
        any(),
    );
    let _: Result<Option<crate::commands::workspace_ui::StoredChatDraft>, CommandError> =
        crate::commands::workspace_ui::chat_draft_read(
            any(),
            any::<crate::commands::workspace_ui::DraftIdentity>(),
        );
    let _: Result<(), CommandError> = crate::commands::workspace_ui::chat_draft_write(
        any(),
        any::<crate::commands::workspace_ui::DraftIdentity>(),
        any(),
    );
    let _: Result<Option<crate::commands::workspace_ui::WorkspaceUiState>, CommandError> =
        crate::commands::workspace_ui::workspace_ui_read(any());
    let _: Result<crate::commands::workspace_ui::WorkspaceUiState, CommandError> =
        crate::commands::workspace_ui::workspace_ui_write(
            any::<crate::commands::workspace_ui::WorkspaceUiWriteRequest>(),
            any(),
        );
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_connection_add_command(
            any::<crate::custom_connections::commands::AddCommandRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::custom_connections::service::ConnectionView>, CommandError> =
        crate::custom_connections::commands::custom_connection_add_json(
            any::<crate::custom_connections::commands::AddJsonRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_connection_add_url(
            any::<crate::custom_connections::commands::AddUrlRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_connection_authorize(
            any::<crate::custom_connections::commands::AuthorizeRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_connection_confirm(
            any::<crate::custom_connections::commands::ConfirmRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::custom_connections::service::ConnectionView>, CommandError> =
        crate::custom_connections::commands::custom_connection_list(
            any::<crate::custom_connections::commands::ListRequest>(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> =
        crate::custom_connections::commands::custom_connection_remove(
            any::<crate::custom_connections::commands::IdentityRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_connection_set_enabled(
            any::<crate::custom_connections::commands::EnabledRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_connection_set_memory(
            any::<crate::custom_connections::commands::MemoryRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_connection_sign_in(
            any::<crate::custom_connections::commands::IdentityRequest>(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> =
        crate::custom_connections::commands::custom_connection_sign_in_cancel(any::<
            crate::custom_connections::commands::IdentityRequest,
        >());
    let _: Result<crate::custom_connections::service::ConnectionView, CommandError> =
        crate::custom_connections::commands::custom_tool_approve(
            any::<crate::custom_connections::commands::ApproveRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::marketplace::LoupistCapabilityBindingDto, CommandError> =
        crate::marketplace::loupist_capability_disable(
            any::<crate::marketplace::LoupistCapabilityRequest>(),
            any(),
        );
    let _: Result<crate::marketplace::LoupistCapabilityBindingDto, CommandError> =
        crate::marketplace::loupist_capability_enable(
            any::<crate::marketplace::LoupistCapabilityRequest>(),
            any(),
        );
    let _: Result<Vec<crate::marketplace::LoupistCapabilityBindingDto>, CommandError> =
        crate::marketplace::loupist_capability_list(any(), any());
    let _: Result<crate::marketplace::MarketplaceSnapshotDto, CommandError> =
        crate::marketplace::marketplace_snapshot(any());
    let _: Result<crate::marketplace::MarketplaceConnectorStatusDto, CommandError> =
        crate::marketplace::connector_config::marketplace_connector_configure(
            any::<crate::marketplace::MarketplaceConnectorConfigureRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::marketplace::MarketplaceCredentialStatusDto>, CommandError> =
        crate::marketplace::credentials::marketplace_credential_delete(
            any::<crate::marketplace::MarketplaceCredentialRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::marketplace::MarketplaceCredentialStatusDto>, CommandError> =
        crate::marketplace::credentials::marketplace_credential_save(
            any::<crate::marketplace::MarketplaceCredentialSaveRequest>(),
            any(),
        );
    let _: Result<Vec<crate::marketplace::MarketplaceCredentialStatusDto>, CommandError> =
        crate::marketplace::credentials::marketplace_credential_verify(
            any::<crate::marketplace::MarketplaceCredentialRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::connection::model_notice_ack(
            any::<crate::providers::connection::ModelNoticeAckRequest>(),
            any(),
        );
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::connection::provider_configure(
            any::<crate::providers::connection::ProviderConfigureRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::connection::provider_connect(
            any::<crate::providers::connection::ProviderConnectionRequest>(),
            any(),
            any(),
        )
        .await;
    let _: () = crate::providers::connection::provider_connect_cancel(any());
    let _: Result<(), CommandError> =
        crate::providers::connection::provider_connect_open_browser(any());
    let _: Result<(), CommandError> = crate::providers::connection::provider_connect_submit_code(
        any::<crate::providers::connection::ProviderSubmitCodeRequest>(),
        any(),
    );
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::connection::provider_custom_remove(
            any::<crate::providers::connection::ProviderConnectionRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::connection::provider_custom_save(
            any::<crate::providers::custom::CustomProviderRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::connection::provider_disconnect(
            any::<crate::providers::connection::ProviderConnectionRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::connection::provider_models(
            any::<crate::providers::connection::ProviderModelsRequest>(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> = crate::providers::connection::provider_open_console(any::<
        crate::providers::connection::ProviderPageRequest,
    >());
    let _: Result<(), CommandError> = crate::providers::connection::provider_open_usage(any::<
        crate::providers::connection::ProviderPageRequest,
    >());
    let _: Result<crate::providers::local::ProviderCustomTestResult, CommandError> =
        crate::providers::local::provider_custom_test(any::<
            crate::providers::local::ProviderCustomTestRequest,
        >())
        .await;
    let _: Result<crate::providers::local::LocalDiscovery, CommandError> =
        crate::providers::local::provider_local_discover().await;
    let _: Result<crate::providers::catalog::ModelCatalogView, CommandError> =
        crate::providers::local::provider_preset_save(
            any::<crate::providers::local::ProviderPresetRequest>(),
            any(),
        )
        .await;
    let _: Result<Option<String>, CommandError> =
        crate::runs::log::run_logs_last_error(any::<crate::runs::log::DiagnosticsRequest>(), any());
    let _: Result<(), CommandError> =
        crate::runs::log::run_logs_reveal(any::<crate::runs::log::DiagnosticsRequest>(), any());
    let _: () = crate::sharing::commands::deep_links::deep_link_dismiss();
    let _: Option<crate::sharing::deep_link::DeepLinkEvent> =
        crate::sharing::commands::deep_links::deep_link_pending();
    let _: Result<Option<String>, CommandError> = crate::sharing::commands::gallery::gallery_image(
        any::<crate::sharing::commands::gallery::GalleryImageRequest>(),
        any(),
    )
    .await;
    let _: Result<crate::sharing::commands::gallery::GalleryView, CommandError> =
        crate::sharing::commands::gallery::gallery_index(
            any::<crate::sharing::commands::gallery::GalleryIndexRequest>(),
            any(),
        )
        .await;
    let _: Result<serde_json::Value, CommandError> =
        crate::sharing::commands::gallery::gallery_listing_request(
            any::<crate::sharing::commands::gallery::GalleryListingRequest>(),
            any(),
        )
        .await;
    let _: Result<Option<&'static str>, CommandError> =
        crate::sharing::commands::gallery::gallery_listing_status(
            any::<crate::sharing::commands::LoupistRequest>(),
            any(),
        )
        .await;
    let _: crate::sharing::commands::github_account::GithubAccount =
        crate::sharing::commands::github_account::github_account(any());
    let _: Result<crate::sharing::commands::github_account::GithubAccount, CommandError> =
        crate::sharing::commands::github_account::github_sign_in(any(), any()).await;
    let _: () = crate::sharing::commands::github_account::github_sign_in_cancel(any());
    let _: Result<(), CommandError> =
        crate::sharing::commands::github_account::github_sign_in_open(any());
    let _: Result<crate::sharing::commands::github_account::GithubAccount, CommandError> =
        crate::sharing::commands::github_account::github_sign_out(any());
    let _: Result<serde_json::Value, CommandError> = crate::sharing::commands::add::pack_add(
        any::<crate::sharing::commands::add::PackAddRequest>(),
        any(),
        any(),
    )
    .await;
    let _: Result<Vec<crate::sharing::commands::added::PackImportView>, CommandError> =
        crate::sharing::commands::updates::pack_apply_update(
            any::<crate::sharing::commands::updates::PackApplyUpdateRequest>(),
            any(),
        )
        .await;
    let _: Result<crate::sharing::commands::updates::UpdateCheck, CommandError> =
        crate::sharing::commands::updates::pack_check_update(
            any::<crate::sharing::commands::updates::PackCheckRequest>(),
            any(),
        )
        .await;
    let _: Result<Option<crate::sharing::commands::updates::UpdateCheck>, CommandError> =
        crate::sharing::commands::updates::pack_choose_newer(
            any::<crate::sharing::commands::updates::PackChooseNewerRequest>(),
            any(),
        )
        .await;
    let _: Result<Vec<crate::sharing::commands::review::SuggestionGroup>, CommandError> =
        crate::sharing::commands::review::pack_decide(
            any::<crate::sharing::commands::review::PackDecideRequest>(),
            any(),
        );
    let _: Result<crate::sharing::import::ItemView, CommandError> =
        crate::sharing::commands::review::pack_item_read(
            any::<crate::sharing::commands::review::PackItemRequest>(),
            any(),
        );
    let _: Result<Vec<crate::sharing::commands::added::PackImportView>, CommandError> =
        crate::sharing::commands::added::pack_list(
            any::<crate::sharing::commands::LoupistRequest>(),
            any(),
        )
        .await;
    let _: Result<Option<crate::sharing::import::OpenResult>, CommandError> =
        crate::sharing::commands::open::pack_open(
            any::<crate::sharing::commands::open::PackOpenRequest>(),
            any(),
        )
        .await;
    let _: Result<Option<serde_json::Value>, CommandError> =
        crate::sharing::commands::added::pack_origin(
            any::<crate::sharing::commands::added::PackOriginRequest>(),
            any(),
        );
    let _: Result<Vec<crate::sharing::commands::added::PackImportView>, CommandError> =
        crate::sharing::commands::added::pack_remove(
            any::<crate::sharing::commands::added::PackRemoveRequest>(),
            any(),
        );
    let _: Result<(), CommandError> = crate::sharing::commands::added::pack_report(
        any::<crate::sharing::commands::added::PackImportRequest>(),
        any(),
    );
    let _: Result<Vec<crate::sharing::commands::review::SuggestionGroup>, CommandError> =
        crate::sharing::commands::review::pack_suggestions(
            any::<crate::sharing::commands::LoupistRequest>(),
            any(),
        );
    let _: Result<serde_json::Value, CommandError> =
        crate::sharing::commands::share::share_copy_text(
            any::<crate::sharing::commands::share::ShareRequest>(),
            any(),
        );
    let _: Result<Option<serde_json::Value>, CommandError> =
        crate::sharing::commands::save::share_export(
            any::<crate::sharing::commands::save::ShareExportRequest>(),
            any(),
        )
        .await;
    let _: Result<serde_json::Value, CommandError> =
        crate::sharing::commands::publish::share_publish(
            any::<crate::sharing::commands::publish::SharePublishRequest>(),
            any(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> = crate::sharing::commands::save::share_reveal(
        any::<crate::sharing::commands::LoupistRequest>(),
        any(),
    );
    let _: Result<crate::sharing::commands::share::ShareSummary, CommandError> =
        crate::sharing::commands::share::share_summary(
            any::<crate::sharing::commands::share::ShareSummaryRequest>(),
            any(),
        );
    let _: Result<crate::updater::UpdateStateDto, CommandError> =
        crate::updater::update_check(any(), any()).await;
    let _: Result<crate::updater::UpdateInstallResult, CommandError> =
        crate::updater::update_install(any(), any(), any::<crate::updater::UpdateInstallRequest>())
            .await;
    let _: Result<crate::updater::UpdateStateDto, CommandError> =
        crate::updater::update_status(any(), any());
    let _: Result<(), CommandError> = crate::web::browser::browser_tab_close(
        any::<crate::web::browser::BrowserTabRequest>(),
        any(),
        any(),
    )
    .await;
    let _: Result<(), CommandError> = crate::web::browser::browser_tab_history(
        any::<crate::web::browser::BrowserTabHistoryRequest>(),
        any(),
        any(),
    )
    .await;
    let _: Result<(), CommandError> = crate::web::browser::browser_tab_navigate(
        any::<crate::web::browser::BrowserTabNavigateRequest>(),
        any(),
        any(),
    )
    .await;
    let _: Result<crate::web::browser::BrowserTabState, CommandError> =
        crate::web::browser::browser_tab_open(
            any::<crate::web::browser::BrowserTabOpenRequest>(),
            any(),
            any(),
            any(),
        )
        .await;
    let _: Result<(), CommandError> = crate::web::browser::browser_tab_reload(
        any::<crate::web::browser::BrowserTabRequest>(),
        any(),
        any(),
    )
    .await;
    let _: Result<(), CommandError> = crate::web::browser::browser_tab_set_bounds(
        any::<crate::web::browser::BrowserTabBoundsRequest>(),
        any(),
        any(),
    )
    .await;
    let _: Result<(), CommandError> = crate::web::browser::browser_tabs_reset(any(), any()).await;
    let _: Option<String> = crate::web::favicon::site_favicon(any()).await;
    let _: crate::power::KeepAwakeStatus = crate::commands::power::world_keep_awake_get(any());
    let _: Result<crate::power::KeepAwakeStatus, CommandError> =
        crate::commands::power::world_keep_awake_set(
            any::<crate::commands::power::WorldKeepAwakeRequest>(),
            any(),
        );
}

struct Command {
    name: &'static str,
    request: Option<String>,
    response: String,
    types: BTreeSet<TypeId>,
}

/// Named TypeScript types a request or response refers to.
struct Names<'a>(&'a mut BTreeSet<TypeId>);

impl TypeVisitor for Names<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if T::output_path().is_some() {
            self.0.insert(TypeId::of::<T>());
        } else {
            T::visit_generics(self);
        }
    }
}

fn command<Request: TS + 'static, Response: TS + 'static>(
    collector: &mut Collector<'_>,
    name: &'static str,
) -> Command {
    collector.visit::<Request>();
    collector.visit::<Response>();
    let mut types = BTreeSet::new();
    let unit = TypeId::of::<Request>() == TypeId::of::<()>();
    if !unit {
        Names(&mut types).visit::<Request>();
    }
    Names(&mut types).visit::<Response>();
    Command {
        name,
        // Commands without a `request` argument take their own arguments.
        request: (!unit).then(|| Request::name(collector.cfg)),
        response: Response::name(collector.cfg),
        types,
    }
}

struct Decl {
    name: String,
    module: String,
    text: String,
    deps: Vec<TypeId>,
}

struct Collector<'a> {
    cfg: &'a Config,
    decls: HashMap<TypeId, Decl>,
}

impl TypeVisitor for Collector<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        let id = TypeId::of::<T>();
        if T::output_path().is_none() || self.decls.contains_key(&id) {
            return;
        }
        let rust = std::any::type_name::<T>();
        let path = rust.split('<').next().unwrap_or(rust);
        let module = match path.rsplit_once("::").map(|(module, _)| module) {
            Some(module) if module.starts_with("loupist_desktop::") => {
                module["loupist_desktop::".len()..].to_owned()
            }
            Some("loupist_desktop") | None => "lib".to_owned(),
            // serde_json::Value and the like.
            Some(_) => "external".to_owned(),
        };
        self.decls.insert(
            id,
            Decl {
                name: T::ident(self.cfg),
                module,
                text: format!(
                    "{}export {}",
                    T::docs().unwrap_or_default(),
                    T::decl(self.cfg)
                ),
                deps: T::dependencies(self.cfg)
                    .into_iter()
                    .map(|dep| dep.type_id)
                    .collect(),
            },
        );
        T::visit_dependencies(self);
    }
}

/// The generated tree: relative path to file contents.
fn generate() -> Result<BTreeMap<PathBuf, String>, String> {
    let cfg = Config::new().with_large_int("number");
    let mut collector = Collector {
        cfg: &cfg,
        decls: HashMap::new(),
    };
    roots(&mut collector);
    let commands = commands(&mut collector);
    let decls = collector.decls;

    // One TypeScript name per declaration; the same external shape may be
    // reached through more than one Rust type.
    let mut owners: BTreeMap<&str, &Decl> = BTreeMap::new();
    let mut collisions = Vec::new();
    for decl in decls.values() {
        match owners.insert(&decl.name, decl) {
            Some(other) if other.module == decl.module && other.text == decl.text => {}
            Some(other) => collisions.push(format!(
                "{} is declared by both {} and {}; add ts(rename = \"...\") to one's #[cfg_attr(test, derive(ts_rs::TS))]",
                decl.name, other.module, decl.module
            )),
            None => {}
        }
    }
    if !collisions.is_empty() {
        collisions.sort();
        return Err(collisions.join("\n"));
    }

    let mut modules: BTreeMap<&str, Vec<&Decl>> = BTreeMap::new();
    let mut files = BTreeMap::new();
    files.insert(
        PathBuf::from(COMMANDS_FILE),
        commands_module(&commands, &decls)?,
    );
    for decl in owners.values() {
        modules.entry(&decl.module).or_default().push(decl);
    }
    let mut index = String::from(HEADER);
    index.push('\n');
    index.push_str(&format!(
        "export type * from \"./{}\";\n",
        COMMANDS_FILE.trim_end_matches(".ts")
    ));
    for (module, mut members) in modules {
        members.sort_by(|a, b| a.name.cmp(&b.name));
        let file = PathBuf::from(format!("{}.ts", module.replace("::", "/")));
        let mut imports: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
        for decl in &members {
            for dep in &decl.deps {
                let target = &decls[dep];
                if target.module != module {
                    let path = relative_import(&file, &target.module);
                    imports.entry(path).or_default().insert(&target.name);
                }
            }
        }
        let mut text = String::from(HEADER);
        if !imports.is_empty() {
            text.push('\n');
        }
        for (path, names) in imports {
            let names = names.into_iter().collect::<Vec<_>>().join(", ");
            text.push_str(&format!("import type {{ {names} }} from \"{path}\";\n"));
        }
        for decl in members {
            text.push('\n');
            text.push_str(&format_decl(&decl.text)?);
            text.push('\n');
        }
        index.push_str(&format!(
            "export type * from \"./{}\";\n",
            file.with_extension("").to_string_lossy()
        ));
        files.insert(file, text);
    }
    files.insert(PathBuf::from("index.ts"), index);
    Ok(files)
}

const COMMANDS_FILE: &str = "bridge_commands.ts";

/// `BridgeCommands`: each registered command's `request` argument and response.
fn commands_module(commands: &[Command], decls: &HashMap<TypeId, Decl>) -> Result<String, String> {
    let mut imports: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    let mut rows = String::new();
    for command in commands {
        for id in &command.types {
            let target = &decls[id];
            let path = relative_import(Path::new(COMMANDS_FILE), &target.module);
            imports.entry(path).or_default().insert(&target.name);
        }
        let request = command.request.as_deref().unwrap_or("never");
        rows.push_str(&format!(
            "{}: {{ request: {request}, response: {}, }}, ",
            command.name, command.response
        ));
    }
    let mut text = String::from(HEADER);
    text.push('\n');
    for (path, names) in imports {
        let names = names.into_iter().collect::<Vec<_>>().join(", ");
        text.push_str(&format!("import type {{ {names} }} from \"{path}\";\n"));
    }
    text.push('\n');
    text.push_str(&format_decl(&format!(
        "/**\n * Each registered command's `request` argument and response (`commands!` in\n * `src-tauri/src/bridge_contract.rs`). `never`: the command takes other arguments.\n */\nexport type BridgeCommands = {{ {rows}}};"
    ))?);
    text.push('\n');
    Ok(text)
}

fn relative_import(from: &Path, module: &str) -> String {
    let depth = from.components().count() - 1;
    let up = if depth == 0 {
        "./".to_owned()
    } else {
        "../".repeat(depth)
    };
    format!("{up}{}", module.replace("::", "/"))
}

// A small printer for ts-rs output: the same tokens, one member per line
// once a type does not fit in WIDTH columns.

#[derive(Clone, Debug, PartialEq)]
enum Node {
    Token(String),
    Group(char, Vec<Node>, char),
}

fn tokens(source: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            let start = i;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            out.push(chars[start..i.min(chars.len())].iter().collect());
        } else if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                i += 1;
            }
            i += 1;
            out.push(chars[start..i.min(chars.len())].iter().collect());
        } else if "{}[]()<>,;|&:?=".contains(c) {
            out.push(c.to_string());
            i += 1;
        } else {
            let start = i;
            while i < chars.len()
                && !chars[i].is_whitespace()
                && !"{}[]()<>,;|&:?=\"".contains(chars[i])
            {
                i += 1;
            }
            if start == i {
                return Err(format!("unexpected character {c:?} in {source}"));
            }
            out.push(chars[start..i].iter().collect());
        }
    }
    Ok(out)
}

fn tree(tokens: &[String], at: &mut usize, close: Option<char>) -> Result<Vec<Node>, String> {
    let mut nodes = Vec::new();
    while *at < tokens.len() {
        let token = &tokens[*at];
        *at += 1;
        let open = match token.as_str() {
            "{" => Some('}'),
            "[" => Some(']'),
            "(" => Some(')'),
            "<" => Some('>'),
            _ => None,
        };
        if let Some(end) = open {
            let children = tree(tokens, at, Some(end))?;
            nodes.push(Node::Group(
                token.chars().next().unwrap_or('{'),
                children,
                end,
            ));
        } else if close.is_some_and(|end| token.len() == 1 && token.starts_with(end)) {
            return Ok(nodes);
        } else {
            nodes.push(Node::Token(token.clone()));
        }
    }
    match close {
        None => Ok(nodes),
        Some(end) => Err(format!("missing {end}")),
    }
}

fn is(node: &Node, token: &str) -> bool {
    matches!(node, Node::Token(text) if text == token)
}

fn split(nodes: &[Node], separator: &str) -> Vec<Vec<Node>> {
    let mut parts = vec![Vec::new()];
    for node in nodes {
        if is(node, separator) {
            parts.push(Vec::new());
        } else if let Some(part) = parts.last_mut() {
            part.push(node.clone());
        }
    }
    parts
}

/// Object members, without the trailing comma ts-rs adds.
fn members(nodes: &[Node]) -> Vec<Vec<Node>> {
    split(nodes, ",")
        .into_iter()
        .filter(|member| !member.is_empty())
        .collect()
}

fn has_comment(nodes: &[Node]) -> bool {
    nodes.iter().any(|node| match node {
        Node::Token(text) => text.starts_with("/*"),
        Node::Group(_, children, _) => has_comment(children),
    })
}

fn inline(nodes: &[Node]) -> String {
    let mut out = String::new();
    let mut previous: Option<&Node> = None;
    for node in nodes {
        let text = match node {
            Node::Token(text) => text.clone(),
            Node::Group('{', children, _) => {
                let members = members(children);
                if members.is_empty() {
                    "{}".to_owned()
                } else {
                    let body = members
                        .iter()
                        .map(|m| inline(m))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ {body} }}")
                }
            }
            Node::Group(open, children, close) => format!("{open}{}{close}", inline(children)),
        };
        if spaced(previous, node) {
            out.push(' ');
        }
        out.push_str(&text);
        previous = Some(node);
    }
    out
}

/// Whether a space separates `node` from the node before it.
fn spaced(previous: Option<&Node>, node: &Node) -> bool {
    match (previous, node) {
        (None, _) => false,
        (_, Node::Token(t))
            if matches!(t.as_str(), "," | ":" | "?" | ";") || t.starts_with('.') =>
        {
            false
        }
        (_, Node::Group('<', _, _)) => false,
        (Some(prev), Node::Group('(', _, _)) if is(prev, "import") => false,
        (Some(prev), _) if is(prev, "?") => false,
        _ => true,
    }
}

fn fits(text: &str, indent: usize) -> bool {
    !text.contains('\n') && indent + text.len() <= WIDTH
}

/// Renders a type expression that starts at column `column` on a line
/// indented by `indent`. A broken union starts on the next line.
fn render(nodes: &[Node], indent: usize, column: usize) -> String {
    let flat = inline(nodes);
    if !has_comment(nodes) && fits(&flat, column) {
        return flat;
    }
    let alternatives = split(nodes, "|");
    if alternatives.len() > 1 {
        let pad = " ".repeat(indent + 2);
        return alternatives
            .iter()
            .map(|alternative| format!("\n{pad}| {}", render(alternative, indent + 2, indent + 4)))
            .collect();
    }
    let mut out = String::new();
    let mut previous: Option<&Node> = None;
    for node in nodes {
        let text = match node {
            Node::Group('{', children, _) => object(children, indent),
            Node::Group(open, children, close) => {
                let column = column + out.len() + 1;
                format!("{open}{}{close}", render(children, indent, column))
            }
            Node::Token(text) => text.clone(),
        };
        if spaced(previous, node) {
            out.push(' ');
        }
        out.push_str(&text);
        previous = Some(node);
    }
    out
}

fn object(children: &[Node], indent: usize) -> String {
    let members = members(children);
    if members.is_empty() {
        return "{}".to_owned();
    }
    let pad = " ".repeat(indent + 2);
    let mut out = String::from("{\n");
    for member in members {
        let mut rest = member.as_slice();
        while let Some(Node::Token(text)) = rest.first() {
            if !text.starts_with("/*") {
                break;
            }
            for line in doc_lines(text, &pad) {
                out.push_str(&line);
                out.push('\n');
            }
            rest = &rest[1..];
        }
        let colon = rest.iter().position(|node| is(node, ":"));
        let line = match colon {
            Some(at) => {
                let key = format!("{}:", inline(&rest[..at]));
                let value = render(&rest[at + 1..], indent + 2, indent + 2 + key.len() + 1);
                if value.starts_with('\n') {
                    format!("{key}{value}")
                } else {
                    format!("{key} {value}")
                }
            }
            None => render(rest, indent + 2, indent + 2),
        };
        out.push_str(&format!("{pad}{line},\n"));
    }
    out.push_str(&" ".repeat(indent));
    out.push('}');
    out
}

/// A JSDoc comment at `pad`: one line when its text fits, else a block.
fn doc_lines(comment: &str, pad: &str) -> Vec<String> {
    let body = comment
        .trim()
        .trim_start_matches("/**")
        .trim_end_matches("*/");
    let lines: Vec<&str> = body
        .lines()
        .map(|line| {
            line.trim()
                .strip_prefix('*')
                .unwrap_or(line.trim())
                .trim_end()
        })
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect();
    let lines: Vec<&str> = {
        let start = lines
            .iter()
            .position(|line| !line.is_empty())
            .unwrap_or(lines.len());
        let end = lines
            .iter()
            .rposition(|line| !line.is_empty())
            .map_or(start, |end| end + 1);
        lines[start..end].to_vec()
    };
    if let [line] = lines.as_slice() {
        let single = format!("{pad}/** {line} */");
        if single.len() <= WIDTH {
            return vec![single];
        }
    }
    let mut out = vec![format!("{pad}/**")];
    out.extend(
        lines
            .iter()
            .map(|line| format!("{pad} * {line}").trim_end().to_owned()),
    );
    out.push(format!("{pad} */"));
    out
}

/// Formats one `export type Name = ...;` declaration and its docs.
fn format_decl(text: &str) -> Result<String, String> {
    let (docs, decl) = match text.find("*/\n") {
        Some(end) if text.starts_with("/**") => (&text[..end + 2], text[end + 3..].trim()),
        _ => ("", text.trim()),
    };
    let (head, body) = decl
        .split_once(" = ")
        .ok_or_else(|| format!("no = in {decl}"))?;
    let body = body
        .trim_end()
        .strip_suffix(';')
        .ok_or_else(|| format!("no ; in {decl}"))?;
    let source = tokens(body)?;
    let nodes = tree(&source, &mut 0, None)?;
    let rendered = render(&nodes, 0, head.len() + 3);
    let mut out = String::new();
    if !docs.is_empty() {
        for line in doc_lines(docs, "") {
            out.push_str(&line);
            out.push('\n');
        }
    }
    out.push_str(head);
    out.push_str(" =");
    if !rendered.starts_with('\n') {
        out.push(' ');
    }
    out.push_str(&rendered);
    out.push(';');
    // The printer only moves whitespace and trailing commas.
    let normalize = |text: &str| -> Result<Vec<String>, String> {
        let mut out: Vec<String> = Vec::new();
        for token in tokens(text)? {
            if token.starts_with("/*") {
                let words = token
                    .trim_start_matches("/**")
                    .trim_end_matches("*/")
                    .lines()
                    .map(|line| line.trim().strip_prefix('*').unwrap_or(line.trim()))
                    .collect::<Vec<_>>()
                    .join(" ");
                out.push(words.split_whitespace().collect::<Vec<_>>().join(" "));
                continue;
            }
            if token == "}" && out.last().is_some_and(|last| last == ",") {
                out.pop();
            }
            // A broken union leads with `|`.
            if token == "|"
                && out
                    .last()
                    .is_none_or(|last| matches!(last.as_str(), "=" | ":" | "<" | "("))
            {
                continue;
            }
            out.push(token);
        }
        Ok(out)
    };
    if normalize(text)? != normalize(&out)? {
        return Err(format!("formatting changed tokens of:\n{text}\n---\n{out}"));
    }
    Ok(out)
}

fn generated_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/contracts/generated")
}

fn read_tree(root: &Path) -> BTreeMap<PathBuf, String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else if path.extension().is_some_and(|ext| ext == "ts") {
                let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                out.insert(relative, std::fs::read_to_string(&path).unwrap_or_default());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn write_tree(root: &Path, files: &BTreeMap<PathBuf, String>) {
    for path in read_tree(root).keys() {
        if !files.contains_key(path) {
            std::fs::remove_file(root.join(path)).expect("remove a stale generated file");
        }
    }
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("generated file parent"))
            .expect("create the generated directory");
        std::fs::write(&path, text).expect("write a generated file");
    }
}

#[test]
fn generated_contracts_match_rust_types() {
    let files = generate().unwrap_or_else(|error| panic!("{error}"));
    let committed = generated_dir();
    if std::env::var_os("LOUPIST_WRITE_CONTRACTS").is_some() {
        write_tree(&committed, &files);
        return;
    }
    let scratch = tempfile::tempdir().expect("create a scratch directory");
    write_tree(scratch.path(), &files);
    let fresh = read_tree(scratch.path());
    let current = read_tree(&committed);
    let mut drift = Vec::new();
    for path in fresh.keys().chain(current.keys()).collect::<BTreeSet<_>>() {
        match (fresh.get(path), current.get(path)) {
            (Some(a), Some(b)) if a == b => {}
            (Some(_), Some(_)) => drift.push(format!("changed: {}", path.display())),
            (Some(_), None) => drift.push(format!("missing: {}", path.display())),
            (None, _) => drift.push(format!("stale: {}", path.display())),
        }
    }
    if !drift.is_empty() {
        let kept = scratch.keep();
        panic!(
            "src/contracts/generated is out of date with the Rust wire types:\n  {}\n\
             Fresh output: {}\nRegenerate with:\n  {REGENERATE}",
            drift.join("\n  "),
            kept.display()
        );
    }
}

#[test]
fn printer_keeps_tokens_and_breaks_long_types() {
    let short = format_decl("export type A = { a: string, b?: number, };").unwrap();
    assert_eq!(short, "export type A = { a: string, b?: number };");
    let long = format_decl(
        "/**\n * Docs.\n */\nexport type E = { \"type\": \"started\", run_id: string, } | { \"type\": \"memory\", run_id: string, memories: Array<MemoryRefDto>, extra: string, };",
    )
    .unwrap();
    assert_eq!(
        long,
        "/** Docs. */\nexport type E =\n  | { \"type\": \"started\", run_id: string }\n  | { \"type\": \"memory\", run_id: string, memories: Array<MemoryRefDto>, extra: string };"
    );
}
