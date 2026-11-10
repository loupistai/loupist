import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "../../i18n";
import type { GraphCamera, LoupistSummary, LoupistBridge, KnowledgeCatalog, KnowledgeLocation, KnowledgePin } from "../../types";
import type { KnowledgeNavigation } from "../../knowledge/location";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { MapView } from "../map/MapView";
import { AddSourceDialog } from "./SourceWorkspace";
export type MemoryWorkspaceProps = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  active?: boolean;
  navigation: KnowledgeNavigation;
  onPatch: (location: KnowledgeLocation, camera?: GraphCamera) => void;
  onOpen: (location: KnowledgeLocation, camera?: GraphCamera) => void;
  onBack: () => void;
  onAsk: (pin: KnowledgePin, threadId?: string) => void;
  refreshKey?: number;
  onManageProviders?: () => void;
  onOpenChat?: (chatId: string) => void;
  onAskInChat?: () => void;
  onManageProcessing?: () => void;
  onOpenLink?: (url: string) => void | Promise<void>;
  onExplainDisagreement?: (pins: KnowledgePin[]) => void;
};

export function MemoryWorkspace({ bridge, loupist, active = true, navigation, onPatch, onOpen, onBack, onAsk, refreshKey = 0, onAskInChat, onManageProcessing, onOpenLink, onOpenChat, onExplainDisagreement }: MemoryWorkspaceProps) {
  const { t } = useTranslation("knowledge");
  const [catalog, setCatalog] = useState<KnowledgeCatalog | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const [adding, setAdding] = useState(false);
  const changed = useCallback(() => setReload(n => n + 1), []);
  const current = catalog?.loupistId === loupist.id ? catalog : null;
  const location = navigation.current;
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    void bridge.knowledgeCatalog({ loupistId: loupist.id }).then(next => { if (!cancelled) { setCatalog(next); setError(null); } }).catch(cause => { if (!cancelled) setError(errorMessage(cause, t("map.catalogError"))); });
    return () => { cancelled = true; };
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `t` only words the failure; a language change must not reload.
  }, [active, bridge, loupist.id, reload, refreshKey]);
  useEffect(() => { setAdding(false); setError(null); }, [loupist.id]);
  useEffect(() => {
    if (!active || !current?.sources.some(s => s.objects.some(o => o.processing?.status === "queued" || o.processing?.status === "running"))) return;
    const timer = setTimeout(changed, 2500);
    return () => clearTimeout(timer);
  }, [active, current, changed]);
  const world = current?.worlds[0];
  const savedMap = location.surface === "world" ? location : navigation.views.world;
  const mapLocation: KnowledgeLocation = savedMap ?? { loupistId: loupist.id, surface: "world", view: { followCurrent: true } };
  const resolvedMap = !mapLocation.recordId || mapLocation.view?.followCurrent
    ? { ...mapLocation, recordId: world?.recordId, revisionId: world?.revisionId } : mapLocation;
  useEffect(() => {
    if (active && current && (location.surface === "world" || location.surface === "home") && (location.surface === "home" || location.recordId !== resolvedMap.recordId || location.revisionId !== resolvedMap.revisionId)) onPatch(resolvedMap);
  // eslint-disable-next-line react-hooks/exhaustive-deps -- `resolvedMap` is rebuilt each render; its record and revision are what the patch compares.
  }, [active, current, location, resolvedMap.recordId, resolvedMap.revisionId, onPatch]);
  return <section className="memory-workspace map-workspace" aria-label={t("map.title")}>
    {error ? <div role="alert">{error}<Button variant="outline" onClick={changed}>{t("map.retry")}</Button></div> : null}
    {!current && !error ? <p role="status">{t("map.loading")}</p> : null}
    {current ? <MapView onOpenChat={onOpenChat} bridge={bridge} catalog={current} location={resolvedMap} readerLocation={["source", "note"].includes(location.surface) ? location : undefined} active={active} refreshKey={refreshKey + reload} onPatch={onPatch} onReaderPatch={onPatch} onOpen={onOpen} onBack={navigation.past.length ? onBack : undefined} onAsk={onAsk} onAskInChat={onAskInChat} onAddSource={() => setAdding(true)} onChanged={changed} onManageProcessing={onManageProcessing} onOpenLink={onOpenLink} onExplainDisagreement={onExplainDisagreement} /> : null}
    <AddSourceDialog key={loupist.id} bridge={bridge} loupistId={loupist.id} open={adding} onOpenChange={setAdding} onSaved={next => { setAdding(false); changed(); onOpen(next); }} />
  </section>;
}
