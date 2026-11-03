import i18n, { useTranslation } from "@/i18n";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ConfigDocument, ConfigScope, ConfigSnapshot, LoupistBridge } from "../types";
import { errorMessage } from "../lib/errors";

export const CONFIG_CHANGED_EVENT = "loupist-configuration-changed";
export const notifyConfigurationChanged = (origin?: object) =>
  window.dispatchEvent(new CustomEvent(CONFIG_CHANGED_EVENT, { detail: { origin } }));

/** File changes are observed at explicit reload, app activation and scope changes. */
export function useConfiguration(bridge: LoupistBridge, scope: ConfigScope) {
  useTranslation();
  const key = scope.kind === "global" ? "global" : scope.loupistId;
  const stableScope = useRef(scope);
  stableScope.current = scope;
  const [state, setState] = useState<{ key: string; snapshot: ConfigSnapshot | null }>({ key, snapshot: null });
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const epoch = useRef(0);
  const snapshot = state.key === key ? state.snapshot : null;
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;

  const reload = useCallback(async () => {
    const requestEpoch = ++epoch.current;
    try {
      const next = await bridge.configRead({ scope: stableScope.current });
      if (requestEpoch !== epoch.current) return;
      setState({ key, snapshot: next });
      setError(null);
      return next;
    } catch (cause) {
      if (requestEpoch === epoch.current) setError(errorMessage(cause, i18n.t("app:useConfiguration.couldNotReadConfiguration")));
    }
  }, [bridge, key]);

  useEffect(() => {
    void reload();
    const refresh = (event: Event) => {
      if (event instanceof CustomEvent && event.detail?.origin === reload) return;
      void reload();
    };
    window.addEventListener("focus", refresh);
    window.addEventListener(CONFIG_CHANGED_EVENT, refresh);
    return () => {
      epoch.current += 1;
      window.removeEventListener("focus", refresh);
      window.removeEventListener(CONFIG_CHANGED_EVENT, refresh);
    };
  }, [reload]);

  const update = useCallback(async (set: ConfigDocument, unset: string[][] = []) => {
    const current = snapshotRef.current;
    if (!current || current.diagnostics.length) return false;
    const requestEpoch = ++epoch.current;
    setBusy(true);
    setError(null);
    try {
      const next = await bridge.configUpdate({ scope: current.scope, expectedRevision: current.revision, set, unset });
      if (requestEpoch === epoch.current) setState({ key, snapshot: next });
      notifyConfigurationChanged();
      return true;
    } catch (cause) {
      const saved = cause && typeof cause === "object" && "code" in cause && cause.code === "config_apply_pending";
      if (requestEpoch === epoch.current) {
        if (saved) {
          try {
            const next = await bridge.configRead({ scope: current.scope });
            if (requestEpoch === epoch.current) setState({ key, snapshot: next });
          } catch { /* Keep the saved-file error visible until a successful reload. */ }
        }
        if (requestEpoch === epoch.current) setError(errorMessage(cause, i18n.t("app:useConfiguration.couldNotSaveConfigurationReloadAndTryAgain")));
      }
      // The file changed even if this scope closed. Other consumers need its new
      // revision; this reader already refreshed and must retain the save diagnostic.
      if (saved) notifyConfigurationChanged(reload);
      return false;
    } finally {
      setBusy(false);
    }
  }, [bridge, key, reload]);

  const reveal = useCallback(async () => {
    try { await bridge.configReveal({ scope: stableScope.current }); }
    catch (cause) { setError(errorMessage(cause, i18n.t("app:useConfiguration.couldNotOpenTheConfigurationFolder"))); }
  }, [bridge]);

  return { snapshot, error, busy, reload, update, reveal };
}
