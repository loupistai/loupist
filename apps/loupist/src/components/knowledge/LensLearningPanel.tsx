import { useEffect, useRef, useState } from "react";
import { useTranslation, Trans } from "../../i18n";
import { formatDate } from "../../i18n/format";
import { acknowledgeMethod, useLearningState } from "../map/learningState";
import { isStandingPlaceholder } from "../map/lensReading";
import { Button } from "../ui/button";
import type { GrowthEvent, LearningReview, KnowledgeMapNode } from "../../contracts/knowledge";
import type { LoupistBridge } from "../../types";

export function LensLearningPanel({ bridge, loupistId, node, onArchived }: {
  bridge: LoupistBridge; loupistId: string; node: Pick<KnowledgeMapNode, "kind" | "recordId" | "learnedAtMs" | "learnedRationale">; onArchived: () => void;
}) {
  const { t } = useTranslation("knowledge");
  const learning = useLearningState(bridge, loupistId);
  const [events, setEvents] = useState<GrowthEvent[]>([]);
  const [review, setReview] = useState<LearningReview>();
  const [confirm, setConfirm] = useState(false);
  const confirmation = useRef<HTMLButtonElement>(null);
  const archiveTrigger = useRef<HTMLButtonElement>(null);
  useEffect(() => { if (confirm) confirmation.current?.focus(); }, [confirm]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  const [archiveError, setArchiveError] = useState(false);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    let active = true;
    setError(false);
    void (async () => {
      const history: GrowthEvent[] = [];
      let cursor: string | undefined;
      do {
        const page = await bridge.knowledgeGrowthEvents({ loupistId, limit: 5000, cursor });
        if (!active) return;
        history.push(...page.events.filter(event => event.recordId === node.recordId));
        cursor = page.nextCursor ?? undefined;
      } while (cursor);
      setEvents(history.sort((a, b) => b.atMs - a.atMs));
    })().catch(() => { if (active) setError(true); });
    if (node.kind === "lens_method" && node.recordId) void bridge.learningReviewList({ loupistId, limit: 200 }).then(reviews => {
      if (!active) return;
      setReview(reviews.filter(item => item.lensRecordId === node.recordId && (item.status === "created" || item.status === "refined")).sort((a, b) => b.createdAtMs - a.createdAtMs)[0]);
    }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- keyed on the node's record; its kind follows the record.
  }, [bridge, loupistId, node.recordId, node.learnedAtMs, reload]);
  // Map provenance is the publication time; review.createdAtMs is admission time.
  const learnedAt = node.learnedAtMs ?? review?.createdAtMs;
  const rationale = review?.rationale ?? node.learnedRationale;
  const kept = !!node.recordId && learnedAt !== undefined && (learning.acknowledged.get(node.recordId) ?? -Infinity) >= learnedAt;
  async function archive() {
    if (!node.recordId || busy) return;
    setBusy(true); setArchiveError(false);
    try { await bridge.knowledgeLensArchive({ loupistId, recordId: node.recordId, archived: true }); onArchived(); }
    catch { setArchiveError(true); }
    finally { setBusy(false); }
  }
  return <>
    {error ? <p role="alert">{t("learning.loadError")} <Button variant="link" onClick={() => setReload(value => value + 1)}>{t("map.retry")}</Button></p> : null}
    {node.kind === "lens_method" && learnedAt !== undefined ? <section className="method-learning" aria-label={t("learning.heading")}>
      <h3>{t("learning.heading")}</h3>
      <p className="learning-meta">{t("learning.learnedOn", { date: formatDate(learnedAt), count: review?.evidenceCount ?? 0 })}</p>
      {rationale ? <blockquote>“{rationale}”</blockquote> : null}
      {!kept && !confirm ? <p className="learning-meta">{t("learning.inUse")}</p> : null}
      {confirm ? <div className="learning-confirm"><p>{t("learning.archiveConfirm")}</p><div className="learning-actions"><Button ref={confirmation} variant="ghost" disabled={busy} onClick={() => void archive()}>{t("learning.archive")}</Button><Button variant="ghost" disabled={busy} onClick={() => { setConfirm(false); requestAnimationFrame(() => archiveTrigger.current?.focus()); }}>{t("learning.cancel")}</Button></div></div> : <div className="learning-actions"><Button variant="outline" size="sm" className="learning-keep" disabled={kept} onClick={() => acknowledgeMethod(bridge, loupistId, node.recordId!, learnedAt)}>{t(kept ? "learning.kept" : "learning.keep")}</Button><Button ref={archiveTrigger} variant="ghost" size="sm" className="learning-archive" onClick={() => setConfirm(true)}>{t("learning.archive")}</Button></div>}
      {archiveError ? <p role="alert">{t("learning.archiveError")}</p> : null}
    </section> : null}
    <ThenNow events={events} />
  </>;
}

function ThenNow({ events }: { events: GrowthEvent[] }) {
  const { t } = useTranslation("knowledge");
  const [earlier, setEarlier] = useState(false);
  const readable = (value?: string | null) => value && !isStandingPlaceholder(value) ? value : "";
  const revisions = events.filter(event => event.kind === "lens_revised" && (readable(event.beforeSummary) || readable(event.afterSummary))).sort((a, b) => b.atMs - a.atMs);
  const latest = revisions[0];
  const history = useRef<HTMLElement>(null);
  const [wide, setWide] = useState(false);
  useEffect(() => {
    const inspector = history.current?.closest<HTMLElement>(".map-inspector, .memory-reader");
    if (!inspector) return;
    const measure = () => setWide(inspector.getBoundingClientRect().width >= 400);
    measure();
    const observer = new ResizeObserver(measure); observer.observe(inspector);
    return () => observer.disconnect();
  }, [latest?.eventId]);
  if (!latest) return null;
  const beforeDate = events.filter(event => event.atMs < latest.atMs && (event.kind === "lens_revised" || event.kind === "lens_created")).sort((a, b) => b.atMs - a.atMs)[0]?.atMs;
  return <section ref={history} className="lens-history" aria-label={t("learning.thenNow")}><h3>{t("learning.thenNow")}</h3>
    {/* The reason leads: it says what changed before the two versions do. */}
    {latest.rationale && !isStandingPlaceholder(latest.rationale) ? <p className="lens-change-reason">{t("learning.changedOn", { date: formatDate(latest.atMs), rationale: latest.rationale })}</p> : null}
    <div className={`lens-then-now${wide ? " is-wide" : ""}`} data-testid="lens-then-now">{(["then", "now"] as const).map(side => {
      const date = side === "now" ? formatDate(latest.atMs) : beforeDate === undefined ? t("learning.unknownDate") : formatDate(beforeDate);
      return <div key={side} data-side={side}><h4><Trans ns="knowledge" i18nKey={`learning.${side}`} values={{ date }} components={{ 0: <span className="learning-date" /> }} /></h4><RevisionSummary key={`${latest.eventId}:${side}`} summary={readable(side === "then" ? latest.beforeSummary : latest.afterSummary) || "—"} /></div>;
    })}</div>
    {revisions.length > 1 ? <><Button variant="link" aria-expanded={earlier} onClick={() => setEarlier(value => !value)}>{t("learning.earlier", { count: revisions.length - 1 })}</Button>{earlier ? <ul className="learning-earlier">{revisions.slice(1).map(event => <li key={event.eventId}>{formatDate(event.atMs)}{event.rationale ? ` · ${event.rationale}` : ""}</li>)}</ul> : null}</> : null}
  </section>;
}


function RevisionSummary({ summary }: { summary: string }) {
  const { t } = useTranslation("knowledge");
  const paragraph = useRef<HTMLParagraphElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  useEffect(() => {
    const element = paragraph.current;
    if (!element || expanded) return;
    const measure = () => setOverflows(element.scrollHeight > element.clientHeight);
    measure();
    const observer = new ResizeObserver(measure); observer.observe(element);
    return () => observer.disconnect();
  }, [summary, expanded]);
  return <><p ref={paragraph} className={expanded ? "" : "learning-summary"}>{summary}</p>{overflows ? <Button variant="link" aria-expanded={expanded} onClick={() => setExpanded(value => !value)}>{t(expanded ? "learning.showLess" : "learning.showMore")}</Button> : null}</>;
}
