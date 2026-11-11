import type * as Wire from "./generated";
const MEMORY_KINDS = ["World", "Lens", "Source", "Journal", "Research"] as const;
export type MemoryKind = (typeof MEMORY_KINDS)[number];

export type KnowledgeReference = {
  kind: "source" | "world" | "world_node" | "world_claim" | "lens" | "journal" | "research";
  recordId: string;
  revisionId: string;
  objectId: string;
  spanId?: string;
  receiptId?: string;
};

export type ChatMemoryKind =
  | "source"
  | "world"
  | "lens"
  | "journal"
  | "research";

export type MemoryRef = {
  read_ref: string;
  kind: ChatMemoryKind;
  title: string;
  excerpt: string;
  as_of: string | null;
  section: string | null;
  access: "search_discovered" | "exact_read";
  knowledge?: KnowledgeReference;
};

export type MemoryUpdateChange = {
  worldJobId?: string | null;
  worldStatus?: string | null;
  recordId: string;
  operation: "create" | "revise" | "rename";
  title: string;
  lesson: string;
  basis: string;
  futureUse: string;
  /** What each requested operation did, when Rust filed outcomes for this change. */
  outcomes?: Wire.FiledOutcome[];
} & (
  | { kind: "Research"; knowledge?: never }
  | { kind: "principle"; statement: string; knowledge: KnowledgeReference }
  | { kind: Exclude<MemoryKind, "Research">; knowledge: KnowledgeReference }
);

export type MapKind = Wire.MapKind;

/** One applied `loupist_setup` change. A market the user did not give is absent. */
export type SetupChange = Wire.SetupChange;

export type MemoryUpdateResult = {
  status: "applied" | "no_change";
  commitId: string | null;
  changes: MemoryUpdateChange[];
  /** Kinds the answer asked the map to track that its commit could not add. */
  skippedKinds?: MapKind[];
  setup?: SetupChange[];
};

/** Kind and delivery recorded by Rust when the item reached this turn. */
export type LensReceipt = { recordId: string; revisionId: string; revisionDigest: string; kind: import("./lens").LensKind; title: string; delivery: "context" | "exact_read" };
