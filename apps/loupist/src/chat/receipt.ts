import type { ChatEvidenceRef, ChatMessage, GrowthEvent, LearnedDigest, LensKind, MemoryUpdateChange } from "../types";
import type { Verdict } from "../contracts/assets";
import { citationSources } from "./evidence";
import { numbersToCheck } from "./numberChecks";

export const SAVED_RECEIPT_KINDS = ["Source", "principle", "Journal", "Research", "World"] as const;
export type SavedReceiptKind = (typeof SAVED_RECEIPT_KINDS)[number];
export type SavedReceiptGroup = { kind: SavedReceiptKind; items: MemoryUpdateChange[] };
export type UsedPrinciple = { recordId: string; title: string };
export type UsedPage = UsedPrinciple & { kind: LensKind };

export type LearnedSummary = { sections: number; ideas: number; pages: LearnedDigest["pages"] };
const CHANGED = new Set(["added", "replaced", "retired"]);

export function learnedSummary(digest: LearnedDigest | undefined): LearnedSummary | null {
  const pages = (digest?.pages ?? []).filter(page => page.outcomes.some(outcome => CHANGED.has(outcome.outcome)));
  if (!pages.length) return null;
  const sections = pages.reduce((total, page) => total + page.outcomes.filter(outcome => CHANGED.has(outcome.outcome)).length, 0);
  return { sections, ideas: pages.filter(page => page.kind === "idea").length, pages };
}
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
  usedPrinciples: UsedPrinciple[];
  usedMethods: UsedPrinciple[];
  usedPages: UsedPage[];
  learned: LearnedSummary | null;
  saved: SavedReceiptGroup[];
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

export function chatReceipt(message: ChatMessage, additions?: MapAdditions, options: { saveOff?: boolean; thesisMoves?: ThesisMove[]; learned?: LearnedDigest } = {}): ChatReceiptData {
  const update = message.memory_update;
  // A chat with saving off (private) cannot have saved anything; never claim it did.
  const changes = update?.status === "applied" && !options.saveOff ? update.changes : [];
  const saved = SAVED_RECEIPT_KINDS.flatMap((kind): SavedReceiptGroup[] => {
    const items = changes.filter((change) => change.kind === kind);
    return items.length ? [{ kind, items }] : [];
  });

  // Kind comes from the host's delivery record, never from answer text or the
  // choice of tool. A principle can be read exactly; a method can be linked.
  const delivered = message.role === "assistant" ? [...new Map((message.lens_receipts ?? []).map(item => [item.recordId, item])).values()] : [];
  const usedPrinciples = delivered.filter(item => item.kind === "principle").map(({ recordId, title }) => ({ recordId, title }));
  const usedMethods = delivered.filter(item => item.kind === "method").map(({ recordId, title }) => ({ recordId, title }));
  const usedPages = delivered.map(({ recordId, title, kind }) => ({ recordId, title, kind }));
  const learned = options.saveOff ? null : learnedSummary(options.learned);

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
    usedPages,
    learned,
    saved,
    numbersToCheck: toCheck,
    empty: !readSources.length && !names && !relationships && !thesisMoves.length && !unsavedTheses.length && !usedPages.length && !learned && !saved.length && !toCheck,
  };
}

/** A finished answer gets a receipt; a running, failed, or stopped one does not. */
export const showsReceipt = (message: ChatMessage) =>
  message.role === "assistant" && message.status !== "streaming" && message.status !== "error" && message.status !== "aborted";

/** "Nothing new saved" is a fact only when the turn was allowed to save. */
export const savesMemory = (message: ChatMessage, saveOff = false) => !saveOff && message.memory_update !== undefined;
