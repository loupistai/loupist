import i18n, { useTranslation } from "@/i18n";
import { useCallback, useEffect, useMemo, useState } from "react";
import type {
  LoupistBridge,
  ModelCatalog,
  ModelRunSelection,
} from "../types";
import type { ModelProviderActions } from "../components/settings/ModelSettingsPanel";
import { errorMessage } from "../lib/errors";
import { visibleCatalogModels } from "../lib/modelSelection";
import { CONFIG_CHANGED_EVENT, notifyConfigurationChanged } from "./useConfiguration";

/** Model catalog loading and per-loupist model run selections. */
export function useModelCatalog(bridge: LoupistBridge) {
  useTranslation();
  const [catalog, setCatalog] = useState<ModelCatalog | null>(null);
  const [resolved, setResolved] = useState(false);
  const [settingsError, setSettingsError] = useState<string | null>(null);
  const [selections, setSelections] = useState<
    Record<string, ModelRunSelection>
  >({});

  useEffect(() => {
    let active = true;
    let initialReadFinished = false;
    let refreshRequested = false;
    let refreshGeneration = 0;
    const refresh = () => {
      if (!active) return;
      if (!initialReadFinished) {
        refreshRequested = true;
        return;
      }
      const generation = ++refreshGeneration;
      void bridge.modelCatalogGet().then((next) => {
        if (!active || generation !== refreshGeneration) return;
        setCatalog(next);
        setSettingsError(null);
      }).catch((cause) => {
        if (!active || generation !== refreshGeneration) return;
        setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotReloadModelSettings")));
      });
    };
    const readSavedCatalog = async () => {
      if (!active) return;
      try {
        const settings = await bridge.modelCatalogGet();
        if (!active) return;
        setCatalog(settings);
        setSettingsError(null);
        // Restore every connected provider missing from the catalog. A usable
        // first account must not hide the others until Settings happens to open.
        // Existing models, including deliberately hidden ones, are kept.
        for (const provider of [...settings.providers, ...settings.custom_providers]) {
          if (provider.credential_source === "missing" || settings.models.some(model => model.provider === provider.id)) continue;
          try {
            const next = await bridge.providerModels(provider.id);
            if (!active) return;
            setCatalog(next);
            setSettingsError(null);
          } catch {
            if (!active) return;
            setSettingsError(i18n.t("app:useModelCatalog.couldNotLoadAvailableModels"));
          }
        }
      } catch (cause) {
        if (active) setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotLoadModelSettings")));
      } finally {
        if (active) {
          setResolved(true);
          initialReadFinished = true;
          if (refreshRequested) refresh();
        }
      }
    };
    const start = () => { void readSavedCatalog(); };
    const stop = bridge.onModelCatalogChanged?.(refresh, start);
    if (!bridge.onModelCatalogChanged) start();
    window.addEventListener("focus", refresh);
    window.addEventListener(CONFIG_CHANGED_EVENT, refresh);
    return () => {
      active = false;
      stop?.();
      window.removeEventListener("focus", refresh);
      window.removeEventListener(CONFIG_CHANGED_EVENT, refresh);
    };
  }, [bridge]);

  // Rust emits provider-status on every state or notice change; patch it in place so cards, the composer and onboarding update live.
  useEffect(() => bridge.onProviderStatus?.(({ provider, status }) => {
    setCatalog((current) => {
      if (!current) return current;
      const providers = current.providers.some((item) => item.id === provider)
        ? current.providers.map((item) => (item.id === provider ? { ...item, status } : item)) : current.providers;
      const custom_providers = current.custom_providers.some((item) => item.id === provider)
        ? current.custom_providers.map((item) => (item.id === provider ? { ...item, status } : item)) : current.custom_providers;
      return { ...current, providers, custom_providers };
    });
  }), [bridge]);

  const acknowledgeModelNotice = useCallback((id: string) => {
    void bridge.modelNoticeAck({ id }).then(() => notifyConfigurationChanged()).catch((cause) => {
      setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotReloadModelSettings")));
    });
  }, [bridge]);

  const changeSelection = useCallback(
    (loupistId: string, selection: ModelRunSelection) => {
      setSelections((current) => ({
        ...current,
        [loupistId]: selection,
      }));
    },
    [],
  );

  /** Keep the host default unpinned when its tagged row is chosen. */
  const resetSelection = useCallback((loupistId: string) => {
    setSelections((current) => {
      const { [loupistId]: _removed, ...rest } = current;
      return rest;
    });
  }, []);

  /** Provider operations used by Settings; each refreshes the catalog it returns. */
  const providerActions = useMemo<ModelProviderActions>(() => {
    const apply = async (pending: Promise<ModelCatalog>) => {
      const next = await pending;
      setCatalog(next);
      setSettingsError(null);
      notifyConfigurationChanged();
      return next;
    };
    return {
      onLoadModels: (provider) => apply(bridge.providerModels(provider)),
      onUpdateModelVisibility: (request) => apply(bridge.modelVisibilityUpdate(request)),
      onDisconnect: (provider) => apply(bridge.providerDisconnect(provider)),
      onRemoveCustomProvider: (provider) => apply(bridge.providerCustomRemove(provider)),
      onCatalogChange: (next) => { setCatalog(next); setSettingsError(null); notifyConfigurationChanged(); },
    };
  }, [bridge]);

  const needsProviderSetup =
    resolved &&
    catalog !== null &&
    catalog.providers.every(
      (provider) => provider.credential_source === "missing",
    ) &&
    catalog.models.every((model) => model.credential_source === "missing");
  const needsVisibleModels =
    resolved &&
    catalog !== null &&
    !needsProviderSetup &&
    visibleCatalogModels(catalog).length === 0;

  return {
    catalog,
    setCatalog,
    resolved,
    settingsError,
    selections,
    changeSelection,
    resetSelection,
    providerActions,
    acknowledgeModelNotice,
    needsProviderSetup,
    needsVisibleModels,
  };
}
