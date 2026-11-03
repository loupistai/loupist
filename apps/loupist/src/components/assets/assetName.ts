/**
 * One name per asset on Korean surfaces: the Korean name when the asset is
 * listed in Korea and the Map knows a Hangul name for it, otherwise the name the
 * user follows it by. Briefings, the Thread, receipts and the page all use it,
 * so "한울정밀" never sits next to "Hanul Precision".
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { AssetDelta } from "../../contracts/assets";
import type { KnowledgeMapNode } from "../../contracts/knowledge";

const KOREAN_MARKETS = new Set(["KRX", "KOSPI", "KOSDAQ", "KONEX", "KS", "KQ"]);
const HANGUL = /[ᄀ-ᇿ㄰-㆏가-힣]/u;
export const isKoreanListed = (market?: string | null) => !!market && KOREAN_MARKETS.has(market.trim().toUpperCase());

export type AssetNameInput = { name: string; assetId?: string; symbol?: string; market?: string | null; entityId?: string | null };
export type AssetNameData = {
  nodes?: readonly Pick<KnowledgeMapNode, "id" | "kind" | "label" | "localNames" | "archived">[] | null;
  pages?: readonly (Pick<AssetDelta, "assetId" | "symbol" | "market" | "entityId"> & { name?: string })[] | null;
};

const same = (a?: string | null, b?: string | null) => !!a && !!b && a.trim().toLowerCase() === b.trim().toLowerCase();

/** The asset's name in `language` (see the module note); pure, so every surface agrees. */
export function localAssetName(asset: AssetNameInput, data: AssetNameData, language: string) {
  if (!language.toLowerCase().startsWith("ko") || HANGUL.test(asset.name)) return asset.name;
  // A name alone (a run's summary line) finds its page by that name.
  const page = (data.pages ?? []).find(item => (asset.assetId && item.assetId === asset.assetId)
    || (!asset.assetId && same(item.symbol, asset.symbol) && (!asset.market || same(item.market, asset.market)))
    || (!asset.assetId && !asset.symbol && same(item.name, asset.name)));
  if (!isKoreanListed(asset.market ?? page?.market)) return asset.name;
  const entities = (data.nodes ?? []).filter(node => node.kind === "entity" && !node.archived);
  const entityId = asset.entityId ?? page?.entityId;
  const node = (entityId ? entities.find(item => item.id === entityId) : undefined)
    ?? entities.find(item => same(item.label, asset.name) || (item.localNames ?? []).some(name => same(name, asset.name)) || same(item.label, asset.symbol));
  return node?.localNames?.find(name => HANGUL.test(name)) ?? asset.name;
}

/** One read of the Map and the asset pages per Loupist, shared by the surfaces on screen. */
const FRESH_MS = 15_000;
const loads = new WeakMap<LoupistBridge, Map<string, { atMs: number; data: Promise<AssetNameData> }>>();
function load(bridge: LoupistBridge, loupistId: string) {
  const byLoupist = loads.get(bridge) ?? new Map<string, { atMs: number; data: Promise<AssetNameData> }>();
  loads.set(bridge, byLoupist);
  const cached = byLoupist.get(loupistId);
  if (cached && Date.now() - cached.atMs < FRESH_MS) return cached.data;
  const data = Promise.all([
    bridge.knowledgeMap({ loupistId, maxNodes: 1000 }).catch(() => null),
    // Read-only: every page's identity; nothing is created.
    bridge.assetDelta?.({ loupistId, sinceMs: Date.now() }).catch(() => []) ?? Promise.resolve([]),
  ]).then(([map, pages]): AssetNameData => ({ nodes: map?.nodes ?? [], pages }));
  byLoupist.set(loupistId, { atMs: Date.now(), data });
  return data;
}

/** Names assets for the current language; reads the Map only in Korean and only when `enabled`. */
export function useAssetNames(bridge: LoupistBridge | undefined, loupistId: string, enabled = true) {
  const { i18n } = useTranslation();
  const language = i18n.language;
  const needed = enabled && language.toLowerCase().startsWith("ko");
  const [data, setData] = useState<AssetNameData>({});
  useEffect(() => {
    if (!needed || !bridge) return undefined;
    let live = true;
    void load(bridge, loupistId).then(next => { if (live) setData(next); });
    return () => { live = false; };
  }, [bridge, loupistId, needed]);
  return useCallback((asset: AssetNameInput) => localAssetName(asset, data, language), [data, language]);
}
