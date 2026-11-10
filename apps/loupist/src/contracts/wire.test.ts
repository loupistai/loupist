/**
 * Compile-time checks that the hand contracts in this folder still describe the
 * Rust wire types generated into `./generated` (src-tauri/src/bridge_contract.rs).
 * `tsc` checks this file with the app, so a changed Rust type fails the build on the
 * line that names it. Prefer aliasing a hand type to its generated type; keep a hand
 * type only where it narrows Rust (literal unions for a Rust `String`) or carries
 * renderer-only fields, and check it here.
 */
import type * as Wire from "./generated";
import type * as assets from "./assets";
import type * as chat from "./chat";
import type * as config from "./config";
import type * as customConnections from "./customConnections";
import type * as earnings from "./earnings";
import type * as knowledge from "./knowledge";
import type * as lens from "./lens";
import type * as loupist from "./loupist";
import type * as memory from "./memory";
import type * as model from "./model";
import type * as runtime from "./runtime";
import type * as scheduled from "./scheduled";
import type * as sharing from "./sharing";
import type * as skills from "./skills";
import type * as userContext from "./userContext";
import type * as watch from "./watch";
import type * as workspace from "./workspace";

type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends (<T>() => T extends B ? 1 : 2) ? true : false;
type Expect<T extends true> = T;
/** Literals widened to their primitive: a TS narrowing of a Rust `String` still compares. */
type Widen<T> = T extends string ? string : T extends number ? number : T extends boolean ? boolean
  : T extends readonly (infer U)[] ? Widen<U>[] : T extends object ? { [K in keyof T]: Widen<T[K]> } : T;
type KeysOf<T> = T extends unknown ? keyof T : never;
type RequiredKeys<T> = { [K in keyof T]-?: {} extends Pick<T, K> ? never : K }[keyof T];
type RequiredKeysOf<T> = T extends unknown ? RequiredKeys<T> : never;
type Shape<T> = T extends unknown ? { keys: keyof T; required: RequiredKeys<T> } : never;
type Empty<T> = [T] extends [never] ? true : false;

/** TS narrows Rust: every TS value is a Rust value, and every Rust value fits TS once widened. */
type Compatible<H, W> = [H] extends [W] ? ([W] extends [Widen<H>] ? true : false) : false;
/** The same fields, required the same way, per variant; field types have their own entries. */
type SameShape<H, W> = Equal<Shape<H>, Shape<W>>;
/** Requests: everything TS sends deserializes, with no field Rust does not know. */
type Accepts<H, W> = [H] extends [W] ? Empty<Exclude<KeysOf<H>, KeysOf<W>>> : false;
/** Renderer models: TS knows every Rust field and requires none Rust may leave out. */
type Covers<H, W> = Equal<[Empty<Exclude<KeysOf<W>, KeysOf<H>>>, Empty<Exclude<RequiredKeysOf<H>, RequiredKeysOf<W>>>], [true, true]>;
/** Discriminated narrowings: every TS variant is a Rust value, and TS names every Rust field. */
type Narrows<H, W> = [H] extends [W] ? Empty<Exclude<KeysOf<W>, KeysOf<H>>> : false;

/** Hand types that narrow a generated type. */
export type CompatibleContracts = [
  Expect<Compatible<model.AgentHarnessSnapshot, Wire.AgentHarnessSnapshot>>,
  Expect<Compatible<chat.ChartDatasetLineage, Wire.ChartDatasetLineage>>,
  Expect<Compatible<chat.ChatArtifactRef, Wire.ChatArtifactRef>>,
  Expect<Compatible<chat.ChatKnowledgePin, Wire.ChatKnowledgePin>>,
  Expect<Compatible<chat.ChatProgress, Wire.ChatProgress>>,
  Expect<Compatible<chat.ChatProgressItem, Wire.ChatProgressItem>>,
  Expect<Compatible<config.ConfigDocument, Wire.ConfigDocument>>,
  Expect<Compatible<config.ConfigSnapshot, Wire.ConfigSnapshot>>,
  Expect<Compatible<model.CustomModelProvider, Wire.CustomProviderView>>,
  Expect<Compatible<chat.GridDocument, Wire.GridDocument>>,
  Expect<Compatible<userContext.HoldingsCsvPreview, Wire.HoldingsCsvPreview>>,
  Expect<Compatible<chat.HomeHandoff, Wire.HomeHandoff>>,
  Expect<Compatible<chat.HomeVocabulary, Wire.HomeVocabulary>>,
  Expect<Compatible<knowledge.JournalCheck, Wire.JournalCheck>>,
  Expect<Compatible<knowledge.JournalOutcome, Wire.JournalOutcome>>,
  Expect<Compatible<knowledge.KnowledgeMapEdge, Wire.KnowledgeMapEdge>>,
  Expect<Compatible<knowledge.KnowledgeMapNode, Wire.KnowledgeMapNode>>,
  Expect<Compatible<memory.KnowledgeReference, Wire.KnowledgeReference>>,
  Expect<Compatible<lens.LensBodyView, Wire.LensBodyView>>,
  Expect<Compatible<memory.LensReceipt, Wire.LensReceipt>>,
  Expect<Compatible<sharing.LicenseConflict, Wire.LicenseConflict>>,
  Expect<Compatible<assets.LineCrossing, Wire.LineCrossing>>,
  Expect<Compatible<loupist.LoupistSummary, Wire.LoupistSummary>>,
  Expect<Compatible<skills.ManagedSkill, Wire.ManagedSkill>>,
  Expect<Compatible<memory.MemoryRef, Wire.MemoryRefDto>>,
  Expect<Compatible<model.ModelNotice, Wire.ModelNoticeView>>,
  Expect<Compatible<sharing.PackImportView, Wire.PackImportView>>,
  Expect<Compatible<sharing.PackPreview, Wire.PackPreview>>,
  Expect<Compatible<sharing.PackSource, Wire.PackSource>>,
  Expect<Compatible<sharing.PackUpdate, Wire.PackUpdate>>,
  Expect<Compatible<knowledge.Principle, Wire.Principle>>,
  Expect<Compatible<runtime.RunActivity, Wire.RunActivity>>,
  Expect<Compatible<scheduled.RunSummaryItem, Wire.RunSummaryItem>>,
  Expect<Compatible<scheduled.ScheduleReceipt, Wire.ScheduleReceipt>>,
  Expect<Compatible<chat.SensitivityCard, Wire.SensitivityCard>>,
  Expect<Compatible<chat.SensitivityGrid, Wire.SensitivityGrid>>,
  Expect<Compatible<skills.SkillCatalog, Wire.SkillCatalog>>,
  Expect<Compatible<skills.SkillDeleteRequest, Wire.SkillDeleteRequest>>,
  Expect<Compatible<skills.SkillDocument, Wire.SkillDocument>>,
  Expect<Compatible<skills.SkillSaveRequest, Wire.SkillSaveRequest>>,
  Expect<Compatible<workspace.WorkspaceEntry, Wire.WorkspaceEntry>>,
  Expect<Compatible<knowledge.WorldSchemaView, Wire.WorldSchemaView>>,
];

/** Hand types with the generated fields and optionality. */
export type SameShapeContracts = [
  Expect<SameShape<assets.AssetChangeRow, Wire.AssetChangeRow>>,
  Expect<SameShape<assets.AssetDelta, Wire.AssetDelta>>,
  Expect<SameShape<scheduled.BriefingAsset, Wire.BriefingAsset>>,
  Expect<SameShape<scheduled.BriefingContext, Wire.BriefingContext>>,
  Expect<SameShape<scheduled.BriefingEarnings, Wire.BriefingEarnings>>,
  Expect<SameShape<scheduled.BriefingSettings, Wire.BriefingSettings>>,
  Expect<SameShape<chat.ChartDataset, Wire.ChartDataset>>,
  Expect<SameShape<chat.ChatArtifactPayload, Wire.ChatArtifactPayload>>,
  Expect<SameShape<chat.ChatArtifactRevision, Wire.ChatArtifactRevision>>,
  Expect<SameShape<chat.ChatArtifactView, Wire.ChatArtifactViewDto>>,
  Expect<SameShape<chat.ChatControlReceipt, Wire.ChatControlReceiptDto>>,
  Expect<SameShape<chat.ChatProgressPatch, Wire.ChatProgressPatch>>,
  Expect<SameShape<chat.ChatSendRequest, Wire.ChatSendRequest>>,
  Expect<SameShape<chat.ChatStreamEvent, Wire.ChatStreamEvent>>,
  Expect<SameShape<customConnections.CustomConnection, Wire.CustomConnection>>,
  Expect<SameShape<knowledge.GraphClaim, Wire.GraphClaim>>,
  Expect<SameShape<knowledge.GraphEdge, Wire.GraphEdge>>,
  Expect<SameShape<knowledge.GraphNode, Wire.GraphNode>>,
  Expect<SameShape<knowledge.GraphProcessingView, Wire.GraphProcessingView>>,
  Expect<SameShape<knowledge.GrowthEvent, Wire.GrowthEvent>>,
  Expect<SameShape<knowledge.GrowthEventsPage, Wire.GrowthEventsPage>>,
  Expect<SameShape<chat.HomeEarnings, Wire.HomeEarnings>>,
  Expect<SameShape<chat.HomeWatch, Wire.HomeWatch>>,
  Expect<SameShape<scheduled.InboxItem, Wire.InboxItem>>,
  Expect<SameShape<scheduled.InboxPage, Wire.InboxPage>>,
  Expect<SameShape<knowledge.JournalInputs, Wire.JournalInputs>>,
  Expect<SameShape<knowledge.JournalRecord, Wire.JournalRecord>>,
  Expect<SameShape<knowledge.KnowledgeCatalog, Wire.KnowledgeCatalogDto>>,
  Expect<SameShape<knowledge.KnowledgeMap, Wire.KnowledgeMap>>,
  Expect<SameShape<knowledge.KnowledgeResearchPage, Wire.KnowledgeResearchPageDto>>,
  Expect<SameShape<knowledge.LearningReviewFinishedEvent, Wire.LearningReviewFinishedEvent>>,
  Expect<SameShape<chat.LoupistArtifactItem, Wire.LoupistArtifactItemDto>>,
  Expect<SameShape<chat.LoupistWorkspace, Wire.LoupistWorkspace>>,
  Expect<SameShape<memory.MemoryUpdateResult, Wire.MemoryUpdateResult>>,
  Expect<SameShape<model.ModelCatalog, Wire.ModelCatalogView>>,
  Expect<SameShape<scheduled.ScheduledRun, Wire.Run>>,
  Expect<SameShape<scheduled.ScheduledTask, Wire.Task>>,
  Expect<SameShape<scheduled.ScheduledTaskListItem, Wire.TaskListItem>>,
  Expect<SameShape<sharing.ShareSummary, Wire.ShareSummary>>,
  Expect<SameShape<knowledge.SourceDetail, Wire.SourceDetailDto>>,
  Expect<SameShape<workspace.StoredChatDraft, Wire.StoredChatDraft>>,
  Expect<SameShape<knowledge.TimePoint, Wire.TimePoint>>,
  Expect<SameShape<knowledge.WorkingNote, Wire.WorkingNote>>,
  Expect<SameShape<knowledge.WorkingNotesPage, Wire.WorkingNotesPage>>,
  Expect<SameShape<workspace.WorkspaceDirectory, Wire.WorkspaceDirectory>>,
  Expect<SameShape<workspace.WorkspaceFile, Wire.WorkspaceFile>>,
  Expect<SameShape<workspace.WorkspaceUiState, Wire.WorkspaceUiState>>,
  Expect<SameShape<knowledge.WorldGraphView, Wire.WorldQueryDto>>,
];

/** Hand request types Rust deserializes. */
export type AcceptsContracts = [
  Expect<Accepts<assets.AssetClassSetRequest, Wire.AssetClassSetRequest>>,
  Expect<Accepts<assets.AssetDeltaRequest, Wire.AssetDeltaRequest>>,
  Expect<Accepts<assets.AssetPageGetRequest, Wire.AssetPageGetRequest>>,
  Expect<Accepts<assets.AssetRefreshRequest, Wire.AssetRefreshRequest>>,
  Expect<Accepts<assets.AssetVerdictUndoRequest, Wire.AssetVerdictUndoRequest>>,
  Expect<Accepts<chat.ChatBranchRequest, Wire.ChatBranchRequest>>,
  Expect<Accepts<chat.ChatCreateRequest, Wire.ChatCreateRequest>>,
  Expect<Accepts<chat.ChatFeedbackRequest, Wire.ChatFeedbackRequest>>,
  Expect<Accepts<config.ConfigUpdateRequest, Wire.ConfigUpdateRequest>>,
  Expect<Accepts<model.CustomProviderRequest, Wire.CustomProviderRequest>>,
  Expect<Accepts<earnings.EarningsSnapshotGetRequest, Wire.EarningsSnapshotGetRequest>>,
  Expect<Accepts<sharing.GalleryListingRequest, Wire.GalleryListingRequest>>,
  Expect<Accepts<knowledge.GrowthEventsQuery, Wire.GrowthEventsQuery>>,
  Expect<Accepts<knowledge.JournalCreateRequest, Wire.JournalCreateRequest>>,
  Expect<Accepts<knowledge.JournalListRequest, Wire.JournalListRequest>>,
  Expect<Accepts<knowledge.JournalUpdateRequest, Wire.JournalUpdateRequest>>,
  Expect<Accepts<knowledge.KnowledgeLensReadRequest, Wire.KnowledgeLensReadRequest>>,
  Expect<Accepts<knowledge.KnowledgeMapQuery, Wire.KnowledgeMapQuery>>,
  Expect<Accepts<knowledge.KnowledgeResearchSearchRequest, Wire.KnowledgeResearchSearchRequest>>,
  Expect<Accepts<knowledge.KnowledgeSourceReadRequest, Wire.KnowledgeSourceReadRequest>>,
  Expect<Accepts<knowledge.KnowledgeSourceRetainRequest, Wire.KnowledgeSourceRetainRequest>>,
  Expect<Accepts<loupist.LoupistCreateRequest, Wire.LoupistCreateRequest>>,
  Expect<Accepts<loupist.LoupistProfileUpdateRequest, Wire.LoupistProfileUpdateRequest>>,
  Expect<Accepts<knowledge.PrincipleCreateRequest, Wire.PrincipleCreateRequest>>,
  Expect<Accepts<knowledge.PrincipleListRequest, Wire.PrincipleListRequest>>,
  Expect<Accepts<knowledge.PrincipleUpdateRequest, Wire.PrincipleUpdateRequest>>,
  Expect<Accepts<model.ProviderConfigureRequest, Wire.ProviderConfigureRequest>>,
  Expect<Accepts<model.ProviderCustomTestRequest, Wire.ProviderCustomTestRequest>>,
  Expect<Accepts<model.ProviderPresetRequest, Wire.ProviderPresetRequest>>,
  Expect<Accepts<chat.ResearchHandoffRequest, Wire.ResearchHandoffRequest>>,
  Expect<Accepts<sharing.ShareExportRequest, Wire.ShareExportRequest>>,
  Expect<Accepts<sharing.SharePublishRequest, Wire.SharePublishRequest>>,
  Expect<Accepts<sharing.ShareRequest, Wire.ShareRequest>>,
  Expect<Accepts<skills.SkillImportRequest, Wire.SkillImportRequest>>,
  Expect<Accepts<assets.UpcomingEvent, Wire.UpcomingEvent>>,
  Expect<Accepts<watch.WatchEventsListRequest, Wire.WatchEventsListRequest>>,
  Expect<Accepts<workspace.WorkspacePathRequest, Wire.WorkspacePathRequest>>,
  Expect<Accepts<workspace.WorkspaceUiTab, Wire.WorkspaceUiTab>>,
  Expect<Accepts<knowledge.WorldGraphQuery, Wire.WorldGraphQuery>>,
];

/** Renderer models built from a wire type. */
export type CoversContracts = [
  Expect<Covers<chat.ChatThread, Wire.ChatThreadDto>>,
  Expect<Covers<chat.ChatMessage, Wire.ChatMessageDto>>,
  Expect<Covers<model.ModelProviderOption, Wire.ModelProviderOption>>,
];

/**
 * Discriminated hand types. Rust serializes `MemoryUpdateChange` by hand (a
 * principle adds `statement`, an absent option is left out), which ts-rs does not
 * see: the generated type is its `Deserialize` shape.
 */
export type NarrowsContracts = [
  Expect<Narrows<memory.MemoryUpdateChange, Wire.MemoryUpdateChange>>,
];

it("keeps the hand contracts on the generated Rust wire types (checked by tsc)", () => {
  expect(true).toBe(true);
});
