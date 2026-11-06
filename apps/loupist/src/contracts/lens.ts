export type LensKind = "principle" | "perspective" | "method" | "subject" | "idea";
export type IdeaStatus = "open" | "supported" | "weakened" | "refuted" | "retired";

export type BasisRefView =
  | { kind: "claim"; claimId: string }
  | { kind: "span"; recordId: string; revisionDigest: string; spanId: string }
  | { kind: "said"; messageId: string }
  | { kind: "page"; recordId: string; sectionKey?: string };

export type SectionView = {
  key: string;
  heading: string;
  markdown: string;
  digest: string;
  basis: { refs: BasisRefView[]; uncertainty?: string };
  /** Derived by Rust from the basis. */
  chip: "source" | "said" | "view" | "none";
  origin: "user" | "loupist" | "learning" | "shared";
  contested: boolean;
  contestedClaimIds: string[];
  updatedAtMs: number;
};

export type LensBodyView =
  | { kind: "principle"; statement: string; why: string; example: string; origin: string }
  | { kind: "perspective" | "method" | "subject"; summary: string; sections: SectionView[] }
  | { kind: "idea"; summary: string; sections: SectionView[]; idea: { status: IdeaStatus; registeredAtMs: number; statusAtMs: number } };

export type AnchorView = { role: "subject" | "mention" | "applied"; nodeId: string | null; name: string };
export type LensLinkView = { recordId: string; title: string; kind: LensKind };

export type LensLogEntryView = {
  opId: string;
  op: "put" | "retire" | "summary" | "idea_status";
  outcome: string;
  key?: string;
  origin: "turn" | "user" | "learning" | "import";
  chatId?: string;
  messageId?: string;
  createdAtMs: number;
  undoesOpId?: string;
};

export type PageView = {
  recordId: string;
  revisionId: string;
  revisionDigest: string;
  kind: LensKind;
  title: string;
  archived: boolean;
  paused: boolean;
  learned: boolean;
  createdAtMs: number;
  updatedAtMs: number;
  body: LensBodyView;
  suggestedSections: string[];
  anchors: AnchorView[];
  links: LensLinkView[];
  backlinks: LensLinkView[];
  /** Newest first, at most 20. */
  history: LensLogEntryView[];
};

export type LensSearchHit = { recordId: string; revisionId: string; kind: LensKind; title: string; summary: string; snippet: string };
export type PageSummary = { recordId: string; title: string; kind: LensKind; summary: string };

export type LearnedPage = { recordId: string; title: string; kind: LensKind; created: boolean; outcomes: Array<{ opId: string; outcome: string; key?: string }> };
export type LearnedDigest = { messageId: string; pages: LearnedPage[] };
