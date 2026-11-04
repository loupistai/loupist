import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import type { ReactNode } from "react";
import type { ChatEvidenceRef } from "../../types";
import type { ChatReceiptData, SavedReceiptGroup, ThesisMove } from "../../chat/receipt";
import { verdictLabel } from "../assets/assetText";
import { evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";
import { TriangleAlertIcon } from "lucide-react";

const t = i18n.getFixedT(null, "chat");

type CountKey =
  | "chatReceipt.readSources" | "chatReceipt.names" | "chatReceipt.namesOnMap" | "chatReceipt.relationships"
  | "chatReceipt.relationshipsOnMap" | "chatReceipt.usedPrinciples" | "chatReceipt.usedMethods" | "chatReceipt.savedSources"
  | "chatReceipt.savedMethods" | "chatReceipt.savedPrinciples" | "chatReceipt.savedJournal" | "chatReceipt.savedNotes"
  | "chatReceipt.savedWorld" | "chatReceipt.toReview" | "chatReceipt.numbersToCheck";

const count = (key: CountKey, value: number): string => t(key, { count: value, formattedCount: formatNumber(value) });

const savedKeys: Record<SavedReceiptGroup["kind"], CountKey> = {
  Source: "chatReceipt.savedSources",
  Lens: "chatReceipt.savedMethods",
  principle: "chatReceipt.savedPrinciples",
  Journal: "chatReceipt.savedJournal",
  Research: "chatReceipt.savedNotes",
  World: "chatReceipt.savedWorld",
};

type Props = {
  receipt: ChatReceiptData;
  /** True when the turn could save and saved nothing: the line says so. */
  showNothingNew: boolean;
  listId: string;
  listExpanded: boolean;
  onOpenSource?: (evidence: ChatEvidenceRef, anchor?: HTMLElement) => void;
  onShowOnMap?: (ids: ReadonlySet<string>) => void;
  /** The panel under the answer that quotes the principles and methods used, with Not this time and Edit. */
  usedListId: string;
  usedExpanded: boolean;
  onToggleUsed: () => void;
  onSaved: (group: SavedReceiptGroup) => void;
  onToggleList: () => void;
  /** Opens the asset page at its thesis history. */
  onOpenThesis?: (move: ThesisMove) => void;
  /** Scrolls to the first number to check and pins its popover. */
  onCheckNumbers?: () => void;
};

type Segment = { key: string; node: ReactNode };

/** One link that opens a short list when it stands for more than one item. */
function ListSegment<T>({ label, listLabel, items, itemKey, itemLabel, itemMeta, onOpen }: {
  label: string;
  listLabel: string;
  items: readonly T[];
  itemKey: (item: T) => string;
  itemLabel: (item: T) => string;
  itemMeta?: (item: T) => string;
  onOpen?: (item: T, anchor?: HTMLElement) => void;
}) {
  if (!onOpen) return <span className="chat-receipt-text">{label}</span>;
  if (items.length === 1) return <button type="button" className="chat-receipt-segment" onClick={(event) => onOpen(items[0], event.currentTarget)}>{label}</button>;
  return <Popover>
    <PopoverTrigger asChild><button type="button" className="chat-receipt-segment">{label}</button></PopoverTrigger>
    <PopoverContent className="chat-receipt-popover" align="start" aria-label={listLabel}>
      <ul>{items.map((item) => <li key={itemKey(item)}>
        <button type="button" onClick={(event) => onOpen(item, event.currentTarget)}>
          <span>{itemLabel(item)}</span>
          {itemMeta ? <small>{itemMeta(item)}</small> : null}
        </button>
      </li>)}</ul>
    </PopoverContent>
  </Popover>;
}

/** What this answer used and saved: one line of links in a fixed order. */
export function ChatReceipt({ receipt, showNothingNew, listId, listExpanded, onOpenSource, onShowOnMap, usedListId, usedExpanded, onToggleUsed, onSaved, onToggleList, onOpenThesis, onCheckNumbers }: Props) {
  useTranslation("chat");
  const segments: Segment[] = [];
  if (receipt.readSources.length) segments.push({ key: "read", node: <ListSegment
    label={count("chatReceipt.readSources", receipt.readSources.length)}
    listLabel={t("chatReceipt.sourcesRead")}
    items={receipt.readSources}
    itemKey={(evidence) => evidence.id}
    itemLabel={evidenceTitle}
    itemMeta={sourceIdentityLabel}
    onOpen={onOpenSource}
  /> });
  const showMap = onShowOnMap ? () => onShowOnMap(receipt.mapHighlightIds) : undefined;
  if (receipt.names) {
    const label = count("chatReceipt.names", receipt.names);
    segments.push({ key: "names", node: showMap
      ? <button type="button" className="chat-receipt-segment" data-learned="true" aria-label={`${label} · ${count("chatReceipt.namesOnMap", receipt.names)}`} onClick={showMap}>{label}</button>
      : <span className="chat-receipt-text" data-learned="true">{label}</span> });
  }
  if (receipt.relationships) {
    const label = count("chatReceipt.relationships", receipt.relationships);
    segments.push({ key: "relationships", node: showMap
      ? <button type="button" className="chat-receipt-segment" data-learned="true" aria-label={count("chatReceipt.relationshipsOnMap", receipt.relationships)} onClick={showMap}>{label}</button>
      : <span className="chat-receipt-text" data-learned="true">{label}</span> });
  }
  for (const move of receipt.thesisMoves) {
    // The move itself: "Hanul Precision: Holding up → Weakening".
    const label = move.from
      ? t("chatReceipt.thesisMovedFrom", { name: move.name, from: verdictLabel(move.from), verdict: verdictLabel(move.verdict) })
      : t("chatReceipt.thesisMoved", { name: move.name, verdict: verdictLabel(move.verdict) });
    segments.push({ key: `thesis:${move.assetId}`, node: onOpenThesis
      ? <button type="button" className="chat-receipt-segment" data-learned="true" onClick={() => onOpenThesis(move)}>{label}</button>
      : <span className="chat-receipt-text" data-learned="true">{label}</span> });
  }
  // The answer may say it changed a read that Rust could not save; the receipt says it was not.
  for (const unsaved of receipt.unsavedTheses) {
    segments.push({ key: `thesis-unsaved:${unsaved.assetId}`, node: <span className="chat-receipt-text" data-unsaved="true">{t("chatReceipt.thesisNotSaved", { name: unsaved.name })}</span> });
  }
  const usedToggle =(key: string, label: string) => <button type="button" className="chat-receipt-segment" data-receipt-used={key}
    aria-expanded={usedExpanded} aria-controls={usedExpanded ? usedListId : undefined} onClick={onToggleUsed}>{label}</button>;
  if (receipt.usedPrinciples.length) segments.push({ key: "principles", node: usedToggle("principles", count("chatReceipt.usedPrinciples", receipt.usedPrinciples.length)) });
  if (receipt.usedMethods.length) segments.push({ key: "methods", node: usedToggle("methods", count("chatReceipt.usedMethods", receipt.usedMethods.length)) });
  for (const group of receipt.saved) segments.push({ key: `saved:${group.kind}`, node: <button
    type="button"
    className="chat-receipt-segment"
    data-learned="true"
    aria-expanded={listExpanded}
    aria-controls={listExpanded ? listId : undefined}
    onClick={() => onSaved(group)}
  >{count(savedKeys[group.kind], group.items.length)}</button> });
  if (receipt.proposals.length) segments.push({ key: "proposals", node: <button
    type="button"
    className="chat-receipt-segment"
    aria-expanded={listExpanded}
    aria-controls={listExpanded ? listId : undefined}
    onClick={onToggleList}
  >{count("chatReceipt.toReview", receipt.proposals.length)}</button> });
  // Shown only when a number did not match its source; always after the other segments.
  if (receipt.numbersToCheck) {
    const label = count("chatReceipt.numbersToCheck", receipt.numbersToCheck);
    segments.push({ key: "numbers", node: onCheckNumbers
      ? <button type="button" className="chat-receipt-segment chat-receipt-check" onClick={onCheckNumbers}><TriangleAlertIcon aria-hidden="true" />{label}</button>
      : <span className="chat-receipt-text chat-receipt-check"><TriangleAlertIcon aria-hidden="true" />{label}</span> });
  }

  if (!segments.length) {
    if (!showNothingNew) return null;
    return <nav className="chat-receipt" aria-label={t("chatReceipt.label")}><div className="chat-receipt-line"><span className="chat-receipt-item" data-receipt="nothing-new"><span className="chat-receipt-separator" aria-hidden="true">·</span><span className="chat-receipt-text">{t("chatReceipt.nothingNew")}</span></span></div></nav>;
  }
  // Every item carries its leading separator; the line clips the one that starts each row.
  return <nav className="chat-receipt" aria-label={t("chatReceipt.label")}>
    <div className="chat-receipt-line">
      {segments.map((segment) => <span className="chat-receipt-item" data-receipt={segment.key} key={segment.key}>
        <span className="chat-receipt-separator" aria-hidden="true">·</span>
        {segment.node}
      </span>)}
    </div>
  </nav>;
}
