import i18n, { useTranslation } from "@/i18n";
import { formatAsOf } from "../../chat/asOf";
import { formatDate, formatDateTime, formatNumber } from "@/i18n/format";
import { useEffect, useState } from "react";
import type { ChatEvidenceRef, KnowledgeLocation, LoupistBridge } from "../../types";
import type { KnowledgeMapNode } from "../../contracts/knowledge";
import { entityLabel } from "../map/labels";
import { firstSentence } from "../../chat/morningMessage";
import { marketFor } from "../../chat/marketStatus";
import { tickerMark } from "../../chat/assetTickers";
import { Button } from "../ui/button";
import { useChatExecution } from "./ChatExecutionContext";
import type { AssetQuote, Verdict } from "../../contracts/assets";
import { useAssetPageOpener } from "../assets/assetOpener";
import { shortDay, verdictLabel } from "../assets/assetText";
import { localAssetName } from "../assets/assetName";
import { CitationChip } from "./CitationChip";

const t = i18n.getFixedT(null, "chat");

export type AssetRef = { symbol: string; market?: string; name?: string };

type CardData = {
  name: string;
  watch: "watching" | "holding" | "none";
  node?: KnowledgeMapNode;
  briefing?: { atMs: number; quote: string };
  /** The read on the user's thesis, when they wrote one for this asset. */
  thesis?: { verdict: Verdict; since: string };
};

const same = (a: string | null | undefined, b: string | null | undefined) => !!a && !!b && a.trim().toLowerCase() === b.trim().toLowerCase();

/** Cards for the same asset share one quote request for this long. */
const QUOTE_REUSE_MS = 5 * 60_000;
const quoteRequests = new Map<string, { atMs: number; quote: Promise<AssetQuote | null> }>();

/** The last close Rust knows (`asset_quote`); several cards for one asset ask once. */
function readQuote(bridge: LoupistBridge, loupistId: string, symbol: string, market: string) {
  const key = JSON.stringify([loupistId, symbol.toUpperCase(), market.toUpperCase()]);
  const cached = quoteRequests.get(key);
  if (cached && Date.now() - cached.atMs < QUOTE_REUSE_MS) return cached.quote;
  const quote = bridge.assetQuote ? bridge.assetQuote({ loupistId, symbol, market }).catch(() => null) : Promise.resolve(null);
  quoteRequests.set(key, { atMs: Date.now(), quote });
  return quote;
}

/** Forgets shared quote requests (tests). */
export const resetQuoteRequests = () => quoteRequests.clear();

/** "₩39,920", "$187.42"; a price without an ISO currency keeps its digits only. */
function priceText(quote: AssetQuote) {
  const currency = quote.currency && /^[A-Z]{3}$/u.test(quote.currency) ? quote.currency : undefined;
  const digits = Math.abs(quote.value) >= 1_000 ? 0 : 2;
  return currency
    ? formatNumber(quote.value, { style: "currency", currency, minimumFractionDigits: 0, maximumFractionDigits: digits })
    : formatNumber(quote.value, { maximumFractionDigits: digits });
}

/** The close's source as a citation, so its hover card names the source and when it was read. */
const quoteEvidence = (quote: AssetQuote): ChatEvidenceRef => ({
  id: `quote:${quote.evidence.tool}:${quote.date}`,
  title: quote.evidence.title,
  kind: "original",
  url: quote.evidence.url,
  provider: quote.evidence.provider,
  tool_name: quote.evidence.tool,
  retrieved_at: quote.retrievedAt ?? "",
  excerpt: "",
  excerpt_truncated: false,
  content_sha256: "",
  warnings: [],
});

/**
 * The asset card above an answer about one asset the user named with `$`
 * (composer spec D2). It reads only what is already on this computer: the
 * watchlist, the Map and the latest briefings, plus the last close Rust reads
 * through the asset page's price read (`asset_quote`), dated and cited. It
 * never shows quantities; without a market connection the price slot is empty.
 */
export function AssetCard({ asset, loupistId, onOpenMapNode, asOf }: {
  asset: AssetRef;
  loupistId: string;
  onOpenMapNode: (location: KnowledgeLocation) => void;
  /** An as-of answer: the card drops today's read and says which day it speaks for. */
  asOf?: string;
}) {
  const { i18n: instance } = useTranslation("chat");
  const scope = useChatExecution();
  const openAsset = useAssetPageOpener();
  const [data, setData] = useState<CardData | null>(null);
  const [quote, setQuote] = useState<AssetQuote | null>(null);
  const [added, setAdded] = useState(false);
  // An as-of answer speaks for an earlier day, so it shows no current price.
  useEffect(() => {
    setQuote(null);
    if (!scope || !asset.market || asOf) return undefined;
    let live = true;
    void readQuote(scope.bridge, loupistId, asset.symbol, asset.market).then((next) => { if (live) setQuote(next); });
    return () => { live = false; };
  }, [scope?.bridge, loupistId, asset.symbol, asset.market, asOf]);
  useEffect(() => {
    if (!scope) return undefined;
    let live = true;
    const { bridge } = scope;
    void Promise.all([
      bridge.watchlistList().catch(() => []),
      bridge.knowledgeMap({ loupistId, maxNodes: 1000 }).catch(() => null),
      bridge.inboxList({ loupistId, limit: 10 }).catch(() => ({ items: [], nextCursor: null })),
      // Read-only: every page's read; a card never creates a page.
      bridge.assetDelta?.({ loupistId, sinceMs: Date.now() }).catch(() => []) ?? Promise.resolve([]),
    ]).then(([watchlist, map, inbox, pages]) => {
      if (!live) return;
      const entry = watchlist.find((item) => item.symbol === asset.symbol && (!asset.market || item.market === asset.market));
      const display = entry?.displayName?.trim() || asset.name?.trim() || asset.symbol;
      // The asset page's own match comes first, so the card and the page name the same Map entity.
      const page = pages.find((item) => item.symbol.toUpperCase() === asset.symbol.toUpperCase() && (!asset.market || item.market.toUpperCase() === asset.market.toUpperCase()));
      const entities = (map?.nodes ?? []).filter((candidate) => candidate.kind === "entity" && !candidate.archived);
      const node = (page?.entityId ? entities.find((candidate) => candidate.id === page.entityId) : undefined)
        ?? entities.find((candidate) => same(candidate.label, display) || same(candidate.label, asset.symbol) || (candidate.localNames ?? []).some((name) => same(name, display)));
      const market = asset.market ?? page?.market;
      // In Korean with a known market, the name follows the asset page's rule (Korean names for Korean listings).
      const name = market && instance.language.toLowerCase().startsWith("ko")
        ? localAssetName({ name: display, symbol: asset.symbol, market, entityId: node?.id }, { nodes: entities }, instance.language)
        : node ? entityLabel(node, instance.language) : display;
      // A briefing may name the asset in any language the Map knows it by.
      const names = [name, display, asset.symbol, node?.label, ...(node?.localNames ?? [])].filter((value): value is string => Boolean(value));
      const mention = inbox.items.find((item) => item.status === "succeeded" && names.some((needle) => item.summary.includes(needle)));
      setData({
        name,
        node,
        watch: node?.held ? "holding" : entry || node?.watched ? "watching" : "none",
        briefing: mention ? { atMs: mention.finishedAtMs ?? mention.startedAtMs, quote: firstSentence(mention.summary) } : undefined,
        thesis: page?.verdict && page.verdictSince ? { verdict: page.verdict, since: page.verdictSince } : undefined,
      });
    });
    return () => { live = false; };
  }, [scope?.bridge, loupistId, asset.symbol, asset.market, instance.language]);
  if (!data) return null;
  const facts = [
    data.node ? t("assetCard.mapNames", { count: data.node.degree, formattedCount: formatNumber(data.node.degree) }) : t("assetCard.notOnMap"),
    data.watch === "holding" ? t("assetCard.holding") : data.watch === "watching" || added ? t("assetCard.watching") : "",
    data.briefing && !asOf ? t("assetCard.lastBriefing", { date: formatDate(data.briefing.atMs, { year: undefined, month: "short", day: "numeric" }), quote: data.briefing.quote }) : "",
    asOf ? t("assetCard.asOf", { date: formatAsOf(asOf) }) : "",
  ].filter(Boolean);
  const closeDay = quote ? formatDate(Date.parse(`${quote.date}T00:00:00Z`), { year: undefined, month: "short", day: "numeric", timeZone: "UTC" }) : "";
  const readAt = quote?.retrievedAt && Number.isFinite(Date.parse(quote.retrievedAt)) ? formatDateTime(Date.parse(quote.retrievedAt)) : "";
  const openPage = openAsset && asset.market ? () => openAsset({ loupistId, symbol: asset.symbol, market: asset.market!, name: data.name }) : undefined;
  return <section className="asset-card" aria-label={t("assetCard.label", { name: data.name })}>
    <div className="asset-card-head">
      <p className="asset-card-title">
        <i className="composer-chip-marker" data-kind={tickerMark(asset.market)} aria-hidden="true" />
        <strong>{data.name}</strong>
        {/* Rows follow the asset's class: a crypto asset names its class instead of an exchange. */}
        {/* The symbol shows once: when the name is the symbol itself, only the market follows. */}
        <span>{[data.name === asset.symbol ? "" : asset.symbol, asset.market && marketFor(asset.market) === "CRYPTO" ? t("assetCard.crypto") : asset.market].filter(Boolean).join(" · ")}</span>
      </p>
      {/* A completed session's close from Rust, never a live tick; its source reads like a citation. */}
      {quote ? <p className="asset-card-price">
        <strong className="gg-num">{priceText(quote)}</strong>
        <span>{t("assetCard.close", { date: closeDay })}</span>
        <CitationChip evidence={quoteEvidence(quote)} label={t("assetCard.priceSource", { source: quote.evidence.title, time: readAt || closeDay })}
          title={quote.evidence.title} source={readAt ? t("assetCard.readAt", { time: readAt }) : t("assetCard.close", { date: closeDay })}
          onOpen={() => openPage?.()}>{quote.evidence.title}</CitationChip>
      </p> : null}
    </div>
    <p className="asset-card-facts">{facts.join(" · ")}</p>
    {data.thesis && !asOf ? <p className="asset-card-facts">{t("assetCard.thesis", { verdict: verdictLabel(data.thesis.verdict), date: shortDay(data.thesis.since) })}</p> : null}
    <div className="asset-card-actions">
      {openPage ? <Button type="button" variant="ghost" size="sm" onClick={openPage}>{t("assetCard.openPage")}</Button> : null}
      {data.node ? <Button type="button" variant="ghost" size="sm" onClick={() => onOpenMapNode({ loupistId, surface: "world", recordId: data.node!.recordId, objectId: data.node!.id, selection: { kind: "node", id: data.node!.id } })}>{t("assetCard.openOnMap")}</Button> : null}
      {data.watch === "none" && !added && scope && asset.market ? <Button type="button" variant="ghost" size="sm" onClick={() => {
        void scope.bridge.watchlistAdd({ symbol: asset.symbol, market: asset.market!, displayName: data.name === asset.symbol ? null : data.name, loupistId }).then(() => setAdded(true)).catch(() => {});
      }}>{t("assetCard.addToWatchlist")}</Button> : null}
    </div>
  </section>;
}
