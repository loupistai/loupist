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
    void bridge
      .modelCatalogGet()
      .then(async (settings) => {
        if (!active) return;
        setCatalog(settings);
        setSettingsError(null);
        // A restored sign-in can precede the library's first catalog. Discover
        // one connected provider so Chat is usable without a Settings detour.
        // A populated catalog (including deliberately hidden models) is kept.
        if (settings.models.length === 0) {
          for (const provider of settings.providers) {
            if (!active) return;
            if (provider.credential_source === "missing") continue;
            try {
              const next = await bridge.providerModels(provider.id);
              if (!active) return;
              setCatalog(next);
              setSettingsError(null);
              if (visibleCatalogModels(next).length > 0) break;
            } catch (cause: unknown) {
              if (!active) return;
              setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotLoadAvailableModels")));
            }
          }
        }
        if (!active) return;
        setResolved(true);
      })
      .catch((cause: unknown) => {
        if (!active) return;
        setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotLoadModelSettings")));
        setResolved(true);
      });
    return () => {
      active = false;
    };
  }, [bridge]);

  useEffect(() => {
    let active = true;
    const refresh = () => { void bridge.modelCatalogGet().then((next) => { if (active) { setCatalog(next); setSettingsError(null); } }).catch((cause) => { if (active) setSettingsError(errorMessage(cause, i18n.t("app:useModelCatalog.couldNotReloadModelSettings"))); }); };
    window.addEventListener("focus", refresh);
    window.addEventListener(CONFIG_CHANGED_EVENT, refresh);
    return () => { active = false; window.removeEventListener("focus", refresh); window.removeEventListener(CONFIG_CHANGED_EVENT, refresh); };
  }, [bridge]);

  // Rust emits provider-status on every state or notice change; patch it in place so cards, the composer and onboarding update live.
  useEffect(() => bridge.onProviderStatus?.(({ provider, status }) => {
    setCatalog((current) => {
      if (!current) return current;
      const providers = current.providers.some((item) => item.id === provider)
        ? current.providers.map((item) => (item.id === provider ? { ...item, status } : item)) : current.providers;
      const custom_providers = current.custom_providers?.some((item) => item.id === provider)
        ? current.custom_providers.map((item) => (item.id === provider ? { ...item, status } : item)) : current.custom_providers;
      return { ...current, providers, custom_providers };
    });
  }), [bridge]);

  /** The "Now using …" line was shown: on send or ×, Rust forgets it. */
  const acknowledgeModelNotice = useCallback((id: string) => {
    setCatalog((current) => (current?.model_notice?.id === id ? { ...current, model_notice: null } : current));
    void bridge.modelNoticeAck({ id }).then(setCatalog).catch(() => {});
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

  /** Back to "Auto": the Loupist follows the app's default run profile again. */
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
      return next;
    };
    return {
      onLoadModels: (provider) => apply(bridge.providerModels(provider)),
      onUpdateModelVisibility: async (request) => { const next = await apply(bridge.modelVisibilityUpdate(request)); notifyConfigurationChanged(); return next; },
      onDisconnect: (provider) => apply(bridge.providerDisconnect(provider)),
      onRemoveCustomProvider: (provider) => apply(bridge.providerCustomRemove(provider)),
      onCatalogChange: (next) => { setCatalog(next); setSettingsError(null); },
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
