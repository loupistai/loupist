// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { SourceDataTable } from "../../commands/knowledge/source_presentation/data";
import type { ChatEvidenceMatch } from "../../domain/chat";

/** End-exclusive UTF-8 offsets into the original presentation text. */
export type CitedPassage = { start: number, end: number, marks: Array<CitedPassageMark> };

/** End-exclusive UTF-8 offsets into the original presentation text. */
export type CitedPassageMark = { start: number, end: number, evidenceId?: string };

/**
 * A renderer-only projection of a field in the immutable delivered capture.
 * This is not a replacement span and never grants citation authority.
 */
export type SourcePresentationDto = {
  text: string,
  table?: SourceDataTable,
  citedPassages?: Array<CitedPassage>,
  focusedMark?: ChatEvidenceMatch,
  sourcePointer: string,
  extractionMethod: string,
  contentOffset: number,
  nextOffset: number | null,
  extractionTruncated: boolean,
};
