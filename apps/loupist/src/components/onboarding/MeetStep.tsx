import { Component, lazy, Suspense, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { LoaderCircleIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { formatRelative, formatTime } from "@/i18n/format";
import { Button } from "@/components/ui/button";
import type { FollowedStatus, GrowthEvent, KnowledgeMap, SchemaProposal, WorldSchemaView } from "@/contracts/knowledge";
import type { WatchlistEntry } from "@/contracts/userContext";
import type { LoupistBridge } from "@/types";
import { frozenLayout } from "@/components/map/layout";
import { claimIdForEdge } from "@/components/map/MapList";
import { nodeCategory } from "@/components/map/kinds";
import { entityLabel } from "@/components/map/labels";
import { relationName } from "@/components/map/relations";
import { mapStatsText } from "@/components/map/stats";
import type { MapCanvasControls } from "@/components/map/MapCanvas";
import "@/styles/map.css";
import "@/styles/onboarding.css";

const MapCanvas = lazy(() => import("@/components/map/MapCanvas"));
const tKnowledge = i18n.getFixedT(null, "knowledge");
/** While the Loupist prepares its first map the screen polls quickly; after that it keeps up slowly. */
const FAST_MS = 500;
const SLOW_MS = 2_000;
const MAX_ROWS = 60;
/** A name added in the last few seconds glows on the mini map. */
const GLOW_MS = 2_400;
const CORE_KINDS = ["company", "product", "person", "event", "market"] as const;
/** A read of what the user follows usually takes under a minute; past this the screen stops saying "Reading". */
const SLOW_READ_MS = 180_000;

type FollowedRow = "queued" | "running" | "slow" | "failed" | "waiting";

/** What the log says about reading what the user follows, from Rust's job state. */
function followedRow(status: FollowedStatus | null, nowMs: number): FollowedRow | null {
  if (!status) return null;
  if (status.state === "queued" || status.state === "running") {
    return status.queuedAtMs !== undefined && nowMs - status.queuedAtMs > SLOW_READ_MS ? "slow" : status.state;
  }
  return status.state === "failed" || status.state === "waiting" ? status.state : null;
}

const FOLLOWED_TEXT = { queued: "wizard.meetQueuedFollowed", running: "wizard.meetReadingFollowed", slow: "wizard.meetFollowedSlow", failed: "wizard.meetFollowedFailed", waiting: "wizard.meetFollowedWaiting" } as const;

class CanvasBoundary extends Component<{ children: ReactNode; fallback: ReactNode }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  render() { return this.state.failed ? this.props.fallback : this.props.children; }
}

type Row = { key: string; mark: string; text: string; atMs?: number };

/** Seconds ago as "now", then minutes, in the interface language. */
function ago(atMs: number, nowMs: number) {
  const seconds = Math.max(0, Math.round((nowMs - atMs) / 1000));
  return seconds < 60 ? formatRelative(0, "second") : formatRelative(-Math.round(seconds / 60), "minute");
}

function schemaLabel(labels: { en: string; ko?: string }, language: string) {
  return language.toLowerCase().startsWith("ko") && labels.ko ? labels.ko : labels.en;
}

/**
 * A kind named inside a sentence ("Tracking: memory makers, …") reads lowercase in
 * English; acronyms ("HBM suppliers") and other languages keep their case.
 */
function inSentence(label: string, language: string) {
  if (!language.toLowerCase().startsWith("en") || label.length < 2) return label;
  const [first, second] = [label[0]!, label[1]!];
  return second === second.toLowerCase() && second !== second.toUpperCase() ? first.toLowerCase() + label.slice(1) : label;
}

function proposalSummary(proposal: SchemaProposal, language: string) {
  const types = proposal.types.map(type => inSentence(schemaLabel(type.labels, language), language)).join(", ");
  const relations = proposal.relations.map(relation => inSentence(schemaLabel(relation.labels, language), language)).join(", ");
  return [types, relations].filter(Boolean).join(" · ");
}

/**
 * Onboarding step 5: the new Loupist's first map grows in front of the user from
 * the same growth log as everything else. Every action is enabled from the
 * first frame; the user never waits for the map.
 */
export function MeetStep({ bridge, loupistId, name, focus, finishing, onFinish }: {
  bridge: LoupistBridge; loupistId: string; name: string; focus: string; finishing: boolean;
  /** `true` asks for the weekday morning update; `false` starts without one. */
  onFinish: (morningUpdate: boolean) => void;
}) {
  const { t, i18n: instance } = useTranslation("onboarding");
  const language = instance.language;
  const [watchlist, setWatchlist] = useState<WatchlistEntry[] | null>(null);
  const [events, setEvents] = useState<GrowthEvent[]>([]);
  const [map, setMap] = useState<KnowledgeMap | null>(null);
  const [schema, setSchema] = useState<WorldSchemaView | null>(null);
  /** Rust's state of the World jobs reading the "What you follow" Sources. */
  const [followed, setFollowed] = useState<FollowedStatus | null>(null);
  const [readFailed, setReadFailed] = useState(false);
  const [deciding, setDeciding] = useState(false);
  const [decisionFailed, setDecisionFailed] = useState(false);
  const [retry, setRetry] = useState(0);
  const [nowMs, setNowMs] = useState(() => Date.now());
  /** The log fades its top edge once rows have scrolled away, so a cut row never reads as content. */
  const [scrolled, setScrolled] = useState(false);
  const controls = useRef<MapCanvasControls | null>(null);
  const list = useRef<HTMLDivElement>(null);
  const followList = useRef(true);
  const proposal = schema?.proposal?.kind === "seed" ? schema.proposal : null;
  const preparing = proposal?.status === "preparing";

  useEffect(() => {
    let active = true;
    void bridge.watchlistList().then(items => { if (active) setWatchlist(items); }).catch(() => { if (active) setWatchlist([]); });
    return () => { active = false; };
  }, [bridge]);

  useEffect(() => {
    let active = true;
    let timer: number | undefined;
    let fast = true;
    const read = async () => {
      try {
        const [page, nextSchema, status] = await Promise.all([
          bridge.knowledgeGrowthEvents({ loupistId, limit: 500 }),
          bridge.worldSchemaGet({ loupistId }),
          // Without the job state the screen still works; it just says nothing about reading.
          bridge.knowledgeFollowedStatus({ loupistId }).catch(() => null),
        ]);
        if (!active) return;
        setFollowed(current => current?.state === status?.state && current?.assets === status?.assets && current?.queuedAtMs === status?.queuedAtMs ? current : status);
        setSchema(nextSchema);
        setEvents(current => {
          if (current.length === page.events.length && current.every((event, index) => event.eventId === page.events[index]?.eventId)) return current;
          return page.events;
        });
        const nextMap = await bridge.knowledgeMap({ loupistId, maxNodes: 500 });
        if (!active) return;
        setMap(current => (current?.worldRevisionId === nextMap.worldRevisionId && current.nodes.length === nextMap.nodes.length ? current : nextMap));
        setReadFailed(false);
        const row = followedRow(status, Date.now());
        fast = row === "queued" || row === "running" || (nextSchema.proposal?.kind === "seed" && nextSchema.proposal.status === "preparing");
      } catch {
        if (active) setReadFailed(true);
      }
      if (active) { setNowMs(Date.now()); timer = window.setTimeout(() => void read(), fast ? FAST_MS : SLOW_MS); }
    };
    void read();
    return () => { active = false; if (timer !== undefined) window.clearTimeout(timer); };
  }, [bridge, loupistId, retry]);

  const entityNodes = useMemo(() => (map?.nodes ?? []).filter(node => node.kind === "entity" && !node.archived), [map]);
  const claimEdges = useMemo(() => (map?.edges ?? []).filter(edge => edge.kind === "claim"), [map]);
  const following = (watchlist?.length ?? 0) > 0;
  /**
   * The ordinary World job reading the "What you follow" Sources: queued or running with a spinner; a
   * plain line once it failed, cannot run yet or takes unusually long. The user can go on at any time.
   */
  const followedState = followedRow(followed, nowMs);
  const readingFollowed = followedState === "queued" || followedState === "running";
  const seeding = preparing || proposal?.status === "pending" || readingFollowed;

  const rows = useMemo<Row[]>(() => {
    const nodes = new Map((map?.nodes ?? []).map(node => [node.id, node]));
    const claims = new Map(claimEdges.map(edge => [claimIdForEdge(edge), edge]));
    const result: Row[] = [];
    const types = schema?.extension.types ?? [];
    const kinds = types.length
      ? types.map(type => inSentence(schemaLabel(type.labels, language), language)).join(", ")
      : CORE_KINDS.map(kind => t(`wizard.meetTrackingKinds.${kind}`)).join(", ");
    result.push({ key: "tracking", mark: "tracking", text: t("wizard.meetTracking", { kinds }) });
    const ordered = [...events].sort((a, b) => a.atMs - b.atMs);
    for (const event of ordered) {
      if (!event.subjectId) continue;
      if (event.kind === "entity_added") {
        const node = nodes.get(event.subjectId);
        const label = node ? entityLabel(node, language) : event.summary.trim();
        if (label) result.push({ key: event.eventId, mark: node ? nodeCategory(node) : "other", text: t("wizard.meetAdded", { name: label }), atMs: event.atMs });
      } else if (event.kind === "claim_added") {
        const edge = claims.get(event.subjectId);
        const a = edge ? nodes.get(edge.source) : undefined;
        const b = edge ? nodes.get(edge.target) : undefined;
        if (edge && a && b) result.push({ key: event.eventId, mark: "link", atMs: event.atMs, text: t("wizard.meetLinked", {
          a: entityLabel(a, language), b: entityLabel(b, language), relation: edge.relation ? relationName(edge.relation, tKnowledge) : tKnowledge("map.kind.other").toLowerCase(),
        }) });
      }
    }
    // The fixed rows stay first; the log keeps its newest rows.
    const fixed = result.filter(row => row.atMs === undefined);
    const logged = result.filter(row => row.atMs !== undefined).slice(-MAX_ROWS);
    return [...fixed, ...logged];
  }, [events, map, claimEdges, schema, language, t]);

  // Newest rows arrive at the bottom; follow them unless the user scrolled up.
  useEffect(() => {
    const element = list.current;
    if (element && followList.current) element.scrollTop = element.scrollHeight;
  }, [rows.length, followedState, preparing]);

  const glowIds = useMemo(() => {
    const recent = new Set(events.filter(event => event.kind === "entity_added" && event.subjectId && nowMs - event.atMs < GLOW_MS).map(event => event.subjectId!));
    return recent;
  }, [events, nowMs]);
  const positions = useMemo(() => map ? frozenLayout(`onboarding:${loupistId}`, map.worldRevisionId, map.nodes, map.edges) : [], [map, loupistId]);
  const counter = mapStatsText(tKnowledge, entityNodes.length, claimEdges.length);

  const decide = async (apply: boolean) => {
    if (!proposal || deciding) return;
    setDeciding(true); setDecisionFailed(false);
    try {
      const next = apply
        ? await bridge.worldSchemaApply({ loupistId, proposalId: proposal.proposalId })
        : await bridge.worldSchemaDismiss({ loupistId, proposalId: proposal.proposalId });
      setSchema(next);
    } catch { setDecisionFailed(true); }
    finally { setDeciding(false); }
  };

  const updateTime = formatTime(new Date(2000, 0, 3, 8, 0));
  const failed = readFailed || proposal?.status === "failed";

  return <>
    <header className="onboarding-meet-heading">
      <h1>{focus ? t("wizard.meetTitle", { name }) : t("wizard.meetTitleReady", { name })}</h1>
      {focus ? <p className="onboarding-meet-focus" title={focus}>{t("wizard.meetFocus", { focus })}</p> : null}
    </header>
    {failed ? <div className="onboarding-meet-banner" role="alert">
      <span>{t("wizard.meetError")}</span>
      {readFailed ? <Button type="button" variant="ghost" size="sm" onClick={() => setRetry(value => value + 1)}>{t("wizard.meetRetry")}</Button> : null}
    </div> : null}
    {proposal?.status === "pending" ? <section className="onboarding-meet-proposal" aria-label={t("wizard.meetProposalTitle")}>
      <h2>{t("wizard.meetProposalTitle")}</h2>
      <p>{proposalSummary(proposal, language)}</p>
      {decisionFailed ? <p className="onboarding-meet-proposal-error" role="alert">{t("wizard.meetProposalError")}</p> : null}
      <div className="onboarding-meet-proposal-actions">
        {/* The consent below is the screen's primary action; this card's choice stays secondary. */}
        <Button type="button" size="sm" variant="outline" disabled={deciding} onClick={() => void decide(true)}>{t("wizard.meetUseThis")}</Button>
        <Button type="button" size="sm" variant="ghost" disabled={deciding} onClick={() => void decide(false)}>{t("wizard.meetKeepDefault")}</Button>
      </div>
    </section> : null}
    <div className="onboarding-meet-columns" data-proposal={proposal?.status === "pending" || undefined}>
      <div className="onboarding-meet-map" data-seeding={seeding || undefined} data-empty={!entityNodes.length || undefined}>
        {entityNodes.length ? <CanvasBoundary fallback={<p className="onboarding-meet-map-empty">{counter}</p>}>
          <Suspense fallback={<div className="map-skeleton" />}>
            <MapCanvas compact={{ glowNodeIds: glowIds }} nodes={positions} edges={map?.edges ?? []} scopeKey={`onboarding:${loupistId}`} controls={controls} onSelect={() => {}} onClear={() => {}} stats={counter} />
          </Suspense>
        </CanvasBoundary> : <p className="onboarding-meet-map-empty">{readingFollowed ? t("wizard.meetMapReading") : tKnowledge("map.emptyTitle")}</p>}
      </div>
      <section className="onboarding-meet-log-section" aria-label={t("wizard.meetLog")}>
        <p className="onboarding-meet-counter" aria-live="polite">
          {preparing || readingFollowed ? <LoaderCircleIcon className="animate-spin" aria-hidden="true" /> : null}{counter}
        </p>
        <div className="onboarding-meet-log" role="log" aria-live="polite" tabIndex={0} ref={list} data-scrolled={scrolled || undefined}
          onScroll={event => {
            const element = event.currentTarget;
            followList.current = element.scrollHeight - element.scrollTop - element.clientHeight < 24;
            setScrolled(element.scrollTop > 0);
          }}>
          {rows.map(row => <div className="onboarding-meet-row" key={row.key}>
            <i className={`onboarding-meet-mark is-${row.mark}`} aria-hidden="true" />
            <span>{row.text}</span>
            {row.atMs !== undefined ? <time>{ago(row.atMs, nowMs)}</time> : null}
          </div>)}
          {readingFollowed ? <div className="onboarding-meet-row is-pending" data-followed={followedState}><LoaderCircleIcon className="onboarding-meet-spinner animate-spin" aria-hidden="true" /><span>{t(FOLLOWED_TEXT[followedState])}</span></div>
            : followedState ? <div className="onboarding-meet-row is-note" data-followed={followedState}><i className={`onboarding-meet-mark ${followedState === "failed" ? "is-warning" : "is-note"}`} aria-hidden="true" /><span>{t(FOLLOWED_TEXT[followedState])}</span></div> : null}
          {preparing ? <div className="onboarding-meet-row is-pending"><LoaderCircleIcon className="onboarding-meet-spinner animate-spin" aria-hidden="true" /><span>{t("wizard.meetReading")}</span></div> : null}
        </div>
      </section>
    </div>
    <footer className="onboarding-meet-actions">
      {following ? <>
        <p className="onboarding-meet-question">{t("wizard.meetConsent", { time: updateTime })}</p>
        <p className="onboarding-holdings-note">{t("wizard.briefingsCost")}</p>
        {/* Two equal answers to one question; both open Chat. */}
        <div className="onboarding-meet-buttons">
          <Button className="onboarding-meet-choice" data-onboarding-consent="yes" disabled={finishing} onClick={() => onFinish(true)}>{t("wizard.meetConsentYes")}</Button>
          <Button className="onboarding-meet-choice" variant="outline" data-onboarding-consent="no" disabled={finishing} onClick={() => onFinish(false)}>{t("wizard.meetConsentNo")}</Button>
        </div>
      </> : <Button className="onboarding-primary" data-onboarding-start-chat disabled={finishing} onClick={() => onFinish(false)}>{t("wizard.meetStart")}</Button>}
      {/* The error banner already speaks for a map that could not be read. With the question there is no
          Start button, so the caption says where either answer leads. */}
      {failed ? null : <p className="onboarding-meet-caption" role="status">{following ? t("wizard.meetEitherChoice") : seeding ? t("wizard.meetKeepsGrowing") : t("wizard.meetReady")}</p>}
    </footer>
  </>;
}
