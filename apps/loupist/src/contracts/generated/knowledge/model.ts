// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../external";
import type { JournalThesis } from "../knowledge/assets";

export type BindingRole = "extracted_from" | "supports" | "opposes" | "context" | "analysis";

export type ClaimEndpoint = { nodeId: string, role: string };

export type ClaimStatus = "active" | "disputed" | "withdrawn";

export type EarlierRelation = "review" | "supersedes" | "contradicts";

export type EarlierResearch = {
  researchId: string,
  journalId: string | null,
  relation: EarlierRelation,
  /** Exact text knowledge.read returned for this earlier journal. */
  deliveredText: string,
  deliveredDigest: string,
  coverage: string,
};

export type EvidenceBinding = { spanId: string, role: BindingRole, attribution: string };

export type GraphClaim = {
  id: string,
  proposition: string,
  attribution: string,
  relation: string,
  endpoints: Array<ClaimEndpoint>,
  qualifiers: Array<Qualifier>,
  evidence: Array<EvidenceBinding>,
  status: ClaimStatus,
  recordedAtMs: number | null,
  publicationTime: TimePoint | null,
  effectiveTime: TimePoint | null,
  /** Fact validity start. Independent of claim status and of `expired_at`. */
  validAt: TimePoint | null,
  /** Fact validity end. Independent of claim status and of `expired_at`. */
  invalidAt: TimePoint | null,
  /** Graphiti system-time expiry. Independent of fact validity and claim status. */
  expiredAt: TimePoint | null,
};

export type GraphCoverage = "sealed_snapshot";

export type GraphEdge = {
  id: string,
  relation: string,
  endpoints: Array<ClaimEndpoint>,
  status: ClaimStatus,
  claim: GraphClaim,
};

export type GraphNode = {
  id: string,
  name: string,
  aliases: Array<string>,
  description: string,
  kind: string,
  sourceRole: string,
  properties: { [key in string]: JsonValue },
};

export type IndexingState = "snapshot_only";

export type JournalCheck = {
  claim: string,
  metric: string,
  direction: string,
  threshold: number,
  unit?: string,
  /** Required for a dated check; optional only for an asset page's line. */
  byDate?: string,
  confidence?: number,
};

export type JournalInputs = {
  sources: Array<JournalSourceInput>,
  worlds: Array<JournalWorldInput>,
  lenses: Array<JournalLensInput>,
  earlier: Array<EarlierResearch>,
};

export type JournalLensInput = {
  deliveryId: string,
  recordId: string,
  revisionId: string,
  revisionDigest: string,
  bodyDigest: string,
  markdown: string,
  status: LensUseStatus,
};

export type JournalOutcome = {
  observed: number,
  observedAt: string,
  result: string,
  note?: string,
  /** Set when the review ran under an as-of date ("Reviewed as of …"). */
  reviewedAsOf?: string,
};

export type JournalRecord = {
  check?: JournalCheck,
  /** A reason for following an asset, or a line that would change the user's mind. */
  thesis?: JournalThesis,
  outcome?: JournalOutcome,
  journalId: string,
  researchId: string,
  loupistId: string,
  judgment: string,
  reasoning: string,
  inputs: JournalInputs,
  inputDigest: string,
  createdAtMs: number,
  entities: Array<string>,
  originChatId: string | null,
  updatedAtMs: number,
  archived: boolean,
};

export type JournalSourceInput = {
  captureId: string,
  representationId: string,
  spanId: string,
  selectedDigest: string,
  receiptId: string,
  recordId: string,
  revisionId: string,
  revisionDigest: string,
  attribution: string,
  coverage: string,
  publicationTime?: TimePoint,
};

export type JournalWorldInput = {
  deliveredViewJson: string,
  deliveryId: string,
  recordId: string,
  revisionId: string,
  revisionDigest: string,
  viewDigest: string,
  nodes: Array<GraphNode>,
  edges: Array<GraphEdge>,
};

/** Discovery pointers to later committed notes, not evidence or read authority. */
export type LaterNote = { noteId: string, chatId: string, title: string, updatedAtMs: number };

export type LensUseStatus = "considered" | "applied" | "adapted" | "rejected";

export type Qualifier = { key: string, value: string };

export type QueryScan = { strategy: string, nodesScanned: number, claimsScanned: number };

export type RecordKind = "source" | "world" | "lens";

export type RetainedSource = {
  recordId: string,
  sourceId: string,
  captureId: string,
  representationId: string | null,
  spanId: string | null,
  selectedDigest: string | null,
  materialDigest: string | null,
  revisionId: string,
  revisionDigest: string,
};

export type TimePoint = { precision: TimePrecision, value?: string | null, approximate?: boolean };

export type TimePrecision = "unknown" | "year" | "month" | "date" | "instant";

export type WorkingNote = {
  laterNotes?: Array<LaterNote>,
  inputs?: JournalInputs,
  previousNoteIds: Array<string>,
  researchId: string,
  chatId: string,
  title: string,
  markdown: string,
  changeSummary: string,
  recordedAtMs: number,
};

export type WorldGraphQuery = {
  loupistId: string,
  recordId: string,
  revisionId: string,
  search?: string,
  centerNodeId?: string | null,
  centerClaimId?: string | null,
  kinds?: Array<string>,
  relations?: Array<string>,
  bindingRoles?: Array<BindingRole>,
  statuses?: Array<ClaimStatus>,
  publicationTimes?: Array<string>,
  /**
   * An as-of cutoff (`YYYY-MM-DD`): only relationships published by then,
   * and only the names they connect, reduced to name and kind.
   */
  publishedBy?: string,
  cursor?: string | null,
  nodeLimit?: number | null,
  edgeLimit?: number | null,
  byteLimit?: number | null,
};
