import type { ChatEvidenceRef, NumberCheck } from "../contracts/chat";

const LINK = /\]\(#evidence-(ev_[0-9a-f]+)\)/g;
const ONLY_LINK = /^\s*\[[^\]\n]*\]\(#evidence-(ev_[0-9a-f]+)\)\s*$/;

export type EarningsCardRef = { evidenceId: string; assetId: string; eventId: string };

/** "earnings:{assetId}:{eventId}" from `asset_page_read`'s derived snapshot evidence. */
export function snapshotRef(evidence: Pick<ChatEvidenceRef, "tool_name" | "read_ref">) {
  if (evidence.tool_name !== "earnings_snapshot" || !evidence.read_ref?.startsWith("earnings:")) return undefined;
  const [, assetId, ...rest] = evidence.read_ref.split(":");
  const eventId = rest.join(":");
  return assetId && eventId ? { assetId, eventId } : undefined;
}

/**
 * A paragraph that is only a link to a results snapshot becomes the compact
 * results card (finance-earnings-snapshot.md §2.2): the paragraph leaves the
 * text and the number checks after it keep pointing at their own links.
 */
export function earningsCards(content: string, evidence: readonly ChatEvidenceRef[] = [], checks?: NumberCheck[]): { text: string; numberChecks: NumberCheck[] | undefined; cards: EarningsCardRef[] } {
  const snapshots = new Map(evidence.flatMap(item => { const ref = snapshotRef(item); return ref ? [[item.id, ref] as const] : []; }));
  if (!snapshots.size) return { text: content, numberChecks: checks, cards: [] as EarningsCardRef[] };
  const parts = content.split(/(\n[ \t]*\n)/);
  const removed: number[] = [];
  const cards: EarningsCardRef[] = [];
  const kept: string[] = [];
  let linkIndex = 0;
  for (const part of parts) {
    const only = ONLY_LINK.exec(part);
    const ref = only ? snapshots.get(only[1]!) : undefined;
    if (only && ref) {
      removed.push(linkIndex);
      linkIndex += 1;
      if (!cards.some(card => card.eventId === ref.eventId && card.assetId === ref.assetId)) cards.push({ evidenceId: only[1]!, ...ref });
      continue;
    }
    linkIndex += [...part.matchAll(LINK)].length;
    kept.push(part);
  }
  if (!cards.length) return { text: content, numberChecks: checks, cards };
  const text = kept.join("").replace(/\n[ \t]*\n(?:[ \t]*\n)+/g, "\n\n").trim();
  const numberChecks = checks?.filter(check => !removed.includes(check.link_index))
    .map(check => ({ ...check, link_index: check.link_index - removed.filter(index => index < check.link_index).length }));
  return { text, numberChecks, cards };
}
