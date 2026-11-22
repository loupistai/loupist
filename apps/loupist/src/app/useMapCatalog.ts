import { useCallback, useEffect, useState } from "react";
import i18n from "@/i18n";
import type { KnowledgeCatalog, LoupistBridge } from "../types";
import { processingSourceCount } from "../knowledge/mapProcessing";
import { errorMessage } from "../lib/errors";

export type MapBuildStatus = { sourceCount: number | null };

type CatalogSnapshot = {
  loupistId: string;
  catalog: KnowledgeCatalog | null;
  error: string | null;
  build: MapBuildStatus | null;
  recovering: boolean;
};

/** The shell and the Map share a catalog and its pending-work refresh. */
export function useMapCatalog(bridge: LoupistBridge, loupistId: string | null, refreshKey = 0) {
  const [snapshot, setSnapshot] = useState<CatalogSnapshot | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const reload = useCallback(() => setReloadKey(value => value + 1), []);

  useEffect(() => {
    if (!loupistId) return;
    return bridge.onWorldSchemaChanged(event => {
      if (event.loupistId === loupistId) reload();
    });
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
        const recovering = graphResult.status === "fulfilled" ? graphResult.value.readiness.status === "recovering" : prior?.recovering ?? false;
        const count = processingSourceCount(catalog);
        return {
          loupistId, catalog, recovering,
          build: recovering || count > 0 ? { sourceCount: count || null } : null,
          error: catalogResult.status === "rejected" ? errorMessage(catalogResult.reason, i18n.t("knowledge:map.catalogError")) : null,
        };
      });
    });
    return () => { live = false; };
  }, [bridge, loupistId, refreshKey, reloadKey]);

  const current = snapshot?.loupistId === loupistId ? snapshot : null;
  const build = current?.build ?? null;
  useEffect(() => {
    if (!build) return;
    const timer = setTimeout(reload, 2_500);
    return () => clearTimeout(timer);
  }, [build, reload]);

  return { catalog: current?.catalog ?? null, error: current?.error ?? null, build, reload, refreshKey: refreshKey + reloadKey };
}

export type MapCatalog = ReturnType<typeof useMapCatalog>;
