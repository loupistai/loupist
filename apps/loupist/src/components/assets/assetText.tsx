/**
 * Sentences and source chips for an asset's thesis. Rust computes every move and
 * its facts; this only words them in the user's language. Shared by the asset
 * page, the Thread's line post and receipts.
 */
import i18n from "@/i18n";
import { formatDay, formatNumber } from "@/i18n/format";
import type { ChatEvidenceRef } from "../../types";
import type { AssetEvidence, AssetMove, AssetRead, LineCrossing, Verdict } from "../../contracts/assets";
import { useCallback } from "react";
import { CitationChip } from "../chat/CitationChip";
import { OpenProvider, useOpenLayer } from "../navigation/Open";
import { displayHost } from "../../lib/displayHost";
import { providerName } from "../../lib/marketProviderName";

const t = i18n.getFixedT(null, "assets");
/** Keys chosen at runtime from Rust-provided kinds. */
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;

export const verdictLabel = (verdict: Verdict) => tDynamic(`verdict.${verdict}`);
export const verdictStep = (verdict: Verdict) => ({ holding: 1, weakening: 2, challenged: 3, broken: 4 })[verdict];

/** A calendar day (YYYY-MM-DD) as "Oct 30", without a time-zone shift. */
export const shortDay = formatDay;

/**
 * A reason that follows a dash or a separator mid-sentence starts in lower case
 * ("Weakening since Oct 26 — the quarterly report shows…"). Only words that are
 * never names are lowered; anything else keeps its capital.
 */
const SENTENCE_START = /^(?:The|A|An|This|That|These|Those|It|Its|Their|They|There|Our|Your|Both|Each|Every|No|Now|Margins?|Revenue|Sales|Operating|Prices?|Shares|Results|Crossed)\b/u;
export const midSentence = (reason: string) => reason.replace(SENTENCE_START, word => word.toLowerCase());

/** A crossed line's reason, in the user's language; it always follows a dash or a separator. */
export function crossedReason(line: LineCrossing, factDate: string) {
  return t("lines.crossedReason", { claim: line.claim, value: lineValue(line.observed, line.unit), date: shortDay(factDate) });
}

export function lineValue(observed: number, unit?: string) {
  if (!unit) return formatNumber(observed, { maximumFractionDigits: 2 });
  return unit === "%" ? `${formatNumber(observed, { maximumFractionDigits: 2 })}%` : `${formatNumber(observed, { maximumFractionDigits: 2 })} ${unit}`;
}

/** One thesis move: "Holding up → Weakening · the 10-Q shows a lower margin." */
export function moveSentence(move: Pick<AssetMove, "from" | "to" | "reason" | "line" | "factDate">) {
  const reason = move.line?.crossed ? crossedReason(move.line, move.factDate) : midSentence(move.reason);
  return t("row.move", { from: move.from ? verdictLabel(move.from) : "", to: verdictLabel(move.to), reason });
}

/**
 * The read in one sentence, the same on the asset page and in the Map inspector: since the
 * user wrote it, back where an undo left it, or since the latest move and why. `move` is
 * the move whose sources the sentence cites, when it names one.
 */
export function readSentence(read: AssetRead): { text: string; move?: AssetMove } {
  const latest = read.latestMove;
  if (!latest) return { text: t("verdict.unreviewed") };
  if (latest.actor === "user" && latest.undoesMoveId) return { text: t("verdict.undone", { verdict: verdictLabel(read.verdict), date: shortDay(latest.factDate) }) };
  const reason = latest.line?.crossed ? crossedReason(latest.line, latest.factDate) : midSentence(latest.reason);
  return { text: `${t("verdict.since", { verdict: verdictLabel(read.verdict), date: shortDay(read.since) })} — ${reason}`, move: latest };
}

/** An asset source as a chat citation, so it opens the way answer citations do. */
export function assetCitation(evidence: AssetEvidence, index: number): ChatEvidenceRef {
  return {
    id: `${evidence.citationUrl ?? evidence.url ?? evidence.recordId ?? evidence.worldId ?? evidence.title}:${index}`,
    title: evidence.title,
    kind: evidence.worldId || evidence.recordId ? "saved_context" : "original",
    url: evidence.url,
    provider: evidence.provider,
    tool_name: evidence.tool,
    retrieved_at: evidence.retrievedAt ?? "",
    published_at: evidence.date,
    excerpt: "",
    excerpt_truncated: false,
    content_sha256: "",
    warnings: [],
  };
}

/** Opens one source; `citation` is the same source as a chat citation. */
export type EvidenceOpener = (evidence: AssetEvidence, citation: ChatEvidenceRef) => void;

/** What makes two pieces of evidence the same source: its link, record, Map claim or dataset row. */
function evidenceKey(evidence: AssetEvidence) {
  const url = evidence.citationUrl ?? evidence.url;
  if (url) return `url:${url}`;
  if (evidence.recordId) return `record:${evidence.recordId}`;
  if (evidence.worldId) return `world:${evidence.worldId}`;
  if (evidence.dataset) return `dataset:${evidence.dataset.dataset_id}:${evidence.dataset.revision}:${evidence.pointer ?? ""}:${evidence.date ?? ""}`;
  return `source:${evidence.tool}:${evidence.title}:${evidence.date ?? ""}`;
}

/** The number a source carries everywhere on one page or in one message. */
export type CitationNumbers = (evidence: AssetEvidence) => number;

/**
 * Numbers sources in reading order: each source gets one number however many
 * rows cite it, and numbering never restarts within the page or message.
 * Pass the evidence in display order so numbers read 1, 2, 3 down the page.
 */
export function citationNumbers(inOrder: Iterable<readonly AssetEvidence[]> = []): CitationNumbers {
  const numbers = new Map<string, number>();
  const numberOf = (evidence: AssetEvidence) => {
    const key = evidenceKey(evidence);
    let number = numbers.get(key);
    if (number === undefined) { number = numbers.size + 1; numbers.set(key, number); }
    return number;
  };
  for (const group of inOrder) for (const evidence of group) numberOf(evidence);
  return numberOf;
}

/**
 * One numbered source. A plain click goes to the surface's own opener; Cmd-click
 * (Ctrl elsewhere) or a middle click opens the source page in the system browser.
 */
function SourceChip({ item, number, onOpen, title = item.title }: { item: AssetEvidence; number: number; onOpen: EvidenceOpener; title?: string }) {
  const evidence = assetCitation(item, number);
  const open = useOpenLayer(useCallback(() => () => onOpen(item, evidence), [evidence, item, onOpen]));
  return <OpenProvider open={open}>
    <CitationChip evidence={evidence} label={i18n.t("chat:evidence.numberedSource", { number, title })} title={title} source={displayHost(item.url) || (providerName(item.provider) ?? "") || title}
      to={{ kind: "evidence", evidence }}>{number}</CitationChip>
  </OpenProvider>;
}

/** Numbered source chips for a row; a source cited twice in the row shows once. */
export function SourceChips({ evidence, numbers, onOpen }: { evidence: readonly AssetEvidence[]; numbers: CitationNumbers; onOpen: EvidenceOpener }) {
  const seen = new Set<number>();
  return <>{evidence.map((item) => {
    const number = numbers(item);
    if (seen.has(number)) return null;
    seen.add(number);
    return <SourceChip key={`${number}:${item.title}`} item={item} number={number} onOpen={onOpen} />;
  })}</>;
}
