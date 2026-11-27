import { useId } from "react";
import { useTranslation } from "@/i18n";
import { formatDate, formatDateTime, formatNumber, formatTime } from "@/i18n/format";
import type { Schedule } from "../../contracts/generated";
import { Button } from "../ui/button";
import { SelectField, SelectItem } from "../ui/select";
import { friendlyTimeZone } from "./scheduledPresentation";

type Kind = Schedule["kind"];
const INTERVAL_HOURS = [1, 2, 3, 4, 6, 8, 12, 24];
/** 0 is Sunday, as the host's weekly `days` count them. */
const WEEK = [0, 1, 2, 3, 4, 5, 6];
const STEP_MINUTES = 15;

/** A clock time as the interface language writes it; the stored hour and minute keep their own zone. */
const clock = (hour: number, minute: number) => formatTime(Date.UTC(2000, 0, 1, hour, minute), "UTC");
/** 2023-01-01 was a Sunday; its week names the seven days in the interface language. */
const weekday = (day: number) => formatDate(Date.UTC(2023, 0, 1 + day), { weekday: "short", year: undefined, month: undefined, day: undefined, timeZone: "UTC" });

function withKind(current: Schedule, kind: Kind): Schedule {
  if (kind === current.kind) return current;
  const timed = current.kind === "daily" || current.kind === "weekly" ? current : null;
  const timezone = timed?.timezone ?? (Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
  const hour = timed?.hour ?? 6;
  const minute = timed?.minute ?? 30;
  if (kind === "daily") return { kind, hour, minute, timezone };
  if (kind === "weekly") return { kind, days: current.kind === "weekly" ? current.days : [1, 2, 3, 4, 5], hour, minute, timezone };
  if (kind === "interval") return { kind, every_minutes: current.kind === "interval" ? current.every_minutes : 24 * 60 };
  return current;
}

/**
 * When a program runs: daily, on chosen days, or every few hours, at a time in the schedule's own
 * time zone. A program that runs once keeps its time; it can switch to a repeating schedule.
 */
export function LearningCadenceField({ value, original, disabled, problem, onChange }: {
  value: Schedule;
  /** The stored schedule: a one-time program can return to it. */
  original: Schedule;
  disabled: boolean;
  problem?: string | null;
  onChange: (value: Schedule) => void;
}) {
  const { t, i18n } = useTranslation("briefings");
  const id = useId();
  const kinds: Kind[] = original.kind === "once" ? ["once", "daily", "weekly", "interval"] : ["daily", "weekly", "interval"];
  const timed = value.kind === "daily" || value.kind === "weekly" ? value : null;
  const times: Array<[number, number]> = [];
  for (let minutes = 0; minutes < 24 * 60; minutes += STEP_MINUTES) times.push([Math.floor(minutes / 60), minutes % 60]);
  if (timed && !times.some(([hour, minute]) => hour === timed.hour && minute === timed.minute)) {
    times.push([timed.hour, timed.minute]);
    times.sort((left, right) => left[0] * 60 + left[1] - (right[0] * 60 + right[1]));
  }
  const intervals = INTERVAL_HOURS.map(hours => hours * 60);
  if (value.kind === "interval" && !intervals.includes(value.every_minutes)) {
    intervals.push(value.every_minutes);
    intervals.sort((left, right) => left - right);
  }
  const intervalLabel = (minutes: number) => minutes % 60 === 0
    ? t("programs.editor.everyHours", { count: minutes / 60, formattedCount: formatNumber(minutes / 60) })
    : t("programs.editor.everyMinutes", { count: minutes, formattedCount: formatNumber(minutes) });
  return <div className="form-field" role="group" aria-labelledby={`${id}-label`} aria-describedby={problem ? `${id}-problem` : undefined} data-program-field="schedule">
    <span id={`${id}-label`} className="schedule-section-label">{t("schedule.learning.when")}</span>
    <SelectField aria-label={t("programs.editor.repeat")} value={value.kind} disabled={disabled} onValueChange={kind => onChange(kind === "once" ? original : withKind(value, kind as Kind))}>
      {kinds.map(kind => <SelectItem key={kind} value={kind}>{t(`programs.editor.kind.${kind}`)}</SelectItem>)}
    </SelectField>
    {value.kind === "weekly" ? <div className="schedule-subject-chips" role="group" aria-label={t("programs.editor.days")} data-program-days>
      {WEEK.map(day => {
        const on = value.days.includes(day);
        return <Button key={day} type="button" size="sm" variant={on ? "default" : "outline"} aria-pressed={on} disabled={disabled}
          onClick={() => onChange({ ...value, days: on ? value.days.filter(item => item !== day) : [...value.days, day].sort((a, b) => a - b) })}>{weekday(day)}</Button>;
      })}
    </div> : null}
    {timed ? <SelectField aria-label={t("programs.editor.time")} value={`${timed.hour}:${timed.minute}`} disabled={disabled} data-program-time
      onValueChange={next => { const [hour, minute] = next.split(":").map(Number); onChange({ ...timed, hour: hour!, minute: minute! }); }}>
      {times.map(([hour, minute]) => <SelectItem key={`${hour}:${minute}`} value={`${hour}:${minute}`}>{clock(hour, minute)}</SelectItem>)}
    </SelectField> : null}
    {value.kind === "interval" ? <SelectField aria-label={t("programs.editor.every")} value={String(value.every_minutes)} disabled={disabled}
      onValueChange={next => onChange({ kind: "interval", every_minutes: Number(next) })}>
      {intervals.map(minutes => <SelectItem key={minutes} value={String(minutes)}>{intervalLabel(minutes)}</SelectItem>)}
    </SelectField> : null}
    {value.kind === "once" ? <p className="field-hint">{t("programs.editor.onceAt", { date: formatDateTime(value.at_ms) })}</p> : null}
    {timed ? <p className="field-hint">{t("programs.editor.timeZone", { zone: friendlyTimeZone(timed.timezone, i18n.language) })}</p> : null}
    {problem ? <p id={`${id}-problem`} className="inline-error" role="alert">{problem}</p> : null}
  </div>;
}
