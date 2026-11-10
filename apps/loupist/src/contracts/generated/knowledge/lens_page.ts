// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

export type Basis = { refs: Array<BasisRef>, uncertainty?: string };

/** The chip a reader shows for a basis. */
export type BasisChip = "source" | "said" | "view" | "none";

export type BasisRef =
  | { "kind": "claim", claimId: string }
  | { "kind": "span", recordId: string, revisionDigest: string, spanId: string }
  | { "kind": "said", messageId: string }
  | { "kind": "page", recordId: string, sectionKey?: SectionKey };

export type IdeaState = { status: IdeaStatus, registeredAtMs: number, statusAtMs: number };

export type IdeaStatus = "open" | "supported" | "weakened" | "refuted" | "retired";

export type LensKind = "principle" | "perspective" | "method" | "subject" | "idea";

export type Origin = "user" | "loupist" | "learning" | "shared";

/** A section's stable identity inside its page: a slug `[a-z0-9-]{1,48}`. */
export type SectionKey = string;
