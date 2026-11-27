import i18n from "@/i18n";
import { connectionText } from "@/lib/connectionCopy";
import type { ChatToolRefusal } from "../../contracts/chat";
import type { SkippedTool } from "../../contracts/scheduled";
import type { LearningBudget, LearningProgram, LearningScope, TaskSubject, WatchPreset } from "../../contracts/generated";
import { formatDate, formatDateTime, formatNumber, formatRelative, formatTime } from "@/i18n/format";
import type { ScheduledCadence } from "../../types";

/** Locale key a bundled check shows until a saved `when_met_summary` replaces it. */
const PRESET_WHEN_MET_KEY = {
  "price-move": "schedule.preset.priceMove.whenMet",
  "fact-changes": "schedule.preset.factChanges.whenMet",
} as const satisfies Partial<Record<WatchPreset, string>>;

const PRESET_WHEN_MET_KEYS = new Set<string>(Object.values(PRESET_WHEN_MET_KEY));

/**
 * The one line under "When it is met". A saved sentence wins. A preset key, or a
 * trigger saved before the field, uses that preset's locale line. Anything else
 * is one generic line.
 */
export function whenMetSummary(stored: string | null | undefined, preset?: WatchPreset | null) {
  const line = stored?.trim() ?? "";
  if (line && !PRESET_WHEN_MET_KEYS.has(line)) return line;
  if (line === PRESET_WHEN_MET_KEY["price-move"] || preset === "price-move") {
    return i18n.t("briefings:schedule.preset.priceMove.whenMet");
  }
  if (line === PRESET_WHEN_MET_KEY["fact-changes"] || preset === "fact-changes") {
    return i18n.t("briefings:schedule.preset.factChanges.whenMet");
  }
  return i18n.t("briefings:schedule.check.whenMetGeneric");
}

/** A task with a check tells the user when that check is met, whatever its report policy. */
export function tellsYouKey(task: { trigger?: unknown; report?: string | null }) {
  if (task.trigger) return "schedule.whenCheckMet" as const;
  return task.report === "when_met" ? "schedule.whenMet" as const : "schedule.everyRun" as const;
}

/** A learning program stays learning. A task with a check is a check. A briefing stays a briefing. Other tasks are scheduled research. */
export function taskKindKey(task: { kind: string; trigger?: unknown }) {
  if (task.kind === "learning") return "schedule.kind.learning" as const;
  if (task.kind === "briefing") return "schedule.kind.briefing" as const;
  if (task.trigger) return "schedule.kind.check" as const;
  return "schedule.kind.custom" as const;
}

export type ScheduleSection = "briefings" | "checks" | "research" | "learning";

/** Briefings, checks, scheduled research, then learning. The same order under Active and All. */
export const SCHEDULE_SECTIONS = ["briefings", "checks", "research", "learning"] as const satisfies readonly ScheduleSection[];

export function scheduleSection(task: { kind: string; trigger?: unknown }): ScheduleSection {
  if (task.kind === "briefing") return "briefings";
  if (task.kind === "learning") return "learning";
  if (task.trigger) return "checks";
  return "research";
}

/** A run is worded by its task's kind: a briefing, a learning program, or scheduled research. */
export function runWording(kind: string): "briefing" | "learning" | "custom" {
  if (kind === "briefing") return "briefing";
  if (kind === "learning") return "learning";
  return "custom";
}

/** Price moves is a check: no model run until it is met. Other presets are model runs. */
export function presetRunsOnCheck(preset: WatchPreset) {
  return preset === "price-move" || preset === "fact-changes";
}

export type ArgumentRow = { key: string; label: string; value: string };

/** Label and value rows for a check's arguments. `asset_ids` use asset names. */
export function triggerArgumentRows(
  args: Record<string, unknown>,
  names: ReadonlyMap<string, string> | null,
): ArgumentRow[] {
  return Object.entries(args).map(([key, value]) => ({
    key,
    label: key === "asset_ids" || key === "asset_names" ? i18n.t("briefings:schedule.check.assets") : key,
    value: key === "asset_ids" ? assetIdList(value, names) : argumentValue(value),
  }));
}

function assetIdList(value: unknown, names: ReadonlyMap<string, string> | null) {
  const ids = Array.isArray(value) ? value.flatMap(item => typeof item === "string" ? [item] : []) : [];
  if (names === null) return ids.join(", ");
  const removed = i18n.t("briefings:schedule.check.removedAsset");
  return ids.map(id => names.get(id) ?? removed).join(", ");
}

function argumentValue(value: unknown): string {
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") return String(value);
  if (Array.isArray(value)) {
    return value.map(item => typeof item === "string" || typeof item === "number" || typeof item === "boolean" ? String(item) : JSON.stringify(item)).join(", ");
  }
  if (value == null) return "";
  return JSON.stringify(value);
}

/** Days a dry run says the check would have been met, with the reading it returned. */
export function metDryRunDays<T extends { outcome: string }>(days: readonly T[]) {
  return days.filter(day => day.outcome === "met");
}

export const unreadableTask = (task: { prompt: string; last_error: string | null }) =>
  task.prompt === "" && task.last_error === "unreadable_task";

export function skippedToolNames(tools: readonly SkippedTool[]) {
  return {
    ordinary: tools.filter(tool => tool.reason !== "keychain_permission_required").map(tool => tool.name),
    savedKeys: tools.filter(tool => tool.reason === "keychain_permission_required").map(tool => tool.name),
  };
}

export function skippedSavedKeyNames(refusals: readonly ChatToolRefusal[] = []) {
  return refusals.filter(refusal => refusal.reason === "saved_key_permission").map(refusal => refusal.connection_name);
}

export function skippedSavedKeyLabel(names: readonly string[] = []) {
  return names.length ? i18n.t("briefings:skippedSavedKeys", { names: names.map(connectionText).join(", ") }) : null;
}

export const weekdays = () => Array.from({ length: 7 }, (_, day) => formatDate(new Date(2023, 0, 1 + day), { weekday: "short", year: undefined, month: undefined, day: undefined }));
const WEEKDAY_PRESET = [1, 2, 3, 4, 5];
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
/** The title Rust stores for the optional briefing. A saved rename is left alone. */
const HOST_BRIEFING_TITLE = "Briefings";

/**
 * The name shown for a scheduled task. A title the user or the Loupist saved is
 * kept. The host defaults are shown in the reader's language, without a morning-only label.
 */
const HOST_EVENING_TITLE = "Evening briefing";

export function scheduledTaskTitle(kind: string, title: string, edition?: string | null) {
  const saved = title.trim();
  if (kind === "briefing" && (edition === "evening" || saved === HOST_EVENING_TITLE)) return i18n.t("briefings:evening");
  if (kind === "briefing" && (!saved || saved === HOST_BRIEFING_TITLE)) return i18n.t("briefings:schedule.kind.briefing");
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

/** Weekday 06:30. Activation replaces the preset's UTC fallback with the host's local zone. */
export const LEARNING_DEFAULT_DAYS = [1, 2, 3, 4, 5];
export const LEARNING_DEFAULT_HOUR = 6;
export const LEARNING_DEFAULT_MINUTE = 30;
export const ASSET_DAILY_PRESET = "asset-daily";

export function learningDefaultCadence(timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC") {
  return cadenceLabel({ kind: "weekly", days: [...LEARNING_DEFAULT_DAYS], hour: LEARNING_DEFAULT_HOUR, minute: LEARNING_DEFAULT_MINUTE, timezone: timeZone });
}

export type AllowanceBucket = { limit: number; used: number; state: string };

/** The Schedule header line for today's background allowance. Warning ink is only the 80% line. */
export function allowanceHeader(bucket: AllowanceBucket | null | undefined): { tone: "warning" | "plain"; text: string } | null {
  if (!bucket || bucket.limit <= 0 || bucket.state === "off" || bucket.state === "ok") return null;
  if (bucket.state === "reached" || bucket.used >= bucket.limit) return { tone: "plain", text: i18n.t("briefings:schedule.learning.allowanceWaiting") };
  if (bucket.state !== "warning") return null;
  const pct = Math.min(99, Math.max(0, Math.round((Number(bucket.used) * 100) / bucket.limit)));
  return { tone: "warning", text: i18n.t("briefings:schedule.learning.allowanceUsed", { pct: formatNumber(pct) }) };
}

/** Why a held task or source is waiting. Allowance and the provider limits each have one sentence. */
export function holdSentence(reason: string | null | undefined): string | null {
  if (reason === "allowance" || reason === "daily_allowance") return i18n.t("briefings:schedule.learning.allowanceWaiting");
  if (reason === "plan_limit" || reason === "out_of_credit") return i18n.t("briefings:schedule.learning.holdPlan");
  return null;
}

/** What Run now asks before it starts a held task. An allowance hold uses its own sentence. */
export function runNowConfirmText(reason: string | null | undefined): string | null {
  if (!holdSentence(reason)) return null;
  if (reason === "allowance" || reason === "daily_allowance") return i18n.t("briefings:schedule.learning.beyondAllowance");
  return holdSentence(reason);
}

/**
 * The confirmation sentence when the host refuses Run now with `ask_first`.
 * The task may not have a stored hold yet: the allowance can run out at the claim.
 */
export function runNowRefusal(cause: unknown): string | null {
  if (!cause || typeof cause !== "object") return null;
  const coded = cause as { code?: unknown; detail?: unknown };
  if (coded.code !== "conflict" || !coded.detail || typeof coded.detail !== "object") return null;
  const detail = coded.detail as { action?: unknown; hold?: { reason?: unknown } };
  if (detail.action !== "ask_first") return null;
  return runNowConfirmText(typeof detail.hold?.reason === "string" ? detail.hold.reason : null);
}

type UsageRun = {
  status: string;
  quiet?: boolean;
  usage?: { requests?: number | null } | null;
  budget?: { requests?: number; outcome?: string | null } | null;
};

/** A refused or cancelled budget stopped at its request limit. */
export function stoppedForBudget(outcome: string | null | undefined) {
  return outcome === "refused" || outcome === "cancelled";
}

/** "{requests} requests · {outcome}". A refused or cancelled budget stops at its request limit. */
export function runUsageText(run: UsageRun): string | null {
  const recorded = run.usage?.requests;
  const requests = typeof recorded === "number" ? recorded : run.budget?.requests;
  if (typeof requests !== "number" || !Number.isFinite(requests)) return null;
  const outcome = run.budget?.outcome;
  const key = stoppedForBudget(outcome) ? "briefings:schedule.learning.usageOutcome.limit"
    : run.status === "running" ? "briefings:schedule.learning.usageOutcome.running"
    : run.status === "failed" || run.status === "interrupted" ? "briefings:schedule.learning.usageOutcome.failed"
    : run.quiet ? "briefings:schedule.learning.usageOutcome.quiet"
    : "briefings:schedule.learning.usageOutcome.finished";
  return i18n.t("briefings:schedule.learning.usage", {
    count: requests,
    formattedCount: formatNumber(requests),
    outcome: i18n.t(key),
  });
}

export type LearningRunSnapshot = {
  status: string;
  hold?: { reason?: string | null } | null;
  latest_run?: {
    status?: string | null;
    quiet?: boolean;
    summary?: string | null;
    budget?: { outcome?: string | null } | null;
  } | null;
};

/**
 * The one last-outcome line for a learning row. A running program, a hold, another
 * program already running, and a budget stop each win over the run's summary.
 */
export function learningRowOutcome(task: LearningRunSnapshot, anotherRunning: boolean): string | null {
  const run = task.latest_run;
  if (run?.status === "running") return i18n.t("briefings:schedule.runningNow");
  const held = holdSentence(task.hold?.reason);
  if (held) return held;
  if (anotherRunning && task.status === "active") return i18n.t("briefings:schedule.learning.waitingOther");
  if (stoppedForBudget(run?.budget?.outcome)) return i18n.t("briefings:schedule.learning.stoppedLimit");
  if (run && (run.status === "failed" || run.status === "interrupted")) return i18n.t("briefings:failure.title.learning");
  if (run?.quiet && run.status === "succeeded") return i18n.t("briefings:schedule.quiet");
  const summary = run?.summary?.trim();
  return summary || null;
}

/** Page outcomes the reader can say. Anything else, including a section key, stays off the screen. */
export function learnedOutcomeText(outcomes: readonly { outcome?: string | null }[]): string | null {
  const labels = [...new Set(outcomes.flatMap(item => {
    if (item.outcome === "added") return [i18n.t("briefings:schedule.learning.outcome.added")];
    if (item.outcome === "updated") return [i18n.t("briefings:schedule.learning.outcome.updated")];
    if (item.outcome === "retired") return [i18n.t("briefings:schedule.learning.outcome.retired")];
    return [];
  }))];
  return labels.length ? labels.join(", ") : null;
}

export function learnedThingCount(pages: readonly { title?: string | null; outcomes?: readonly unknown[] }[]): number {
  return pages.reduce((sum, page) => sum + Math.max(page.outcomes?.length ?? 0, page.title?.trim() ? 1 : 0), 0);
}

export type SubjectChipModel = { key: string; label: string; assetId?: string };

/** Three subject chips on a row, then a count. A missing asset is not opened and is not shown as an id. */
export function subjectChipModel(subjects: readonly TaskSubject[] | undefined, names: ReadonlyMap<string, string>, limit = 3): { shown: SubjectChipModel[]; extra: number } {
  const removed = i18n.t("briefings:schedule.check.removedAsset");
  const all = (subjects ?? []).map((subject, index): SubjectChipModel => {
    if ("asset" in subject) {
      const label = names.get(subject.asset);
      return { key: `asset:${subject.asset}`, label: label || removed, ...(label ? { assetId: subject.asset } : {}) };
    }
    if ("name" in subject) return { key: `name:${index}`, label: subject.name };
    if ("world" in subject) return { key: `world:${subject.world}`, label: i18n.t("briefings:schedule.learning.subjectWorld") };
    const id = "lens" in subject ? subject.lens : String(index);
    return { key: `lens:${id}`, label: i18n.t("briefings:schedule.learning.subjectLens") };
  });
  const cap = Math.max(0, limit);
  return { shown: all.slice(0, cap), extra: Math.max(0, all.length - cap) };
}

export function learningScopeText(scope: LearningScope): string {
  const connections = scope.connections === "all_enabled"
    ? i18n.t("briefings:schedule.learning.scope.all")
    : i18n.t("briefings:schedule.learning.scope.some", { count: scope.connections.length, formattedCount: formatNumber(scope.connections.length) });
  const web = i18n.t(scope.web ? "briefings:schedule.learning.scope.webOn" : "briefings:schedule.learning.scope.webOff");
  return `${connections} · ${web}`;
}

export function learningKeepsText(keeps: LearningProgram["keeps"]): string {
  const parts = [
    keeps.sources ? i18n.t("briefings:schedule.learning.keep.sources") : null,
    keeps.lens ? i18n.t("briefings:schedule.learning.keep.lens") : null,
    keeps.asset_reads ? i18n.t("briefings:schedule.learning.keep.reads") : null,
  ].filter((part): part is string => Boolean(part));
  return parts.length ? parts.join(", ") : i18n.t("briefings:schedule.learning.keep.none");
}

export function learningReportText(report: string | null | undefined): string {
  if (report === "every_run") return i18n.t("briefings:schedule.everyRun");
  if (report === "when_met") return i18n.t("briefings:schedule.whenMet");
  if (report === "on_condition") return i18n.t("briefings:schedule.learning.report.condition");
  return i18n.t("briefings:schedule.learning.report.whenNew");
}

export function learningDeliveryText(to: string | null | undefined): string {
  if (to === "none") return i18n.t("briefings:schedule.postSchedule");
  if (to === "briefing") return i18n.t("briefings:schedule.learning.delivery.briefing");
  return i18n.t("briefings:schedule.postBot");
}

export function learningBudgetText(budget: LearningBudget | null | undefined): string | null {
  if (!budget) return null;
  return i18n.t("briefings:schedule.learning.budget", { soft: formatNumber(budget.soft), hard: formatNumber(budget.hard) });
}

export function learningDepthText(depth: LearningProgram["depth"] | null | undefined): string | null {
  if (depth === "light") return i18n.t("briefings:schedule.learning.depth.light");
  if (depth === "standard") return i18n.t("briefings:schedule.learning.depth.standard");
  if (depth === "deep") return i18n.t("briefings:schedule.learning.depth.deep");
  return null;
}

/** The cost note on a program proposal. Without a resolved budget the request count is left out. */
export function learningCostNote(budget: { hard?: number | null } | null | undefined) {
  const count = budget?.hard;
  if (typeof count !== "number" || !Number.isFinite(count)) return i18n.t("chat:messageSchedules.learningProgram.costPlain");
  return i18n.t("chat:messageSchedules.learningProgram.cost", { count, formattedCount: formatNumber(count) });
}

/** The host's cap refusal, so the card can say it in the interface language. */
/** The Loupist's cap of active learning programs, when the host refused a turn-on for it; else null. */
export function learningCapReached(message: string): number | null {
  const cap = /up to (\d+) learning programs/iu.exec(message)?.[1];
  return cap ? Number(cap) : null;
}

/** A learning preset's name, for display; null for an id this build does not know (never the raw id). */
export function learningPresetText(preset: string | null | undefined): string | null {
  switch (preset) {
    case "asset-daily": return i18n.t("briefings:schedule.learning.trackDaily");
    case "theme": return i18n.t("briefings:schedule.learning.preset.theme");
    case "company-filings": return i18n.t("briefings:schedule.learning.preset.companyFilings");
    case "macro-series": return i18n.t("briefings:schedule.learning.preset.macroSeries");
    case "question": return i18n.t("briefings:schedule.learning.preset.question");
    case "market": return i18n.t("briefings:schedule.learning.preset.market");
    default: return null;
  }
}

export function runUndoText(kept: number) {
  return i18n.t("briefings:schedule.learning.undid", { count: kept, formattedCount: formatNumber(kept) });
}

export function runsNewestFirst<T extends { started_at_ms: number; id?: string }>(runs: readonly T[]): T[] {
  return [...runs].sort((left, right) => right.started_at_ms - left.started_at_ms || (right.id ?? "").localeCompare(left.id ?? ""));
}

type DailyTask = {
  id: string;
  kind: string;
  preset?: string | null;
  subjects?: readonly TaskSubject[] | null;
  status?: string;
  hold?: { reason?: string | null } | null;
  latest_run?: { status?: string | null } | null;
};

function aboutAsset(task: DailyTask, assetId: string) {
  return task.kind === "learning" && (task.subjects ?? []).some(subject => "asset" in subject && subject.asset === assetId);
}

/** The asset's own daily program: the asset-daily preset, or the one program that is only about this asset. */
export function trackDailyTask<T extends DailyTask>(tasks: readonly T[], assetId: string): T | undefined {
  const learning = tasks.filter(task => aboutAsset(task, assetId));
  return learning.find(task => task.preset === ASSET_DAILY_PRESET) ?? learning.find(task => (task.subjects?.length ?? 0) === 1);
}

export function otherLearningPrograms<T extends DailyTask>(tasks: readonly T[], assetId: string, dailyId?: string): T[] {
  return tasks.filter(task => aboutAsset(task, assetId) && task.id !== dailyId);
}

export type TrackDailyState = "off" | "on" | "running" | "waiting";

/** Off, on, running, or waiting for the allowance. A program that is not active is off. */
export function trackDailyState(task: DailyTask | null | undefined): TrackDailyState {
  if (!task || task.status !== "active") return "off";
  if (task.latest_run?.status === "running") return "running";
  if (task.hold?.reason === "allowance" || task.hold?.reason === "daily_allowance") return "waiting";
  return "on";
}
