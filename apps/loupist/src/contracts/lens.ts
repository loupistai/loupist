import type * as Wire from "./generated";
export type LensKind = Wire.LensKind;
export type IdeaStatus = Wire.IdeaStatus;

export type BasisRefView =
  | { kind: "claim"; claimId: string }
  | { kind: "span"; recordId: string; revisionDigest: string; spanId: string }
  | { kind: "said"; messageId: string }
  | { kind: "page"; recordId: string; sectionKey?: string };

export type SectionView = Wire.SectionView;

export type LensBodyView =
  | { kind: "principle"; statement: string; why: string; example: string; origin: string }
  | { kind: "perspective" | "method" | "subject"; summary: string; sections: SectionView[] }
  | { kind: "idea"; summary: string; sections: SectionView[]; idea: { status: IdeaStatus; registeredAtMs: number; statusAtMs: number } };

export type AnchorView = Wire.AnchorView;
export type LensLinkView = Wire.LensLinkView;

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
export type PageSummary = Wire.PageSummary;

export type LearnedPage = Wire.LearnedPage;
export type LearnedDigest = Wire.LearnedDigest;
