import i18n, { useTranslation } from "@/i18n";
import "@/styles/answer-checks.css";
import { CalendarIcon, XIcon } from "lucide-react";
import { useId, useState, type KeyboardEvent } from "react";
import { asOfProblem, EARLIEST_AS_OF, formatAsOf, quickAsOf, todayIso } from "../../chat/asOf";
import { AppDateField } from "../ui/app-date-field";
import { Button } from "../ui/button";

const t = i18n.getFixedT(null, "chat");

/**
 * The `/asof` date panel (finance-point-in-time.md §2.1): it takes the
 * assist menu's frame. Enter in the field sets the date; Escape returns to
 * the menu (the composer handles it).
 */
export function AsOfDatePanel({ initial, onSet, onCancel, onBack }: {
  initial?: string;
  onSet: (date: string) => void;
  onCancel: () => void;
  /** Escape returns to the `/` menu. */
  onBack: () => void;
}) {
  useTranslation("chat");
  const [value, setValue] = useState(initial ?? "");
  const headingId = useId();
  const errorId = useId();
  const problem = value ? asOfProblem(value) : "invalid";
  const error = value && problem === "future" ? t("asOf.invalidFuture") : value && problem === "early" ? t("asOf.invalidEarly") : null;
  const set = () => { if (!problem) onSet(value); };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onBack();
    }
  };
  return <div className="composer-assist composer-asof-panel" role="group" aria-labelledby={headingId} onKeyDown={onKeyDown}>
    <p id={headingId} className="composer-asof-title">{t("asOf.panelTitle")}</p>
    <AppDateField autoFocus grouped={false} value={value} min={EARLIEST_AS_OF} max={todayIso()} aria-label={t("asOf.panelTitle")}
      aria-invalid={Boolean(error)} aria-describedby={error ? errorId : undefined}
      onChange={setValue}
      onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); set(); } }} />
    {error ? <p id={errorId} className="composer-asof-error" role="alert">{error}</p> : null}
    <div className="composer-asof-quick">
      <Button type="button" variant="ghost" size="sm" onClick={() => setValue(quickAsOf("month"))}>{t("asOf.quickMonth")}</Button>
      <Button type="button" variant="ghost" size="sm" onClick={() => setValue(quickAsOf("yearStart"))}>{t("asOf.quickYearStart")}</Button>
      <Button type="button" variant="ghost" size="sm" onClick={() => setValue(quickAsOf("year"))}>{t("asOf.quickYear")}</Button>
    </div>
    <p className="composer-asof-explain">{t("asOf.explain")}</p>
    <div className="composer-asof-actions">
      <Button type="button" variant="ghost" size="sm" onClick={onCancel}>{t("asOf.cancel")}</Button>
      <Button type="button" size="sm" disabled={Boolean(problem)} onClick={set}>{t("asOf.set")}</Button>
    </div>
  </div>;
}

/** The as-of chip, always first in the tray: the label reopens the panel, × removes the date. */
export function AsOfChip({ asOf, onEdit, onRemove }: { asOf: string; onEdit?: () => void; onRemove?: () => void }) {
  useTranslation("chat");
  const label = t("asOf.chip", { date: formatAsOf(asOf) });
  return <li className="composer-chip composer-asof-chip" data-kind="as_of">
    <button type="button" className="composer-asof-label" aria-label={`${label} · ${t("asOf.chipEdit")}`} aria-description={t("asOf.chip", { date: formatAsOf(asOf, "long") })} onClick={onEdit}>
      <CalendarIcon aria-hidden="true" />
      <span>{label}</span>
    </button>
    <button type="button" aria-label={t("asOf.remove")} onClick={onRemove}><XIcon aria-hidden="true" /></button>
  </li>;
}

/** The caption tag on an as-of request and its answer (§2.3). */
export function AsOfTag({ asOf, kind }: { asOf: string; kind: "request" | "answer" }) {
  useTranslation("chat");
  const text = kind === "request" ? t("asOf.chip", { date: formatAsOf(asOf) }) : t("asOf.knownAsOf", { date: formatAsOf(asOf) });
  return <span className="as-of-tag" data-kind={kind} title={kind === "answer" ? t("asOf.tooltip", { date: formatAsOf(asOf) }) : undefined}
    aria-label={t("asOf.chip", { date: formatAsOf(asOf, "long") })}>{text}</span>;
}
