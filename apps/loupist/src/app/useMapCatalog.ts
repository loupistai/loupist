import { useCallback, useEffect, useMemo, useState } from "react";
import i18n from "@/i18n";
import type { GraphProcessingView, KnowledgeCatalog, LoupistBridge } from "../types";
import { worldProcessing } from "../knowledge/mapProcessing";
import { errorMessage } from "../lib/errors";

type CatalogSnapshot = {
  loupistId: string;
  catalog: KnowledgeCatalog | null;
  error: string | null;
  readiness: GraphProcessingView["readiness"] | null;
  world: GraphProcessingView | null;
};

/** The shell and the Map share a catalog and its pending-work refresh. */
export function useMapCatalog(bridge: LoupistBridge, loupistId: string | null, refreshKey = 0) {
  const [snapshot, setSnapshot] = useState<CatalogSnapshot | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const reload = useCallback(() => setReloadKey(value => value + 1), []);

  useEffect(() => {
    if (!loupistId) return;
    const stopWorld = bridge.onWorldSchemaChanged(event => {
      if (event.loupistId === loupistId) reload();
    });
    const stopProvider = bridge.onProviderStatus?.(reload);
    return () => { stopWorld(); stopProvider?.(); };
  }, [bridge, loupistId, reload]);

  useEffect(() => {
    if (!loupistId) return;
    let live = true;
    void Promise.allSettled([
      bridge.knowledgeCatalog({ loupistId }),
      bridge.knowledgeGraphSettingsGet({ loupistId }),
    ]).then(([catalogResult, graphResult]) => {
      if (!live) return;
      setSnapshot(previous => {
        const prior = previous?.loupistId === loupistId ? previous : null;
        const catalog = catalogResult.status === "fulfilled" && catalogResult.value.loupistId === loupistId
          ? catalogResult.value : prior?.catalog ?? null;
        return {
          loupistId, catalog,
          readiness: graphResult.status === "fulfilled" ? graphResult.value.readiness : prior?.readiness ?? null,
          world: graphResult.status === "fulfilled" ? graphResult.value : prior?.world ?? null,
          error: catalogResult.status === "rejected" ? errorMessage(catalogResult.reason, i18n.t("knowledge:map.catalogError")) : null,
        };
      });
    });
    return () => { live = false; };
  }, [bridge, loupistId, refreshKey, reloadKey]);

  const current = snapshot?.loupistId === loupistId ? snapshot : null;
  const catalog = current?.catalog ?? null;
  const readiness = current?.readiness ?? null;
  const processing = useMemo(() => worldProcessing(catalog, readiness), [catalog, readiness]);
  useEffect(() => {
    if (!processing.pending) return;
    const timer = setTimeout(reload, 2_500);
    return () => clearTimeout(timer);
  }, [processing.pending, current, reload]);

  // A chosen World model that became unavailable can go back to Recommended
  // from where World says so.
  const world = current?.world ?? null;
  const backToRecommended = useCallback(() => {
    if (!loupistId || !world) return;
    void bridge.knowledgeGraphSettingsSet({ loupistId, settings: { ...world.settings, worldModelChoice: "recommended" } })
      .catch(() => undefined).finally(reload);
  }, [bridge, loupistId, world, reload]);
  const onUseRecommendedModel = world?.settings.worldModelChoice === "chosen" && world.readiness.status === "model_unavailable" ? backToRecommended : undefined;

  return { catalog, error: current?.error ?? null, processing, reload, refreshKey: refreshKey + reloadKey, onUseRecommendedModel };
}

export type MapCatalog = ReturnType<typeof useMapCatalog>;
