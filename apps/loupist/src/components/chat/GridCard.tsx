import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { chatArtifactRef } from "../../chat/artifactRef";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import "@/styles/grid.css";
import { ArrowDownIcon, ArrowRightIcon, ArrowUpIcon, ChevronDownIcon, EllipsisIcon } from "lucide-react";
import { Popover } from "radix-ui";
import { useEffect, useId, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Spinner } from "@/components/ui/spinner";
import type { AssetsGrid, ChatArtifactRef, ChatArtifactView, ChatEvidenceRef, ConflictGrid, GridEmptyCell, GridValueCell } from "../../types";
import type { GridBlock } from "../../chat/chartBlocks";
import { GRID_SCHEMA } from "../../contracts/chat";
import { citationNumber } from "../../chat/numberChecks";
import { citationLabel, evidencePresentation, evidenceTitle, sourceIdentityLabel } from "../../chat/evidence";
import { emptyReason, gridCounts, gridCsv, gridMarkdown, gridTsv, isValueCell, nextSort, sortedRows, type GridSort } from "../../chat/grid";
import { exportFileName, utf8Bytes } from "../../chat/exportChat";
import { useAssetNames } from "../assets/assetName";
import { formatAsOf } from "../../chat/asOf";
import { useChatExecution } from "./ChatExecutionContext";
import { NumberChip } from "./NumberChip";
import { CitationChip } from "./CitationChip";
import { useGo, useOpenProps } from "../navigation/Open";

const t = i18n.getFixedT(null, "chat");

/** Saves an export through the native dialog (Rust `export_save`); `null` when the user cancels. */
export type GridExport = (request: { suggestedName: string; bytes: number[]; kind: "csv" | "md" }) => Promise<string | null>;

/** "Apr 1, 2026" / "2026년 4월 1일": the same words as the composer's as-of date. */
const asOfDate = (value: string) => formatAsOf(value);
/** "Oct 30" in the conflict table's narrow Dated column; the full date is its title. */
const shortDate = (value: string) => formatDate(Date.parse(`${value}T00:00:00Z`), { timeZone: "UTC", year: undefined, month: "short", day: "numeric" });

/** A value: its number chip when the cited evidence is retained, the printed value otherwise. */
function ValueCell({ cell, asset, evidenceRefs, asOf }: { cell: GridValueCell; asset?: string; evidenceRefs: readonly ChatEvidenceRef[]; asOf?: string }) {
  const evidence = evidenceRefs.find((item) => item.id === cell.evidence_id);
  if (!evidence) return <span className="gg-num">{cell.value}</span>;
  return <NumberChip check={cell.check} evidence={evidence} evidenceRefs={evidenceRefs} asOf={asOf} asset={asset}>{cell.value}</NumberChip>;
}

/** An empty cell says why; with the run's note, hover or focus opens it. */
function EmptyCell({ cell, spoken }: { cell: GridEmptyCell; spoken: string }) {
  const [open, setOpen] = useState(false);
  const reason = emptyReason(cell.empty);
  if (!cell.note) return <span className="grid-card-empty"><span aria-hidden="true">{reason}</span><span className="sr-only">{spoken}</span></span>;
  return <Popover.Root open={open} onOpenChange={setOpen}>
    <Popover.Trigger asChild>
      <button type="button" className="grid-card-empty" aria-label={spoken} data-empty={cell.empty}
        onPointerEnter={() => setOpen(true)} onPointerLeave={() => setOpen(false)} onFocus={() => setOpen(true)} onBlur={() => setOpen(false)}>
        {reason}
      </button>
    </Popover.Trigger>
    <Popover.Portal>
      <Popover.Content className="grid-card-note" side="top" align="start" sideOffset={4} onOpenAutoFocus={(event) => event.preventDefault()}>{cell.note}</Popover.Content>
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
 * the right edge, and counts the columns still hidden past that edge.
 */
export function columnFit(viewport: number, first: number, columns: number, scrollLeft = 0) {
  const room = viewport - first;
  if (columns <= 0 || room <= 0 || first + columns * MEASURE_MIN <= viewport) return { width: undefined, hidden: 0 };
  const shown = Math.max(1, Math.floor(room / MEASURE_MIN - PEEK));
  const width = Math.min(MEASURE_MAX, Math.max(MEASURE_MIN, room / (shown + PEEK)));
  const whole = Math.floor((scrollLeft + room + 1) / width);
  return { width, hidden: Math.max(0, columns - whole) };
}

/** A slash term ("Net debt/EBITDA") breaks only after its slash, never between its words. */
export const headerLabel = (label: string) => (label.includes("/") ? label.replace(/ /gu, "\u00a0").replace(/\//gu, "/\u200b") : label);

/** Marks the scrollable edges, sizes the columns and counts what is hidden past the right edge. */
function useScrollEdges(columns: number) {
  const ref = useRef<HTMLDivElement>(null);
  const [edges, setEdges] = useState({ start: false, end: false });
  const [fit, setFit] = useState<{ width?: number; hidden: number }>({ hidden: 0 });
  const update = () => {
    const node = ref.current;
    if (!node) return;
    const first = node.querySelector<HTMLElement>(".grid-card-asset-head")?.offsetWidth ?? 0;
    const next = columnFit(node.clientWidth, first, columns, node.scrollLeft);
    setFit((current) => (current.width === next.width && current.hidden === next.hidden ? current : next));
    const start = node.scrollLeft > 1;
    const end = node.scrollWidth - node.clientWidth - node.scrollLeft > 1;
    // Scroll and resize events arrive in bursts; an unchanged pair keeps the same state object.
    setEdges((current) => (current.start === start && current.end === end ? current : { start, end }));
  };
  useEffect(() => {
    update();
    const node = ref.current;
    if (!node || typeof ResizeObserver === "undefined") return undefined;
    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `update` reads only refs; re-measure when the columns change.
  }, [columns]);
  // The table's width changes with the columns' width: look again once it has.
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `update` reads only refs; re-measure when the fitted width changes.
  useEffect(() => { update(); }, [fit.width]);
  const more = () => {
    const node = ref.current;
    if (!node || !fit.width) return;
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const step = Math.max(1, Math.floor((node.clientWidth - (node.querySelector<HTMLElement>(".grid-card-asset-head")?.offsetWidth ?? 0)) / fit.width)) * fit.width;
    node.scrollBy?.({ left: step, behavior: reduced ? "auto" : "smooth" });
  };
  return { ref, edges, fit, update, more };
}

export type GridCardProps = {
  title: string;
  grid: AssetsGrid;
  evidenceRefs: readonly ChatEvidenceRef[];
  onExport?: GridExport;
  onAsk?: () => void;
  askDisabled?: boolean;
};

/**
 * A grid answer (finance-grid-answers.md §2.1): assets × measures, every value
 * a number chip Rust checked, empty cells with their reason, sortable columns
 * and exports that keep each cell's source or formula.
 */
export function GridCard({ title, grid, evidenceRefs, onExport, onAsk, askDisabled }: GridCardProps) {
  useTranslation("chat");
  const go = useGo();
  const scope = useChatExecution();
  const [sort, setSort] = useState<GridSort>(null);
  const copyId = useId();
  const { state: copyState, copy: copyText } = useCopyFeedback(copyId);
  const copied = copyState === "copied";
  const [saveFailed, setSaveFailed] = useState<"csv" | "md" | null>(null);
  const card = useRef<HTMLElement>(null);
  const scroll = useScrollEdges(grid.columns.length);
  const names = useAssetNames(scope?.bridge, scope?.loupistId ?? "", Boolean(scope));
  const order = useMemo(() => sortedRows(grid, sort), [grid, sort]);
  const counts = useMemo(() => gridCounts(grid), [grid]);

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
    const reduced = typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    chip.scrollIntoView?.({ block: "center", inline: "center", behavior: reduced ? "auto" : "smooth" });
    chip.click();
  };
  const sortLabel = (column: number) => sort?.column !== column ? t("grid.sortNone") : sort.direction === "desc" ? t("grid.sortDesc") : t("grid.sortAsc");

  return <section ref={card} className="grid-card" data-layout="assets" aria-label={t("grid.label", { title })}>
    <header className="grid-card-header">
      <div className="grid-card-heading">
        <h3 className="grid-card-title" title={title}>{title}</h3>
        {grid.subtitle ? <p className="grid-card-subtitle">{grid.subtitle}</p> : null}
      </div>
      <CardActions onCopy={copy} copied={copied} onExport={onExport ? (kind) => void save(kind) : undefined}
        onAsk={onAsk} askDisabled={askDisabled} askLabel={t("chatBlock.askAbout", { title })} />
    </header>
    {saveFailed ? <p className="grid-card-error" role="alert">{t("grid.saveFailed")} <Button type="button" variant="link" size="sm" onClick={() => void save(saveFailed)}>{t("grid.retry")}</Button></p> : null}
    <div ref={scroll.ref} className="grid-card-scroll" role="region" tabIndex={0} aria-label={t("grid.scroll")}
      data-fade-start={scroll.edges.start || undefined} data-fade-end={scroll.edges.end || undefined} onScroll={scroll.update}>
      <table className="grid-card-table" style={{ "--grid-columns": grid.columns.length, ...(scroll.fit.width ? { width: `calc(var(--grid-first) + ${grid.columns.length} * ${scroll.fit.width}px)` } : {}) } as CSSProperties}>
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
                    {column.period || column.basis ? <span className="grid-card-period">{[column.period, column.basis].filter(Boolean).join(" · ")}</span> : null}
                  </span>
                </button>
              </th>;
            })}
          </tr>
        </thead>
        <tbody>
          {order.map((rowIndex) => {
            const row = grid.rows[rowIndex]!;
            const linked = Boolean(row.asset_id && go && scope);
            const label = names({ name: row.label, assetId: row.asset_id, symbol: row.symbol, market: row.market });
            return <tr key={`${row.symbol}:${row.market}`}>
              <th scope="row" className="grid-card-asset">
                {linked
                  ? <button type="button" className="grid-card-name" onClick={() => go!({ kind: "asset", target: { loupistId: scope!.loupistId, symbol: row.symbol, market: row.market, name: label } })}>{label}</button>
                  : <span className="grid-card-name">{label}</span>}
                <span className="grid-card-symbol">{row.symbol} · {row.market}</span>
              </th>
              {grid.columns.map((column, columnIndex) => {
                const cell = grid.cells[rowIndex]?.[columnIndex];
                if (!cell) return <td key={column.id} />;
                return isValueCell(cell)
                  ? <td key={column.id} className="grid-card-value gg-num" data-status={cell.check.status}><ValueCell cell={cell} asset={row.symbol} evidenceRefs={evidenceRefs} asOf={grid.as_of} /></td>
                  : <td key={column.id} className="grid-card-empty-cell"><EmptyCell cell={cell} spoken={t("grid.emptySpoken", { asset: label, measure: column.label, reason: emptyReason(cell.empty) })} /></td>;
              })}
            </tr>;
          })}
        </tbody>
      </table>
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
    {grid.notes?.length ? <div className="grid-card-notes">{grid.notes.slice(0, 3).map((note, index) => <p key={index}>{note}</p>)}</div> : null}
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

/** Reserved height while building: min(rows × 40 + 96, 420); rows are unknown until the table arrives. */
const BUILDING_ROWS = 4;

/**
 * A grid or conflict card in the answer itself. The block keeps its reserved
 * frame from the moment the Loupist starts building the table until it fills.
 */
export function ChatGridBlock({ block, onAsk, askDisabled, onAskAgain }: {
  /** A failed table puts the question that asked for it back in the composer. */
  onAskAgain?: () => void;
  block: GridBlock;
  onAsk?: (artifact: ChatArtifactRef) => void;
  askDisabled?: boolean;
}) {
  useTranslation("chat");
  const scope = useChatExecution();
  const [view, setView] = useState<ViewState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);
  const artifactId = block.kind === "ready" ? block.artifact.artifact_id : null;
  const revision = block.kind === "ready" ? block.artifact.revision : null;
  useEffect(() => {
    if (!artifactId) return undefined;
    if (!scope) { setView({ status: "unavailable" }); return undefined; }
    let live = true;
    setView({ status: "loading" });
    void scope.bridge.chatArtifactRead(scope.loupistId, scope.threadId, artifactId, revision ?? undefined)
      .then((next) => { if (live) setView({ status: "ready", view: next }); })
      .catch(() => { if (live) setView({ status: "unavailable" }); });
    return () => { live = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the scope's fields; a new scope object for the same thread must not re-read.
  }, [artifactId, revision, scope?.bridge, scope?.loupistId, scope?.threadId, attempt]);

  const title = block.kind === "ready" ? (view.status === "ready" ? view.view.artifact.title : block.artifact.title) : block.title;
  const loaded = view.status === "ready" && block.kind === "ready" && !block.updating ? view.view : null;
  const payload = loaded?.revision.payload;
  if (loaded && payload?.kind === "grid" && payload.schema === GRID_SCHEMA && scope) {
    const evidenceRefs = loaded.evidence_refs ?? [];
    if (payload.grid.layout === "conflict") {
      return <ConflictCard title={title} conflict={payload.grid} evidenceRefs={evidenceRefs} />;
    }
    return <GridCard title={title} grid={payload.grid} evidenceRefs={evidenceRefs}
      onExport={(request) => scope.bridge.exportSave(request)}
      onAsk={onAsk ? () => onAsk(chatArtifactRef(loaded)) : undefined} askDisabled={askDisabled} />;
  }
  const building = block.kind === "building" || (block.kind === "ready" && block.updating);
  let state: ReactNode;
  if (building) state = <p role="status">{t("grid.building")}<span className="chat-artifact-block-shimmer" aria-hidden="true" /></p>;
  else if (block.kind === "failed") state = <div role="alert"><p>{t("grid.failed")}</p>{onAskAgain ? <Button type="button" variant="ghost" size="sm" data-ask-again onClick={onAskAgain}>{t("chatBlock.askAgain")}</Button> : null}</div>;
  else if (view.status === "unavailable" || loaded) {
    state = <div role="alert"><p>{t("grid.unavailable")}</p>{scope ? <Button type="button" variant="outline" size="sm" onClick={() => setAttempt((value) => value + 1)}>{t("grid.retry")}</Button> : null}</div>;
  } else state = <p role="status"><Spinner />{t("grid.loading")}</p>;
  return <section className="grid-card grid-card-reserved" data-state={building ? "building" : block.kind === "failed" ? "failed" : "loading"}
    aria-label={t("grid.label", { title })} aria-busy={building || undefined}
    style={{ minHeight: `${Math.min(BUILDING_ROWS * 40 + 96, 420)}px` }}>
    <header className="grid-card-header"><div className="grid-card-heading"><h3 className="grid-card-title" title={title}>{title}</h3></div></header>
    <div className="grid-card-state">{state}</div>
  </section>;
}
