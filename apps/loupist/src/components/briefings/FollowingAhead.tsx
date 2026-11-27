import { useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import { formatDate, formatDay } from "@/i18n/format";
import type { AssetRow } from "../../contracts/assets";
import type { JournalRecord } from "../../contracts/knowledge";
import type { BriefingCheck } from "../../contracts/generated/briefings";
import type { InboxItem } from "../../contracts/scheduled";
import type { LoupistBridge, ScheduledTaskListItem } from "../../types";
import { useAssetPageOpener } from "../assets/assetOpener";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";

const HORIZON_DAYS = 60;
const DAY_MS = 86_400_000;
/** Inbox pages read to find the newest briefing; older briefings say nothing about what is ahead now. */
const BRIEFING_PAGES = 5;

export type AheadEntry = {
  id: string;
  atMs: number;
  /** Local calendar day, YYYY-MM-DD; empty for an undated check. */
  day: string;
  bucket: "overdue" | "today" | "tomorrow" | "date" | "undated";
  kind: "follow-up" | "results" | "check";
  /** A results row: whether the day is announced or only expected. */
  estimated?: boolean;
  /** Follow-up terms. A check's claim is rendered in the kind label. */
  detail: string;
  claim?: string;
  assetId?: string;
  taskId?: string;
  untied?: boolean;
  /** When the newest briefing that listed this check ran. */
  briefingAtMs?: number;
};

/** The newest briefing's Ahead section: when it ran and the checks it listed. */
export type BriefingAhead = { atMs: number; ahead: readonly BriefingCheck[] };

/** The newest inbox item that is a briefing with its sections, or null. */
export function newestBriefing(items: readonly InboxItem[]): BriefingAhead | null {
  const item = items.find(entry => entry.kind === "briefing" && entry.briefingContext);
  return item?.briefingContext ? { atMs: item.finishedAtMs ?? item.startedAtMs, ahead: item.briefingContext.ahead } : null;
}

async function readNewestBriefing(bridge: LoupistBridge, loupistId: string) {
  let cursor: string | null = null;
  for (let page = 0; page < BRIEFING_PAGES; page += 1) {
    const result = await bridge.inboxList({ loupistId, cursor, limit: 50 });
    const found = newestBriefing(result.items);
    if (found || !result.nextCursor) return found;
    cursor = result.nextCursor;
  }
  return null;
}

const calendarDay = (ms: number) => {
  const date = new Date(ms);
  return Date.UTC(date.getFullYear(), date.getMonth(), date.getDate());
};

const dayKey = (stamp: number) => {
  const date = new Date(stamp);
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  return `${date.getUTCFullYear()}-${month}-${day}`;
};

const storedDay = (value: string) => {
  const [year, month, date] = value.slice(0, 10).split("-").map(Number);
  if (!year || !month || !date) return null;
  return Date.UTC(year, month - 1, date);
};

/**
 * Results follow-ups and dated journal checks for the next 60 days, overdue first, then
 * the newest briefing's Ahead rows (plan 4.7): a check the Journal already lists here is
 * marked as from that briefing, an undated one is added last, and one resolved, archived
 * or deleted since is left out. `assets_list` has no entity id, so a check is tied only by
 * `thesis.assetId`.
 */
export function aheadEntries(input: {
  now: number;
  tasks: readonly ScheduledTaskListItem[];
  journal: readonly JournalRecord[];
  assets: readonly Pick<AssetRow, "assetId">[];
  briefing?: BriefingAhead | null;
}): AheadEntry[] {
  const today = calendarDay(input.now);
  const last = today + (HORIZON_DAYS - 1) * DAY_MS;
  const followed = new Set(input.assets.map(asset => asset.assetId));
  const bucketOf = (stamp: number): AheadEntry["bucket"] => stamp < today ? "overdue" : stamp === today ? "today" : stamp === today + DAY_MS ? "tomorrow" : "date";
  const keep = (stamp: number) => stamp < today || stamp <= last;
  const rows: AheadEntry[] = [];
  for (const task of input.tasks) {
    if (task.status !== "active" || task.preset !== "results-follow-up" || task.schedule.kind !== "once" || task.next_run_at_ms == null) continue;
    const stamp = calendarDay(task.next_run_at_ms);
    if (!keep(stamp)) continue;
    const assetId = task.subjects?.map(subject => "asset" in subject ? subject.asset : null).find(id => id && followed.has(id)) ?? undefined;
    rows.push({ id: `task:${task.id}`, atMs: task.next_run_at_ms, day: dayKey(stamp), bucket: bucketOf(stamp), kind: "follow-up", detail: task.terms ?? "", assetId: assetId ?? undefined, taskId: task.id });
    // The results the follow-up is about, on their own stored day (plan 4.4); the run row stays.
    const event = task.event ? storedDay(task.event.date) : null;
    if (event != null && keep(event)) {
      rows.push({ id: `event:${task.id}`, atMs: event, day: dayKey(event), bucket: bucketOf(event), kind: "results", estimated: task.event!.precision === "estimated", detail: "", assetId: assetId ?? undefined, taskId: task.id });
    }
  }
  for (const entry of input.journal) {
    const byDate = entry.check?.byDate;
    if (!byDate || entry.outcome || entry.archived) continue;
    const stamp = storedDay(byDate);
    if (stamp == null || !keep(stamp)) continue;
    const assetId = entry.thesis?.assetId && followed.has(entry.thesis.assetId) ? entry.thesis.assetId : undefined;
    rows.push({
      id: `journal:${entry.journalId}`, atMs: stamp, day: dayKey(stamp), bucket: bucketOf(stamp), kind: "check",
      detail: "", claim: entry.check?.claim ?? "", assetId, untied: !assetId,
    });
  }
  const listed = new Map(rows.map(row => [row.id, row]));
  for (const item of input.briefing?.ahead ?? []) {
    const entry = input.journal.find(record => record.journalId === item.journalId);
    if (!entry || entry.outcome || entry.archived) continue;
    const id = `journal:${item.journalId}`;
    const shown = listed.get(id);
    if (shown) { shown.briefingAtMs = input.briefing!.atMs; continue; }
    const stamp = item.byDate ? storedDay(item.byDate) : null;
    if (stamp != null && !keep(stamp)) continue;
    const assetId = entry.thesis?.assetId && followed.has(entry.thesis.assetId) ? entry.thesis.assetId : undefined;
    const row: AheadEntry = {
      id, atMs: stamp ?? input.briefing!.atMs, day: stamp == null ? "" : dayKey(stamp), bucket: stamp == null ? "undated" : bucketOf(stamp), kind: "check",
      detail: "", claim: entry.check?.claim || item.title, assetId, untied: !assetId, briefingAtMs: input.briefing!.atMs,
    };
    listed.set(id, row);
    rows.push(row);
  }
  const order = { overdue: 0, today: 1, tomorrow: 2, date: 3, undated: 4 };
  return rows.sort((left, right) => order[left.bucket] - order[right.bucket] || left.day.localeCompare(right.day) || left.atMs - right.atMs);
}

export function FollowingAhead({ bridge, loupistId, assets, now, onOpenTask }: {
  bridge: LoupistBridge;
  loupistId: string;
  assets: readonly AssetRow[] | null;
  /** Fixed clock for tests. Omitted, the view reads the clock once per load. */
  now?: number;
  /** Opens the task in Schedule. The shell owns that navigation. */
  onOpenTask?: (taskId: string) => void;
}) {
  const { t } = useTranslation("briefings");
  const openAsset = useAssetPageOpener();
  const [entries, setEntries] = useState<AheadEntry[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [showWait, setShowWait] = useState(false);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let live = true;
    setEntries(null);
    setFailed(false);
    setShowWait(false);
    const timer = window.setTimeout(() => { if (live) setShowWait(true); }, 300);
    if (!assets) return () => { live = false; window.clearTimeout(timer); };
    const clock = now ?? Date.now();
    void Promise.all([
      bridge.scheduledTasksList({ loupist_id: loupistId }),
      bridge.journalList({ loupistId }),
      readNewestBriefing(bridge, loupistId),
    ]).then(([tasks, journal, briefing]) => {
      if (!live) return;
      window.clearTimeout(timer);
      setEntries(aheadEntries({ now: clock, tasks, journal, assets, briefing }));
    }).catch(() => {
      if (!live) return;
      window.clearTimeout(timer);
      setFailed(true);
    });
    return () => { live = false; window.clearTimeout(timer); };
  }, [bridge, loupistId, assets, now, attempt]);
  const assetName = (assetId: string) => assets?.find(asset => asset.assetId === assetId)?.name ?? "";
  if (failed) return <div role="alert" className="following-line">
    <span>{t("following.ahead.failed")}</span>{" "}
    <Button variant="outline" size="sm" onClick={() => setAttempt(value => value + 1)}>{t("retry")}</Button>
  </div>;
  if (!entries) return showWait ? <p className="following-line" role="status"><Spinner aria-hidden="true" />{t("following.ahead.loading")}</p> : null;
  if (!entries.length) return <p className="following-ahead-empty">{t("following.ahead.empty")}</p>;
  const groups = new Map<string, AheadEntry[]>();
  for (const entry of entries) {
    const key = `${entry.bucket}:${entry.day}`;
    groups.set(key, [...(groups.get(key) ?? []), entry]);
  }
  return <div className="following-ahead">
    {[...groups.entries()].map(([key, rows]) => {
      const first = rows[0]!;
      const heading = first.bucket === "date" ? formatDay(first.day) : t(`following.ahead.${first.bucket}`);
      return <section key={key} aria-label={heading}>
        <h2 className="following-ahead-day">{heading}</h2>
        <ul>
          {rows.map(entry => <li key={entry.id} className="following-ahead-row">
            <span className="following-ahead-kind">{entry.kind === "follow-up" ? t("following.ahead.followUp")
              : entry.kind === "results" ? t(entry.estimated ? "following.ahead.resultsExpected" : "following.ahead.results", { date: formatDay(entry.day) })
              : t("following.ahead.checkDue", { claim: entry.claim ?? "" })}</span>
            {entry.assetId ? <button type="button" className="following-ahead-asset" onClick={() => openAsset?.({ loupistId, assetId: entry.assetId!, name: assetName(entry.assetId!) })}>{assetName(entry.assetId)}</button>
              : entry.untied ? <span className="following-ahead-untied">{t("following.ahead.untied")}</span> : null}
            {entry.detail ? <span className="following-ahead-detail">{entry.detail}</span> : null}
            {entry.briefingAtMs != null ? <span className="following-ahead-from">{t("following.ahead.fromBriefing", { date: formatDate(entry.briefingAtMs, { year: undefined, month: "short", day: "numeric" }) })}</span> : null}
            {entry.kind === "follow-up" && entry.taskId ? <Button variant="ghost" size="sm" onClick={() => onOpenTask?.(entry.taskId!)}>{t("following.ahead.open")}</Button> : null}
          </li>)}
        </ul>
      </section>;
    })}
  </div>;
}
