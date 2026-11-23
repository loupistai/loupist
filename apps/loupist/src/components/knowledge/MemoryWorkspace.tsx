import { useEffect, useState } from "react";
import { useTranslation } from "../../i18n";
import type { GraphCamera, LoupistSummary, LoupistBridge, KnowledgeLocation, KnowledgePin } from "../../types";
import type { KnowledgeNavigation } from "../../knowledge/location";
import type { MapCatalog } from "../../app/useMapCatalog";
import { Button } from "../ui/button";
import { MapView } from "../map/MapView";
import { AddSourceDialog } from "./SourceWorkspace";
export type MemoryWorkspaceProps = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  active?: boolean;
  mapVisit?: { key: number; anchor?: string };
  navigation: KnowledgeNavigation;
  onPatch: (location: KnowledgeLocation, camera?: GraphCamera) => void;
  onOpen: (location: KnowledgeLocation, camera?: GraphCamera) => void;
  onBack: () => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void;
  catalogState: MapCatalog;
  onManageProviders?: () => void;
  onOpenChat?: (chatId: string) => void;
  onAskInChat?: () => void;
  onManageProcessing?: () => void;
  onOpenLink?: (url: string) => void | Promise<void>;
  onExplainDisagreement?: (pins: KnowledgePin[]) => void;
};

export function MemoryWorkspace({ bridge, loupist, active = true, mapVisit, navigation, onPatch, onOpen, onBack, onAsk, catalogState, onAskInChat, onManageProcessing, onManageProviders, onOpenLink, onOpenChat, onExplainDisagreement }: MemoryWorkspaceProps) {
  const { t } = useTranslation("knowledge");
  const { catalog: current, processing, error, reload: changed, refreshKey, onUseRecommendedModel } = catalogState;
  const [adding, setAdding] = useState(false);
  const location = navigation.current;
  useEffect(() => { setAdding(false); }, [loupist.id]);
  const world = current?.worlds[0];
  const savedMap = location.surface === "world" ? location : navigation.views.world;
  const mapLocation: KnowledgeLocation = savedMap ?? { loupistId: loupist.id, surface: "world" };
  const resolvedMap = { ...mapLocation, recordId: world?.recordId, revisionId: world?.revisionId };
  useEffect(() => {
    if (active && current && (location.surface === "world" || location.surface === "home") && (location.surface === "home" || location.recordId !== resolvedMap.recordId || location.revisionId !== resolvedMap.revisionId)) onPatch(resolvedMap);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `resolvedMap` is rebuilt each render; its record and revision are what the patch compares.
  }, [active, current, location, resolvedMap.recordId, resolvedMap.revisionId, onPatch]);
  return <section className="memory-workspace map-workspace" aria-label={t("map.title")}>
    {error ? <div role="alert">{error}<Button variant="outline" onClick={changed}>{t("map.retry")}</Button></div> : null}
    {!current && !error ? <p role="status">{t("map.loading")}</p> : null}
    {current ? <MapView key={mapVisit?.key} initialAnchor={mapVisit?.anchor} onOpenChat={onOpenChat} bridge={bridge} catalog={current} worldProcessing={processing} location={resolvedMap} readerLocation={["source", "note"].includes(location.surface) ? location : undefined} active={active} refreshKey={refreshKey} onPatch={onPatch} onReaderPatch={onPatch} onOpen={onOpen} onBack={navigation.past.length ? onBack : undefined} onAsk={onAsk} onAskInChat={onAskInChat} onAddSource={() => setAdding(true)} onChanged={changed} onManageProcessing={onManageProcessing} onManageProviders={onManageProviders} onUseRecommendedModel={onUseRecommendedModel} onOpenLink={onOpenLink} onExplainDisagreement={onExplainDisagreement} /> : null}
    <AddSourceDialog key={loupist.id} bridge={bridge} loupistId={loupist.id} open={adding} onOpenChange={setAdding} onSaved={next => { setAdding(false); changed(); onOpen(next); }} />
  </section>;
}
