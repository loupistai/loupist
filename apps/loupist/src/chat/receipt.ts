import type { ChatEvidenceRef, ChatMessage, GrowthEvent, MemoryUpdateChange } from "../types";
import type { Verdict } from "../contracts/assets";
import { citationSources } from "./evidence";
import { numbersToCheck } from "./numberChecks";

/** Saved kinds in receipt order. A Lens change is a method; perspective edits share it. */
export const SAVED_RECEIPT_KINDS = ["Source", "Lens", "principle", "Journal", "Research", "World"] as const;
export type SavedReceiptKind = (typeof SAVED_RECEIPT_KINDS)[number];
export type SavedReceiptGroup = { kind: SavedReceiptKind; items: MemoryUpdateChange[] };
export type UsedPrinciple = { recordId: string; title: string };
export type MapAdditions = { nodeIds: ReadonlySet<string>; edgeIds: ReadonlySet<string>; highlightIds: ReadonlySet<string> };
/** A read this answer moved on an asset page. */
export type ThesisMove = { assetId: string; name: string; verdict: Verdict; from?: Verdict };

export type ChatReceiptData = {
  /** Retained sources the answer read and can cite. */
  readSources: ChatEvidenceRef[];
  names: number;
  relationships: number;
  /** Reads this answer moved, after the relationships segment. */
  thesisMoves: ThesisMove[];
  /** Reads the answer said it changed but Rust did not save (the page changed first). */
  unsavedTheses: { assetId: string; name: string }[];
  mapHighlightIds: ReadonlySet<string>;
  /** Lens context supplied to this answer. Delivery does not establish application. */
  usedPrinciples: UsedPrinciple[];
  usedMethods: UsedPrinciple[];
  saved: SavedReceiptGroup[];
  proposals: MemoryUpdateChange[];
  /** Number links that did not match their source (not found or differs). */
  numbersToCheck: number;
  empty: boolean;
};

const emptyIds: ReadonlySet<string> = new Set();

/** Moves this message made, from its `thesis_moved` growth events: where each read started and where it ended. */
export function thesisMovesFor(events: readonly GrowthEvent[] | undefined, messageId: string): ThesisMove[] {
  const moves = new Map<string, ThesisMove>();
  for (const event of [...(events ?? [])].sort((a, b) => a.atMs - b.atMs)) {
    if (event.kind !== "thesis_moved" || event.originMessageId !== messageId || !event.subjectId || !event.afterSummary) continue;
    const from = moves.get(event.subjectId)?.from ?? (event.beforeSummary as Verdict | null) ?? undefined;
    moves.set(event.subjectId, { assetId: event.subjectId, name: event.summary, verdict: event.afterSummary as Verdict, ...(from ? { from } : {}) });
  }
  return [...moves.values()];
}

export function chatReceipt(message: ChatMessage, additions?: MapAdditions, options: { saveOff?: boolean; thesisMoves?: ThesisMove[] } = {}): ChatReceiptData {
  const update = message.memory_update;
  // A chat with saving off (private) cannot have saved anything; never claim it did.
  const changes = update?.status === "applied" && !options.saveOff ? update.changes : [];
  const savedChanges = changes.filter((change) => change.operation !== "proposed");
  const saved = SAVED_RECEIPT_KINDS.flatMap((kind): SavedReceiptGroup[] => {
    const items = savedChanges.filter((change) => change.kind === kind);
    return items.length ? [{ kind, items }] : [];
  });
  const proposals = changes.filter((change) => change.operation === "proposed");

  // Kind comes from the host's delivery record, never from answer text or the
  // choice of tool. A principle can be read exactly; a method can be linked.
  const delivered = message.role === "assistant" ? [...new Map((message.lens_receipts ?? []).map(item => [item.recordId, item])).values()] : [];
  const usedPrinciples = delivered.filter(item => item.kind === "principle").map(({ recordId, title }) => ({ recordId, title }));
  const usedMethods = delivered.filter(item => item.kind === "method").map(({ recordId, title }) => ({ recordId, title }));

  const readSources = citationSources(message.evidence_refs ?? []);
  const names = additions?.nodeIds.size ?? 0;
  const relationships = additions?.edgeIds.size ?? 0;
  const thesisMoves = options.thesisMoves ?? [];
  const unsavedTheses = (message.unsaved_theses ?? []).map(({ assetId, name }) => ({ assetId, name }));
  const toCheck = numbersToCheck(message);
  return {
    readSources,
    names,
    relationships,
    thesisMoves,
    unsavedTheses,
    mapHighlightIds: names + relationships > 0 && additions ? additions.highlightIds : emptyIds,
    usedPrinciples,
    usedMethods,
    saved,
    proposals,
    numbersToCheck: toCheck,
    empty: !readSources.length && !names && !relationships && !thesisMoves.length && !unsavedTheses.length && !usedPrinciples.length && !usedMethods.length && !saved.length && !proposals.length && !toCheck,
  };
}

/** A finished answer gets a receipt; a running, failed, or stopped one does not. */
export const showsReceipt = (message: ChatMessage) =>
  message.role === "assistant" && message.status !== "streaming" && message.status !== "error" && message.status !== "aborted";

/** "Nothing new saved" is a fact only when the turn was allowed to save. */
export const savesMemory = (message: ChatMessage, saveOff = false) => !saveOff && message.memory_update !== undefined;
