import i18n from "@/i18n";
import { formatDate, formatDateTime, formatNumber, formatRelative, formatTime } from "@/i18n/format";
import type { ScheduledCadence } from "../../types";

export const weekdays = () => Array.from({ length: 7 }, (_, day) => formatDate(new Date(2023, 0, 1 + day), { weekday: "short", year: undefined, month: undefined, day: undefined }));
const WEEKDAY_PRESET = [1, 2, 3, 4, 5];
export const deviceTimeZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
export function friendlyTimeZone(timeZone: string, locale = "en") {
  const language = locale.toLowerCase() === "en-xa" ? "en" : locale;
  try {
    const name = new Intl.DateTimeFormat(language, { timeZone, timeZoneName: "long" }).formatToParts(new Date()).find(part => part.type === "timeZoneName")?.value;
    if (!name || name === timeZone || /^(?:GMT|UTC)\b/i.test(name)) return timeZone;
    return name;
  } catch {
    return timeZone;
  }
}
const isWeekdays = (days: number[]) => [...days].sort((left, right) => left - right).join(",") === WEEKDAY_PRESET.join(",");
const clockLabel = (hour: number, minute: number) => formatTime(new Date(2000, 0, 1, hour, minute));
const scheduledTime = (at: number | null | undefined, timezone?: string) => at == null ? "—" : formatDateTime(at, { dateStyle: "medium", timeStyle: "short", ...(timezone ? { timeZone: timezone } : {}) });
/** Titles Rust stores for the optional briefing and the automatic results task. A saved rename is left alone. */
const HOST_BRIEFING_TITLE = "Briefings";
const HOST_EARNINGS_TITLE = "Results before and after";

/**
 * The name shown for a scheduled task. A title the user or the Loupist saved is
 * kept. The host defaults are shown in the reader's language, without a morning-only label.
 */
export function scheduledTaskTitle(kind: string, title: string) {
  const saved = title.trim();
  if (kind === "briefing" && (!saved || saved === HOST_BRIEFING_TITLE)) return i18n.t("briefings:schedule.kind.briefing");
  if (kind === "earnings" && (!saved || saved === HOST_EARNINGS_TITLE)) return i18n.t("briefings:schedule.earningsTask");
  return title;
}

export const cadencePhrase = (schedule: ScheduledCadence) => {
  if (schedule.kind === "once") return i18n.t("briefings:presentation.once", { time: scheduledTime(schedule.at_ms) });
  if (schedule.kind === "interval") return i18n.t("briefings:presentation.interval", { count: schedule.every_minutes, countLabel: formatNumber(schedule.every_minutes) });
  const clock = clockLabel(schedule.hour, schedule.minute);
  if (schedule.kind === "daily") return i18n.t("briefings:presentation.daily", { clock });
  const days = [...schedule.days].sort((left, right) => left - right);
  if (isWeekdays(days)) return i18n.t("briefings:presentation.weekdays", { clock });
  if (days.length === 7) return i18n.t("briefings:presentation.everyDay", { clock });
  const labels = weekdays();
  if (days.length === 1) return i18n.t("briefings:presentation.weekly", { day: labels[days[0]], clock });
  return i18n.t("briefings:presentation.selectedDays", { days: days.map((day) => labels[day]).filter(Boolean).join(", "), clock });
};
export const cadenceLabel = (schedule: ScheduledCadence) => {
  if (schedule.kind !== "daily" && schedule.kind !== "weekly") return cadencePhrase(schedule);
  // People read "Korean Standard Time", not an IANA id.
  return i18n.t("briefings:presentation.zonedCadence", { cadence: cadencePhrase(schedule), timezone: friendlyTimeZone(schedule.timezone, i18n.language) });
};
export function relativeRun(at: number | null | undefined, now = Date.now(), timezone?: string) {
  if (at == null) return "—";
  const zone = timezone ? { timeZone: timezone } : {};
  const dayKey = (value: number) => formatDate(value, { year: "numeric", month: "2-digit", day: "2-digit", ...zone });
  const time = formatDateTime(at, { timeStyle: "short", ...zone });
  const target = dayKey(at);
  for (const offset of [0, 1, -1]) {
    if (target === dayKey(now + offset * 86_400_000)) {
      const relative = formatRelative(offset, "day");
      return i18n.t("briefings:presentation.relativeTime", { day: relative.charAt(0).toUpperCase() + relative.slice(1), time });
    }
  }
  if (at > now && at - now < 6 * 86_400_000) return i18n.t("briefings:presentation.relativeTime", { day: formatDate(at, { weekday: "long", year: undefined, month: undefined, day: undefined, ...zone }), time });
  return scheduledTime(at, timezone);
}
