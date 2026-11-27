import { useState } from "react";
import { useTranslation } from "@/i18n";
import { SelectField, SelectItem } from "./select";

const ISO_DAY = /^(\d{4})-(\d{2})-(\d{2})$/u;
const languageOf = (language: string) => language === "en-XA" ? "en" : language || "en";

function parts(value: string) {
  const match = ISO_DAY.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) return null;
  return { year, month, day };
}

const daysInMonth = (year: number, month: number) => new Date(year, month, 0).getDate();
const iso = (year: number, month: number, day: number) => `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;

type Props = {
  id?: string;
  value: string;
  min?: string;
  max?: string;
  required?: boolean;
  disabled?: boolean;
  "aria-label": string;
  "aria-invalid"?: boolean;
  "aria-describedby"?: string;
  className?: string;
  autoFocus?: boolean;
  /** Standalone fields name the three menus as one group. A field already inside a named group leaves that name to its parent. */
  grouped?: boolean;
  onChange: (value: string) => void;
  /** Whether some but not all of the three parts are chosen: `value` is then empty, but the field is not. */
  onIncomplete?: (incomplete: boolean) => void;
};

/** Year, month and day in the app language. A native date input follows the operating system. */
export function AppDateField({ id, value, min, max, required, disabled, className, autoFocus, grouped = true, onChange, onIncomplete, ...props }: Props) {
  const { t, i18n } = useTranslation("common");
  const language = languageOf(i18n.language);
  const parsed = parts(value);
  // A year chosen before the month would otherwise snap back, because the parent value stays empty until all three are set.
  const [draft, setDraft] = useState<{ year: number | ""; month: number | ""; day: number | "" } | null>(null);
  if (parsed && draft && (draft.year !== parsed.year || draft.month !== parsed.month || draft.day !== parsed.day)) setDraft(null);
  const current = draft ?? parsed;
  const earliest = parts(min ?? "") ?? { year: new Date().getFullYear() - 30, month: 1, day: 1 };
  const latest = parts(max ?? "") ?? { year: new Date().getFullYear() + 20, month: 12, day: 31 };
  const year = current?.year ?? "";
  const month = current?.month ?? "";
  const day = current?.day ?? "";
  const years = [];
  for (let item = earliest.year; item <= latest.year; item += 1) years.push(item);
  if (typeof year === "number" && !years.includes(year)) years.push(year);
  years.sort((left, right) => left - right);
  const months = Array.from({ length: 12 }, (_, index) => index + 1).filter((item) => {
    if (year === "") return true;
    if (year === earliest.year && item < earliest.month) return false;
    if (year === latest.year && item > latest.month) return false;
    return true;
  });
  if (typeof month === "number" && !months.includes(month)) months.push(month);
  months.sort((left, right) => left - right);
  const monthCount = year !== "" && month !== "" ? daysInMonth(year, month) : 31;
  const days = Array.from({ length: monthCount }, (_, index) => index + 1).filter((item) => {
    if (year === "" || month === "") return true;
    if (year === earliest.year && month === earliest.month && item < earliest.day) return false;
    if (year === latest.year && month === latest.month && item > latest.day) return false;
    return true;
  });
  if (typeof day === "number" && !days.includes(day)) days.push(day);
  days.sort((left, right) => left - right);
  const monthName = (item: number) => new Intl.DateTimeFormat(language, { month: "short" }).format(new Date(2026, item - 1, 1));
  const commit = (nextYear: number | "", nextMonth: number | "", nextDay: number | "") => {
    if (nextYear === "" || nextMonth === "" || nextDay === "") {
      setDraft({ year: nextYear, month: nextMonth, day: nextDay });
      onIncomplete?.(nextYear !== "" || nextMonth !== "" || nextDay !== "");
      if (value) onChange("");
      return;
    }
    setDraft(null);
    onIncomplete?.(false);
    let nextMonthClamped = nextMonth;
    let nextDayClamped = nextDay;
    if (nextYear === earliest.year) nextMonthClamped = Math.max(nextMonthClamped, earliest.month);
    if (nextYear === latest.year) nextMonthClamped = Math.min(nextMonthClamped, latest.month);
    nextDayClamped = Math.min(nextDayClamped, daysInMonth(nextYear, nextMonthClamped));
    if (nextYear === earliest.year && nextMonthClamped === earliest.month) nextDayClamped = Math.max(nextDayClamped, earliest.day);
    if (nextYear === latest.year && nextMonthClamped === latest.month) nextDayClamped = Math.min(nextDayClamped, latest.day);
    onChange(iso(nextYear, nextMonthClamped, nextDayClamped));
  };
  const described = props["aria-invalid"] || props["aria-describedby"] ? { "aria-invalid": props["aria-invalid"] || undefined, "aria-describedby": props["aria-describedby"] } : {};
  return <div className={className ? `app-date-field ${className}` : "app-date-field"} role={grouped ? "group" : undefined} aria-label={grouped ? props["aria-label"] : undefined} aria-invalid={grouped ? props["aria-invalid"] || undefined : undefined} aria-describedby={grouped ? props["aria-describedby"] : undefined}>
    <SelectField id={id} aria-label={t("date.year")} disabled={disabled} required={required} autoFocus={autoFocus} value={String(year)} placeholder={t("date.year")} {...described} onValueChange={value => commit(value === "" ? "" : Number(value), month, day)}>
      <SelectItem value="">{t("date.year")}</SelectItem>
      {years.map((item) => <SelectItem key={item} value={String(item)}>{item}</SelectItem>)}
    </SelectField>
    <SelectField aria-label={t("date.month")} disabled={disabled} required={required} value={String(month)} placeholder={t("date.month")} {...described} onValueChange={value => commit(year, value === "" ? "" : Number(value), day)}>
      <SelectItem value="">{t("date.month")}</SelectItem>
      {months.map((item) => <SelectItem key={item} value={String(item)}>{monthName(item)}</SelectItem>)}
    </SelectField>
    <SelectField aria-label={t("date.day")} disabled={disabled} required={required} value={String(day)} placeholder={t("date.day")} {...described} onValueChange={value => commit(year, month, value === "" ? "" : Number(value))}>
      <SelectItem value="">{t("date.day")}</SelectItem>
      {days.map((item) => <SelectItem key={item} value={String(item)}>{item}</SelectItem>)}
    </SelectField>
  </div>;
}
