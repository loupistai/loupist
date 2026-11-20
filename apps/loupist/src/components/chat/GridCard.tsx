import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { chatArtifactRef } from "../../chat/artifactRef";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import "@/styles/grid.css";
import { ArrowDownIcon, ArrowRightIcon, ArrowUpIcon, ChevronDownIcon, EllipsisIcon } from "lucide-react";
import { Popover } from "radix-ui";
import { memo, useEffect, useId, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Spinner } from "@/components/ui/spinner";
import type { AssetsGrid, ChatArtifactRef, ChatArtifactView, ChatEvidenceRef, ConflictGrid, GridColumn, GridEmptyCell, GridRow, GridValueCell } from "../../types";
import { sameBlock, type GridBlock } from "../../chat/chartBlocks";
import { GRID_SCHEMA } from "../../contracts/chat";
import { citationNumber } from "../../chat/numberChecks";
import { citationLabel, evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { emptyReason, gridCounts, gridCsv, gridMarkdown, gridTsv, gridDecimals, isValueCell, nextSort, shownCheck, sortedRows, type GridSort } from "../../chat/grid";
import { exportFileName, utf8Bytes } from "../../chat/exportChat";
import { useAssetNames } from "../assets/assetName";
import { formatAsOf } from "../../chat/asOf";
import { useChatExecution } from "./ChatExecutionContext";
import { NumberChip } from "./NumberChip";
import { CitationChip } from "./CitationChip";
import { CitedNote } from "./CitedNote";
import { citedPlainText } from "../../chat/citedText";
import { CLOSE_DELAY_MS } from "./useEvidencePopover";
import { useGo, useOpenProps } from "../navigation/Open";
import { useConversationScroll } from "../ai-elements/conversation";
import { savedAssetTarget } from "../../chat/navigation";
import { useAssetPageOpener } from "../assets/assetOpener";
import type { AssetPageTarget } from "../../contracts/assets";
import { titledExport } from "../../lib/exportTitle";

const t = i18n.getFixedT(null, "chat");

/**
 * The exchange-style identity Rust's `assets_grid` links a row by: a symbol and a market of ASCII
 * letters, digits, period, underscore or hyphen. Such a row opens its asset page by symbol and
 * market even when the stored row carries no id; a comparison label ("Peer median") does not.
 */
const LISTING_TOKEN = /^[A-Za-z0-9._-]+$/u;
const isListing = (row: GridRow) => LISTING_TOKEN.test(row.symbol) && LISTING_TOKEN.test(row.market);

/** Saves an export through the native dialog (Rust `export_save`); `null` when the user cancels. */
export type GridExport = (request: { suggestedName: string; bytes: number[]; kind: "csv" | "md" }) => Promise<string | null>;

/** "Apr 1, 2026" / "2026년 4월 1일": the same words as the composer's as-of date. */
const asOfDate = (value: string) => formatAsOf(value);
/** "Oct 30" in the conflict table's narrow Dated column; the full date is its title. */
const shortDate = (value: string) => formatDate(Date.parse(`${value}T00:00:00Z`), { timeZone: "UTC", year: undefined, month: "short", day: "numeric" });

/** A value: its number chip when the cited evidence is retained, the printed value otherwise. */
function ValueCell({ cell, decimals, asset, evidenceRefs, asOf }: { cell: GridValueCell; decimals?: number; asset?: string; evidenceRefs: readonly ChatEvidenceRef[]; asOf?: string }) {
  const evidence = evidenceRefs.find((item) => item.id === cell.evidence_id);
  const check = shownCheck(cell, decimals);
  if (!evidence) return <span className="gg-num">{check.text}</span>;
  return <NumberChip check={check} evidence={evidence} evidenceRefs={evidenceRefs} asOf={asOf} asset={asset}>{check.text}</NumberChip>;
}

/**
 * An empty cell says why; with the run's note, hover or focus opens it. The note is the
 * cell's description too, so a screen reader hears it on focus. The pointer may cross
 * from the cell into the note (a citation in it stays reachable), as with a number chip.
 */
function EmptyCell({ cell, spoken, evidenceRefs }: { cell: GridEmptyCell; spoken: string; evidenceRefs: readonly ChatEvidenceRef[] }) {
  const [open, setOpen] = useState(false);
  const timer = useRef<number | null>(null);
  const noteId = useId();
  useEffect(() => () => { if (timer.current !== null) window.clearTimeout(timer.current); }, []);
  const reason = emptyReason(cell.empty);
  if (!cell.note) return <span className="grid-card-empty"><span aria-hidden="true">{reason}</span><span className="sr-only">{spoken}</span></span>;
  const schedule = (next: boolean, delay: number) => {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => { timer.current = null; setOpen(next); }, delay);
  };
  const keep = () => { if (timer.current !== null) window.clearTimeout(timer.current); timer.current = null; };
  const described = citedPlainText(cell.note, (id) => {
    const evidence = evidenceRefs.find((item) => item.id === id);
    return evidence ? sourceIdentityLabel(evidence) || evidenceTitle(evidence) : t("citedNote.source");
  });
  return <Popover.Root open={open} onOpenChange={setOpen}>
    <Popover.Trigger asChild>
      <button type="button" className="grid-card-empty" aria-label={spoken} aria-describedby={noteId} data-empty={cell.empty}
        onPointerEnter={() => { keep(); setOpen(true); }} onPointerLeave={() => schedule(false, CLOSE_DELAY_MS)}
        onFocus={() => { keep(); setOpen(true); }} onBlur={() => schedule(false, CLOSE_DELAY_MS)}
        // Hover and focus already opened the note; the click that follows them must not toggle it shut.
        onClick={(event) => { event.preventDefault(); keep(); setOpen(true); }}>
        {reason}
      </button>
    </Popover.Trigger>
    <span id={noteId} hidden>{described}</span>
    <Popover.Portal>
      <Popover.Content className="grid-card-note" side="top" align="start" sideOffset={4} onOpenAutoFocus={(event) => event.preventDefault()}
        onPointerEnter={keep} onPointerLeave={() => schedule(false, CLOSE_DELAY_MS)}><CitedNote text={cell.note} evidenceRefs={evidenceRefs} /></Popover.Content>
    </Popover.Portal>
  </Popover.Root>;
}

type Actions = { onCopy: () => void; copied: boolean; onExport?: (kind: "csv" | "md") => void; onAsk?: () => void; askDisabled?: boolean; askLabel: string };

/** A menu opens on its first item, never with focus on the whole menu. */
function useFirstItemFocus() {
  const ref = useRef<HTMLDivElement>(null);
  const onOpenChange = (open: boolean) => {
    if (open) requestAnimationFrame(() => ref.current?.querySelector<HTMLElement>('[role="menuitem"]:not([data-disabled])')?.focus());
  };
  return { ref, onOpenChange };
}

/** Copy, Export ▾ and Ask; at 700 px they fold into one ⋯ menu (§2.4). */
function CardActions({ onCopy, copied, onExport, onAsk, askDisabled, askLabel }: Actions) {
  useTranslation("chat");
  const exportMenu = useFirstItemFocus();
  const moreMenu = useFirstItemFocus();
  return <>
    <div className="grid-card-actions">
      <Button type="button" variant="ghost" size="sm" onClick={onCopy}>{copied ? t("grid.copied") : t("grid.copy")}</Button>
      {onExport ? <DropdownMenu onOpenChange={exportMenu.onOpenChange}>
        <DropdownMenuTrigger asChild><Button type="button" variant="ghost" size="sm" data-grid-export>{t("grid.export")}<ChevronDownIcon aria-hidden="true" /></Button></DropdownMenuTrigger>
        <DropdownMenuContent ref={exportMenu.ref} align="end" className="grid-card-menu">
          <DropdownMenuItem onSelect={() => onExport("csv")}>{t("grid.exportCsv")}</DropdownMenuItem>
          <DropdownMenuItem onSelect={() => onExport("md")}>{t("grid.exportMarkdown")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu> : null}
      {onAsk ? <Button type="button" variant="ghost" size="sm" disabled={askDisabled} aria-label={askLabel} onClick={onAsk}>{t("grid.ask")}</Button> : null}
    </div>
    <div className="grid-card-more">
      <DropdownMenu onOpenChange={moreMenu.onOpenChange}>
        <DropdownMenuTrigger asChild><Button type="button" variant="ghost" size="icon-sm" aria-label={t("grid.more")} data-grid-more><EllipsisIcon aria-hidden="true" /></Button></DropdownMenuTrigger>
        <DropdownMenuContent ref={moreMenu.ref} align="end" className="grid-card-menu">
          <DropdownMenuItem onSelect={onCopy}>{t("grid.copy")}</DropdownMenuItem>
          {onExport ? <DropdownMenuItem onSelect={() => onExport("csv")}>{t("grid.exportCsv")}</DropdownMenuItem> : null}
          {onExport ? <DropdownMenuItem onSelect={() => onExport("md")}>{t("grid.exportMarkdown")}</DropdownMenuItem> : null}
          {onAsk ? <DropdownMenuItem disabled={askDisabled} onSelect={onAsk}>{t("grid.ask")}</DropdownMenuItem> : null}
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  </>;
}

/** Measure columns: at least 112 px, at most 168 px (§2.1). */
const MEASURE_MIN = 112;
const MEASURE_MAX = 168;
/** The share of the next column that shows at the right edge, so a wide grid looks wider than its card. */
const PEEK = 0.45;

/**
 * Sizes measure columns so that, when not all fit, the next one peeks in at
 * the right edge, and counts the columns still hidden past that edge. `widest`
 * is the widest value or period line the columns hold: a column is never
 * narrower than that (up to 168 px), so a long amount such as
 * "333,605,938,000,000원" never runs into the next column.
 */
export function columnFit(viewport: number, first: number, columns: number, scrollLeft = 0, widest = 0) {
  const least = Math.min(MEASURE_MAX, Math.max(MEASURE_MIN, Math.ceil(widest)));
  const room = viewport - first;
  if (columns <= 0 || room <= 0 || first + columns * least <= viewport) return { width: undefined, hidden: 0 };
  const shown = Math.max(1, Math.floor(room / least - PEEK));
  const width = Math.min(MEASURE_MAX, Math.max(least, room / (shown + PEEK)));
  const whole = Math.floor((scrollLeft + room + 1) / width);
  return { width, hidden: Math.max(0, columns - whole) };
}

/** A slash term ("Net debt/EBITDA") breaks only after its slash, never between its words. */
export const headerLabel = (label: string) => (label.includes("/") ? label.replace(/ /gu, "\u00a0").replace(/\//gu, "/\u200b") : label);

/**
 * The period and basis every measure shares, written once in the card's header;
 * `null` when they differ, so each measure carries its own.
 */
export function sharedPeriod(grid: AssetsGrid): string | null {
  const [first, ...rest] = grid.columns;
  if (!first || rest.some((column) => column.period !== first.period || column.basis !== first.basis)) return null;
  return [first.period, first.basis].filter(Boolean).join(" · ");
}

/**
 * Marks the scrollable edges, sizes the columns and counts what is hidden past
 * the right edge. A table with more measures than assets whose asset rows do not
 * fit is `turned`: measures become its rows.
 */
function useScrollEdges(columns: number, assets: number, content: AssetsGrid) {
  const ref = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ start: false, end: false });
  const [fit, setFit] = useState<{ width?: number; hidden: number }>({ hidden: 0 });
  const [turned, setTurned] = useState(false);
  // What the asset-row layout's columns must hold, measured only while that layout is drawn.
  const assetWidest = useRef<number | null>(null);
  // New content is measured in asset rows again, in the same frame, before it is painted.
  const [measured, setMeasured] = useState(content);
  if (measured !== content) {
    setMeasured(content);
    setTurned(false);
  }
  const update = () => {
    const node = ref.current;
    if (!node) return;
    const drawnTurned = node.querySelector<HTMLElement>(".grid-card-table")?.dataset.rows === "measures";
    const first = node.querySelector<HTMLElement>(".grid-card-asset-head")?.offsetWidth ?? 0;
    if (!drawnTurned) {
      // A value or period line, with its cell's inline padding: what a column must hold without running over.
      let widest = 0;
      for (const line of node.querySelectorAll<HTMLElement>(".grid-card-value > *, .grid-card-period")) {
        const cell = line.closest<HTMLElement>("td, th");
        const style = cell ? getComputedStyle(cell) : null;
        const padding = style ? (Number.parseFloat(style.paddingLeft) || 0) + (Number.parseFloat(style.paddingRight) || 0) : 0;
        widest = Math.max(widest, line.offsetWidth + padding);
      }
      assetWidest.current = widest;
    }
    // The turn reads the width the asset rows need, never the turned table's own, so a resize cannot flip it back and forth.
    const turn = columns > assets && assetWidest.current !== null && columnFit(node.clientWidth, first, columns, 0, assetWidest.current).hidden > 0;
    setTurned(turn);
    const next = turn || drawnTurned ? { width: undefined, hidden: 0 } : columnFit(node.clientWidth, first, columns, node.scrollLeft, assetWidest.current ?? 0);
    setFit((current) => (current.width === next.width && current.hidden === next.hidden ? current : next));
    const start = node.scrollLeft > 1;
    const end = node.scrollWidth - node.clientWidth - node.scrollLeft > 1;
    // Scroll and resize events arrive in bursts; an unchanged pair keeps the same state object.
    setEdges((current) => (current.start === start && current.end === end ? current : { start, end }));
  };
  // Measured before paint, so a table that turns is never drawn in asset rows first.
  useLayoutEffect(() => {
    update();
    const node = ref.current;
    if (!node || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `update` reads only refs; re-measure when the columns or rows change.
  }, [columns, assets]);
  // The table's width changes with the columns' width or the layout: look again once it has.
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `update` reads only refs; re-measure when the fitted width, the layout or the content changes.
  useLayoutEffect(() => { update(); }, [fit.width, turned, measured]);
  const more = () => {
    const node = ref.current;
    if (!node || !fit.width) return;
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const step = Math.max(1, Math.floor((node.clientWidth - (node.querySelector<HTMLElement>(".grid-card-asset-head")?.offsetWidth ?? 0)) / fit.width)) * fit.width;
    node.scrollBy?.({ left: step, behavior: reduced ? "auto" : "smooth" });
  };
  return { ref, edges, fit, turned, update, more };
}

export type GridCardProps = {
  title: string;
  grid: AssetsGrid;
  evidenceRefs: readonly ChatEvidenceRef[];
  onExport?: GridExport;
  onAsk?: () => void;
  askDisabled?: boolean;
  /** The table filled a frame the reader watched being prepared: its rows fade in a moment apart. */
  filled?: boolean;
  /** The Loupist a detached table belongs to, where no chat scope names it (Work). */
  loupistId?: string;
};

/**
 * A grid answer (finance-grid-answers.md §2.1): assets × measures, every value
 * a number chip Rust checked, empty cells with their reason, sortable columns
 * and exports that keep each cell's source or formula.
 */
export function GridCard({ title, grid, evidenceRefs, onExport, onAsk, askDisabled, filled = false, loupistId }: GridCardProps) {
  useTranslation("chat");
  const go = useGo();
  const conversation = useConversationScroll();
  const scope = useChatExecution();
  // In an answer the chat's opener; in Work, outside any chat, the shell's asset page opener.
  const shellOpen = useAssetPageOpener();
  const openAsset = go ? (target: AssetPageTarget) => go({ kind: "asset", target }) : shellOpen;
  const rowLoupist = scope?.loupistId ?? loupistId;
  const [sort, setSort] = useState<GridSort>(null);
  const copyId = useId();
  const { state: copyState, copy: copyText } = useCopyFeedback(copyId);
  const copied = copyState === "copied";
  const [saveFailed, setSaveFailed] = useState<"csv" | "md" | null>(null);
  const card = useRef<HTMLElement>(null);
  const scroll = useScrollEdges(grid.columns.length, grid.rows.length, grid);
  const shared = useMemo(() => sharedPeriod(grid), [grid]);
  const names = useAssetNames(scope?.bridge, scope?.loupistId ?? "", Boolean(scope));
  const order = useMemo(() => sortedRows(grid, sort), [grid, sort]);
  const counts = useMemo(() => gridCounts(grid), [grid]);
  // Values the table rounds show one number of decimals per column, so the column reads evenly.
  const decimals = useMemo(() => gridDecimals(grid), [grid]);

  const copy = () => { void copyText(gridTsv(grid)); };
  const save = async (kind: "csv" | "md") => {
    if (!onExport) return;
    setSaveFailed(null);
    const text = kind === "csv" ? gridCsv(grid, evidenceRefs) : gridMarkdown(title, grid, evidenceRefs);
    try {
      await onExport({ suggestedName: `${exportFileName(title)}.${kind}`, kind, bytes: utf8Bytes(text) });
    } catch {
      setSaveFailed(kind);
    }
  };
  const firstToCheck = () => {
    const chip = card.current?.querySelector<HTMLButtonElement>('[data-number-check="check"]');
    if (!chip) return;
    // The thread stays at the number, not at its end.
    conversation?.holdScrollPosition();
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    chip.scrollIntoView?.({ block: "center", inline: "center", behavior: reduced ? "auto" : "smooth" });
    chip.click();
  };
  const sortLabel = (column: number) => sort?.column !== column ? t("grid.sortNone") : sort.direction === "desc" ? t("grid.sortDesc") : t("grid.sortAsc");
  // A measure's own period and basis, unless every measure shares them and the header says them once.
  const periodLine = (column: GridColumn) => shared === null && (column.period || column.basis)
    ? <span className="grid-card-period">{[column.period, column.basis].filter(Boolean).join(" · ")}</span> : null;
  const assetName = (row: GridRow) => {
    const label = names({ name: row.label, assetId: row.asset_id, symbol: row.symbol, market: row.market });
    // By the stored id, else by its ticker when the row is exchange-style; a comparison label stays text.
    const target = (row.asset_id || isListing(row)) && openAsset && rowLoupist
      ? savedAssetTarget(rowLoupist, { assetId: row.asset_id, ticker: isListing(row) ? row.symbol : undefined, name: row.label }) : null;
    return { label, name: <>
      {target
        ? <button type="button" className="grid-card-name" onClick={() => openAsset!(target)}>{label}</button>
        : <span className="grid-card-name">{label}</span>}
      <span className="grid-card-symbol">{row.symbol} · {row.market}</span>
    </> };
  };
  const cellAt = (rowIndex: number, columnIndex: number, asset: string, key: string) => {
    const row = grid.rows[rowIndex]!;
    const column = grid.columns[columnIndex]!;
    const cell = grid.cells[rowIndex]?.[columnIndex];
    if (!cell) return <td key={key} />;
    return isValueCell(cell)
      ? <td key={key} className="grid-card-value gg-num" data-status={cell.check.status}><ValueCell cell={cell} decimals={decimals[columnIndex]} asset={row.symbol} evidenceRefs={evidenceRefs} asOf={grid.as_of} /></td>
      : <td key={key} className="grid-card-empty-cell"><EmptyCell cell={cell} evidenceRefs={evidenceRefs} spoken={t("grid.emptySpoken", { asset, measure: column.label, reason: emptyReason(cell.empty) })} /></td>;
  };
  const assets = grid.rows.map(assetName);

  // Measures as rows: each row names a measure, each column an asset. Not sortable.
  const turnedTable = () => <table className="grid-card-table" data-rows="measures" style={{ "--grid-columns": grid.rows.length } as CSSProperties}>
    <caption className="sr-only">{title}</caption>
    <colgroup><col className="grid-card-asset-col" />{grid.rows.map((row) => <col key={`${row.symbol}:${row.market}`} />)}</colgroup>
    <thead>
      <tr>
        <th scope="col" className="grid-card-asset-head">{t("grid.measure")}</th>
        {grid.rows.map((row, rowIndex) => <th key={`${row.symbol}:${row.market}`} scope="col" className="grid-card-asset-column">{assets[rowIndex]!.name}</th>)}
      </tr>
    </thead>
    <tbody>
      {grid.columns.map((column, columnIndex) => <tr key={column.id}>
        <th scope="row" className="grid-card-asset grid-card-measure-row">
          <span className="grid-card-measure-label">{headerLabel(column.label)}</span>
          {periodLine(column)}
        </th>
        {grid.rows.map((row, rowIndex) => cellAt(rowIndex, columnIndex, assets[rowIndex]!.label, `${row.symbol}:${row.market}`))}
      </tr>)}
    </tbody>
  </table>;

  const assetTable = () => <table className="grid-card-table" data-rows="assets" style={{ "--grid-columns": grid.columns.length, ...(scroll.fit.width ? { width: `calc(var(--grid-first) + ${grid.columns.length} * ${scroll.fit.width}px)` } : {}) } as CSSProperties}>
    <caption className="sr-only">{title}</caption>
    <colgroup><col className="grid-card-asset-col" />{grid.columns.map((column) => <col key={column.id} />)}</colgroup>
    <thead>
      <tr>
        <th scope="col" className="grid-card-asset-head">{t("grid.asset")}</th>
        {grid.columns.map((column, index) => {
          const sorted = sort?.column === index ? sort.direction : null;
          return <th key={column.id} scope="col" aria-sort={sorted === "desc" ? "descending" : sorted === "asc" ? "ascending" : "none"}>
            <button type="button" className="grid-card-sort" onClick={() => setSort((current) => nextSort(current, index))} title={sortLabel(index)}>
              <span className="grid-card-measure">
                <span className="grid-card-measure-label">{headerLabel(column.label)}{sorted === "desc" ? <ArrowDownIcon aria-hidden="true" /> : sorted === "asc" ? <ArrowUpIcon aria-hidden="true" /> : null}</span>
                {periodLine(column)}
              </span>
            </button>
          </th>;
        })}
      </tr>
    </thead>
    <tbody>
      {order.map((rowIndex) => {
        const row = grid.rows[rowIndex]!;
        return <tr key={`${row.symbol}:${row.market}`}>
          <th scope="row" className="grid-card-asset">{assets[rowIndex]!.name}</th>
          {grid.columns.map((column, columnIndex) => cellAt(rowIndex, columnIndex, assets[rowIndex]!.label, column.id))}
        </tr>;
      })}
    </tbody>
  </table>;

  return <section ref={card} className="grid-card" data-layout="assets" data-filled={filled || undefined} aria-label={t("grid.label", { title })}>
    <header className="grid-card-header">
      <div className="grid-card-heading">
        <h3 className="grid-card-title" title={title}>{title}</h3>
        {grid.subtitle ? <p className="grid-card-subtitle">{grid.subtitle}</p> : null}
        {shared ? <p className="grid-card-subtitle grid-card-shared-period">{shared}</p> : null}
      </div>
      <CardActions onCopy={copy} copied={copied} onExport={onExport ? (kind) => void save(kind) : undefined}
        onAsk={onAsk} askDisabled={askDisabled} askLabel={t("chatBlock.askAbout", { title })} />
    </header>
    {saveFailed ? <p className="grid-card-error" role="alert">{t("grid.saveFailed")} <Button type="button" variant="link" size="sm" onClick={() => void save(saveFailed)}>{t("grid.retry")}</Button></p> : null}
    <div ref={scroll.ref} className="grid-card-scroll" role="region" tabIndex={0} aria-label={t("grid.scroll")}
      data-fade-start={scroll.edges.start || undefined} data-fade-end={scroll.edges.end || undefined} onScroll={scroll.update}>
      {scroll.turned ? turnedTable() : assetTable()}
    </div>
    <div className="grid-card-foot">
      <p className="grid-card-caption">
        <span className="gg-num">{t("grid.caption", { rows: formatNumber(grid.rows.length), columns: formatNumber(grid.columns.length), linked: formatNumber(counts.linked) })}</span>
        {counts.toCheck ? <> · <button type="button" className="grid-card-check" onClick={firstToCheck}>{t("grid.toCheck", { count: counts.toCheck, formattedCount: formatNumber(counts.toCheck) })}</button></> : null}
        {grid.as_of ? <> · <span>{t("grid.knownAsOf", { date: asOfDate(grid.as_of) })}</span></> : null}
      </p>
      {/* Columns hidden past the right edge: say how many and scroll to them. */}
      {scroll.fit.hidden ? <button type="button" className="grid-card-more-columns" data-grid-more-columns onClick={scroll.more}
        aria-label={t("grid.moreColumnsLabel", { count: scroll.fit.hidden, formattedCount: formatNumber(scroll.fit.hidden) })}>
        {t("grid.moreColumns", { count: scroll.fit.hidden, formattedCount: formatNumber(scroll.fit.hidden) })}<ArrowRightIcon aria-hidden="true" />
      </button> : null}
    </div>
    {grid.notes?.length ? <div className="grid-card-notes">{grid.notes.slice(0, 3).map((note, index) => <p key={index}><CitedNote text={note} evidenceRefs={evidenceRefs} /></p>)}</div> : null}
  </section>;
}

export type ConflictCardProps = {
  title: string;
  conflict: ConflictGrid;
  evidenceRefs: readonly ChatEvidenceRef[];
};

function ConflictSource({ evidence, evidenceRefs }: { evidence: ChatEvidenceRef; evidenceRefs: readonly ChatEvidenceRef[] }) {
  const to = { kind: "evidence", evidence } as const;
  const link = useOpenProps(to);
  const number = citationNumber(evidence, evidenceRefs);
  if (number !== null) return <>{evidenceTitle(evidence)}{" "}<CitationChip evidence={evidence} label={citationLabel(evidence, evidenceRefs)} title={evidenceTitle(evidence)}
    source={sourceIdentityLabel(evidence) || evidencePresentation(evidence).label} to={to}>{number}</CitationChip></>;
  return link.available ? <button type="button" {...link.props}>{evidenceTitle(evidence)}</button> : <span>{evidenceTitle(evidence)}</span>;
}

/**
 * Where sources disagree (finance-korea-retail-disagreement.md §2.3): each
 * source's report with its date and basis, why they differ, and which one the
 * answer relies on. A disagreement is information, so no warning colors.
 */
export function ConflictCard({ title, conflict, evidenceRefs }: ConflictCardProps) {
  useTranslation("chat");
  const goTo = useGo();
  // The Map is the Loupist's, so the card offers it only inside that Loupist's chat.
  const loupistId = useChatExecution()?.loupistId;
  const { measure } = conflict;
  const label = [t("conflict.title"), measure.entity_label, [measure.period, measure.label].filter(Boolean).join(" ")].join(" · ");
  const mapEntity = conflict.map_link ? conflict.world_entity_id : undefined;
  return <section className="grid-card conflict-card" data-layout="conflict" aria-label={t("conflict.label", { title })}>
    <header className="grid-card-header">
      <div className="grid-card-heading">
        <p className="conflict-card-label">{label}</p>
        {conflict.as_of ? <p className="grid-card-subtitle">{t("grid.knownAsOf", { date: asOfDate(conflict.as_of) })}</p> : null}
      </div>
      {mapEntity && goTo && loupistId ? <div className="grid-card-actions conflict-card-actions">
        <Button type="button" variant="ghost" size="sm" data-conflict-map
          onClick={() => goTo({ kind: "page", location: { loupistId, surface: "world", view: { onlyDisputed: true, highlightEntityIds: [mapEntity] } } })}>{t("conflict.seeMap")}</Button>
      </div> : null}
    </header>
    <div className="grid-card-scroll" role="region" tabIndex={0} aria-label={t("grid.scroll")}>
      <table className="conflict-card-table">
        <caption className="sr-only">{label}</caption>
        <thead>
          <tr>
            <th scope="col">{t("conflict.source")}</th>
            <th scope="col">{t("conflict.says")}</th>
            <th scope="col">{t("conflict.dated")}</th>
            <th scope="col">{t("conflict.basis")}</th>
            <th scope="col"><span className="sr-only">{t("conflict.used")}</span></th>
          </tr>
        </thead>
        <tbody>
          {conflict.reports.map((report, index) => {
            const evidence = evidenceRefs.find((item) => item.id === report.evidence_id);
            const used = conflict.used === index;
            const says: ReactNode = report.check && evidence
              ? <NumberChip check={report.check} evidence={evidence} evidenceRefs={evidenceRefs} asOf={conflict.as_of}>{report.says}</NumberChip>
              : report.says;
            return <tr key={report.evidence_id} data-used={used || undefined}>
              <th scope="row" className="conflict-card-source">
                {evidence
                  ? <ConflictSource evidence={evidence} evidenceRefs={evidenceRefs} />
                  : <span>{t("conflict.source")}</span>}
              </th>
              <td className={report.check ? "gg-num" : "conflict-card-claim"}>{says}</td>
              <td className="gg-num conflict-card-dated">{report.dated ? <time dateTime={report.dated} title={asOfDate(report.dated)}>{shortDate(report.dated)}</time> : t("conflict.undated")}</td>
              <td>{report.basis ?? ""}</td>
              <td className="conflict-card-used">{used ? t("conflict.used") : ""}</td>
            </tr>;
          })}
        </tbody>
      </table>
    </div>
    <p className="conflict-card-why">{t("conflict.why")} {t(`conflict.reason.${conflict.reason}`)}. {conflict.explanation}</p>
    {conflict.used_reason ? <p className="conflict-card-why">{conflict.used_reason}</p> : null}
  </section>;
}

type ViewState = { status: "loading" } | { status: "ready"; view: ChatArtifactView } | { status: "unavailable" };

/** Rows reserved while building; the real count is unknown until the table arrives, so the frame may grow when it fills. */
const BUILDING_ROWS = 3;

/**
 * A grid or conflict card in the answer itself. The block keeps its reserved
 * frame from the moment the Loupist starts building the table until it fills.
 */
export const ChatGridBlock = memo(function ChatGridBlock({ block, onAsk, askDisabled, onAskAgain }: {
  /** A failed table puts the question that asked for it back in the composer. */
  onAskAgain?: () => void;
  block: GridBlock;
  onAsk?: (artifact: ChatArtifactRef) => void;
  askDisabled?: boolean;
}) {
  useTranslation("chat");
  const scope = useChatExecution();
  const staged = block.kind === "ready" ? block.staged : undefined;
  const [read, setView] = useState<ViewState>(() => staged ? { status: "ready", view: staged } : { status: "loading" });
  // Staged content draws in the render that delivers it: a card that was preparing never shows a Loading frame first.
  const view: ViewState = staged ? { status: "ready", view: staged } : read;
  const [attempt, setAttempt] = useState(0);
  const artifactId = block.kind === "ready" ? block.artifact.artifact_id : null;
  const revision = block.kind === "ready" ? block.artifact.revision : null;
  useEffect(() => {
    if (!artifactId) return undefined;
    // Delivered before the commit: drawn as delivered, so the card fills while the answer is written.
    if (staged) { setView({ status: "ready", view: staged }); return undefined; }
    if (!scope) { setView({ status: "unavailable" }); return undefined; }
    let live = true;
    // A card already drawing this revision keeps it while the stored copy is read, so it never blinks.
    setView((current) => current.status === "ready" && current.view.artifact.id === artifactId && current.view.revision.revision === revision ? current : { status: "loading" });
    void scope.bridge.chatArtifactRead(scope.loupistId, scope.threadId, artifactId, revision ?? undefined)
      .then((next) => { if (live) setView({ status: "ready", view: next }); })
      .catch(() => { if (live) setView({ status: "unavailable" }); });
    return () => { live = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the scope's fields; a new scope object for the same thread must not re-read.
  }, [artifactId, revision, staged, scope?.bridge, scope?.loupistId, scope?.threadId, attempt]);

  const title = block.kind === "ready" ? (view.status === "ready" ? view.view.artifact.title : block.artifact.title) : block.title;
  const loaded = view.status === "ready" && block.kind === "ready" && !block.updating ? view.view : null;
  const payload = loaded?.revision.payload;
  const building = block.kind === "building" || (block.kind === "ready" && block.updating);
  // A table that was preparing while the reader watched fills with a fade, rows a moment apart; one read later is drawn at once.
  const [wasBuilding, setWasBuilding] = useState(building);
  if (building && !wasBuilding) setWasBuilding(true);
  if (loaded && payload?.kind === "grid" && payload.schema === GRID_SCHEMA && scope) {
    const evidenceRefs = loaded.evidence_refs ?? [];
    if (payload.grid.layout === "conflict") {
      return <ConflictCard title={title} conflict={payload.grid} evidenceRefs={evidenceRefs} />;
    }
    return <GridCard title={title} grid={payload.grid} evidenceRefs={evidenceRefs} filled={wasBuilding}
      onExport={(request) => scope.bridge.exportSave(titledExport(request))}
      onAsk={onAsk ? () => onAsk(chatArtifactRef(loaded)) : undefined} askDisabled={askDisabled} />;
  }
  let state: ReactNode;
  // Ruled paper at the real row heights, still: the header rule and three rows, one bar per cell.
  if (building) state = <div className="grid-card-ruled" aria-hidden="true">{Array.from({ length: BUILDING_ROWS + 1 }, (_, row) => <i key={row}><b /><b /><b /><b /></i>)}</div>;
  else if (block.kind === "failed") state = <div role="alert"><p>{t("grid.failed")}</p>{onAskAgain ? <Button type="button" variant="ghost" size="sm" data-ask-again onClick={onAskAgain}>{t("chatBlock.askAgain")}</Button> : null}</div>;
  else if (view.status === "unavailable" || loaded) {
    state = <div role="alert"><p>{t("grid.unavailable")}</p>{scope ? <Button type="button" variant="outline" size="sm" onClick={() => setAttempt((value) => value + 1)}>{t("grid.retry")}</Button> : null}</div>;
  } else state = <p role="status"><Spinner />{t("grid.loading")}</p>;
  return <section className="grid-card grid-card-reserved" data-state={building ? "building" : block.kind === "failed" ? "failed" : "loading"}
    aria-label={t("grid.label", { title })} aria-busy={building || undefined}>
    <header className="grid-card-header"><div className="grid-card-heading">
      <h3 className="grid-card-title" title={title}>{title}</h3>
      {building ? <span className="grid-card-preparing" role="status">{t("grid.building")}</span> : null}
    </div></header>
    <div className="grid-card-state">{state}</div>
  </section>;
}, (before, after) => sameBlock(before.block, after.block) && before.onAsk === after.onAsk && before.askDisabled === after.askDisabled && before.onAskAgain === after.onAskAgain);
