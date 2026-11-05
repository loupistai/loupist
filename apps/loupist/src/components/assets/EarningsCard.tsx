import i18n, { useTranslation } from "@/i18n";
import { formatDateTime, formatNumber } from "@/i18n/format";
import { useEffect, useId, useState, type ReactNode } from "react";
import { CheckIcon, CircleIcon, EllipsisIcon, TriangleAlertIcon } from "lucide-react";
import type { LoupistBridge } from "../../types";
import type { AssetEvidence } from "../../contracts/assets";
import type { EarningsDifference, EarningsMeasure, EarningsSnapshot, EarningsValue } from "../../contracts/earnings";
import { useOnline } from "../../app/useOnline";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { Dialog, DialogContent, focusDialogTitle } from "../ui/dialog";
import { SensitivityCard } from "../chat/SensitivityCard";
import { amount, citationNumbers, shortDay, SourceChips, type CitationNumbers } from "./assetText";
import { amountText, basisLabel, CalcChip, chainForm, differenceParts, measureTitle, snapshotEvidence, sourceLine } from "./earningsText";
import "@/styles/earnings.css";

const t = i18n.getFixedT(null, "assets");
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;
/** Measures shown before "Show {n} more". */
const MEASURES = 6;
const NARROW = "(max-width: 700px)";
/** Private-use markers that find the implied range inside its translated sentence. */
const LOW = "";
const HIGH = "";

/** Whether the window is 700 px wide or narrower: the table becomes one block per measure. */
export function useNarrow() {
  const query = () => typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia(NARROW).matches;
  const [narrow, setNarrow] = useState(query);
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const media = window.matchMedia(NARROW);
    const update = () => setNarrow(media.matches);
    update();
    media.addEventListener?.("change", update);
    return () => media.removeEventListener?.("change", update);
  }, []);
  return narrow;
}

const reported = (snapshot: Pick<EarningsSnapshot, "status">) => snapshot.status === "reported" || snapshot.status === "final";

/** "Expected Nov 4", "Reported Oct 30" or, for a Korean legal deadline, "Due by … (filing deadline)". */
export function earningsStatusText(snapshot: Pick<EarningsSnapshot, "status" | "after" | "reportDate" | "dateSource">) {
  if (reported(snapshot)) return t("earnings.reportedOn", { date: shortDay(snapshot.after?.date ?? snapshot.reportDate) });
  if (snapshot.dateSource === "legal_deadline") return t("glance.dueBy", { date: shortDay(snapshot.reportDate) });
  return t("earnings.expectedOn", { date: shortDay(snapshot.reportDate) });
}

const captureTime = (ms: number) => formatDateTime(ms, { year: undefined, month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });

/** "Before: captured Oct 29, 6:00 PM · After: Oct 30", with the late-capture note. */
function captureLine(snapshot: EarningsSnapshot) {
  const parts: string[] = [];
  if (snapshot.before) parts.push(t("earnings.capturedBefore", { time: captureTime(snapshot.before.capturedAtMs) }));
  if (snapshot.after) parts.push(t("earnings.after", { date: shortDay(snapshot.after.date) }));
  if (snapshot.before?.lateAsOf) parts.push(t("earnings.capturedLate", { date: shortDay(snapshot.before.lateAsOf) }));
  // A no-break space before each "·": a wrapped line never starts with one.
  return parts.join("\u00a0· ");
}

type Open = (evidence: AssetEvidence) => void;

/** A date or name inside a source line never breaks; the line wraps between its parts instead. */
const unbroken = (text: string) => text.replace(/ (?=[^·]*$)/gu, "\u00a0");

/** One value and its source line: "$1.27bn" over "Results release [1]"; the chip stays with the last word. */
function ValueBlock({ value, period, numbers, onOpen }: { value: EarningsValue; period: string; numbers: CitationNumbers; onOpen: Open }) {
  const source = sourceLine(value, period);
  return <div className="earnings-value">
    <span className="earnings-amount gg-num">{amountText(value)}</span>
    <span className="earnings-source" title={source}><span className="earnings-source-text">{unbroken(source)}</span>{value.evidence.length ? <>{"\u00a0"}<SourceChips evidence={value.evidence} numbers={numbers} onOpen={onOpen} /></> : null}</span>
  </div>;
}

/** Words first, then the calculated number as a chip. */
function DifferenceBlock({ difference }: { difference: EarningsDifference }) {
  const { words, calc, numberText, muted } = differenceParts(difference);
  return <div className="earnings-diff" data-muted={muted || undefined}>
    {words ? <span className="earnings-diff-words">{words}</span> : null}
    {calc ? <span className="earnings-diff-number"><CalcChip calc={calc}>{numberText ? numberText(calc.display) : calc.display}</CalcChip></span> : null}
  </div>;
}

/** The three cells of one measure, shared by the table row and the stacked block. */
function measureCells(snapshot: EarningsSnapshot, measure: EarningsMeasure, first: boolean, numbers: CitationNumbers, onOpen: Open, onOpenConnections?: () => void) {
  const before = !reported(snapshot);
  const expected: ReactNode = measure.expected.length
    ? measure.expected.map(value => <ValueBlock key={value.rowId} value={value} period={measure.period} numbers={numbers} onOpen={onOpen} />)
    : measure.guidance ? <span className="earnings-empty">—</span>
      : snapshot.status === "upcoming" && !snapshot.before ? (first ? <span className="earnings-empty">{t("earnings.willCapture", { date: snapshot.captureAtMs ? captureTime(snapshot.captureAtMs) : shortDay(snapshot.reportDate) })}</span> : <span className="earnings-empty">—</span>)
        : first && snapshot.noGuidance ? <NoExpectations snapshot={snapshot} onOpenConnections={onOpenConnections} />
          : <span className="earnings-empty">—</span>;
  const reportedCell: ReactNode = measure.reported.length
    ? measure.reported.map(value => <ValueBlock key={value.rowId} value={value} period={measure.period} numbers={numbers} onOpen={onOpen} />)
    : <span className="earnings-empty">{before ? t("earnings.notReported") : "—"}</span>;
  const difference: ReactNode = measure.differences.length
    ? measure.differences.map((item, index) => <DifferenceBlock key={index} difference={item} />)
    : <span className="earnings-empty">{before ? "" : "—"}</span>;
  return { expected, reported: reportedCell, difference };
}

function NoExpectations({ snapshot, onOpenConnections }: { snapshot: EarningsSnapshot; onOpenConnections?: () => void }) {
  return <div className="earnings-none">
    <span>{t("earnings.noExpectations")}</span>
    {snapshot.consensusNeedsKey ? <span className="earnings-note">{t("earnings.consensusNeedsKey")}{onOpenConnections ? <> <button type="button" className="asset-inline-link" onClick={onOpenConnections}>{t("earnings.openConnections")}</button></> : null}</span> : null}
  </div>;
}

/** FILED SO FAR: each report with a filed / not-yet glyph, then revisions or "unchanged". */
function Chain({ snapshot, numbers, onOpen }: { snapshot: EarningsSnapshot; numbers: CitationNumbers; onOpen: Open }) {
  useTranslation("assets");
  return <>
    <ul className="earnings-chain">{snapshot.chain.map(step => <li key={step.stage} data-filed={step.filed || undefined}>
      {step.filed ? <CheckIcon className="earnings-glyph" aria-hidden="true" /> : <CircleIcon className="earnings-glyph" aria-hidden="true" />}
      <span className="sr-only">{t(step.filed ? "earnings.filed" : "earnings.notFiled")}</span>
      <span>{tDynamic(`earnings.stage.${step.stage}`)}{step.filed
        ? <>{step.date ? ` · ${shortDay(step.date)}` : ""}{chainForm(step.form, step.stage) ? ` (${chainForm(step.form, step.stage)})` : ""}</>
        : step.dueBy ? ` · ${t("earnings.dueBy", { date: shortDay(step.dueBy) })}` : ""}</span>
      {step.evidence.length ? <SourceChips evidence={step.evidence} numbers={numbers} onOpen={onOpen} /> : null}
    </li>)}</ul>
    {snapshot.revisions.map((revision, index) => <p key={index} className="earnings-revision">
      <TriangleAlertIcon className="earnings-glyph" aria-hidden="true" />
      <span>{t("earnings.revised", { measure: measureTitle({ key: revision.key, label: revision.label, period: "", expected: [], reported: [], differences: [] }), from: amountText(revision.from), to: amountText(revision.to) })}</span>
      <CalcChip calc={revision.calc} />
      <SourceChips evidence={revision.evidence} numbers={numbers} onOpen={onOpen} />
    </p>)}
    {snapshot.unchanged ? <p className="earnings-unchanged">{t("earnings.unchanged")}</p> : null}
  </>;
}

type Props = {
  bridge: LoupistBridge;
  loupistId: string;
  loupistName: string;
  snapshot: EarningsSnapshot;
  /** The asset's name in the interface language. */
  name: string;
  /** Source numbers shared with the page; the card numbers its own otherwise. */
  numbers?: CitationNumbers;
  /** `compact`: the chat card — header, table and FILED SO FAR only. */
  variant?: "full" | "compact";
  onOpenEvidence: Open;
  /** Fills a new topic Chat's composer; never sends. */
  onAsk?: (text: string) => void;
  onOpenConnections?: () => void;
  onChanged?: (snapshot: EarningsSnapshot) => void;
  /** Arrived from "See before and after": the card outlines itself once. */
  highlight?: boolean;
};

/**
 * The grid dialog's sources, numbered as on the card: the dated close beside
 * the price in the title, the statements beside the cash flow, as an answer's
 * card shows them.
 */
function gridSources(implied: NonNullable<EarningsSnapshot["implied"]>, numbers: CitationNumbers, onOpen: Open) {
  const close = implied.evidence.find(item => item.date === implied.closeDate) ?? implied.evidence[0];
  const statements = implied.evidence.filter(item => item !== close);
  return {
    price: close ? <SourceChips evidence={[close]} numbers={numbers} onOpen={onOpen} /> : undefined,
    inputs: statements.length ? { cash_flow: <SourceChips evidence={statements} numbers={numbers} onOpen={onOpen} /> } : undefined,
  };
}

/**
 * Results before and after (finance-earnings-snapshot.md §2.1): what was
 * expected the day before and what was reported, side by side, a source on
 * every number. Rust computes every difference; there is no beat/miss label.
 */
export function EarningsCard({ bridge, loupistId, loupistName, snapshot, name, numbers: shared, variant = "full", onOpenEvidence, onAsk, onOpenConnections, onChanged, highlight }: Props) {
  useTranslation(["assets", "chat"]);
  const titleId = useId();
  const online = useOnline();
  const narrow = useNarrow();
  const [current, setCurrent] = useState(snapshot);
  const [preparing, setPreparing] = useState(false);
  const [failed, setFailed] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const [allMeasures, setAllMeasures] = useState(false);
  const [arrived, setArrived] = useState(false);
  const [gridOpen, setGridOpen] = useState(false);
  // Arriving from "See before and after" marks the card once (1 s motion, or a still outline for 2 s).
  const [marked, setMarked] = useState(Boolean(highlight));
  useEffect(() => {
    if (!highlight) return;
    setMarked(true);
    const timer = window.setTimeout(() => setMarked(false), 2000);
    return () => window.clearTimeout(timer);
  }, [highlight]);
  useEffect(() => { setCurrent(snapshot); }, [snapshot]);
  const compact = variant === "compact";
  const numbers = shared ?? citationNumbers(snapshotEvidence(current, compact));
  const busy = preparing || current.preparing;
  const release = current.chain.find(step => (step.stage === "release" || step.stage === "preliminary") && step.filed);
  const releaseEvidence = release?.evidence ?? [];

  const prepare = async () => {
    if (busy || !online) return;
    setPreparing(true);
    setFailed(false);
    setAnnouncement(t("earnings.capturing", { name }));
    try {
      const next = await bridge.earningsPrepare({ loupistId, assetId: current.assetId, eventId: current.eventId });
      setArrived(true);
      setCurrent(next);
      onChanged?.(next);
      setAnnouncement(t("earnings.captured"));
    } catch {
      setFailed(true);
      setAnnouncement("");
    } finally {
      setPreparing(false);
    }
  };
  const questionSubject = `${name} (${current.symbol} · ${current.market})`;
  const ask = () => onAsk?.(t("earnings.askPrompt", { name, symbol: current.symbol, market: current.market, period: current.period }));

  const measures = current.measures.length ? current.measures : [{ period: current.period, expected: [], reported: [], differences: [] } satisfies EarningsMeasure];
  const shown = allMeasures ? measures : measures.slice(0, MEASURES);
  const hidden = measures.length - shown.length;
  const expectedHeader = current.before ? `${t("earnings.col.expected")} · ${shortDay(current.before.lateAsOf ?? new Date(current.before.capturedAtMs).toISOString().slice(0, 10))}` : t("earnings.col.expected");
  const reportedHeader = current.after ? `${t("earnings.col.reported")} · ${shortDay(current.after.date)}` : t("earnings.col.reported");
  const isReported = reported(current);
  const showFailure = failed || (current.failed?.side === "before" && !isReported);
  const offlineReason = online ? undefined : t("earnings.offline");

  const table = narrow
    ? <div className="earnings-stack" data-arrived={arrived || undefined}>
      {shown.map((measure, index) => {
        const cells = measureCells(current, measure, index === 0, numbers, onOpenEvidence, onOpenConnections);
        return <section key={`${measure.key ?? measure.label}:${measure.period}:${index}`} className="earnings-stack-measure" aria-label={measureTitle(measure) || t("earnings.col.measure")}>
          {measureTitle(measure) ? <h3>{measureTitle(measure)}</h3> : null}
          <dl>
            <div><dt>{expectedHeader}</dt><dd>{cells.expected}</dd></div>
            <div><dt>{reportedHeader}</dt><dd>{cells.reported}</dd></div>
            {measure.differences.length ? <div><dt>{t("earnings.col.difference")}</dt><dd>{cells.difference}</dd></div> : null}
          </dl>
        </section>;
      })}
    </div>
    : <table className="earnings-table" data-arrived={arrived || undefined}>
      <caption className="sr-only">{t("earnings.caption")}</caption>
      <thead><tr>
        <th scope="col">{t("earnings.col.measure")}</th>
        <th scope="col" className="gg-num">{expectedHeader}</th>
        <th scope="col" className="gg-num">{reportedHeader}</th>
        <th scope="col">{t("earnings.col.difference")}</th>
      </tr></thead>
      <tbody>{shown.map((measure, index) => {
        const cells = measureCells(current, measure, index === 0, numbers, onOpenEvidence, onOpenConnections);
        return <tr key={`${measure.key ?? measure.label}:${measure.period}:${index}`}>
          <th scope="row">{measureTitle(measure) || <span className="earnings-empty">—</span>}{measure.basis ? <small className="earnings-basis">{basisLabel(measure.basis)}</small> : null}</th>
          <td>{cells.expected}</td>
          <td>{cells.reported}</td>
          <td>{cells.difference}</td>
        </tr>;
      })}</tbody>
    </table>;

  const direction = current.reaction ? (current.reaction.to > current.reaction.from ? "up" : current.reaction.to < current.reaction.from ? "down" : undefined) : undefined;
  const subRows: { key: string; label: string; content: ReactNode }[] = [];
  if (!compact && current.implied) {
    const implied = current.implied;
    // The range itself is the one calculation chip over the reverse-DCF grid.
    const low = `${formatNumber(implied.lowPct, { maximumFractionDigits: 1 })}%`;
    const high = `${formatNumber(implied.highPct, { maximumFractionDigits: 1 })}%`;
    const marked = t("earnings.impliedText", { low: LOW, high: HIGH, years: formatNumber(implied.years), date: shortDay(implied.closeDate), price: amount(implied.price, implied.currency) });
    const [before, rest = ""] = marked.split(LOW);
    const [between, after = ""] = rest.split(HIGH);
    subRows.push({ key: "implied", label: t("earnings.implied"), content: <>
      {before}<CalcChip calc={implied.calc}>{`${low}${between ?? ""}${high}`}</CalcChip>{after} <SourceChips evidence={implied.evidence} numbers={numbers} onOpen={onOpenEvidence} />
      {implied.grid ? <> <button type="button" className="earnings-grid-link" aria-haspopup="dialog" onClick={() => setGridOpen(true)}>{t("earnings.openGrid")}</button></> : null}
    </> });
  }
  if (!compact && isReported && (current.reaction || current.reactionWaiting)) {
    const reaction = current.reaction;
    subRows.push({ key: "reaction", label: t("earnings.reaction"), content: reaction ? <>
      {t("earnings.reactionText", { from: amount(reaction.from, reaction.currency), to: amount(reaction.to, reaction.currency), fromDate: shortDay(reaction.fromDate), toDate: shortDay(reaction.toDate) })}{" "}
      {/* The page's only price color: the reaction percentage. */}
      <span className="earnings-reaction" data-direction={direction}><CalcChip calc={reaction.calc} /></span> <SourceChips evidence={reaction.evidence} numbers={numbers} onOpen={onOpenEvidence} />
    </> : <span className="earnings-empty">{t("earnings.waitingClose")}</span> });
  }
  if (!compact && isReported) {
    subRows.push({ key: "tone", label: t("earnings.tone"), content: current.tone
      ? <>{t("earnings.toneRead", { loupist: loupistName })} {current.tone.text} <SourceChips evidence={current.tone.evidence} numbers={numbers} onOpen={onOpenEvidence} /></>
      : <span className="earnings-empty">{t("earnings.noTranscript")}</span> });
  }
  if (current.chain.length) subRows.push({ key: "chain", label: t("earnings.chain"), content: <Chain snapshot={current} numbers={numbers} onOpen={onOpenEvidence} /> });

  return <section className="earnings-card" data-variant={variant} data-status={current.status} data-highlight={marked || undefined} aria-labelledby={titleId} data-earnings-event={current.eventId}>
    <div className="earnings-head">
      {/* In an answer the card names the company; on its own page the page does. */}
      <h2 id={titleId} className="earnings-label">{compact ? t("earnings.titleNamed", { name, period: current.period }) : t("earnings.title", { period: current.period })}</h2>
      <p className="earnings-status">{earningsStatusText(current)}{releaseEvidence.length ? <> <SourceChips evidence={releaseEvidence} numbers={numbers} onOpen={onOpenEvidence} /></> : null}</p>
      {!compact ? <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button type="button" variant="ghost" size="icon-sm" className="earnings-menu" aria-label={t("page.options")}><EllipsisIcon aria-hidden="true" /></Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {releaseEvidence[0] ? <DropdownMenuItem onSelect={() => onOpenEvidence(releaseEvidence[0]!)}>{t("earnings.openRelease")}</DropdownMenuItem> : null}
          <DropdownMenuItem disabled={busy || !online} onSelect={() => void prepare()}>{t("earnings.prepareAgain")}</DropdownMenuItem>
          {onAsk ? <DropdownMenuItem onSelect={ask}>{t("earnings.ask")}</DropdownMenuItem> : null}
        </DropdownMenuContent>
      </DropdownMenu> : null}
    </div>
    {captureLine(current) ? <p className="earnings-capture">{captureLine(current)}</p> : null}
    {table}
    {hidden > 0 ? <Button type="button" variant="ghost" size="sm" className="earnings-more" onClick={() => setAllMeasures(true)}>{t("earnings.showMore", { formattedCount: formatNumber(hidden) })}</Button> : null}
    {current.consensusLate ? <p className="earnings-note">{t("earnings.consensusLate")}</p> : null}
    {current.consensusNeedsKey && !current.noGuidance ? <p className="earnings-note">{t("earnings.consensusNeedsKey")}{onOpenConnections ? <> <button type="button" className="asset-inline-link" onClick={onOpenConnections}>{t("earnings.openConnections")}</button></> : null}</p> : null}
    {subRows.length ? <dl className="earnings-subrows">{subRows.map(row => <div key={row.key} data-row={row.key}><dt>{row.label}</dt><dd>{row.content}</dd></div>)}</dl> : null}
    {showFailure && !compact ? <p className="earnings-failed" role="alert">{t("earnings.failed", { name })} <Button type="button" variant="link" size="sm" disabled={busy || !online} onClick={() => void prepare()}>{t("earnings.retry")}</Button></p> : null}
    {!compact ? <div className="earnings-actions">
      {current.status === "upcoming" && !current.before ? <span title={offlineReason}>
        <Button type="button" variant="outline" size="sm" className="earnings-prepare" disabled={busy || !online} aria-busy={busy || undefined} aria-description={offlineReason} onClick={() => void prepare()}>
          {busy ? <><Spinner />{t("earnings.preparing")}</> : t("earnings.prepare")}
        </Button>
      </span> : null}
      {onAsk ? <Button type="button" variant="outline" className="earnings-ask" onClick={ask}>{t("earnings.ask")}</Button> : null}
    </div> : null}
    <p className="sr-only" aria-live="polite">{announcement}</p>
    {!compact && current.implied?.grid ? <Dialog open={gridOpen} onOpenChange={setGridOpen}>
      <DialogContent size="md" className="sensitivity-dialog" aria-describedby={undefined} onOpenAutoFocus={focusDialogTitle}>
        {/* The whole reverse-DCF grid behind PRICE IMPLIED, at the same dated close. */}
        <SensitivityCard variant="dialog" subject={name} sources={gridSources(current.implied, numbers, onOpenEvidence)}
          grid={{ ...current.implied.grid, price: current.implied.grid.price ?? { value: current.implied.price, display: amount(current.implied.price, current.implied.currency), currency: current.implied.currency, date: current.implied.closeDate } }}
          onChangeRates={onAsk ? (prompt) => { setGridOpen(false); onAsk(`${questionSubject}: ${prompt}`); } : undefined} />
      </DialogContent>
    </Dialog> : null}
  </section>;
}

/**
 * The two most material measures in one line each — measure · expected →
 * reported · difference — for the Thread message, the briefing and the chat.
 */
export function EarningsMiniTable({ measures, numbers, onOpenEvidence }: { measures: readonly EarningsMeasure[]; numbers?: CitationNumbers; onOpenEvidence?: Open }) {
  useTranslation("assets");
  const own = numbers ?? citationNumbers(measures.slice(0, 2).flatMap(measure => [...measure.expected.map(value => value.evidence), ...measure.reported.map(value => value.evidence)]));
  return <table className="earnings-mini">
    <caption className="sr-only">{t("earnings.caption")}</caption>
    <tbody>{measures.slice(0, 2).map((measure, index) => {
      const expected = measure.expected[0];
      const value = measure.reported[0];
      const sources = [...(expected?.evidence ?? []), ...(value?.evidence ?? [])];
      return <tr key={index}>
        <th scope="row">{measureTitle(measure)}</th>
        <td className="gg-num">{expected ? amountText(expected) : "—"} → {value ? amountText(value) : "—"}{onOpenEvidence && sources.length ? <> <SourceChips evidence={sources} numbers={own} onOpen={onOpenEvidence} /></> : null}</td>
        <td>{measure.differences[0] ? <DifferenceBlock difference={measure.differences[0]} /> : null}</td>
      </tr>;
    })}</tbody>
  </table>;
}
