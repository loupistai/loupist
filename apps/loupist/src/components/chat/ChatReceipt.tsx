import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import type { ReactNode } from "react";
import type { MapKind, SetupChange } from "../../types";
import type { ChatReceiptData, SavedReceiptGroup, ThesisMove } from "../../chat/receipt";
import { verdictLabel } from "../assets/assetText";
import { TriangleAlertIcon } from "lucide-react";

const t = i18n.getFixedT(null, "chat");

type CountKey =
  | "chatReceipt.names" | "chatReceipt.namesOnMap" | "chatReceipt.relationships"
  | "chatReceipt.relationshipsOnMap" | "chatReceipt.usedPages" | "chatReceipt.savedSources" | "chatReceipt.learned" | "chatReceipt.ideas"
  | "chatReceipt.savedPrinciples" | "chatReceipt.savedJournal" | "chatReceipt.savedNotes"
  | "chatReceipt.savedWorld" | "chatReceipt.numbersToCheck";

const count = (key: CountKey, value: number): string => t(key, { count: value, formattedCount: formatNumber(value) });

const kindLabel = (kind: MapKind) => (i18n.language.toLowerCase().startsWith("ko") && kind.labelKo) || kind.label;

function setupLabel(change: SetupChange): string {
  switch (change.change) {
    case "followed": return t("chatReceipt.following", { name: change.name ?? change.symbol });
    case "unfollowed": return t("chatReceipt.unfollowed", { name: change.symbol });
    case "focus": return t("chatReceipt.focusUpdated");
    case "tracks": return t("chatReceipt.mapTracks", { list: change.kinds.map(kindLabel).join(", ") });
  }
}

const setupKey = (change: SetupChange) => change.change === "followed" || change.change === "unfollowed" ? `${change.change}:${change.symbol}:${change.market ?? ""}` : change.change;

const savedKeys: Record<SavedReceiptGroup["kind"], CountKey> = {
  Source: "chatReceipt.savedSources",
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
  onShowOnMap?: (ids: ReadonlySet<string>) => void;
  usedListId: string;
  usedExpanded: boolean;
  onToggleUsed: () => void;
  learnedListId: string;
  learnedExpanded: boolean;
  onToggleLearned: () => void;
  onSaved: (group: SavedReceiptGroup) => void;
  /** Opens the asset page at its thesis history. */
  onOpenThesis?: (move: ThesisMove) => void;
  /** Scrolls to the first number to check and pins its popover. */
  onCheckNumbers?: () => void;
};

type Segment = { key: string; node: ReactNode };

/** What this answer used and saved: one line of links in a fixed order. */
export function ChatReceipt({ receipt, showNothingNew, listId, listExpanded, onShowOnMap, usedListId, usedExpanded, onToggleUsed, learnedListId, learnedExpanded, onToggleLearned, onSaved, onOpenThesis, onCheckNumbers }: Props) {
  useTranslation("chat");
  const segments: Segment[] = [];
  const learnedToggle = (key: string, label: string) => <button type="button" className="chat-receipt-segment" data-learned="true" data-receipt-learned={key}
    aria-expanded={learnedExpanded} aria-controls={learnedExpanded ? learnedListId : undefined} onClick={onToggleLearned}>{label}</button>;
  if (receipt.learned) segments.push({ key: "learned", node: learnedToggle("sections", count("chatReceipt.learned", receipt.learned.sections)) });
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
  if (receipt.learned?.ideas) segments.push({ key: "ideas", node: learnedToggle("ideas", count("chatReceipt.ideas", receipt.learned.ideas)) });
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
  for (const change of receipt.setup) {
    segments.push({ key: `setup:${setupKey(change)}`, node: <span className="chat-receipt-text" data-learned="true">{setupLabel(change)}</span> });
  }
  for (const kind of receipt.skippedKinds) {
    segments.push({ key: `kind-skipped:${kind.name}`, node: <span className="chat-receipt-text" data-unsaved="true">{t("chatReceipt.kindNotAdded", { name: kindLabel(kind) })}</span> });
  }
  if (receipt.usedPages.length) segments.push({ key: "used", node: <button type="button" className="chat-receipt-segment" data-receipt-used="pages"
    aria-expanded={usedExpanded} aria-controls={usedExpanded ? usedListId : undefined} onClick={onToggleUsed}>{count("chatReceipt.usedPages", receipt.usedPages.length)}</button> });
  for (const group of receipt.saved) segments.push({ key: `saved:${group.kind}`, node: <button
    type="button"
    className="chat-receipt-segment"
    data-learned="true"
    aria-expanded={listExpanded}
    aria-controls={listExpanded ? listId : undefined}
    onClick={() => onSaved(group)}
  >{count(savedKeys[group.kind], group.items.length)}</button> });
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
