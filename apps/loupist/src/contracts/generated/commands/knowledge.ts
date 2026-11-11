// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { SourcePresentationDto } from "../commands/knowledge/source_presentation";
import type { CustomOrigin } from "../custom_connections/taint";
import type { SourceProcessing } from "../knowledge";
import type { GraphPublicSettings } from "../knowledge/graph_jobs";
import type { LensKind } from "../knowledge/lens_page";
import type { GraphCoverage, GraphEdge, GraphNode, IndexingState, JournalSourceInput, QueryScan, RecordKind, TimePoint } from "../knowledge/model";
import type { SourceWorldClaim, WorldSourceTarget } from "../knowledge/world_source";

export type JournalSourceReadDto = {
  mediaType: string,
  byteLength: number,
  text: string | null,
  pinned: JournalSourceInput,
};

export type KnowledgeCatalogDto = {
  loupistId: string,
  worlds: Array<KnowledgeRecordSummaryDto>,
  lenses: Array<KnowledgeRecordSummaryDto>,
  sources: Array<KnowledgeRecordSummaryDto>,
};

export type KnowledgeCatalogRequest = { loupistId: string };

export type KnowledgeConnectionRequest = { loupistId: string, connectionId: string };

export type KnowledgeConnectionsRequest = { loupistId: string };

export type KnowledgeGraphJobRequest = { loupistId: string, jobId: string };

export type KnowledgeGraphLoupistRequest = { loupistId: string };

export type KnowledgeGraphPauseRequest = { loupistId: string, paused: boolean };

export type KnowledgeGraphSettingsRequest = { loupistId: string, settings: GraphPublicSettings };

export type KnowledgeJournalReadRequest = { loupistId: string, journalId: string };

export type KnowledgeJournalSourceReadRequest = {
  loupistId: string,
  journalId: string,
  receiptId: string,
};

export type KnowledgeLensPagesForNodeRequest = { loupistId: string, nodeId: string };

export type KnowledgeLensReadRequest = {
  loupistId: string,
  recordId: string,
  revisionId?: string | null,
};

export type KnowledgeLensSearchRequest = {
  loupistId: string,
  query: string,
  kinds?: Array<LensKind>,
};

export type KnowledgeObjectRefDto = {
  origin?: CustomOrigin,
  summary: string | null,
  originLabel: string | null,
  acquiredAt: TimePoint | null,
  publicationTime: TimePoint | null,
  processing: SourceProcessing | null,
  id: string,
  title: string,
  /** A Source the host wrote keeps an English title; the interface shows this key. */
  titleKey?: string,
};

export type KnowledgePinDto = {
  kind: string,
  loupistId: string,
  recordId: string | null,
  revisionId: string | null,
  objectId: string,
  spanId: string | null,
  label: string,
};

export type KnowledgeRecordSummaryDto = {
  titleKey?: string,
  summary?: string,
  lensKind: LensKind | null,
  recordId: string,
  kind: RecordKind,
  title: string,
  revisionId: string,
  revisionDigest: string,
  learned: boolean,
  readOnly: boolean,
  indexingState: string,
  objects: Array<KnowledgeObjectRefDto>,
  updatedAtMs?: number,
  archived?: boolean,
};

export type KnowledgeResearchPageDto = { loupistId: string, records: Array<ResearchRecordDto> };

export type KnowledgeResearchSearchRequest = {
  loupistId: string,
  query?: string | null,
  claimId?: string | null,
};

export type KnowledgeSourceFile = { mediaType: string, fileName: string, bytesBase64: string };

export type KnowledgeSourceReadRequest = {
  loupistId: string,
  recordId: string,
  revisionId: string,
  sourceId?: string | null,
  spanId?: string | null,
  purpose: string,
  worldRecordId?: string | null,
  worldRevisionId?: string | null,
};

export type KnowledgeSourceRetainRequest = {
  loupistId: string,
  title: string,
  originLocator: string,
  coverage: string,
  publicationTime?: TimePoint | null,
  text: string,
  file?: KnowledgeSourceFile | null,
};

export type LensSearchHitDto = {
  recordId: string,
  revisionId: string,
  kind: LensKind,
  title: string,
  summary: string,
  snippet: string,
};

export type MemoryLearnedRequest = { loupistId: string, messageIds: Array<string> };

export type MemoryOpUndoRequest = { loupistId: string, opId: string };

export type MemorySectionForgetRequest = { loupistId: string, recordId: string, key: string };

export type ResearchJournalDto = {
  sectionId: string,
  conclusion: string,
  uncertainty: string | null,
};

export type ResearchRecordDto = {
  id: string,
  chatId: string | null,
  title: string,
  summary: string | null,
  recordedAt: string,
  journal: ResearchJournalDto | null,
  inputs: Array<KnowledgePinDto>,
};

export type SourceDetailDto = {
  origin?: CustomOrigin,
  relatedClaims: Array<SourceWorldClaim>,
  processing: SourceProcessing | null,
  sourceId: string,
  recordId: string,
  revisionId: string,
  title: string,
  titleKey?: string,
  sourceClass: string,
  originLabel: string,
  coverage: string,
  material: string,
  verification: string,
  derivation: string | null,
  spans: Array<SourceSpanViewDto>,
  selectedSpanId: string | null,
  publicationTime: TimePoint | null,
  acquiredAt: TimePoint | null,
};

export type SourceSpanViewDto = {
  spanId: string,
  selector: string,
  contextNote: string,
  mediaType: string,
  text: string | null,
  presentation?: SourcePresentationDto,
};

export type WorldQueryDto = {
  originLabel: string,
  appliedCursor: string | null,
  sourceTargets: { [key in string]: WorldSourceTarget },
  recordId: string,
  revisionId: string,
  revisionDigest: string,
  nodes: Array<GraphNode>,
  edges: Array<GraphEdge>,
  nextCursor: string | null,
  coverage: GraphCoverage,
  indexingState: IndexingState,
  truncated: boolean,
  scan: QueryScan,
};
