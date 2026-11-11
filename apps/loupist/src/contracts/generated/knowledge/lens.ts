// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { Basis, BasisChip, IdeaState, LensKind, Origin, SectionKey } from "../knowledge/lens_page";

export type AnchorRole = "subject" | "mention" | "applied";

export type AnchorView = { role: AnchorRole, nodeId: string | null, name: string };

export type LearnedDigest = { messageId: string, pages: Array<LearnedPage> };

export type LearnedOutcome = { opId: string, outcome: string, key?: string };

export type LearnedPage = {
  recordId: string,
  title: string,
  kind: LensKind,
  created: boolean,
  outcomes: Array<LearnedOutcome>,
};

export type LensBodyView =
  | { "kind": "principle", statement: string, why: string, example: string, origin: string }
  | { "kind": "perspective", summary: string, sections: Array<SectionView> }
  | { "kind": "method", summary: string, sections: Array<SectionView> }
  | { "kind": "subject", summary: string, sections: Array<SectionView> }
  | { "kind": "idea", summary: string, sections: Array<SectionView>, idea: IdeaState };

export type LensLinkView = { recordId: string, title: string, kind: LensKind };

export type LensLogEntry = {
  opId: string,
  op: string,
  outcome: string,
  key?: string,
  origin: string,
  chatId?: string,
  messageId?: string,
  createdAtMs: number,
  undoesOpId?: string,
};

/** One page as the bridge and the `lens_read` tool deliver it. */
export type PageRead = {
  recordId: string,
  revisionId: string,
  revisionDigest: string,
  kind: LensKind,
  title: string,
  archived: boolean,
  paused: boolean,
  learned: boolean,
  createdAtMs: number,
  updatedAtMs: number,
  body: LensBodyView,
  /** Suggested section keys for this kind: an ordering hint, not a closed set. */
  suggestedSections: Array<string>,
  anchors: Array<AnchorView>,
  links: Array<LensLinkView>,
  backlinks: Array<LensLinkView>,
  /** Newest first, at most 20. */
  history: Array<LensLogEntry>,
};

export type PageSummary = { recordId: string, title: string, kind: LensKind, summary: string };

export type SectionView = {
  key: SectionKey,
  heading: string,
  markdown: string,
  digest: string,
  basis: Basis,
  chip: BasisChip,
  origin: Origin,
  contested: boolean,
  contestedClaimIds: Array<string>,
  updatedAtMs: number,
};
