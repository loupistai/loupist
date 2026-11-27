import { useState } from "react";
import { CalendarClockIcon } from "lucide-react";
import type { WorldAsOfReport } from "../../contracts/knowledge";
import { EARLIEST_AS_OF, asOfProblem, formatAsOf, todayIso } from "../../chat/asOf";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { AppDateField } from "../ui/app-date-field";
import { Button } from "../ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

export type WorldAsOfControl = { date?: string; onChange: (date: string | undefined) => void };

/** The toolbar's "As of": a date field that offers no day after today, and Today to return. */
export function WorldAsOfButton({ control, disabled }: { control: WorldAsOfControl; disabled?: boolean }) {
  const { t } = useTranslation("knowledge");
  const [open, setOpen] = useState(false);
  const { date, onChange } = control;
  const label = date ? t("asOf.button", { date: formatAsOf(date) }) : t("asOf.label");
  return <Popover open={open} onOpenChange={setOpen}>
    <PopoverTrigger asChild><Button className="map-toolbar-button" variant="ghost" disabled={disabled} data-map-as-of aria-pressed={!!date} aria-label={label} title={label}>
      <CalendarClockIcon /><span className="map-control-label">{label}</span>
    </Button></PopoverTrigger>
    <PopoverContent align="start" className="map-filters" aria-label={t("asOf.field")}>
      <AppDateField value={date ?? ""} min={EARLIEST_AS_OF} max={todayIso()} aria-label={t("asOf.field")}
        onChange={day => { if (day && !asOfProblem(day)) onChange(day); }} />
      <Button variant="outline" size="sm" disabled={!date} onClick={() => { onChange(undefined); setOpen(false); }}>{t("asOf.today")}</Button>
    </PopoverContent>
  </Popover>;
}

/** "Showing what was published by {date}. {n} later facts are hidden." */
export function WorldAsOfBanner({ date, report }: { date: string; report?: WorldAsOfReport }) {
  const { t } = useTranslation("knowledge");
  const hidden = report?.facts ?? 0;
  return <div className="map-lens-banner" role="status" data-map-as-of-banner>
    <p>{t("asOf.banner", { date: formatAsOf(date, "long"), count: hidden, formattedCount: formatNumber(hidden) })}</p>
  </div>;
}
