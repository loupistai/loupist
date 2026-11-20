import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber, storedPriceWasRounded } from "@/i18n/format";
import { Fragment, useCallback, useEffect, useId, useMemo, useRef, useState, type FormEvent } from "react";
import { ArrowLeftIcon, ArrowUpIcon, NetworkIcon, RefreshCwIcon } from "lucide-react";
import type { ChatThread, LoupistBridge, LoupistSummary } from "../../types";
import { assetExchangeLabel, assetTicker, type AssetEvidence, type AssetMapCounts, type AssetOpenRequest, type AssetPage as AssetPageData, type AssetPageTarget, type AssetPricePoint, type AssetStoredPrice } from "../../contracts/assets";
import { AssetTile } from "../market/AssetTile";
import { PriceText } from "../market/PriceText";
import { ConvertedAmount } from "../market/ConvertedAmount";
import { priceAsOf } from "../market/priceAsOf";
import { chartPoints, offeredRanges, PriceChart, rangePoints, type PriceRange } from "./PriceChart";
import { useOnline } from "../../app/useOnline";
import { Button } from "../ui/button";
import { Spinner } from "../ui/spinner";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "../ui/dialog";
import { connectionLabel } from "./connectionName";
import { AssetThesis } from "./AssetThesis";
import { citationNumbers, moveSentence, readSentence, shortDay, SourceChips } from "./assetText";
import { providerName } from "@/lib/marketProviderName";
import { useAssetNames } from "./assetName";
import "@/styles/asset-page.css";
import { commandErrorCode } from "@/lib/errors";

const t = i18n.getFixedT(null, "assets");
const HISTORY_ROWS = 10;
/** Past this many characters the ask field's placeholder leaves the name out. */
const PLACEHOLDER_NAME = 28;
/** The host's error for a run that needs an AI account it does not have (delegated price setup). */
const NO_MODEL = new Set(["provider_unavailable", "not_configured", "signed_out", "pi_unavailable"]);

type Props = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  target: AssetPageTarget;
  threads: readonly ChatThread[];
  onBack: () => void;
  /** Puts text into a Chat composer; never sends. */
  onAsk: (text: string) => void;
  onOpenChat: (threadId: string) => void;
  onOpenMap: (entityId?: string) => void;
  onOpenLink: (url: string) => void | Promise<void>;
  onOpenSource: (recordId: string) => void;
  onOpenConnections: () => void;
  onChanged?: () => void;
  /** The name the page shows, once it has read the asset, so the app header shows the same one. */
  onName?: (name: string) => void;
};

/** "Map · 3 suppliers, 2 customers": a count of zero is left out, and with none the line just says Map. */
function mapLine(map: AssetMapCounts) {
  const parts = [
    map.suppliers ? t("assetPage.map.suppliers", { count: map.suppliers, formattedCount: formatNumber(map.suppliers) }) : "",
    map.customers ? t("assetPage.map.customers", { count: map.customers, formattedCount: formatNumber(map.customers) }) : "",
  ].filter(Boolean);
  return parts.length ? `${t("assetPage.map.label")} · ${parts.join(", ")}` : t("assetPage.map.label");
}

/** The first request for a target: its id, else a saved link's ticker (the host matches it exactly). */
export function openRequest(target: Pick<AssetPageTarget, "loupistId" | "assetId" | "ticker">): AssetOpenRequest {
  return target.assetId ? { loupistId: target.loupistId, assetId: target.assetId } : { loupistId: target.loupistId, ticker: target.ticker ?? null };
}

/** What happened after Check now, until the page shows its result. */
type CheckNote = "follow" | "noModel" | "failed" | null;

/**
 * One asset over the current tab: its name and reference fields, the one price the
 * host's saved read replays (value, currency, stated date, provider and source, or why
 * there is none), its chart, the user's thesis with its history, the ask box and the
 * Map line. No figure on the page is computed here; nothing reads a provider but the host.
 */
export function AssetPage({ bridge, loupist, target, threads, onBack, onAsk, onOpenChat, onOpenMap, onOpenLink, onOpenSource, onOpenConnections, onChanged, onName }: Props) {
  useTranslation(["assets", "chat"]);
  const loupistId = loupist.id;
  const online = useOnline();
  const [page, setPage] = useState<AssetPageData | null>(null);
  /** The page could not be opened: `unknown` and `ambiguous` are a saved link's ticker the host could not place. */
  const [failure, setFailure] = useState<"load" | "unknown" | "ambiguous" | null>(null);
  const [reload, setReload] = useState(0);
  const [points, setPoints] = useState<AssetPricePoint[] | null>(null);
  const [priceSource, setPriceSource] = useState<AssetStoredPrice | null>(null);
  const [seriesFailed, setSeriesFailed] = useState(false);
  const [seriesRetry, setSeriesRetry] = useState(0);
  const historyRequest = useRef<{ loupistId: string; assetId: string; retry: number; pending: Promise<AssetPricePoint[]> } | null>(null);
  const [chartRange, setChartRange] = useState<PriceRange | null>(null);
  const [checking, setChecking] = useState(false);
  const [checkNote, setCheckNote] = useState<CheckNote>(null);
  /** Undo, Follow or unlinking failed; cleared by the next action. */
  const [actionError, setActionError] = useState<string | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const [allHistory, setAllHistory] = useState(false);
  const [question, setQuestion] = useState("");
  const askInputRef = useRef<HTMLInputElement>(null);
  const quoteRef = useRef<HTMLDivElement>(null);
  /** Check now ran from the price line: when the control used goes, focus stays in the line. */
  const focusInQuote = useRef(false);
  const titleId = useId();
  const historyRef = useRef<HTMLElement>(null);
  /** The asset the host confirmed for this target; rereads ask for it by id. */
  const confirmed = useRef<string | null>(null);
  const changedWhileOpening = useRef(false);
  const assetId = page?.asset.assetId;
  const names = useAssetNames(bridge, loupistId, Boolean(page));

  // Opening the page may replay its saved read (the host reuses a result under 15 minutes old).
  useEffect(() => {
    let live = true;
    setFailure(null);
    const request = confirmed.current ? { loupistId, assetId: confirmed.current, storedOnly: true } : openRequest({ loupistId, assetId: target.assetId, ticker: target.ticker });
    bridge.assetPageGet(request)
      .then(loaded => {
        if (!live) return;
        confirmed.current = loaded.asset.assetId;
        setPage(loaded);
        if (changedWhileOpening.current) {
          changedWhileOpening.current = false;
          setReload(value => value + 1);
        }
        void bridge.assetPageSeen({ loupistId, assetId: loaded.asset.assetId }).catch(() => undefined);
      })
      .catch(cause => {
        if (!live) return;
        const code = commandErrorCode(cause);
        setFailure(code === "unknown_asset" ? "unknown" : code === "ambiguous_asset" ? "ambiguous" : "load");
      });
    return () => { live = false; };
  }, [bridge, loupistId, target.assetId, target.ticker, reload]);

  // Reread the stored page when a follow, setup or price changes.
  useEffect(() => bridge.onAssetsChanged?.(() => {
    if (confirmed.current) setReload(value => value + 1);
    else changedWhileOpening.current = true;
  }), [bridge]);

  // The chart: the history read (with reuse) or the points replays gathered, read again when the page changes.
  const revision = page?.revision;
  const priceDate = page?.currentPrice?.date;
  const priceValue = page?.currentPrice?.value;
  useEffect(() => {
    if (!assetId) return;
    let live = true;
    const previous = historyRequest.current;
    const request = { loupistId, assetId };
    let pending: Promise<AssetPricePoint[]>;
    if (previous?.loupistId === loupistId && previous.assetId === assetId && previous.retry === seriesRetry) {
      // Let the opening history read settle before displaying its stored points.
      pending = previous.pending.catch(() => []).then(() => bridge.assetPriceSeries({ ...request, storedOnly: true }));
    } else {
      pending = bridge.assetPriceSeries(request);
      historyRequest.current = { ...request, retry: seriesRetry, pending };
    }
    pending.then(next => { if (live) { setPoints(chartPoints(next)); setSeriesFailed(false); } })
      .catch(() => { if (live) { setPoints(null); setSeriesFailed(true); } });
    return () => { live = false; };
  }, [bridge, loupistId, assetId, revision, priceDate, priceValue, seriesRetry]);

  useEffect(() => {
    if (target.section !== "history" || !page) return;
    historyRef.current?.scrollIntoView?.({ block: "start" });
  // eslint-disable-next-line react-hooks/exhaustive-deps -- scrolls once per opened asset, not on every page refresh.
  }, [target.section, page?.asset.assetId]);

  const changed = useCallback(() => { setReload(value => value + 1); onChanged?.(); }, [onChanged]);

  // A note's action (Try again, Find a price) leaves when the note changes; focus moves to the
  // line's new note instead of falling to the page. Focus elsewhere ends this.
  useEffect(() => {
    if (!focusInQuote.current) return;
    const active = document.activeElement;
    if (active && active !== document.body) { if (!quoteRef.current?.contains(active)) focusInQuote.current = false; return; }
    const note = quoteRef.current?.querySelector<HTMLElement>("[data-price-note]");
    (note ?? quoteRef.current)?.focus();
  });

  const ticker = page ? assetTicker(page.asset) : null;
  const exchange = page?.asset.reference?.exchange?.trim() || null;
  const name = page ? names({ name: page.asset.name, assetId: page.asset.assetId, symbol: ticker ?? undefined, market: exchange, entityId: page.entityId }) : target.name ?? target.ticker ?? "";
  const busy = checking || Boolean(page?.priceSetupRunning);
  const loaded = Boolean(page);
  useEffect(() => { if (loaded && name) onName?.(name); }, [loaded, name, onName]);

  /** Check now: replay without reuse and find a saved read if the replay still fails. */
  const checkNow = async () => {
    if (!page || busy || !online) return;
    focusInQuote.current = Boolean(quoteRef.current?.contains(document.activeElement));
    setChecking(true);
    setActionError(null);
    setCheckNote(null);
    setAnnouncement(t("page.checking", { name }));
    try {
      const next = await bridge.assetPriceCheck({ loupistId, assetId: page.asset.assetId });
      setPage(next);
      if (next.asset.followedAtMs === null) {
        setCheckNote("follow");
        setAnnouncement("");
        return;
      }
      // Reconcile with a setup that may have settled while the command reply was in flight.
      if (next.priceSetupRunning) setReload(value => value + 1);
      setAnnouncement(next.savedRead?.state === "ready" && next.currentPrice
        ? `${t("price.checked", { name })} ${priceAsOf(next.currentPrice.date, next.currentPrice.kind)}` : "");
    } catch (cause) {
      setAnnouncement("");
      setCheckNote(NO_MODEL.has(commandErrorCode(cause) ?? "") ? "noModel" : "failed");
    } finally {
      setChecking(false);
    }
  };

  // Undo names the move the user saw; a refresh in between does not stale it.
  const undo = async (moveId: string) => {
    if (!page) return;
    setActionError(null);
    try { setPage(await bridge.assetVerdictUndo({ loupistId, assetId: page.asset.assetId, moveId })); }
    catch (cause) {
      const stale = commandErrorCode(cause) === "conflict";
      setActionError(t(stale ? "error.undoStale" : "error.undo"));
      if (stale) changed();
    }
  };

  const follow = async () => {
    if (!page) return;
    setActionError(null);
    try { await bridge.assetFollow({ loupistId, assetId: page.asset.assetId }); changed(); }
    catch { setActionError(t("error.follow", { name })); }
  };

  const unlink = async () => {
    if (!page) return;
    setActionError(null);
    try { setPage(await bridge.assetEntitySet({ loupistId, assetId: page.asset.assetId, entityId: null })); onChanged?.(); }
    catch { setActionError(t("error.unlink")); }
  };

  const openEvidence = useCallback((evidence: AssetEvidence) => {
    if (evidence.url) { void onOpenLink(evidence.url); return; }
    if (evidence.worldId) { onOpenMap(page?.entityId ?? undefined); return; }
    if (evidence.recordId) onOpenSource(evidence.recordId);
  }, [onOpenLink, onOpenMap, onOpenSource, page?.entityId]);

  // The source is the provider's page or the retained price receipt.
  const price = page?.currentPrice ?? null;
  const priceTool = page?.savedRead?.current.tool;
  const priceEvidence = useMemo((): AssetEvidence[] => price ? [{
    tool: priceTool ?? "price", provider: price.provider, title: providerName(price.provider) ?? price.provider, url: price.sourceUrl ?? undefined, dataset: price.dataset, date: price.date,
  }] : [], [price, priceTool]);

  // A question names the listing it is about; an asset without a ticker is its name alone.
  const questionSubject = ticker ? `${name} (${exchange ? `${ticker}, ${exchange}` : ticker})` : name;
  const thesisPrompt = ticker ? t("noThesis.prompt", { name, symbol: ticker, market: exchange ?? "" }) : t("noThesis.promptNoTicker", { name });
  const ask = (text: string) => onAsk(`${questionSubject} ${text}`.trimEnd() + (text ? "" : " "));
  const submitQuestion = (event: FormEvent) => { event.preventDefault(); ask(question.trim()); };
  const focusAsk = () => { askInputRef.current?.scrollIntoView?.({ block: "center" }); askInputRef.current?.focus(); };

  const exchangeLabel = assetExchangeLabel(page?.asset.reference);
  const meta = page ? [ticker ? <span key="ticker" className="gg-num">{ticker}</span> : null, exchangeLabel ? <span key="exchange">{exchangeLabel}</span> : null].filter(Boolean) : [];
  const checkBlocked = !online ? t("offlineTooltip") : undefined;
  const header = <header className="asset-page-header">
    <Button type="button" variant="ghost" size="sm" className="asset-back" onClick={onBack}><ArrowLeftIcon aria-hidden="true" />{i18n.t("knowledge:memoryPage.back")}</Button>
    <div className="asset-hero">
      <AssetTile name={name} symbol={ticker ?? target.ticker} size="lg" />
      <div className="asset-page-title">
        {/* Two lines at most; the pointer shows a longer name whole. */}
        <h1 id={titleId} title={name}>{name}</h1>
        {page ? <p className="asset-page-meta">
          {/* The reference fields as the host stored them; an asset known by name alone starts at its follow state. */}
          {meta.map((item, index) => <Fragment key={index}>{index ? <span aria-hidden="true">·</span> : null}{item}</Fragment>)}
          {meta.length ? <span aria-hidden="true">·</span> : null}
          {page.asset.followedAtMs === null
            ? <Button type="button" variant="ghost" size="sm" className="asset-meta-action" onClick={() => void follow()}>{t("page.follow")}</Button>
            : <span>{t("page.watching")}</span>}
        </p> : null}
      </div>
      <div className="asset-page-actions">
        {/* A disabled button takes no pointer: its wrapper carries the reason as a tooltip. */}
        <span title={checkBlocked}>
          {/* Busy keeps the button focusable (aria-disabled), so a keyboard user stays on it while it checks. */}
          <Button type="button" variant="ghost" className="asset-check" aria-label={t("page.checkNow")} aria-description={checkBlocked} aria-busy={busy || undefined}
            aria-disabled={busy || undefined} onClick={() => void checkNow()} disabled={!page || !!checkBlocked}>
            {busy ? <Spinner /> : <RefreshCwIcon aria-hidden="true" />}<span className="asset-check-text">{t("page.checkNow")}</span>
          </Button>
        </span>
        <Button type="button" className="asset-ask" onClick={focusAsk} disabled={!page}>{t("assetPage.ask.button", { loupist: loupist.name })}</Button>
      </div>
    </div>
  </header>;

  if (failure === "unknown" || failure === "ambiguous") return <section className="asset-page" aria-labelledby={titleId}>
    {header}
    <section className="asset-card asset-unresolved" role="status">
      <p>{t(failure === "unknown" ? "saved.unknown" : "saved.ambiguous", { ticker: target.ticker ?? "" })}</p>
      <div><Button type="button" variant="ghost" size="sm" onClick={onBack}>{t("saved.back")}</Button></div>
    </section>
  </section>;

  if (failure === "load") return <section className="asset-page" aria-labelledby={titleId}>
    {header}
    <div className="asset-page-error" role="alert"><p>{t("error.load")}</p><Button type="button" variant="outline" size="sm" onClick={() => setReload(value => value + 1)}>{t("error.retry")}</Button></div>
  </section>;

  if (!page) return <section className="asset-page" aria-labelledby={titleId} aria-busy="true">
    {header}
    <div className="asset-page-body">
      <div className="asset-page-main">
        <div className="asset-card asset-skeleton-card"><span /><span /><span /></div>
      </div>
    </div>
  </section>;

  const history = allHistory ? page.history : page.history.slice(0, HISTORY_ROWS);
  const firstReason = [...page.reasons].sort((a, b) => a.createdAtMs - b.createdAtMs)[0];
  const wroteRow = firstReason && (allHistory || page.history.length < HISTORY_ROWS);
  const read = page.read;
  const latest = read ? readSentence(read).move : undefined;
  // One number per source, in reading order: the price, then the thesis and its history.
  const numbers = citationNumbers([
    priceEvidence,
    latest?.evidence ?? [],
    ...page.reasons.map(reason => reason.lastMove?.evidence ?? []),
    ...page.lines.map(line => line.last?.evidence ?? []),
    ...history.map(move => move.evidence),
  ]);

  const state = page.savedRead?.state ?? "none";
  const connection = page.savedRead ? connectionLabel(page.savedRead.current.server ?? page.savedRead.current.tool) : "";
  const currency = price?.currency?.trim() || undefined;
  // Check now under its own name in the line: "Try again" after a failure, "Find a price" without one.
  const retry = <Button type="button" variant="link" size="sm" disabled={!online} aria-disabled={busy || undefined} aria-busy={busy || undefined} onClick={() => void checkNow()}>{t(state === "failed" ? "error.retry" : "price.find")}</Button>;
  const reconnect = <Button type="button" variant="link" size="sm" onClick={onOpenConnections}>{t("price.openConnections")}</Button>;
  // Why the shown price is not current, or why there is none: one line, with what to do about it.
  const priceNote = page.asset.followedAtMs === null && (!price || checkNote === "follow") ? <p className="asset-quote-note" tabIndex={-1} data-price-note="follow" role="status">{t("price.followRequired")}</p>
    // While the Loupist looks, the line says so and offers nothing: Check now waits in the header.
    : page.priceSetupRunning ? <p className="asset-quote-note" tabIndex={-1} data-price-note="setup" role="status">{t("price.setupStarted", { loupist: loupist.name, name })}</p>
      : checkNote === "noModel" ? <p className="asset-quote-note" tabIndex={-1} data-price-note="no-model" role="alert">{t("price.noModel", { loupist: loupist.name })}</p>
        : checkNote === "failed" ? <p className="asset-quote-note" tabIndex={-1} data-price-note="check-failed" role="alert">{t("error.checkNow", { name })}</p>
          : state === "needs_connection" ? <p className="asset-quote-note" tabIndex={-1} data-price-note="needs-connection">{t(price ? "price.needsConnectionKept" : "price.needsConnection", { connection })} {reconnect}</p>
            : state === "failed" ? <p className="asset-quote-note" tabIndex={-1} data-price-note="failed">{t(price ? "price.failedKept" : "price.failed", { connection, loupist: loupist.name })} {retry}</p>
              : !price ? <p className="asset-quote-note" tabIndex={-1} data-price-note="none">{t("price.none", { loupist: loupist.name })} {retry}</p> : null;
  const priceLine = <div ref={quoteRef} className="asset-quote" data-price-read={state} tabIndex={-1}>
    {price ? <>
      <PriceText value={price.value} currency={currency} digits="auto" size="figure" />
      <small className="asset-quote-meta" data-price-date={price.date}>{priceAsOf(price.date, price.kind)}{providerName(price.provider) ? ` · ${providerName(price.provider)}` : ""}
        {" "}<SourceChips evidence={priceEvidence} numbers={numbers} onOpen={evidence => {
          if (evidence.url) void onOpenLink(evidence.url);
          else setPriceSource(price);
        }} /></small>
      {/* The display currency, from the stored price's own currency; none when the result stated none. */}
      <ConvertedAmount value={price.value} currency={currency} />
    </> : null}
    {priceNote}
  </div>;

  const ranges = points ? offeredRanges(points) : [];
  const range = chartRange && ranges.includes(chartRange) ? chartRange : ranges.at(-1) === "1Y" ? "3M" : ranges.at(-1);
  const chart = points && range && ranges.length
    ? <div className="asset-card asset-chart-card"><PriceChart points={rangePoints(points, range)} currency={currency} range={range} ranges={ranges} onRange={setChartRange} name={name} /></div>
    // A price exists but the chart could not be read: say so and offer a retry, never an empty frame.
    : seriesFailed && price ? <div className="asset-page-error" role="alert"><p>{t("assetPage.chart.failed")}</p><Button type="button" variant="outline" size="sm" onClick={() => setSeriesRetry(value => value + 1)}>{t("error.retry")}</Button></div>
      : null;

  // The ask box: a question about this asset goes into Bot's message box; nothing is sent from here.
  const suggestions = [t("assetPage.ask.recent"), page.reasons.length ? t("assetPage.ask.checkThesis") : undefined].filter((item): item is string => Boolean(item));
  const askBox = <section className="asset-card asset-ask-box" aria-labelledby={`${titleId}-ask`}>
    <h2 id={`${titleId}-ask`} className="asset-card-title">{t("assetPage.ask.title", { loupist: loupist.name })}</h2>
    <form className="asset-ask-field" onSubmit={submitQuestion}>
      {/* A long name would push the words that say what the field is for out of view (in Korean they come last). */}
      <input ref={askInputRef} value={question} onChange={event => setQuestion(event.target.value)} placeholder={name.length > PLACEHOLDER_NAME ? t("assetPage.ask.placeholderAny") : t("assetPage.ask.placeholder", { name })} aria-label={t("assetPage.ask.placeholder", { name })} maxLength={2000} />
      <button type="submit" aria-label={t("assetPage.ask.send")} title={t("assetPage.ask.send")}><ArrowUpIcon aria-hidden="true" /></button>
    </form>
    <ul className="asset-ask-chips" aria-label={t("assetPage.ask.suggestions")}>
      {suggestions.map(text => <li key={text}><button type="button" onClick={() => ask(text)}>{text}</button></li>)}
      {!page.reasons.length ? <li><button type="button" onClick={() => onAsk(thesisPrompt)}>{t("assetPage.ask.writeThesis")}</button></li> : null}
    </ul>
    <p className="asset-ask-note">{t("assetPage.ask.note")}</p>
  </section>;

  const mapRow = page.map && page.entityId ? <section className="asset-card asset-lines-card">
    <div className="asset-line-row">
      <NetworkIcon aria-hidden="true" />
      <span>{mapLine(page.map)}</span>
      <Button type="button" variant="ghost" size="sm" onClick={() => onOpenMap(page.entityId ?? undefined)}>{t("assetPage.open")}</Button>
      <Button type="button" variant="ghost" size="sm" onClick={() => void unlink()}>{t("assetPage.map.unlink")}</Button>
    </div>
  </section> : null;

  return <section className="asset-page" aria-label={t("page.label", { name })}>
    <Dialog open={priceSource !== null} onOpenChange={open => { if (!open) setPriceSource(null); }}>
      {priceSource ? <DialogContent>
        <DialogHeader>
          <DialogTitle>{providerName(priceSource.provider) ?? priceSource.provider}</DialogTitle>
          <DialogDescription>{priceAsOf(priceSource.date, priceSource.kind)}</DialogDescription>
        </DialogHeader>
        <DialogBody>
          <p><PriceText value={priceSource.value} currency={priceSource.currency ?? undefined} digits="auto" /></p>
          <dl className="evidence-receipt">
            {storedPriceWasRounded(priceSource.value, priceSource.currency) ? <>
              <dt>{t("price.storedValue")}</dt><dd className="gg-num">{String(priceSource.value)}</dd>
            </> : null}
            <dt>{i18n.t("chat:workspaceEvidence.dataset")}</dt>
            <dd><code>{priceSource.dataset.dataset_id}</code> · {i18n.t("chat:workspaceEvidence.datasetVersion", { version: priceSource.dataset.revision })}</dd>
          </dl>
        </DialogBody>
      </DialogContent> : null}
    </Dialog>
    {header}
    {priceLine}
    {!online ? <p className="asset-page-offline">{t("offline")}</p> : null}
    {actionError ? <p className="inline-error asset-action-error" role="alert">{actionError}</p> : null}
    <p className="sr-only" aria-live="polite">{announcement}</p>
    <div className="asset-page-body">
      <div className="asset-page-main">
        {chart}
        {/* No moves and no thesis yet: nothing to show, so no section. */}
        {history.length || wroteRow ? <section className="asset-card asset-history" ref={historyRef} aria-labelledby={`${titleId}-history`}>
          <h2 id={`${titleId}-history`} className="asset-card-title">{t("history.title")}</h2>
          <ol>
            {history.map(move => <li key={move.moveId}>
              <span className="asset-change-day gg-num">{shortDay(move.factDate)}</span>
              <p>{moveSentence(move)} <SourceChips evidence={move.evidence} numbers={numbers} onOpen={openEvidence} /></p>
            </li>)}
            {wroteRow ? <li>
              <span className="asset-change-day gg-num">{formatDate(firstReason.createdAtMs, { year: undefined, month: "short", day: "numeric" })}</span>
              <p>{t("history.wrote")}</p>
            </li> : null}
          </ol>
          {page.history.length > HISTORY_ROWS && !allHistory ? <Button type="button" variant="ghost" size="sm" onClick={() => setAllHistory(true)}>{t("page.showAll")}</Button> : null}
        </section> : null}
      </div>

      <aside className="asset-page-side">
        {askBox}
        <AssetThesis bridge={bridge} loupistId={loupistId} loupistName={loupist.name} page={page} name={name} threads={threads} numbers={numbers}
          onChanged={changed} onUndo={moveId => void undo(moveId)} onOpenEvidence={openEvidence} onOpenChat={onOpenChat}
          onWorkItOut={() => onAsk(thesisPrompt)} />
        {mapRow}
      </aside>
    </div>
    <p className="asset-page-notice">{i18n.t("knowledge:map.disclosure")}</p>
  </section>;
}
