import type { JournalInputs, JournalRecord } from "../../types";
import { useTranslation } from "../../i18n";
import { formatDate, formatMeasure, formatNumber } from "../../i18n/format";
import { todayIso } from "../../chat/asOf";
import { Button } from "../ui/button";
import { useLookBack } from "./lookBack";
import "@/styles/answer-checks.css";

/** A check's day as the app writes check dates: "Oct 1" this year, "Oct 1, 2025" otherwise. */
function checkDay(day: string) {
  const date = new Date(`${day}T12:00:00`);
  return formatDate(date, { month: "short", day: "numeric", year: date.getFullYear() === new Date().getFullYear() ? undefined : "numeric" });
}

/** A check still open after its date: "Overdue since Oct 1" instead of "To check by Oct 1". */
const isOverdue = (entry: JournalRecord) => Boolean(entry.check?.byDate && !entry.outcome && entry.check.byDate < todayIso());

export function journalCounts(entries: JournalRecord[]) {
  return entries.reduce((counts, entry) => {
    if (entry.archived) return counts;
    if (entry.outcome?.result === "came_true") counts.cameTrue += 1;
    else if (entry.outcome?.result === "did_not") counts.didNot += 1;
    else if (entry.outcome?.result === "unclear") counts.unclear += 1;
    else if (entry.check) counts.pending += 1;
    return counts;
  }, { cameTrue: 0, didNot: 0, unclear: 0, pending: 0 });
}
export function ResearchJournalStatus({ entry }: { entry: JournalRecord }) {
  const { t } = useTranslation("knowledge");
  return entry.outcome ? <span className={entry.outcome.result === "came_true" ? "journal-status-positive" : "journal-status"}>{t(`journal.result.${entry.outcome.result}`)}</span>
    : entry.check?.byDate ? <span className={isOverdue(entry) ? "journal-status journal-status-late" : "journal-status"}>{t(isOverdue(entry) ? "journal.overdueSince" : "journal.toCheckBy", { date: checkDay(entry.check.byDate) })}</span> : null;
}
export function ResearchJournalCheck({ entry }: { entry: JournalRecord }) {
  const { t } = useTranslation("knowledge");
  const check = entry.check;
  if (!check) return null;
  const threshold = formatMeasure(check.threshold, check.unit);
  // The user's own words for the check; the structured line only when there are none (never both).
  const line = check.claim.trim() ? null : check.byDate
    ? t(check.direction === "above" ? "journal.checkAbove" : "journal.checkBelow", { metric: check.metric, threshold, date: formatDate(new Date(`${check.byDate}T12:00:00`)) })
    : t(check.direction === "above" ? "journal.lineAbove" : "journal.lineBelow", { metric: check.metric, threshold });
  return <>
    <section className="journal-check-card" aria-label={t("journal.check")}><h3>{t("journal.check")}</h3>{check.claim.trim() ? <p>{check.claim}</p> : null}{line ? <p>{line}</p> : null}{check.confidence !== undefined ? <p className="journal-meta">{t("journal.userConfidence", { confidence: check.confidence })}</p> : null}</section>
  </>;
}

type JournalSource = JournalInputs["sources"][number];

/**
 * Then · Now for a Journal entry (finance-point-in-time.md §2.5): what the
 * user judged and with which saved sources, then what the check found. Look
 * back opens a new Chat with only what was known when the entry was written.
 */
export function JournalThenNow({ entry, onOpenSource }: { entry: JournalRecord; onOpenSource?: (source: JournalSource) => void }) {
  const { t } = useTranslation("knowledge");
  const lookBack = useLookBack();
  const day = (value: string) => formatDate(new Date(`${value}T12:00:00`));
  const check = entry.check;
  const outcome = entry.outcome;
  const late = isOverdue(entry);
  // Only a reviewed entry was checked; an open one says so instead of showing today's date.
  const now = outcome ? t("journal.now", { date: formatDate(entry.updatedAtMs) }) : t("journal.nowUnchecked");
  return <div className="journal-then-now">
    <section aria-label={t("journal.then", { date: formatDate(entry.createdAtMs) })}>
      <h3>{t("journal.then", { date: formatDate(entry.createdAtMs) })}</h3>
      <p>{t("journal.youJudged", { judgment: entry.judgment })}</p>
      {entry.inputs.sources.length ? <p className="journal-then-now-with">
        <span>{t("journal.with")}</span>
        {entry.inputs.sources.map((source, index) => <button key={source.receiptId} type="button" className="composer-chip" data-kind="source" onClick={() => onOpenSource?.(source)}>
          <span>{source.attribution || t("researchReader.sourceNumber", { index: formatNumber(index + 1) })}</span>
        </button>)}
      </p> : <p className="journal-then-now-empty">{t("journal.noInputs")}</p>}
      {lookBack ? <Button type="button" variant="outline" size="sm" onClick={() => lookBack({ loupistId: entry.loupistId, journalId: entry.journalId, judgment: entry.judgment, createdAtMs: entry.createdAtMs })}>{t("journal.lookBack")}</Button> : null}
    </section>
    <section aria-label={now}>
      <h3>{now}</h3>
      {outcome && check ? <div className="journal-outcome-card" aria-label={t("journal.outcome")}>
        <p className={outcome.result === "came_true" ? "journal-status-positive" : "journal-status"}>{t(`journal.observed.${outcome.result}`, { date: day(outcome.observedAt), metric: check.metric, observed: formatMeasure(outcome.observed, check.unit) })}</p>
        {outcome.note ? <p>{outcome.note}</p> : null}
        {outcome.reviewedAsOf ? <p className="journal-then-now-reviewed">{t("journal.reviewedAsOf", { date: day(outcome.reviewedAsOf) })}</p> : null}
      </div> : check?.byDate ? <p className={late ? "journal-then-now-late" : undefined}>{t(late ? "journal.overdueSince" : "journal.toCheckBy", { date: checkDay(check.byDate) })}</p>
        : <p className="journal-then-now-empty">{t("journal.noCheck")}</p>}
    </section>
  </div>;
}
