import { useContext, useEffect, useState } from "react";
import type { LoupistBridge } from "../../types";
import { EARLIEST_AS_OF, formatAsOf } from "../../chat/asOf";
import { useTranslation } from "../../i18n";
import { dateValue } from "../../knowledge/presentation";
import { parseChange, type ChangeRow } from "../assets/worldChanges";
import { Button } from "../ui/button";
import { WorldAsOfContext } from "./useMapData";

/** Rows per page of the feed. */
export const HISTORY_PAGE = 20;
/** The feed kinds History lists: what was added, what ended, and where Sources disagree. */
const HISTORY_KINDS = ["added", "ended", "conflict", "disputed"];

export type HistoryRow = ChangeRow & { claimId?: string };

type Feed = { status: "loading" | "error" | "ready"; rows: HistoryRow[]; next: string | null; more: boolean };

/**
 * History: the change feed about one entity on the published axis, oldest first, a page at a
 * time. Under the World's as-of date the host reads the feed of the World that had read only
 * what was published by then.
 */
export function MapEntityHistory({ bridge, loupistId, entityId, name, onOpenClaim }: {
  bridge: LoupistBridge;
  loupistId: string;
  entityId: string;
  /** The entity's name as World shows it. */
  name: string;
  onOpenClaim: (claimId: string) => void;
}) {
  const { t } = useTranslation("knowledge");
  const asOf = useContext(WorldAsOfContext);
  const key = `${loupistId}\u0000${entityId}\u0000${asOf ?? ""}`;
  const [feed, setFeed] = useState<Feed & { key: string }>({ key, status: "loading", rows: [], next: null, more: false });
  const [request, setRequest] = useState<{ key: string; cursor?: string; attempt: number }>({ key, attempt: 0 });
  const [waited, setWaited] = useState(false);
  const current = feed.key === key ? feed : { key, status: "loading" as const, rows: [], next: null, more: false };
  const page = request.key === key ? request : { key, attempt: 0 };

  useEffect(() => {
    let live = true;
    setWaited(false);
    const timer = window.setTimeout(() => { if (live) setWaited(true); }, 300);
    setFeed(previous => previous.key === key ? { ...previous, status: "loading" } : { key, status: "loading", rows: [], next: null, more: false });
    void bridge.knowledgeWorldChanges({
      loupistId, since: page.cursor ?? EARLIEST_AS_OF, axis: "published", about: [{ node: entityId }], kinds: HISTORY_KINDS, limit: HISTORY_PAGE, ...(asOf ? { asOf } : {}),
    }).then(view => {
      if (!live) return;
      const parsed = view.changes.flatMap((value, index) => {
        const row = parseChange(value, index);
        const claimId = value && typeof value === "object" && typeof (value as { claimId?: unknown }).claimId === "string" ? (value as { claimId: string }).claimId : undefined;
        return row ? [{ ...row, claimId }] : [];
      });
      setFeed(previous => {
        const before = previous.key === key && page.cursor ? previous.rows : [];
        const seen = new Set(before.map(row => row.id));
        return { key, status: "ready", rows: [...before, ...parsed.filter(row => !seen.has(row.id))], next: view.next, more: !!view.next };
      });
    }).catch(() => { if (live) setFeed(previous => ({ ...previous, key, status: "error" })); });
    return () => { live = false; window.clearTimeout(timer); };
  }, [bridge, loupistId, entityId, asOf, key, page.cursor, page.attempt]);

  const date = (row: HistoryRow) => row.published ? t("asOf.history.published", { date: dateValue(row.published) }) : t("asOf.history.undated");
  const kind = (row: HistoryRow) => t(row.group === "ended" ? "asOf.history.ended" : row.group === "disagreement" ? "asOf.history.disagreement" : "asOf.history.added");
  return <section className="map-entity-history" aria-label={t("asOf.history.title")} data-map-history>
    <h3>{t("asOf.history.title")}</h3>
    {current.rows.length ? <ul>{current.rows.map(row => <li key={row.id}>
      {row.claimId ? <Button variant="link" onClick={() => onOpenClaim(row.claimId!)}>{row.proposition}</Button> : <p>{row.proposition}</p>}
      <small>{kind(row)} · {date(row)}{row.sourceTitle ? ` · ${row.sourceTitle}` : ""}</small>
    </li>)}</ul> : null}
    {current.status === "loading" && waited ? <p role="status">{t("asOf.history.loading")}</p> : null}
    {current.status === "error" ? <p role="alert">{t("asOf.history.error")} <Button variant="outline" size="sm" onClick={() => setRequest({ key, cursor: page.cursor, attempt: page.attempt + 1 })}>{t("asOf.history.retry")}</Button></p> : null}
    {current.status === "ready" && !current.rows.length ? <p role="status">{asOf ? t("asOf.history.emptyBy", { name, date: formatAsOf(asOf, "long") }) : t("asOf.history.empty", { name })}</p> : null}
    {current.status === "ready" && current.more && current.next ? <Button variant="link" onClick={() => setRequest({ key, cursor: current.next!, attempt: 0 })}>{t("asOf.history.more")}</Button> : null}
  </section>;
}
