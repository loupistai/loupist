import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import { useCallback, useEffect, useId, useState } from "react";
import { XIcon } from "lucide-react";
import type { LoupistBridge, LoupistSummary } from "../../types";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import type { HoldingsSummary, WatchlistEntry } from "../../contracts/userContext";
import { readKnowledgeMap } from "../../knowledge/mapRead";
import { entityLabel } from "../../knowledge/mapItems";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Spinner } from "../ui/spinner";
import { useAssetPageOpener } from "../assets/assetOpener";
import { useWatchSettings } from "../assets/WatchFor";
import { HoldingsSheet } from "../holdings/HoldingsSheet";
import { WatchingReader } from "./Watching";
import "@/styles/watch.css";
// Chips, the add form and meta lines share the Lens page's styles.
import "@/styles/memory-page.css";

const t = i18n.getFixedT(null, "briefings");
const tk = i18n.getFixedT(null, "knowledge");
const day = (ms: number) => formatDate(ms, { year: undefined, month: "short", day: "numeric" });

/** "AAPL", "005930:KRX" or a bare six-digit Korean code, which is a KRX listing. */
export function parseFollow(input: string): { symbol: string; market: string } {
  const [symbol = "", market] = input.trim().toUpperCase().split(":").map(part => part.trim());
  return { symbol, market: market || (/^\d{6}$/.test(symbol) ? "KRX" : "UNKNOWN") };
}

/** "Stocks and funds: 14 assets · Oct 12 · Crypto: 3 assets · Nov 1": counts and dates only, never quantities. */
function holdingsLine(holdings: HoldingsSummary | null) {
  if (!holdings || (!holdings.stocks && !holdings.crypto)) return tk("memoryPage.about.noHoldings");
  const kind = (value: HoldingsSummary["stocks"]) => value
    ? tk("memoryPage.about.kindCount", { count: value.count, formattedCount: formatNumber(value.count), date: day(value.importedAtMs) })
    : tk("memoryPage.about.kindNone");
  return tk("memoryPage.about.holdingsByKind", { stocks: kind(holdings.stocks), crypto: kind(holdings.crypto) });
}

const sameName = (left: string | null | undefined, right: string) => Boolean(left) && left!.trim().toLocaleLowerCase() === right.trim().toLocaleLowerCase();
/** A followed asset by the name the Map uses: its node's local name when one matches, else the stored display name. */
function followedName(entry: WatchlistEntry, nodes: readonly KnowledgeMapNode[]) {
  const node = nodes.find(candidate => [candidate.label, ...(candidate.localNames ?? [])].some(name => sameName(entry.displayName, name) || sameName(entry.symbol, name)));
  return node ? entityLabel(node, i18n.language) : entry.displayName ?? undefined;
}

export type FollowingPageProps = {
  bridge: LoupistBridge;
  /** The Loupist whose Watching settings and alerts show; what you follow is shared by every Loupist. */
  loupist: LoupistSummary;
  /** Leaves the page (Schedule shows it in place of its Watching reader). */
  onBack?: () => void;
};

/**
 * Following: what the user follows (shared by every Loupist), their holdings summary,
 * and what this Loupist watches for and found. Following is not a holding or a trade,
 * and nothing here starts a model run.
 */
export function FollowingPage({ bridge, loupist, onBack }: FollowingPageProps) {
  useTranslation(["briefings", "knowledge"]);
  const loupistId = loupist.id;
  const [watchlist, setWatchlist] = useState<WatchlistEntry[] | null>(null);
  const [nodes, setNodes] = useState<KnowledgeMapNode[]>([]);
  const [holdings, setHoldings] = useState<HoldingsSummary | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [reload, setReload] = useState(0);
  const [adding, setAdding] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mapping, setMapping] = useState(false);
  const [holdingsOpen, setHoldingsOpen] = useState(false);
  const [applying, setApplying] = useState(false);
  const { settings, setSettings, reload: reloadWatch } = useWatchSettings(bridge, loupistId);
  const openAsset = useAssetPageOpener();
  const headingId = useId(), listId = useId(), holdingsId = useId();
  const refresh = useCallback(() => setReload(value => value + 1), []);

  useEffect(() => {
    let live = true;
    setLoadFailed(false);
    void Promise.all([
      bridge.watchlistList(),
      bridge.holdingsSummary().catch(() => null),
      readKnowledgeMap(bridge, loupistId).then(map => map.nodes.filter(node => node.kind === "entity" && (node.watched || node.held))).catch(() => []),
    ]).then(([entries, summary, followed]) => { if (live) { setWatchlist(entries); setHoldings(summary); setNodes(followed); } })
      .catch(() => { if (live) setLoadFailed(true); });
    return () => { live = false; };
  }, [bridge, loupistId, reload]);

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true); setError(null);
    try { await action(); refresh(); void reloadWatch(); }
    catch (cause) { setError(errorMessage(cause, tk("memoryPage.actionError"))); }
    finally { setBusy(false); }
  };
  const add = () => {
    const { symbol, market } = parseFollow(adding);
    if (!symbol) return;
    void run(async () => {
      await bridge.watchlistAdd({ symbol, market, loupistId }); setAdding("");
      // One cheap status read: say so quietly while the Map is still reading what was followed.
      const status = await bridge.knowledgeFollowedStatus({ loupistId }).catch(() => null);
      setMapping(status?.state === "queued" || status?.state === "running" || status?.state === "waiting");
    });
  };
  // "Watch all": consent and the class defaults for every followed asset with no decision yet.
  const undecided = settings ? settings.notWatched.length : 0;
  const offerWatchAll = Boolean(settings) && (settings!.consentOff || undecided > 0);
  const watchAll = async () => {
    setApplying(true); setError(null);
    try { setSettings(await bridge.watchDefaultsApply({ loupistId })); }
    catch (cause) { setError(errorMessage(cause, t("following.watchAllFailed"))); }
    finally { setApplying(false); }
  };

  return <section className="following-page" data-testid="following-page" aria-labelledby={headingId}>
    {onBack ? <Button variant="ghost" className="briefings-back" onClick={onBack}>{t("schedule.back")}</Button> : null}
    <header className="following-head">
      <h1 id={headingId}>{t("following.title")}</h1>
      <p className="briefings-meta">{t("following.intro")}</p>
    </header>

    <div className="following-body">
    <div className="following-main">
    <section className="memory-card following-list" data-testid="memory-about" aria-labelledby={listId}>
      <h2 id={listId} className="sr-only">{tk("memoryPage.about.follow")}</h2>
      {loadFailed ? <div role="alert" className="inline-error"><span>{t("following.loadFailed")}</span> <Button variant="outline" size="sm" onClick={refresh}>{t("retry")}</Button></div>
        : watchlist === null ? <p role="status" className="briefings-meta"><Spinner /> {t("loading")}</p>
          : watchlist.length ? <ul className="memory-chips" data-testid="memory-chips">
            {watchlist.map(entry => {
              const name = followedName(entry, nodes);
              const label = name ? `${name} · ${entry.symbol}` : entry.symbol;
              return <li key={`${entry.symbol}:${entry.market}`}>
                {openAsset ? <button type="button" className="memory-chip-open" onClick={() => openAsset({ loupistId, symbol: entry.symbol, market: entry.market, name: name ?? undefined })}>{label}</button> : <span>{label}</span>}
                <Button variant="ghost" size="icon-sm" disabled={busy} aria-label={tk("memoryPage.about.remove", { name: name ?? entry.symbol })} onClick={() => void run(() => bridge.watchlistRemove({ symbol: entry.symbol, market: entry.market }))}><XIcon /></Button>
              </li>;
            })}
          </ul> : <p className="following-empty" data-following-empty>{t("following.empty")}</p>}
      <form className="memory-follow-add" data-testid="memory-follow-add" onSubmit={event => { event.preventDefault(); add(); }}>
        <Input aria-label={tk("memoryPage.about.addLabel")} placeholder={tk("memoryPage.about.addPlaceholder")} value={adding} onChange={event => setAdding(event.target.value)} />
        <Button type="submit" size="sm" variant="outline" disabled={busy || !adding.trim()}>{tk("memoryPage.about.add")}</Button>
      </form>
      <p className="memory-meta memory-follow-hint">{tk("memoryPage.about.addHint")}</p>
      {mapping ? <p className="memory-meta" role="status" data-follow-mapping>{tk("memoryPage.about.addingToMap")}</p> : null}
      {error ? <p className="memory-reader-error" role="alert">{error}</p> : null}
    </section>

    {offerWatchAll && watchlist?.length ? <div className="settings-row following-watch-all" data-watch-all>
      <p className="settings-row-description">{t("following.watchAllDetail")}</p>
      <Button disabled={applying} onClick={() => void watchAll()}>{applying ? <Spinner /> : null}{t("following.watchAll", { n: formatNumber(undecided || watchlist.length) })}</Button>
    </div> : null}

    {settings && watchlist?.length ? <WatchingReader bridge={bridge} loupist={loupist} settings={settings} onSettings={setSettings} embedded /> : null}
    </div>
    {/* Side column at wide sizes, after the list when compact: holdings are counts and dates only. */}
    <aside className="following-side" aria-labelledby={holdingsId}>
      <h2 id={holdingsId} className="following-caption">{t("following.holdingsTitle")}</h2>
      <p className="memory-meta" data-holdings-kinds>{holdingsLine(holdings)}</p>
      <Button variant="outline" size="sm" className="memory-holdings-update" data-testid="memory-holdings-update" onClick={() => setHoldingsOpen(true)}>{t("following.importHoldings")}</Button>
      <HoldingsSheet bridge={bridge} open={holdingsOpen} onOpenChange={open => { setHoldingsOpen(open); if (!open) refresh(); }} />
    </aside>
    </div>
  </section>;
}
