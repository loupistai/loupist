import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber, formatTime } from "@/i18n/format";
import { Fragment, useCallback, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ArrowLeftIcon, ChevronDownIcon, MessageCircleIcon, RefreshCwIcon } from "lucide-react";
import type { ChatThread, LoupistBridge, LoupistSummary } from "../../types";
import { ASSET_CLASSES, type AssetCapitalFacts, type AssetChangeRow, type AssetCheckFailure, type AssetClass, type AssetEvidence, type AssetFlows, type AssetGlanceRow, type AssetPage as AssetPageData, type AssetPageTarget } from "../../contracts/assets";
import { useOnline } from "../../app/useOnline";
import { marketFor } from "../../chat/marketStatus";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { DropdownMenu, DropdownMenuContent, DropdownMenuRadioGroup, DropdownMenuRadioItem, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { connectionText } from "../marketplace/connectionCopy";
import english from "@/i18n/locales/en/connections.json";
import { AssetThesis } from "./AssetThesis";
import { checkKind, citationNumbers, dilutionText, FLOW_VALUE, flowText, GLANCE_KIND, glanceValue, rowSentence, shortDay, SourceChips } from "./assetText";
import { useAssetNames } from "./assetName";
import { lastAssetVisit, recordAssetVisit, rememberedRange, rememberRange } from "./assetVisit";
import { EarningsCard, earningsStatusText } from "./EarningsCard";
import { CalcChip, snapshotEvidence } from "./earningsText";
import { WatchFor } from "./WatchFor";
import type { EarningsEventSummary, EarningsSnapshot } from "../../contracts/earnings";
import type { WatchKind, WatchKindState } from "../../contracts/watch";
import "@/styles/asset-page.css";
import "@/styles/earnings.css";

const t = i18n.getFixedT(null, "assets");
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;
const DAY_MS = 86_400_000;
const PAGE_ROWS = 50;
const HISTORY_ROWS = 10;
type Range = "since" | "7" | "30";

type Props = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  target: AssetPageTarget;
  threads: readonly ChatThread[];
  onBack: () => void;
  /** Puts text into a Chat composer; never sends. */
  onAsk: (text: string) => void;
  /** Puts text into a new topic Chat's composer; never sends ("Ask about these results"). */
  onAskInNewChat?: (text: string) => void;
  onOpenChat: (threadId: string) => void;
  onOpenMap: (entityId?: string) => void;
  onOpenLink: (url: string) => void | Promise<void>;
  onOpenSource: (recordId: string) => void;
  onOpenConnections: () => void;
  onAddSource: () => void;
  onChanged?: () => void;
};

/**
 * A connection as the Connections tab names it. The bundled market-data runtime
 * is listed there under "Market data", the plain name used on this page.
 */
const connectionLabel = (name: string) => name === english.catalog.openbb_platform_name ? i18n.t("connections:catalog.MarketData") : connectionText(name);
const capitalized = (text: string) => text.charAt(0).toLocaleUpperCase() + text.slice(1);

/** Failed checks grouped by connection: "Couldn't check results, price · KRX didn't answer". */
function byConnection(failures: readonly { kind: string; connection: string }[]) {
  const groups = new Map<string, string[]>();
  for (const failure of failures) groups.set(failure.connection, [...groups.get(failure.connection) ?? [], failure.kind]);
  return [...groups].map(([connection, kinds]) => ({ connection, kinds }));
}

/** The checks a watcher switch covers once it is on (Rust's not-yet kinds). */
const WATCHED_CHECKS: Partial<Record<WatchKind, readonly string[]>> = {
  insiders: ["insiderTrades"], holders: ["ownershipChanges", "stakeFilings"], flows: ["investorFlows"],
  tvl: ["tvl"], positioning: ["positioning"], oilInventory: ["inventories"],
};

const localDay = (ms: number) => { const at = new Date(ms); return `${at.getFullYear()}-${String(at.getMonth() + 1).padStart(2, "0")}-${String(at.getDate()).padStart(2, "0")}`; };

/**
 * "Next earnings" reads the results events Rust recorded: the calendar or IR
 * date Rust put At a glance, else the next event still ahead, else the report
 * a Korean results chain is still due to file. "Not announced" only when none
 * of these exists. The row says whether the date is expected, confirmed or a
 * filing deadline.
 */
export function withNextEarnings(glance: readonly AssetGlanceRow[], events: readonly EarningsEventSummary[], snapshot: EarningsSnapshot | null, now = Date.now()): AssetGlanceRow[] {
  const rows = [...glance];
  const today = localDay(now);
  const at = rows.findIndex(row => row.key === "nextEarnings");
  if (at >= 0) {
    const row = rows[at]!;
    const date = typeof row.facts.date === "string" ? row.facts.date : "";
    const source = row.facts.dueBy ? "legal_deadline" : events.find(event => event.reportDate === date)?.dateSource ?? "calendar";
    rows[at] = { ...row, facts: { ...row.facts, source } };
    return rows;
  }
  const ahead = [...events, ...(snapshot ? [snapshot] : [])]
    .filter(event => (event.status === "upcoming" || event.status === "before_captured") && event.reportDate >= today)
    .sort((a, b) => a.reportDate.localeCompare(b.reportDate))[0];
  const due = snapshot?.chain.find(step => !step.filed && step.dueBy && step.dueBy >= today)?.dueBy;
  const facts = ahead ? { date: ahead.reportDate, period: ahead.period, source: ahead.dateSource }
    : due ? { date: due, period: snapshot!.period, source: "legal_deadline" }
      : { notAnnounced: true };
  const after = rows.findIndex(row => row.key === "lastResults");
  rows.splice(after < 0 ? rows.length : after + 1, 0, { key: "nextEarnings", facts, evidence: [] });
  return rows;
}

/** What a Korean equity's checks cannot read from these connections (spec §3.1); one sentence names them. */
const KOREA_NOT_YET = new Set(["creditBalances", "shortSaleBalances", "lockUps"]);
/** A private-use marker for a capital row's calculated dilution inside its sentence. */
const DILUTION = "";

/** A change row's sentence; a capital row's calculated dilution is its calculation chip (a filed one is covered by the row's source). */
function ChangeText({ row }: { row: AssetChangeRow }) {
  const dilution = row.kind === "capital" ? (row.facts as Partial<AssetCapitalFacts>).dilution : undefined;
  if (dilution?.basis !== "calculated" || !dilution.calc) return <>{rowSentence(row)}</>;
  const [before, after = ""] = rowSentence(row, { dilution: DILUTION }).split(DILUTION);
  const shown = dilutionText(dilution);
  return <>{before}<CalcChip calc={dilution.calc} spoken={shown}>{shown}</CalcChip>{after}</>;
}

/**
 * Investor flows for a Korean equity (finance-korea-retail-disagreement.md §2.1):
 * net buying by investor group (below zero is net selling), the short-selling
 * share and the lending balance, each a calculation chip with the date it
 * covers. Not prices, so no price colors; the lag is always stated. Without
 * Korea Investment, one line says how to connect it.
 */
function InvestorFlows({ headingId, flows, needsConnection, onOpenConnections }: { headingId: string; flows?: AssetFlows; needsConnection?: AssetCheckFailure; onOpenConnections: () => void }) {
  useTranslation("assets");
  const groups = flows ? [flows.rows.filter(row => row.unit === "KRW"), flows.rows.filter(row => row.unit !== "KRW")] : [];
  return <section className="asset-flows" aria-labelledby={headingId}>
    <div className="asset-flows-head">
      <h2 id={headingId} className="asset-section-label">{t("korea.flows")}</h2>
      {flows ? <p className="asset-flows-window">{t("korea.window", { days: formatNumber(flows.days), date: shortDay(flows.to) })}</p> : null}
    </div>
    {flows ? <>
      {groups.map((rows, group) => rows.length ? <Fragment key={group}>
        {/* Net buying by investor group: the header says what the sign means. */}
        {group === 0 ? <p className="asset-flows-subhead">{t("korea.netBuying")}</p> : null}
        <dl className="asset-glance asset-flows-rows" data-group={group === 0 ? "net" : "market"}>{rows.map(row => {
          const text = flowText(row, flows.to);
          const [before, after = ""] = text.sentence.split(FLOW_VALUE);
          return <div key={row.key} data-flow={row.key}>
            <dt>{text.label}</dt>
            <dd className="gg-num">{before}<CalcChip calc={row.calc} spoken={text.spoken}>{text.value}</CalcChip>{after}
              {row.key === "lending" ? <span className="asset-flows-hint">{t("korea.lendingHint")}</span> : null}
            </dd>
          </div>;
        })}</dl>
      </Fragment> : null)}
      <p className="asset-flows-caveat">{t("korea.caveat")}</p>
      <p className="asset-flows-from">{t("korea.from")}</p>
    </> : needsConnection ? <p className="asset-flows-needs-key">
      <button type="button" className="asset-inline-link" onClick={onOpenConnections}>{t("korea.connect", { connection: connectionLabel(needsConnection.connection) })}</button>
    </p> : null}
  </section>;
}

/** Rows grouped by fact day, newest first, as Rust ordered them. */
function byDay(rows: readonly AssetChangeRow[]) {
  const groups: { day: string; rows: AssetChangeRow[] }[] = [];
  for (const row of rows) {
    const last = groups.at(-1);
    if (last && last.day === row.factDate) last.rows.push(row);
    else groups.push({ day: row.factDate, rows: [row] });
  }
  return groups;
}

/**
 * One asset the user follows, over the current tab: their reasons and lines,
 * the Loupist's read, what changed with a date and a source on every row, and
 * what the Loupist checks. Every number and date comes from Rust.
 */
export function AssetPage({ bridge, loupist, target, threads, onBack, onAsk, onAskInNewChat, onOpenChat, onOpenMap, onOpenLink, onOpenSource, onOpenConnections, onAddSource, onChanged }: Props) {
  useTranslation(["assets", "chat"]);
  const loupistId = loupist.id;
  const online = useOnline();
  const [entered] = useState(() => Date.now());
  const [page, setPage] = useState<AssetPageData | null>(null);
  const [failed, setFailed] = useState(false);
  const [reload, setReload] = useState(0);
  const [lastVisit, setLastVisit] = useState<number | undefined>(undefined);
  const [range, setRange] = useState<Range | null>(null);
  const [rows, setRows] = useState<AssetChangeRow[] | null>(null);
  const [shownRows, setShownRows] = useState(PAGE_ROWS);
  const [allHistory, setAllHistory] = useState(false);
  const [checking, setChecking] = useState(false);
  /** Check now, Undo, Follow or the class menu failed; cleared by the next action. */
  const [actionError, setActionError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [arrived, setArrived] = useState<Set<string>>(new Set());
  const titleId = useId();
  const historyRef = useRef<HTMLElement>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const earlierRef = useRef<HTMLElement>(null);
  const assetId = page?.assetId;
  const names = useAssetNames(bridge, loupistId);
  /** The results card the page shows now (Rust decides the window), and every event for "Earlier results". */
  const [snapshot, setSnapshot] = useState<EarningsSnapshot | null>(null);
  const [events, setEvents] = useState<EarningsEventSummary[]>([]);
  const [expanded, setExpanded] = useState<Record<string, EarningsSnapshot | null>>({});
  /** This asset's watch switches, as "Watch for" shows them. */
  const [watchKinds, setWatchKinds] = useState<readonly WatchKindState[]>([]);

  useEffect(() => {
    if (!assetId || !bridge.earningsSnapshotGet) return;
    let live = true;
    void Promise.all([
      bridge.earningsSnapshotGet({ loupistId, assetId }).catch(() => null),
      bridge.earningsEventsList({ loupistId, assetId }).catch(() => []),
    ]).then(([current, list]) => { if (live) { setSnapshot(current); setEvents(list); } });
    return () => { live = false; };
  }, [bridge, loupistId, assetId, page?.revision]);

  const expand = useCallback(async (eventId: string) => {
    if (expanded[eventId] !== undefined) { setExpanded(current => { const next = { ...current }; delete next[eventId]; return next; }); return; }
    const loaded = await bridge.earningsSnapshotGet({ loupistId, assetId: assetId!, eventId }).catch(() => null);
    setExpanded(current => ({ ...current, [eventId]: loaded }));
  }, [bridge, loupistId, assetId, expanded]);

  // "See before and after": scroll to the card, or open the event's row in Earlier results.
  const reduced = typeof window !== "undefined" && typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  const scrollToResults = useCallback((eventId?: string) => {
    if (snapshot && (!eventId || eventId === snapshot.eventId)) { resultsRef.current?.scrollIntoView?.({ block: "start", behavior: reduced ? "auto" : "smooth" }); return; }
    if (eventId && expanded[eventId] === undefined) void expand(eventId);
    earlierRef.current?.scrollIntoView?.({ block: "start", behavior: reduced ? "auto" : "smooth" });
  }, [snapshot, expanded, expand, reduced]);
  useEffect(() => {
    if (target.section !== "results" || !page) return;
    const frame = window.requestAnimationFrame(() => scrollToResults(target.eventId));
    return () => window.cancelAnimationFrame(frame);
    // Once per opening, after the card or the events arrive.
  }, [target.section, target.eventId, page?.assetId, snapshot?.eventId, events.length]);

  const since = useCallback((choice: Range, visit: number | undefined) => {
    if (choice === "since" && visit !== undefined) return visit;
    return Date.now() - (choice === "7" ? 7 : 30) * DAY_MS;
  }, []);

  useEffect(() => {
    let live = true;
    setFailed(false);
    bridge.assetPageGet({ loupistId, assetId: target.assetId, symbol: target.assetId ? undefined : target.symbol, market: target.assetId ? undefined : target.market, name: target.name })
      .then(async (loaded) => {
        if (!live) return;
        const visit = lastAssetVisit(loupistId, loaded.assetId);
        setLastVisit(visit);
        setRange(current => current ?? (visit === undefined ? "30" : rememberedRange() ?? "since"));
        // The price "since your last look" needs the visit; reread with it when there is one.
        setPage(visit === undefined ? loaded : await bridge.assetPageGet({ loupistId, assetId: loaded.assetId, sinceMs: visit }).catch(() => loaded));
        void bridge.assetPageSeen({ loupistId, assetId: loaded.assetId }).catch(() => undefined);
      })
      .catch(() => { if (live) setFailed(true); });
    return () => { live = false; };
  }, [bridge, loupistId, target.assetId, target.symbol, target.market, target.name, reload]);

  useEffect(() => () => { if (assetId) recordAssetVisit(loupistId, assetId, entered); }, [loupistId, assetId, entered]);

  const loadRows = useCallback(async (choice: Range) => {
    if (!assetId) return [];
    const deltas = await bridge.assetDelta({ loupistId, assetId, sinceMs: since(choice, lastVisit) }).catch(() => []);
    return deltas.find(delta => delta.assetId === assetId)?.rows ?? [];
  }, [bridge, loupistId, assetId, lastVisit, since]);

  useEffect(() => {
    if (!assetId || !range) return;
    let live = true;
    setRows(null);
    void loadRows(range).then(next => { if (live) { setRows(next); setShownRows(PAGE_ROWS); } });
    return () => { live = false; };
  }, [assetId, range, loadRows, page?.revision]);

  useEffect(() => {
    if (target.section !== "history" || !page) return;
    historyRef.current?.scrollIntoView({ block: "start" });
  }, [target.section, page?.assetId]);

  const changed = useCallback(() => { setReload(value => value + 1); onChanged?.(); }, [onChanged]);

  const checkNow = async () => {
    if (!page || checking || !online) return;
    setChecking(true);
    setActionError(null);
    setAnnouncement(t("page.checking", { name }));
    const before = new Set((rows ?? []).map(row => row.id));
    try {
      const refreshed = await bridge.assetRefresh({ loupistId, assetId: page.assetId, sinceMs: lastVisit });
      setPage(refreshed);
      const next = await loadRows(range ?? "30");
      const fresh = next.filter(row => !before.has(row.id));
      setArrived(new Set(fresh.map(row => row.id)));
      setRows(next);
      setAnnouncement(t("page.newChanges", { count: fresh.length, formattedCount: formatNumber(fresh.length) }));
    } catch {
      setAnnouncement("");
      setActionError(t("error.checkNow", { name }));
    } finally {
      setChecking(false);
    }
  };

  // Undo names the move the user saw; a refresh in between does not stale it.
  const undo = async (moveId: string) => {
    if (!page) return;
    setActionError(null);
    try { setPage(await bridge.assetVerdictUndo({ loupistId, assetId: page.assetId, moveId, sinceMs: lastVisit })); }
    catch (cause) {
      const stale = (cause as { code?: unknown } | null)?.code === "conflict";
      setActionError(t(stale ? "error.undoStale" : "error.undo"));
      if (stale) changed();
    }
  };

  const setClass = async (next: AssetClass) => {
    if (!page || next === page.class) return;
    setActionError(null);
    try { setPage(await bridge.assetClassSet({ loupistId, assetId: page.assetId, class: next, sinceMs: lastVisit })); }
    catch { setActionError(t("error.classSet")); }
  };

  const follow = async () => {
    if (!page) return;
    setActionError(null);
    try {
      await bridge.watchlistAdd({ symbol: page.symbol, market: page.market, displayName: page.name, loupistId });
      changed();
    } catch { setActionError(t("error.follow", { name })); }
  };

  const openEvidence = useCallback((evidence: AssetEvidence) => {
    if (evidence.url) { void onOpenLink(evidence.url); return; }
    if (evidence.worldId) { onOpenMap(page?.entityId); return; }
    if (evidence.recordId) onOpenSource(evidence.recordId);
  }, [onOpenLink, onOpenMap, onOpenSource, page?.entityId]);

  const chooseRange = (next: Range) => { setRange(next); rememberRange(next); };
  const ranges: Range[] = lastVisit === undefined ? ["7", "30"] : ["since", "7", "30"];
  const rangeKeys = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!range || !["ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    const index = ranges.indexOf(range);
    const next = ranges[(index + (event.key === "ArrowRight" ? 1 : ranges.length - 1)) % ranges.length]!;
    chooseRange(next);
    event.currentTarget.querySelector<HTMLButtonElement>(`[data-range="${next}"]`)?.focus();
  };

  const visibleRows = useMemo(() => (rows ?? []).slice(0, shownRows), [rows, shownRows]);
  const crypto = page ? marketFor(page.market) === "CRYPTO" : false;
  // The class menu names the kind once: a market that only restates it (Crypto, PRIVATE, REAL) is left out.
  const restatesClass = page ? (crypto && page.class === "crypto") || (page.market.toUpperCase() === "PRIVATE" && page.class === "private") || (page.market.toUpperCase() === "REAL" && page.class === "real_asset") : false;
  const market = crypto ? i18n.t("chat:assetCard.crypto") : page?.market;
  const savedAt = page?.refreshedAtMs ?? page?.createdAtMs;
  const name = page ? names({ name: page.name, assetId: page.assetId, symbol: page.symbol, market: page.market, entityId: page.entityId }) : target.name ?? target.symbol ?? "";
  // Nothing to check automatically: sources only (private, real, prediction) or offline.
  const checkBlocked = !online ? t("offlineTooltip") : page?.checks.sourcesOnly ? t("page.nothingToCheck") : undefined;

  const header = <header className="asset-page-header">
    <Button type="button" variant="ghost" size="sm" onClick={onBack}><ArrowLeftIcon aria-hidden="true" />{i18n.t("knowledge:memoryPage.back")}</Button>
    <div className="asset-page-title">
      <h1 id={titleId}>{name}</h1>
      {page ? <p className="asset-page-meta">
        <span className="gg-num">{page.symbol}</span>
        {restatesClass ? null : <><span aria-hidden="true">·</span><span>{market}</span></>}
        <span aria-hidden="true">·</span>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <Button type="button" variant="ghost" size="sm" className="asset-class-trigger" aria-label={`${t("class.menu")}: ${tDynamic(`class.${page.class}`)}`}>{tDynamic(`class.${page.class}`)}<ChevronDownIcon aria-hidden="true" /></Button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuRadioGroup value={page.class} onValueChange={value => void setClass(value as AssetClass)}>
              {ASSET_CLASSES.map(item => <DropdownMenuRadioItem key={item} value={item}>{tDynamic(`class.${item}`)}</DropdownMenuRadioItem>)}
            </DropdownMenuRadioGroup>
          </DropdownMenuContent>
        </DropdownMenu>
        <span aria-hidden="true">·</span>
        {page.watch === "none"
          ? <Button type="button" variant="ghost" size="sm" className="asset-meta-action" onClick={() => void follow()}>{t("page.follow")}</Button>
          : <span>{t(page.watch === "holding" ? "page.youHold" : "page.watching")}</span>}
        <span aria-hidden="true">·</span>
        <span>{page.refreshedAtMs ? t("page.lastChecked", { time: formatTime(page.refreshedAtMs) }) : t("page.notChecked")}</span>
      </p> : null}
    </div>
    <div className="asset-page-actions">
      <Button type="button" variant="outline" className="asset-action-wide" onClick={() => page && onAsk(`$${page.symbol} `)} disabled={!page}>{t("page.askAbout")}</Button>
      <Button type="button" variant="outline" size="icon" className="asset-action-icon" aria-label={t("page.askAbout")} title={t("page.askAbout")} onClick={() => page && onAsk(`$${page.symbol} `)} disabled={!page}><MessageCircleIcon aria-hidden="true" /></Button>
      {/* A disabled button takes no pointer: its wrapper carries the reason as a tooltip. */}
      <span className="asset-action-wide" title={checkBlocked}>
        <Button type="button" variant="ghost" className="asset-check" onClick={() => void checkNow()} disabled={!page || !!checkBlocked || checking} aria-description={checkBlocked} aria-busy={checking || undefined}>
          {checking ? <Spinner /> : t("page.checkNow")}
        </Button>
      </span>
      <span className="asset-action-icon" title={checkBlocked ?? t("page.checkNow")}>
        <Button type="button" variant="ghost" size="icon" aria-label={t("page.checkNow")} aria-description={checkBlocked} onClick={() => void checkNow()} disabled={!page || !!checkBlocked || checking}>{checking ? <Spinner /> : <RefreshCwIcon aria-hidden="true" />}</Button>
      </span>
    </div>
  </header>;

  if (failed) return <section className="asset-page" aria-labelledby={titleId}>
    {header}
    <div className="asset-page-error" role="alert"><p>{t("error.load")}</p><Button type="button" variant="outline" size="sm" onClick={() => setReload(value => value + 1)}>{t("error.retry")}</Button></div>
  </section>;

  if (!page) return <section className="asset-page" aria-labelledby={titleId} aria-busy="true">
    {header}
    <div className="asset-page-body">
      <div className="asset-page-main">
        <div className="asset-thesis asset-skeleton-card"><span /><span /><span /></div>
        <div className="asset-skeleton-rows"><span /><span /><span /><span /></div>
      </div>
    </div>
  </section>;

  const history = allHistory ? page.history : page.history.slice(0, HISTORY_ROWS);
  const earlier = events.filter(event => event.eventId !== snapshot?.eventId).slice(0, 4);
  // "Last results" and "Next earnings" lead to the results card (or Earlier results) when there is one.
  const hasResults = Boolean(snapshot || earlier.length);
  const glanceRows = page.class === "equity" && !page.checks.sourcesOnly && page.glance.length > 0 ? withNextEarnings(page.glance, events, snapshot) : page.glance;
  const firstReason = [...page.reasons].sort((a, b) => a.createdAtMs - b.createdAtMs)[0];
  const wroteRow = firstReason && (allHistory || page.history.length < HISTORY_ROWS);
  const sinceDate = range ? since(range, lastVisit) : Date.now();
  const checks = page.checks;
  // A Korean equity: investor flows from Korea Investment, or the line that says how to turn it on.
  const korean = page.class === "equity" && marketFor(page.market) === "KRX";
  const flowsNeedConnection = korean && !page.flows ? checks.failures.find(failure => failure.kind === "flows" && failure.reason === "needs_connection") : undefined;
  const flowsSection = page.flows || flowsNeedConnection
    ? <InvestorFlows headingId={`${titleId}-flows`} flows={page.flows} needsConnection={flowsNeedConnection} onOpenConnections={onOpenConnections} /> : null;
  const koreaNotYet = korean && checks.notYet.length > 0 && checks.notYet.every(kind => KOREA_NOT_YET.has(kind));
  // Rust lists only what the last check read; failed and switched-off checks follow on their own lines.
  const failures = byConnection(checks.failures.filter(failure => failure.reason === "failed"));
  // Investor flows say what they need in their own section, once.
  const disconnected = byConnection(checks.failures.filter(failure => failure.reason === "needs_connection" && !(flowsNeedConnection && failure.kind === "flows")));
  const unread = new Set(checks.failures.map(failure => failure.kind));
  const kindList = (kinds: readonly string[]) => kinds.map(checkKind).join(", ");
  // A kind a watcher switch already checks is not "not checked yet".
  const watching = new Set(watchKinds.filter(state => state.enabled && !state.needsConnection).flatMap(state => WATCHED_CHECKS[state.kind] ?? []));
  const notYet = checks.notYet.filter(kind => !watching.has(kind));
  // One number per source, in reading order: the thesis card, What changed, history, then At a glance.
  const read = page.read;
  const latest = read && !read.started && read.latestMove && !(read.latestMove.actor === "user" && read.latestMove.undoesMoveId) ? read.latestMove : undefined;
  const numbers = citationNumbers([
    latest?.evidence ?? [],
    ...page.reasons.map(reason => reason.lastMove?.evidence ?? []),
    ...page.lines.map(line => line.last?.evidence ?? []),
    ...(snapshot ? snapshotEvidence(snapshot) : []),
    ...visibleRows.map(row => row.evidence),
    ...history.map(move => move.evidence),
    ...page.glance.map(row => row.evidence),
  ]);
  return <section className="asset-page" aria-label={t("page.label", { name })}>
    {header}
    {!online ? <p className="asset-page-offline">{t("offline", { time: savedAt ? formatTime(savedAt) : "" })}</p> : null}
    {actionError ? <p className="inline-error asset-action-error" role="alert">{actionError}</p> : null}
    <p className="sr-only" aria-live="polite">{announcement}</p>
    <div className="asset-page-body">
      <div className="asset-page-main">
        <AssetThesis bridge={bridge} loupistId={loupistId} loupistName={loupist.name} page={page} name={name} threads={threads} numbers={numbers}
          onChanged={changed} onUndo={moveId => void undo(moveId)} onOpenEvidence={openEvidence} onOpenChat={onOpenChat}
          onWorkItOut={() => onAsk(t("noThesis.prompt", { symbol: page.symbol }))} />

        {snapshot ? <div ref={resultsRef} className="asset-results">
          <EarningsCard bridge={bridge} loupistId={loupistId} loupistName={loupist.name} snapshot={snapshot} name={name} numbers={numbers}
            onOpenEvidence={openEvidence} onAsk={onAskInNewChat ?? onAsk} onOpenConnections={onOpenConnections}
            onChanged={next => setSnapshot(next)} highlight={target.section === "results" && (!target.eventId || target.eventId === snapshot.eventId)} />
        </div> : null}

        <section className="asset-changed" aria-labelledby={`${titleId}-changed`}>
          <div className="asset-changed-head">
            <h2 id={`${titleId}-changed`} className="asset-section-label">{t("changed.title")}</h2>
            {range ? <div className="asset-segmented" role="group" aria-label={t("changed.title")} onKeyDown={rangeKeys}>
              {ranges.map(choice => <button key={choice} type="button" data-range={choice} aria-pressed={range === choice} tabIndex={range === choice ? 0 : -1} onClick={() => chooseRange(choice)}>
                {t(choice === "since" ? "changed.sinceLook" : choice === "7" ? "changed.days7" : "changed.days30")}
              </button>)}
            </div> : null}
          </div>
          {lastVisit === undefined ? <p className="asset-caption">{t("changed.firstLook")}</p> : null}
          {rows === null ? <div className="asset-skeleton-rows"><span /><span /><span /><span /></div>
            : visibleRows.length ? <ol className="asset-changes">
              {byDay(visibleRows).map(group => group.rows.map((row, index) => <li key={row.id} className="asset-change" data-kind={row.kind} data-arrived={arrived.has(row.id) || undefined}>
                <span className="asset-change-day gg-num">{index === 0 ? shortDay(group.day) : ""}</span>
                <span className="asset-change-kind">{row.new ? <i className="asset-new-dot" aria-label={t("row.new")} /> : null}{tDynamic(`kind.${row.kind}`)}</span>
                <div className="asset-change-body">
                  <p><ChangeText row={row} /> <SourceChips evidence={row.evidence} numbers={numbers} onOpen={openEvidence} /></p>
                  {row.sections ? (row.sections.length
                    ? <ul className="asset-sections">{row.sections.map(section => <li key={section.section}>{t("row.section", { section: tDynamic(`section.${section.section}`), added: formatNumber(section.added), removed: formatNumber(section.removed), changed: formatNumber(section.changed) })}</li>)}</ul>
                    : <p className="asset-sections">{t("row.notCompared")}</p>) : null}
                </div>
              </li>))}
            </ol>
              : <div className="asset-changes-empty"><p>{t("changed.empty", { date: formatDate(sinceDate, { year: undefined, month: "short", day: "numeric" }) })}</p>{page.refreshedAtMs ? <p className="asset-caption">{t("page.lastChecked", { time: formatTime(page.refreshedAtMs) })}</p> : null}</div>}
          {rows && rows.length > shownRows ? <Button type="button" variant="ghost" size="sm" onClick={() => setShownRows(value => value + PAGE_ROWS)}>{t("page.showEarlier")}</Button> : null}
        </section>

        {/* No moves and no thesis yet: nothing to show, so no section. */}
        {history.length || wroteRow ? <section className="asset-history" ref={historyRef} aria-labelledby={`${titleId}-history`}>
          <h2 id={`${titleId}-history`} className="asset-section-label">{t("history.title")}</h2>
          <ol>
            {history.map(move => <li key={move.moveId}>
              <span className="asset-change-day gg-num">{shortDay(move.factDate)}</span>
              <p>{rowSentence({ kind: "thesis", factDate: move.factDate, facts: { from: move.from, to: move.to, reason: move.reason, line: move.line } })} <SourceChips evidence={move.evidence} numbers={numbers} onOpen={openEvidence} /></p>
            </li>)}
            {wroteRow ? <li>
              <span className="asset-change-day gg-num">{formatDate(firstReason.createdAtMs, { year: undefined, month: "short", day: "numeric" })}</span>
              <p>{t("history.wrote")}</p>
            </li> : null}
          </ol>
          {page.history.length > HISTORY_ROWS && !allHistory ? <Button type="button" variant="ghost" size="sm" onClick={() => setAllHistory(true)}>{t("page.showAll")}</Button> : null}
        </section> : null}

        {earlier.length ? <section className="earnings-earlier" ref={earlierRef} aria-labelledby={`${titleId}-earlier`}>
          <h2 id={`${titleId}-earlier`} className="asset-section-label">{t("earnings.earlier")}</h2>
          <ol>{earlier.map(event => {
            const open = expanded[event.eventId];
            return <li key={event.eventId}>
              <button type="button" className="earnings-earlier-row" aria-expanded={open !== undefined} onClick={() => void expand(event.eventId)}>
                <span>{t("earnings.title", { period: event.period })}</span>
                <span>{earningsStatusText(event)}</span>
              </button>
              {open ? <EarningsCard bridge={bridge} loupistId={loupistId} loupistName={loupist.name} snapshot={open} name={name}
                onOpenEvidence={openEvidence} onAsk={onAskInNewChat ?? onAsk} onOpenConnections={onOpenConnections}
                onChanged={next => setExpanded(current => ({ ...current, [next.eventId]: next }))} highlight={target.section === "results" && target.eventId === event.eventId} /> : null}
            </li>;
          })}</ol>
        </section> : null}
      </div>

      <aside className="asset-page-side">
        <section aria-labelledby={`${titleId}-glance`}>
          <h2 id={`${titleId}-glance`} className="asset-section-label">{t("glance.title")}</h2>
          {checks.sourcesOnly ? <div className="asset-sources-only">
            <p>{t("checks.sourcesOnly")}</p>
            <Button type="button" variant="outline" size="sm" onClick={onAddSource}>{t("checks.addSource")}</Button>
          </div> : page.glance.length ? <dl className="asset-glance">
            {glanceRows.map(row => {
              // The last check could not read this kind: the saved value stays, dimmed and dated.
              const stale = unread.has(GLANCE_KIND[row.key]);
              const price = row.key === "price" || row.key === "drawdown";
              return <div key={row.key}>
                <dt>{tDynamic(`glance.${row.key}`)}</dt>
                <dd className={`gg-num${price ? " asset-price" : ""}`} data-stale={stale || undefined} data-direction={price && !stale ? (Number(row.facts.change) < 0 ? "down" : Number(row.facts.change) > 0 ? "up" : undefined) : undefined}>
                  {row.facts.notAnnounced ? <span className="asset-caption">{t("earnings.notAnnounced")}</span>
                    : hasResults && (row.key === "lastResults" || row.key === "nextEarnings")
                      ? <button type="button" className="asset-glance-link" onClick={() => scrollToResults()}>{glanceValue(row)}</button>
                      : glanceValue(row)} <SourceChips evidence={row.evidence} numbers={numbers} onOpen={openEvidence} />
                  {/* The last filing and results already name their date. */}
                  {stale && row.date && row.key !== "lastFiling" && row.key !== "lastResults" ? <span className="asset-stale-date">{t("glance.asOf", { date: shortDay(row.date) })}</span> : null}
                </dd>
              </div>;
            })}
          </dl> : <p className="asset-caption">{t("page.notChecked")}</p>}
        </section>
        {/* After At a glance; at 700 px the side column follows What changed, so flows do too. */}
        {flowsSection}
        <section aria-labelledby={`${titleId}-checks`}>
          <h2 id={`${titleId}-checks`} className="asset-section-label">{t("checks.title", { loupist: loupist.name })}</h2>
          {checks.kinds.length ? <p>{t("checks.from", { kinds: capitalized(kindList(checks.kinds)), connections: checks.connections.map(connectionLabel).join(", ") })}</p> : null}
          {notYet.length ? <p className="asset-caption">{koreaNotYet ? t("korea.notChecked") : t("checks.notYet", { kinds: kindList(notYet) })}</p> : null}
          {failures.map(failure => <p key={`failed:${failure.connection}`} className="asset-check-problem">
            {t("checks.failed", { kind: kindList(failure.kinds), connection: connectionLabel(failure.connection) })}{" "}
            <Button type="button" variant="link" size="sm" disabled={!online || checking} onClick={() => void checkNow()}>{t("error.retry")}</Button>
          </p>)}
          {disconnected.map(failure => <p key={`connection:${failure.connection}`} className="asset-check-problem">
            <button type="button" className="asset-inline-link" onClick={onOpenConnections}>{t("checks.needsConnection", { connection: connectionLabel(failure.connection), kind: kindList(failure.kinds) })}</button>
          </p>)}
        </section>
        <WatchFor bridge={bridge} loupistId={loupistId} loupistName={loupist.name} assetId={page.assetId} name={name} onOpenConnections={onOpenConnections} onKinds={setWatchKinds} />
        {page.map ? <section aria-labelledby={`${titleId}-map`}>
          <h2 id={`${titleId}-map`} className="asset-section-label">{t("map.title")}</h2>
          <p>{t("map.counts", { suppliers: formatNumber(page.map.suppliers), customers: formatNumber(page.map.customers) })}</p>
          <Button type="button" variant="outline" size="sm" onClick={() => onOpenMap(page.entityId)}>{t("map.open")}</Button>
        </section> : null}
      </aside>
    </div>
    <p className="asset-page-notice">{i18n.t("knowledge:map.disclosure")}</p>
  </section>;
}
