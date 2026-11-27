import { useEffect, useState } from "react";
import type { AssetPage } from "../../contracts/assets";
import type { LoupistBridge } from "../../types";
import type { WorldChangesView } from "../../contracts/knowledge";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { dateValue } from "../../knowledge/presentation";
import { AppDateField } from "../ui/app-date-field";
import { Button } from "../ui/button";
import { useGo } from "../navigation/Open";
import { ChangeGroup } from "./ChangeRows";
import { groupChanges, rowsFromFeed, type ChangeRow } from "./worldChanges";

type WindowChoice = { kind: "move" } | { kind: "days" } | { kind: "date"; day: string };

const isoDay = (ms: number) => {
  const day = new Date(ms);
  return `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, "0")}-${String(day.getDate()).padStart(2, "0")}`;
};

/** What World learned after the latest move, under the thesis read. */
export function SinceThisRead({ bridge, loupistId, page, name, onOpenMap, onOpenSource }: {
  bridge: LoupistBridge;
  loupistId: string;
  page: AssetPage;
  name: string;
  onOpenMap: () => void;
  onOpenSource: (recordId: string) => void;
}) {
  const { t } = useTranslation("assets");
  const go = useGo();
  const moveDate = page.read?.latestMove?.factDate ?? page.read?.since;
  const hasRead = !!page.read;
  const [picked, setChoice] = useState<WindowChoice | null>(null);
  const fallback: WindowChoice = hasRead && moveDate ? { kind: "move" } : { kind: "days" };
  const choice = picked && !(picked.kind === "move" && !(hasRead && moveDate)) ? picked : fallback;
  const [feed, setFeed] = useState<WorldChangesView | "loading" | "error" | "page">("page");
  const [attempt, setAttempt] = useState(0);
  const [waited, setWaited] = useState(false);
  const assetId = page.asset.assetId;
  const linked = !!page.entityId;
  const since = choice.kind === "days" ? isoDay(Date.now() - 30 * 86_400_000) : choice.kind === "date" ? choice.day : undefined;
  const usePage = choice.kind === "move";
  const skipFetch = !linked || usePage || (choice.kind === "date" && !choice.day);

  useEffect(() => {
    if (skipFetch) { setFeed("page"); setWaited(false); return; }
    let live = true;
    setFeed("loading");
    setWaited(false);
    const timer = window.setTimeout(() => { if (live) setWaited(true); }, 300);
    void bridge.knowledgeWorldChanges({
      loupistId, since, axis: "recorded", about: [{ asset: assetId }],
    }).then(view => { if (live) setFeed(view); }).catch(() => { if (live) setFeed("error"); });
    return () => { live = false; window.clearTimeout(timer); };
  }, [bridge, loupistId, assetId, since, skipFetch, attempt]);

  if (!linked) return <section className="since-read" aria-label={t(hasRead ? "sinceRead.title" : "sinceRead.recent")}>
    <h2>{t(hasRead ? "sinceRead.title" : "sinceRead.recent")}</h2>
    <p>{t("sinceRead.link", { name })}</p>
    <Button type="button" variant="outline" size="sm" onClick={onOpenMap}>{t("sinceRead.openWorld")}</Button>
  </section>;

  const rows = usePage
    ? rowsFromFeed(page.sinceRead?.changes ?? [], page.sinceRead?.lens ?? [])
    : feed !== "loading" && feed !== "error" && feed !== "page" ? rowsFromFeed(feed.changes, feed.lens) : [];
  const groups = groupChanges(rows);
  const pending = usePage
    ? page.sinceRead?.pending ?? 0
    : feed !== "loading" && feed !== "error" && feed !== "page" ? feed.pending : 0;
  const labelDate = choice.kind === "days" ? dateValue(since ?? "") : choice.kind === "date" ? dateValue(choice.day) : moveDate ? dateValue(moveDate) : "";
  const openRow = (row: ChangeRow) => {
    if (row.lensRecordId) {
      go?.({ kind: "page", location: { loupistId, surface: "lens", recordId: row.lensRecordId } });
      return;
    }
    const passage = row.passage;
    if (!passage) return;
    if (go && passage.spanId) {
      go({ kind: "page", location: { loupistId, surface: "source", recordId: passage.recordId, revisionId: passage.revisionId, selection: { kind: "source", spanId: passage.spanId, sourceId: passage.recordId } } });
      return;
    }
    onOpenSource(passage.recordId);
  };

  return <section className="since-read" aria-label={t(hasRead ? "sinceRead.title" : "sinceRead.recent")}>
    <h2>{t(hasRead ? "sinceRead.title" : "sinceRead.recent")}</h2>
    <div className="map-segmented" role="group" aria-label={t("sinceRead.title")}>
      {hasRead && moveDate ? <button type="button" aria-pressed={choice.kind === "move"} onClick={() => setChoice({ kind: "move" })}>{t("sinceRead.sinceMove", { date: dateValue(moveDate) })}</button> : null}
      <button type="button" aria-pressed={choice.kind === "days"} onClick={() => setChoice({ kind: "days" })}>{t("sinceRead.days")}</button>
      <button type="button" aria-pressed={choice.kind === "date"} onClick={() => setChoice({ kind: "date", day: choice.kind === "date" ? choice.day : isoDay(Date.now() - 30 * 86_400_000) })}>{t("sinceRead.sinceDate")}</button>
    </div>
    {choice.kind === "date" ? <AppDateField aria-label={t("sinceRead.sinceDateLabel")} value={choice.day} max={isoDay(Date.now())} onChange={day => { if (day) setChoice({ kind: "date", day }); }} /> : null}
    {pending > 0 ? <p className="since-read-pending" role="status">{t("sinceRead.pending", { name, count: pending, formattedCount: formatNumber(pending) })}</p> : null}
    {feed === "loading" && waited ? <p role="status">{t("sinceRead.loading")}</p> : null}
    {feed === "error" ? <p role="alert">{t("sinceRead.error")}<Button type="button" variant="outline" size="sm" onClick={() => setAttempt(value => value + 1)}>{t("sinceRead.retry")}</Button></p> : null}
    {feed !== "loading" && feed !== "error" && !rows.length && pending === 0 ? <p role="status">{t("sinceRead.empty", { date: labelDate })}</p> : null}
    <ChangeGroup title={t("sinceRead.ended")} rows={groups.ended} onOpen={openRow} showAllLabel={count => t("sinceRead.showAll", { count, formattedCount: formatNumber(count) })} />
    <ChangeGroup title={t("sinceRead.disagreements")} rows={groups.disagreements} onOpen={openRow} showAllLabel={count => t("sinceRead.showAll", { count, formattedCount: formatNumber(count) })} />
    <ChangeGroup title={t("sinceRead.sections")} rows={groups.lens} onOpen={openRow} showAllLabel={count => t("sinceRead.showAll", { count, formattedCount: formatNumber(count) })} />
    <ChangeGroup title={t("sinceRead.added")} rows={groups.added} onOpen={openRow} showAllLabel={count => t("sinceRead.showAll", { count, formattedCount: formatNumber(count) })} />
  </section>;
}
