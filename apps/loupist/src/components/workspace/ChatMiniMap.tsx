import { Component, lazy, Suspense, useMemo, useRef, useState, type ReactNode } from "react";
import type { KnowledgeLocation } from "../../types";
import { useTranslation } from "../../i18n";
import { formatNumber } from "../../i18n/format";
import { Button } from "../ui/button";
import { frozenLayout } from "../map/layout";
import type { MapCanvasControls } from "../map/MapCanvas";
import { MapList } from "../map/MapList";
import { useChatMap } from "./ChatMapContext";
import "../../styles/map.css";
const MapCanvas = lazy(() => import("../map/MapCanvas"));
class MiniCanvasBoundary extends Component<{ children: ReactNode; onError: () => void }, { failed: boolean }> {
  state = { failed: false };
  static getDerivedStateFromError() { return { failed: true }; }
  componentDidCatch() { this.props.onError(); }
  render() { return this.state.failed ? null : this.props.children; }
}
export function ChatMiniMap({ onOpenFull }: { onOpenFull: (location: KnowledgeLocation) => void }) {
  const { t } = useTranslation("knowledge");
  const state = useChatMap();
  const controls = useRef<MapCanvasControls | null>(null);
  const [list, setList] = useState(false);
  const map = state?.map;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the layout is keyed on the chat's identity, not the state object.
  const positions = useMemo(() => map && state ? frozenLayout(`chat:${state.loupistId}:${state.chatId}`, map.worldRevisionId, map.nodes, map.edges) : [], [map, state?.loupistId, state?.chatId]);
  if (!state) return null;
  const nameCount = map?.nodes.length ?? 0;
  const linkCount = map?.edges.length ?? 0;
  const stats = t("map.stats", { names: t("map.chatStatsNames", { count: nameCount, formattedCount: formatNumber(nameCount) }), links: t("map.statsLinks", { count: linkCount, formattedCount: formatNumber(linkCount) }) });
  const full = (selection?: KnowledgeLocation["selection"]) => onOpenFull({ loupistId: state.loupistId, surface: "world", selection });
  return <section className="chat-mini-map" aria-busy={state.loading}>
    <header className="chat-mini-map-header">
      <Button variant="link" size="sm" onClick={() => full()}>{t("map.chatOpenFull")}</Button>
      <Button variant="ghost" size="sm" aria-pressed={list} onClick={() => setList(!list)}>{t(list ? "map.canvasView" : "map.listView")}</Button>
    </header>
    {state.error ? <div className="map-error" role="alert">{t("map.error")}<Button variant="ghost" onClick={state.retry}>{t("map.retry")}</Button></div> : null}
    <div className="chat-mini-map-body">
      {state.loading && !map ? <div className="map-skeleton" role="status">{t("map.loading")}</div> : !map?.nodes.length ? <div className="map-empty" data-testid="map-empty"><p>{t("map.chatEmpty")}</p></div> : <>
        <div className="map-stats" data-testid="map-stats" role="status">{stats}</div>
        {list ? <MapList nodes={map.nodes} edges={map.edges} onSelect={node => full({ kind: "node", id: node.id })} onClaim={id => full({ kind: "claim", id })} /> : <MiniCanvasBoundary onError={() => setList(true)}><Suspense fallback={<div className="map-skeleton" />}><MapCanvas compact={{ highlightIds: state.highlightIds, glowNodeIds: state.glowNodeIds, glowEdgeIds: state.glowEdgeIds, liveNodeIds: state.liveNodeIds }} nodes={positions} edges={map.edges} scopeKey={`chat:${state.loupistId}:${state.chatId}`} controls={controls} onSelect={node => state.select(new Set([node.id]))} onClear={() => state.select()} stats={stats} /></Suspense></MiniCanvasBoundary>}
      </>}
    </div>
  </section>;
}
