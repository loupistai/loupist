import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useId, type ReactNode } from "react";
import { FigureWarnings } from "./FigureWarnings";
import type { ChatMessage, MapKind, MemorySaveSkipReason, MemoryUpdateResult, SetupChange } from "../../types";
import type { ChatReceiptData, SavedReceiptGroup, ThesisMove } from "../../chat/receipt";
import { verdictLabel } from "../assets/assetText";
import { CircleCheckIcon, TriangleAlertIcon } from "lucide-react";
import { useChatSurface } from "./turnMotion";

const t = i18n.getFixedT(null, "chat");

type CountKey =
  | "chatReceipt.names" | "chatReceipt.namesOnMap" | "chatReceipt.relationships"
  | "chatReceipt.relationshipsOnMap" | "chatReceipt.usedPages" | "chatReceipt.savedSources" | "chatReceipt.learned" | "chatReceipt.ideas"
  | "chatReceipt.savedPrinciples" | "chatReceipt.savedJournal" | "chatReceipt.savedNotes"
  | "chatReceipt.savedWorld" | "chatReceipt.numbersToCheck" | "chatReceipt.readPrinciples";

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

const savedKeys: Record<Exclude<SavedReceiptGroup["kind"], "PriceRead">, CountKey> = {
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
  message?: ChatMessage;
};

type Segment = { key: string; node: ReactNode };

export function MemorySaveWarning({ reason, items }: { reason?: MemorySaveSkipReason; items?: MemoryUpdateResult["discardedMemoryItems"] }) {
  useTranslation("chat");
  const listLabelId = useId();
  if (!reason && !items?.length) return null;
  return <div className="memory-save-warning">
    {reason ? <p>{t(`memorySaving.skipped.${reason}`)}</p> : null}
    {items?.length ? <>
      {reason ? <p>{t("memorySaving.claimNotKept")}</p> : null}
      <p id={listLabelId}>{t("memorySaving.notKept")}</p>
      <ul aria-labelledby={listLabelId}>{items.map((item, index) => <li key={`${item.kind}:${item.title}:${index}`}>
        {t(`memorySaving.discardedKind.${item.kind}`)} · {item.title}
      </li>)}</ul>
    </> : null}
  </div>;
}

/**
 * Bot's one line under an answer: how many figures were checked against their source
 * and how many sources it used. It opens the activity sheet at the sources; figures that
 * did not match have their own button, which lists the comparisons. Nothing at all when
 * the answer used no source and cited no figure.
 */
export function AnswerTrace({ figures, sources, toCheck, onOpen, message }: { figures: number; sources: number; toCheck: number; onOpen: () => void; message: ChatMessage }) {
  useTranslation("chat");
  const parts = [
    figures ? t("bot.figuresChecked", { count: figures, formattedCount: formatNumber(figures) }) : null,
    sources ? t("bot.sources", { count: sources, formattedCount: formatNumber(sources) }) : null,
  ].filter(Boolean);
  if (!parts.length && !toCheck) return null;
  return <div className="bot-trace" role="group" aria-label={t("bot.traceLabel")}>
    {parts.length ? <button type="button" className="bot-trace-open" data-trace="sources" onClick={onOpen}>
      <CircleCheckIcon aria-hidden="true" />{parts.join(" · ")}
    </button> : null}
    {toCheck ? <FigureWarnings message={message} label={t("bot.toCheck", { count: toCheck, formattedCount: formatNumber(toCheck) })}><button type="button" className="bot-trace-check" data-trace="check">
      <TriangleAlertIcon aria-hidden="true" />{t("bot.toCheck", { count: toCheck, formattedCount: formatNumber(toCheck) })}
    </button></FigureWarnings> : null}
  </div>;
}

/** What this answer used and saved: one line of links in a fixed order. Bot shows it only with the pointer or focus on the answer. */
export function ChatReceipt({ receipt: given, showNothingNew, listId, listExpanded, onShowOnMap, usedListId, usedExpanded, onToggleUsed, learnedListId, learnedExpanded, onToggleLearned, onSaved, onOpenThesis, message }: Props) {
  useTranslation("chat");
  const bot = useChatSurface().kind === "bot";
  // Bot's trace line already counts figures to check; the receipt does not repeat it there.
  const receipt = bot && given.numbersToCheck ? { ...given, numbersToCheck: 0 } : given;
  const skipped = message?.memory_update?.skipReason;
  const discarded = message?.memory_update?.discardedMemoryItems;
  const segments: Segment[] = [];
  const usedToggle = (key: string, label: string) => <button type="button" className="chat-receipt-segment" data-receipt-used={key}
    aria-expanded={usedExpanded} aria-controls={usedExpanded ? usedListId : undefined} onClick={onToggleUsed}>{label}</button>;
  // The first "it remembered" moment: a principle the user taught shaped this answer, so it leads.
  if (receipt.usedPrinciples.length) segments.push({ key: "used-principles", node: usedToggle("principles", count("chatReceipt.readPrinciples", receipt.usedPrinciples.length)) });
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
    const label = kind.name === "journal_save"
      ? t("chatReceipt.journalNotSaved")
      : t("chatReceipt.kindNotAdded", { name: kindLabel(kind) });
    segments.push({ key: `kind-skipped:${kind.name}`, node: <span className="chat-receipt-text" data-unsaved="true">{label}</span> });
  }
  // The principles lead the line, so the pages segment counts every other page (a method, a perspective…), never one twice.
  const otherPages = receipt.usedPrinciples.length ? receipt.usedPages.filter(page => page.kind !== "principle") : receipt.usedPages;
  if (otherPages.length) segments.push({ key: "used", node: usedToggle("pages", count("chatReceipt.usedPages", otherPages.length)) });
  for (const group of receipt.saved) {
    if (group.kind === "PriceRead") {
      for (const item of group.items) segments.push({
        key: `price-read:${item.recordId}`,
        node: <span className="chat-receipt-text" data-learned="true">
          {t("toolProgress.price_read_save.done")} · {item.title}
        </span>,
      });
    } else {
      segments.push({ key: `saved:${group.kind}`, node: <button
        type="button"
        className="chat-receipt-segment"
        data-learned="true"
        aria-expanded={listExpanded}
        aria-controls={listExpanded ? listId : undefined}
        onClick={() => onSaved(group)}
      >{count(savedKeys[group.kind], group.items.length)}</button> });
    }
  }
  // Shown only when a number did not match its source; always after the other segments.
  if (receipt.numbersToCheck) {
    const label = count("chatReceipt.numbersToCheck", receipt.numbersToCheck);
    segments.push({ key: "numbers", node: message
      ? <FigureWarnings message={message} label={label}><button type="button" className="chat-receipt-segment chat-receipt-check"><TriangleAlertIcon aria-hidden="true" />{label}</button></FigureWarnings>
      : <span className="chat-receipt-text chat-receipt-check"><TriangleAlertIcon aria-hidden="true" />{label}</span> });
  }

  if (!segments.length && !discarded?.length) {
    if (!skipped && !showNothingNew) return null;
    if (!skipped) segments.push({ key: "nothing-new", node: <span className="chat-receipt-text">{t("chatReceipt.nothingNew")}</span> });
  }
  // Every item carries its leading separator; the line clips the one that starts each row.
  return <nav className="chat-receipt" data-save-skipped={skipped || Boolean(discarded?.length) || undefined} aria-label={t("chatReceipt.label")}>
    <MemorySaveWarning reason={skipped} items={discarded} />
    {segments.length ? <div className="chat-receipt-line">
      {segments.map((segment) => <span className="chat-receipt-item" data-receipt={segment.key} key={segment.key}>
        <span className="chat-receipt-separator" aria-hidden="true">·</span>
        {segment.node}
      </span>)}
    </div> : null}
  </nav>;
}
