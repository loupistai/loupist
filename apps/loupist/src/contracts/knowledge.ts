import type * as Wire from "./generated";
/**
 * Renderer mirror of `loupist_desktop::knowledge` schema 1.
 * Struct fields are camelCase. Enum values are snake_case, matching serde.
 * `appliedCursor` and `originLabel` are presentation-only and are not sent
 * back to Rust.
 */

export type KnowledgeRecordKind = "source" | "world" | "lens";

export type IndexingState = Wire.IndexingState;

export type ClaimStatus = Wire.ClaimStatus;

export type BindingRole = Wire.BindingRole;

export type TimePrecision = Wire.TimePrecision;

export type TimePoint = {
  precision: TimePrecision;
  value?: string;
  approximate?: boolean;
};

export type ClaimEndpoint = Wire.ClaimEndpoint;

export type EvidenceBinding = Wire.EvidenceBinding;

export type SourceClass =
  | "publisher_document"
  | "dataset"
  | "user_statement"
  | "tool_result"
  | "analysis"
  | "imported_contributor"
  | "code_revision"
  | "web_page"
  | "historical_document"
  | "user_file";

export type Coverage =
  | "included_full"
  | "included_excerpt"
  | "upstream_truncated"
  | "metadata_only"
  | "omitted";

export type RepresentationRole =
  | "original"
  | "provider_payload"
  | "extracted_text"
  | "structured"
  | "excerpt"
  | "derived";

export type SourceProcessing = Wire.SourceProcessing;

export type CustomConnectionOrigin = { kind: "custom_connection"; connectionId: string; name: string };

/**
 * A Source the host wrote keeps its English title for the World; the interface
 * shows its title and origin through this key instead (`knowledge:hostSource.*`).
 */
export type SourceTitleKey = "followed";

export type KnowledgeObjectRef = {
  origin?: CustomConnectionOrigin;
  summary?: string;
  originLabel?: string;
  acquiredAt?: TimePoint;
  publicationTime?: TimePoint;
  processing?: SourceProcessing;
  id: string;
  title: string;
  titleKey?: SourceTitleKey;
};

export type KnowledgeRecordSummary = {
  summary?: string;
  lensKind?: import("./lens").LensKind | null;
  recordId: string;
  kind: KnowledgeRecordKind;
  title: string;
  titleKey?: SourceTitleKey;
  revisionId: string;
  revisionDigest: string;
  learned?: boolean;
  readOnly: boolean;
  indexingState: IndexingState;
  objects: KnowledgeObjectRef[];
  updatedAtMs?: number;
  archived?: boolean;
};

export type KnowledgeCatalogRequest = Wire.KnowledgeCatalogRequest;

export type KnowledgeCatalog = {
  loupistId: string;
  worlds: KnowledgeRecordSummary[];
  lenses: KnowledgeRecordSummary[];
  sources: KnowledgeRecordSummary[];
};

/** Host `WorldGraphQuery`. */
export type WorldGraphQuery = {
  loupistId: string;
  recordId: string;
  revisionId: string;
  search: string;
  centerNodeId?: string;
  centerClaimId?: string;
  kinds: string[];
  relations: string[];
  bindingRoles: BindingRole[];
  statuses: ClaimStatus[];
  publicationTimes?: string[];
  cursor?: string;
  nodeLimit: number;
  edgeLimit: number;
  byteLimit: number;
};

export type GraphNode = {
  id: string;
  name: string;
  aliases: string[];
  description: string;
  kind: string;
  sourceRole: string;
  properties: Record<string, string>;
};

export type GraphClaim = {
  id: string;
  proposition: string;
  attribution: string;
  relation: string;
  endpoints: ClaimEndpoint[];
  qualifiers: { key: string; value: string }[];
  evidence: EvidenceBinding[];
  status: ClaimStatus;
  recordedAtMs: number | null;
  publicationTime: TimePoint | null;
  effectiveTime: TimePoint | null;
  validAt: TimePoint | null;
  invalidAt: TimePoint | null;
  expiredAt: TimePoint | null;
};

export type GraphEdge = {
  id: string;
  relation: string;
  endpoints: ClaimEndpoint[];
  status: ClaimStatus;
  claim: GraphClaim;
};

export type WorldSourceTarget = Wire.WorldSourceTarget;

/** Host `WorldGraphView` with the query's presentation fields (`knowledge_world_query`). */
export type WorldGraphView = {
  recordId: string;
  revisionId: string;
  revisionDigest: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  nextCursor: string | null;
  coverage: "sealed_snapshot";
  indexingState: IndexingState;
  truncated: boolean;
  appliedCursor: string | null;
  originLabel: string;
  sourceTargets: Record<string, WorldSourceTarget>;
  scan: Wire.QueryScan;
};

export type KnowledgeSourceReadRequest = {
  loupistId: string;
  recordId: string;
  revisionId: string;
  sourceId?: string;
  spanId?: string;
  worldRecordId?: string;
  worldRevisionId?: string;
  purpose: "renderer_inspection";
};

/** Selection offsets address the displayed retained passage in DOM UTF-16 units. */
export type KnowledgeSourceSelectionRequest = {
  source: KnowledgeSourceReadRequest;
  selection: SourceTextSelection;
};

export type SourceTextSelection = Wire.SourceTextSelection;

export type SourceSpanView = {
  spanId: string;
  selector: string;
  contextNote: string;
  mediaType: string;
  text: string | null;
  /** Readable field projection only; text remains the exact retained span. */
  presentation?: {
    text: string;
    sourcePointer: "/content";
    extractionMethod: string;
    contentOffset: number;
    nextOffset: number | null;
    extractionTruncated: boolean;
  };
};

export type SourceRelatedClaim = Wire.SourceWorldClaim;

export type SourceDetail = {
  origin?: CustomConnectionOrigin;
  relatedClaims: SourceRelatedClaim[];
  processing: SourceProcessing | null;
  sourceId: string;
  recordId: string;
  revisionId: string;
  title: string;
  titleKey?: SourceTitleKey;
  sourceClass: SourceClass;
  originLabel: string;
  coverage: Coverage;
  material: RepresentationRole;
  verification: "verified_bytes" | "declared_digest" | "unresolved";
  derivation: string | null;
  spans: SourceSpanView[];
  selectedSpanId: string | null;
  publicationTime: TimePoint | null;
  acquiredAt: TimePoint | null;
};

export type KnowledgeLensReadRequest = {
  loupistId: string;
  recordId: string;
  revisionId?: string;
};

export type KnowledgePin = {
  selection?: SourceTextSelection;
  kind: "source" | "world_node" | "world_claim" | "lens" | "journal" | "research";
  loupistId: string;
  recordId?: string;
  revisionId?: string;
  objectId: string;
  spanId?: string;
  label: string;
};

export type ResearchRecord = {
  id: string;
  chatId?: string | null;
  title: string;
  recordedAt: string;
  summary?: string;
  journal?: {
    sectionId: string;
    conclusion: string;
    uncertainty?: string;
  };
  inputs: KnowledgePin[];
};

export type KnowledgeResearchSearchRequest = {
  loupistId: string;
  query?: string;
  claimId?: string;
};

export type KnowledgeResearchPage = {
  loupistId: string;
  records: ResearchRecord[];
};

export type WorkingNote = {
  /** Later committed notes on the same research: discovery pointers, not evidence. */
  laterNotes?: Wire.LaterNote[];
  inputs?: JournalInputs;
  previousNoteIds: string[];
  researchId: string;
  chatId: string;
  title: string;
  markdown: string;
  changeSummary: string;
  recordedAtMs: number;
};

export type WorkingNoteReadRequest = Wire.WorkingNoteReadRequest;

export type WorkingNotesRequest = Wire.WorkingNotesRequest;

export type WorkingNotesPage = {
  current: WorkingNote | null;
};

export type RetainedSource = Wire.RetainedSource;

export type KnowledgeSourceFile = Wire.KnowledgeSourceFile;

export type KnowledgeSourceRetainRequest = {
  loupistId: string;
  title: string;
  originLocator: string;
  coverage: "included_full" | "included_excerpt" | "metadata_only";
  publicationTime?: TimePoint;
  text: string;
  file?: KnowledgeSourceFile;
};

/** One World processing job; the renderer never reads the job's stored document. */
export type GraphJobView = Wire.GraphJobView;

export type GraphProcessingSettings = {
  enrichmentEnabled: boolean;
  paused: boolean;
  helperModelProfileId: string | null;
  thinkingLevel: string;
  runOptions: Record<string, string>;
  maxLlmCalls: number;
  timeoutMs: number;
  researchFocus?: string;
  relationHints?: string[];
  anchors?: { name: string; aliases?: string[]; identifiers?: string[] }[];
  fastModel?: string | null;
};

/** Display names. `en` is required; `ko` is omitted when unset. */
export type SchemaLabels = { en: string; ko?: string };
export type SchemaCategory = "company" | "person" | "product" | "market" | "event" | "other";
export type ExtensionField = Wire.ExtensionField;
export type ExtensionType = Wire.ExtensionType;
export type ExtensionRelation = Wire.ExtensionRelation;
export type WorldExtension = Wire.WorldExtension;
export type SchemaChangeProducer = "preset" | "chat" | "review" | "undo";
export type SchemaChange = {
  changeId: string;
  producer: SchemaChangeProducer;
  summary: string;
  addedTypes: string[];
  addedRelations: string[];
  removedTypes: string[];
  removedRelations: string[];
  /** Display labels for every name above, so a removed kind still reads in plain words. */
  kindLabels: Array<{ name: string; labels: SchemaLabels }>;
  createdAtMs: number;
  undoable: boolean;
};
/** What the map tracks beyond the basics, newest change first (at most 20). */
export type WorldSchemaView = { extension: WorldExtension; changes: SchemaChange[] };
export type SchemaRemovalKind = "type" | "relation";

/**
 * Where reading what the user follows stands for one Loupist (Rust `FollowedStatus`):
 * `none` nothing followed was written; `off` World processing is off or paused;
 * `waiting` it cannot read now (no model, an account to reconnect); `failed` a read
 * stopped and will not retry by itself.
 */
export type FollowedState = Wire.FollowedState;

export type FollowedStatus = Wire.FollowedStatus;

/** Why World processing is not ready, by code; the interface words it. */
export type GraphReadinessNote =
  | "recovery_required" | "recovering" | "runtime_unavailable" | "awaiting_provider" | "choose_model" | "model_unavailable" | "connect_provider"
  | "isolation_unsupported" | "runtime_missing" | "choose_connected_model" | "not_connected" | "account_waiting";

/**
 * Whether World processing keeps this computer awake (Rust `KeepAwakeStatus`).
 * One choice for every Loupist; it holds only while plugged in and work remains.
 */
export type KeepAwakeStatus = Wire.KeepAwakeStatus;
export type WorldKeepAwakeRequest = Wire.WorldKeepAwakeRequest;

export type GraphProcessingView = {
  settings: GraphProcessingSettings;
  readiness: {
    status: "ready" | "disabled" | "paused" | "model_required" | "provider_disconnected" | "runtime_unavailable" | "model_unavailable" | "reconciliation_required" | "recovering";
    note?: GraphReadinessNote | null;
    providerName?: string | null;
    modelName?: string | null;
    authKind?: string | null;
  };
};

export type KnowledgeJournalReadRequest = Wire.KnowledgeJournalReadRequest;

export type KnowledgeJournalSourceReadRequest = Wire.KnowledgeJournalSourceReadRequest;

export type JournalSourceRead = {
  mediaType: string;
  byteLength: number;
  text?: string;
  pinned: {
    receiptId: string;
    recordId: string;
    revisionId: string;
    spanId: string;
    coverage: string;
    attribution: string;
    publicationTime?: TimePoint;
  };
};

export type JournalInputs = {
  sources: JournalSourceRead["pinned"][];
  worlds: {
    recordId: string;
    revisionId: string;
    nodes: GraphNode[];
    edges: GraphEdge[];
  }[];
  lenses: {
    recordId: string;
    revisionId: string;
    /** The page as delivered, rendered to Markdown with section keys. */
    markdown: string;
    status: string;
  }[];
  earlier: {
    researchId: string;
    journalId: string | null;
    relation: "review" | "supersedes" | "contradicts";
    deliveredText: string;
    deliveredDigest: string;
    coverage: string;
  }[];
};

export type JournalCheck = {
  claim: string;
  metric: string;
  direction: "above" | "below";
  threshold: number;
  unit?: string;
  /** Required except on an asset page's line. */
  byDate?: string;
  confidence?: number;
};
/** A reason for following an asset, or a line that would change the user's mind. */
export type JournalThesis = Wire.JournalThesis;
export type JournalOutcome = {
  observed: number;
  observedAt: string;
  result: "came_true" | "did_not" | "unclear";
  note?: string;
  /** Set when the review ran under an as-of date. */
  reviewedAsOf?: string;
};
export type JournalRecord = {
  journalId: string; researchId: string; loupistId: string;
  judgment: string; reasoning: string; entities: string[];
  check?: JournalCheck; outcome?: JournalOutcome; thesis?: JournalThesis;
  originChatId: string | null; createdAtMs: number; updatedAtMs: number; archived: boolean;
  inputs: JournalInputs; inputDigest: string;
};
export type JournalInput = { judgment: string; reasoning?: string; entities?: string[]; check?: JournalCheck | null; thesis?: JournalThesis | null };
export type JournalListRequest = { loupistId: string; includeArchived?: boolean };
export type JournalCreateRequest = JournalInput & { loupistId: string };
export type JournalUpdateRequest = JournalInput & { loupistId: string; journalId: string; expectedUpdatedAtMs: number };
export type JournalArchiveRequest = Wire.JournalArchiveRequest;

export type GraphCamera = {
  x: number;
  y: number;
  scale: number;
};

export type KnowledgeSelection =
  | { kind: "node"; id: string }
  | { kind: "claim"; id: string }
  | { kind: "source"; sourceId?: string; spanId?: string; worldRecordId?: string; worldRevisionId?: string }
  | { kind: "lens"; recordId: string }
  | { kind: "research"; recordId: string; sectionId?: string }
  | { kind: "journal"; journalId: string }
  | { kind: "snapshot"; pin: KnowledgePin };

export type ConnectionHop = { claimId: string; relation: string; fromRole: string; toRole: string; reversed: boolean; nodeId: string; name: string };
export type ConnectionStatus = Wire.ConnectionStatus;
export type ConnectionView = Wire.ConnectionView;

export type KnowledgeViewState = {
  highlightEntityIds?: string[];
  onlyDisputed?: boolean;
  followCurrent?: boolean;
  sourceFilter?: "all" | "processing" | "attention";
  sourceScrollTop?: number;
  readerScrollTop?: number;
  researchLibraryScrollTop?: number;
  journalNew?: boolean;
  journalCheckOpen?: boolean;
  focusResearchId?: string;
  focusSourceId?: string;
  inspectorCollapsed?: boolean;
  inspectorScrollTop?: number;
  namesScrollTop?: number;
  camera?: GraphCamera;
  search?: string;
  kinds?: string[];
  relations?: string[];
  bindingRoles?: BindingRole[];
  statuses?: ClaimStatus[];
  publicationTimes?: string[];
  findInView?: string;
  claimId?: string;
  listMode?: boolean;
  /** Open the Map's What's new drawer since the last visit, then clear it. */
  whatsNew?: boolean;
};

export type KnowledgeSurface = "home" | "world" | "lens" | "source" | "note";

export type KnowledgeLocation = {
  loupistId: string;
  surface: KnowledgeSurface;
  recordId?: string;
  revisionId?: string;
  objectId?: string;
  selection?: KnowledgeSelection;
  view?: KnowledgeViewState;
};

/** Committed Memory changes; ordered by (atMs, eventId), oldest first. */
export type GrowthEventKind =
  | "entity_added" | "claim_added" | "claim_disputed" | "claim_withdrawn"
  | "lens_created" | "lens_revised" | "lens_renamed" | "lens_archived" | "lens_restored"
  /** A World publication disputed, withdrew or dropped a claim a page section cites. */
  | "lens_contested"
  | "source_retained" | "schema_extended" | "schema_reduced" | "growth_truncated"
  /** An asset page's read moved: summary is the asset's name, before/after the steps. */
  | "thesis_moved";

export type GrowthEvent = {
  rationale?: string;
  eventId: string;
  loupistId: string;
  atMs: number;
  kind: GrowthEventKind;
  recordId: string | null;
  subjectId: string | null;
  originChatId: string | null;
  originMessageId: string | null;
  actor: "user" | "loupist" | "import" | "processing";
  origin?: PrincipleOrigin | { kind: "learning_review"; reviewId: string } | { kind: "memory_reset"; counts: MemoryResetCounts } | { labels: SchemaLabels };
  summary: string;
  beforeSummary: string | null;
  afterSummary: string | null;
  /** Set when a `source_retained` summary is a host-written Source's stored English title. */
  titleKey?: SourceTitleKey;
};

export type GrowthEventsQuery = {
  loupistId: string;
  sinceMs?: number;
  untilMs?: number;
  /** 1–5000; defaults to 500. Time bounds are inclusive. */
  limit?: number;
  cursor?: string;
};

export type GrowthEventsPage = {
  events: GrowthEvent[];
  nextCursor: string | null;
};

export type KnowledgeMapNodeKind = "entity" | "lens_perspective" | "lens_principle" | "lens_method" | "lens_subject" | "lens_idea" | "journal";
export type KnowledgeMapEdgeKind = "claim" | "lens_link" | "applies_to" | "about";
export type KnowledgeMapQuery = { loupistId: string; maxNodes?: number };
export type KnowledgeMapNode = {
  origin?: CustomConnectionOrigin;
  learnedAtMs?: number;
  learnedRationale?: string;
  id: string;
  kind: KnowledgeMapNodeKind;
  label: string;
  /** Host-selected original-language aliases. Omitted when empty. */
  localNames?: string[];
  entityKind?: string;
  /** Map colour for an extension type. Omitted for core kinds. */
  categoryOverride?: SchemaCategory;
  /** Extension type display names. Omitted for core kinds. */
  kindLabels?: SchemaLabels;
  degree: number;
  firstSeenAtMs?: number;
  recordId?: string;
  archived?: boolean;
  watched: boolean;
  held: boolean;
};
export type KnowledgeMapEdge = {
  id: string;
  kind: KnowledgeMapEdgeKind;
  source: string;
  target: string;
  relation?: string;
  status?: ClaimStatus;
  weight?: number;
  firstSeenAtMs?: number;
};
export type KnowledgeMapItem = { symbol: string; market: string; displayName: string | null };
export type KnowledgeMapCount = Wire.KnowledgeMapCount;
export type KnowledgeMap = {
  nodes: KnowledgeMapNode[];
  edges: KnowledgeMapEdge[];
  unmatchedHoldings: KnowledgeMapItem[];
  unmatchedWatchlist: KnowledgeMapItem[];
  /** Counts of projected nodes/edges; multi-endpoint claims are projected as stars. */
  counts: Record<KnowledgeMapNodeKind | KnowledgeMapEdgeKind, KnowledgeMapCount>;
  worldRevisionId: string | null;
  /** Latest contributing data timestamp; stable when data is unchanged. */
  generatedAtMs: number;
};

export type PrincipleOrigin = "settings" | "feedback" | "chat" | "shared";
export type Principle = {
  recordId: string; revisionId: string; loupistId: string;
  statement: string; why?: string; example?: string;
  origin: PrincipleOrigin; originChatId?: string; originMessageId?: string;
  createdAtMs: number; updatedAtMs: number; archived: boolean;
};
export type PrincipleListRequest = { loupistId: string; includeArchived?: boolean };
export type PrincipleCreateRequest = { loupistId: string; statement: string; why?: string; example?: string;
  origin: "settings" | "feedback"; originChatId?: string; originMessageId?: string };
export type PrincipleUpdateRequest = { loupistId: string; recordId: string; expectedRevisionId: string;
  statement?: string; why?: string; example?: string };
export type PrincipleArchiveRequest = Wire.PrincipleArchiveRequest;

export type LearningReview = {
  reviewId: string; status: "created" | "refined" | "deferred" | "failed";
  reason?: string | null; lensRecordId?: string | null; rationale?: string | null;
  evidenceCount: number; createdAtMs: number;
};
export type LearningReviewQuery = { loupistId: string; limit?: number };
export type MemoryLearningSettings = { learnMethods: boolean };
/** Turns that read or applied a principle or method, and the chats they were in (newest first, at most five). */
export type MemoryUsage = Wire.MemoryUsage;
export type LearningReviewFinishedEvent = { loupistId: string; review: LearningReview };

export type MemoryControls = Wire.MemoryControls;
export type MemoryResetCounts = { principles: number; pages: number; sections: number; names: number; relationships: number; sources: number };
