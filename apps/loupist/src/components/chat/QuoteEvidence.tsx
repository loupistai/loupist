import { useTranslation } from "@/i18n";
import { formatDateTime } from "@/i18n/format";
import { Popover } from "radix-ui";
import { useRef } from "react";
import type { ChatEvidenceRef } from "../../types";
import { quoteEvidence, type QuoteRead, type QuoteRow } from "../../chat/quoteEvidence";

const clock = (value: string | undefined) => {
  if (!value) return null;
  const time = Date.parse(value);
  return Number.isNaN(time) ? null : formatDateTime(time);
};

const shownRows = 8;

/** The readable receipt for one saved quote or a basket. Renders only fields that were saved. */
export function QuoteEvidenceBody({ read, evidence, mode, checked }: {
  read: QuoteRead;
  evidence: ChatEvidenceRef;
  mode: "batch" | "cell";
  /** Host check for a payload that did not parse. Not a price read out of broken JSON. */
  checked?: { value: string; symbol?: string };
}) {
  const { t } = useTranslation("chat");
  const retrieved = clock(evidence.retrieved_at);
  const provider = evidence.provider?.trim();
  const focus = mode === "cell" ? read.focus : null;
  const listed = focus ? [] : read.rows.slice(0, shownRows);
  const hidden = focus ? 0 : read.rows.length - listed.length;
  const coverage = read.rows.length === 0
    ? t("quoteEvidence.coverageNone")
    : read.partial
      ? t("quoteEvidence.coverageAtLeast", { count: read.rows.length })
      : t("quoteEvidence.coverage", { count: read.rows.length });
  const note = mode !== "cell" || checked ? null
    : read.unmatched === "pointer" ? t("quoteEvidence.pointerMiss")
    : read.unmatched === "asset" && read.requestedSymbol ? t("quoteEvidence.assetMiss", { symbol: read.requestedSymbol })
    : read.unmatched === "assetConflict" ? t("quoteEvidence.assetConflict")
    : read.unmatched === "unspecified" ? t("quoteEvidence.notOneQuote")
    : null;
  return <div className="quote-evidence-body">
    <header>
      <strong>{focus ? focus.symbol : t("quoteEvidence.title")}</strong>
      {provider ? <p>{t("quoteEvidence.provider", { provider })}</p> : null}
      {retrieved ? <p>{t("quoteEvidence.retrieved", { time: retrieved })}</p> : null}
      {!focus ? <p>{coverage}</p> : null}
      {read.partial ? <p>{t("quoteEvidence.incomplete")}</p> : null}
      {note ? <p className="quote-evidence-note">{note}</p> : null}
    </header>
    {read.rows.length === 0 && checked ? <dl>
      {checked.symbol ? <div><dt>{t("quoteEvidence.ticker")}</dt><dd>{checked.symbol}</dd></div> : null}
      <div><dt>{t("quoteEvidence.checkedValue")}</dt><dd className="gg-num">{checked.value}</dd></div>
    </dl> : null}
    {focus ? <QuoteRowView row={focus} showSymbol={false} showMissing /> : null}
    {listed.length ? <ul>{listed.map((row) => <li key={`${row.pointer ?? ""}:${row.symbol}:${row.price}:${row.name ?? ""}`}>
      <QuoteRowView row={row} showSymbol showMissing={false} />
    </li>)}</ul> : null}
    {hidden > 0 ? <p className="quote-evidence-note">{t("quoteEvidence.more", { count: hidden })}</p> : null}
    {mode === "batch" || read.partial || read.rows.length === 0 ? <details>
      <summary>{t("quoteEvidence.technicalOriginal")}</summary>
      <pre>{read.original}{read.originalShortened ? "…" : ""}</pre>
    </details> : null}
  </div>;
}

function QuoteRowView({ row, showSymbol, showMissing }: { row: QuoteRow; showSymbol: boolean; showMissing: boolean }) {
  const { t } = useTranslation("chat");
  const quoteTime = clock(row.quoteTime);
  return <>
    {showSymbol || row.name ? <p className="quote-evidence-symbol">{showSymbol ? <span>{row.symbol}</span> : null}{row.name ? <span>{row.name}</span> : null}</p> : null}
    <dl>
      {!showSymbol ? <div><dt>{t("quoteEvidence.ticker")}</dt><dd>{row.symbol}</dd></div> : null}
      <div><dt>{t("quoteEvidence.lastPrice")}</dt><dd className="gg-num">{row.currency ? `${row.price} ${row.currency}` : row.price}</dd></div>
    </dl>
    {quoteTime ? <p className="quote-evidence-note">{t("quoteEvidence.quoteTime", { time: quoteTime })}</p> : showMissing ? <p className="quote-evidence-note">{t("quoteEvidence.quoteTimeMissing")}</p> : null}
  </>;
}

/** One line for a citation card: symbols and prices that were actually saved. */
export function quotePreviewLine(read: QuoteRead): string {
  if (!read.rows.length) return "";
  const shown = read.rows.slice(0, 3).map((row) => [row.symbol, row.price, row.currency].filter(Boolean).join(" "));
  return shown.join(" · ");
}

/** The control that opened the receipt, or the citation that replaced it after a remount. */
function placedAnchor(stored: HTMLElement, evidenceId: string): HTMLElement {
  if (stored.isConnected && stored.getBoundingClientRect().width > 0) return stored;
  return document.querySelector<HTMLElement>(`button.loupist-citation[data-evidence-id="${CSS.escape(evidenceId)}"]`) ?? stored;
}

/**
 * Anchored receipt for a source list, receipt, or citation click.
 * The click's node can be replaced when the answer re-renders. Measuring that
 * detached node pins the receipt at the top of the window, over the titlebar.
 */
export function QuoteEvidencePopover({ evidence, anchor, pointer, onClose }: {
  evidence: ChatEvidenceRef;
  anchor: HTMLElement;
  pointer?: string;
  onClose: () => void;
}) {
  const { t } = useTranslation("chat");
  const read = quoteEvidence(evidence, pointer ? { pointer } : undefined);
  const anchorRef = useRef(anchor);
  anchorRef.current = anchor;
  const virtualRef = useRef({
    getBoundingClientRect: () => placedAnchor(anchorRef.current, evidence.id).getBoundingClientRect(),
    getClientRects: () => placedAnchor(anchorRef.current, evidence.id).getClientRects(),
  });
  const threadBoundary = placedAnchor(anchor, evidence.id).closest<HTMLElement>(".message-scroll") ?? undefined;
  if (!read) return null;
  return <Popover.Root open onOpenChange={(next) => { if (!next) onClose(); }}>
    <Popover.Anchor virtualRef={virtualRef} />
    <Popover.Portal>
      <Popover.Content className="quote-evidence" role="dialog" aria-modal={false} aria-label={t("quoteEvidence.title")} side="top" align="start" sideOffset={6}
        updatePositionStrategy="always"
        collisionBoundary={threadBoundary} collisionPadding={{ top: threadBoundary ? 8 : 112, right: 8, bottom: 8, left: 8 }}
        onCloseAutoFocus={(event) => { event.preventDefault(); placedAnchor(anchor, evidence.id).focus(); }}>
        <QuoteEvidenceBody read={read} evidence={evidence} mode="batch" />
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}
