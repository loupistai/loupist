import i18n from "@/i18n";
import type { ChatEvidenceRef, MemoryRef, MemoryUpdateChange } from "../types";
import { isCitationSource } from "./evidence";

const t = i18n.getFixedT(null, "chat");

export const exactMemoryReads = (refs: readonly MemoryRef[] | undefined) => {
  const seen = new Set<string>();
  return (refs ?? []).flatMap((ref) => {
    if (ref.access !== "exact_read" || !ref.knowledge) return [];
    const target = ref.knowledge;
    const key = JSON.stringify([target.kind, target.recordId, target.revisionId, target.objectId, target.spanId ?? null]);
    if (seen.has(key)) return [];
    seen.add(key);
    return [{ key, ref }];
  });
};

/** The exact Memory read named by this receipt's host readRef. */
export function memoryReadForEvidence(
  evidence: ChatEvidenceRef,
  refs: readonly MemoryRef[] | undefined,
): MemoryRef | null {
  if (evidence.kind !== "saved_context" || !evidence.read_ref) return null;
  return (refs ?? []).find((ref) => ref.access === "exact_read" && ref.knowledge && ref.read_ref === evidence.read_ref) ?? null;
}

/** Execution output, Memory search, and an unmatched receipt open in work progress. */
export function isInlineWorkEvidence(
  evidence: ChatEvidenceRef,
  refs: readonly MemoryRef[] | undefined,
): boolean {
  return !isCitationSource(evidence) && memoryReadForEvidence(evidence, refs) === null;
}

export const memoryChangeVerb = (change: Pick<MemoryUpdateChange, "operation" | "kind">) => {
  const operation = change.operation === "revise" || change.operation === "rename" || change.operation === "proposed" ? change.operation : "create";
  return change.kind === "Research" ? t(`memoryActivity.${operation}Note`) : t(`memoryActivity.${operation}Kind`, { kind: change.kind });
};
