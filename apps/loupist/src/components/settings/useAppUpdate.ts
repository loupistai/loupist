import i18n, { useTranslation } from "../../i18n";
import { useCallback, useEffect, useRef, useState } from "react";
import { errorMessage } from "../../lib/errors";
import type {
  LoupistBridge,
  UpdateInstallRequest,
  UpdateState,
} from "../../types";

const statusWithPhase = (
  status: UpdateState | null,
  phase: UpdateState["phase"],
): UpdateState | null => (status ? { ...status, phase, error: null } : status);

export function useAppUpdate(bridge: LoupistBridge) {
  useTranslation("settings");
  const [status, setStatus] = useState<UpdateState | null>(null);
  const [statusError, setStatusError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [actionPhase, setActionPhase] = useState<UpdateState["phase"] | null>(null);
  const requestEpoch = useRef(0);
  const polling = useRef(false);
  const acting = useRef(false);
  const mounted = useRef(true);

  const refreshStatus = useCallback(async () => {
    if (polling.current) return;
    polling.current = true;
    const epoch = requestEpoch.current;
    try {
      const next = await bridge.updateStatus();
      if (!mounted.current || epoch !== requestEpoch.current) return;
      setStatus(next);
      setStatusError(null);
      if (!next.error) setActionError(null);
    } catch (cause) {
      if (!mounted.current || epoch !== requestEpoch.current) return;
      setStatusError(errorMessage(cause, i18n.t("settings:useAppUpdate.couldNotReadUpdateStatus")));
    } finally {
      polling.current = false;
    }
  }, [bridge]);

  const checkForUpdates = useCallback(async () => {
    if (acting.current) return;
    acting.current = true;
    requestEpoch.current++;
    setActionPhase("checking");
    setStatus((current) => statusWithPhase(current, "checking"));
    setActionError(null);
    try {
      const nextStatus = await bridge.updateCheck();
      if (!mounted.current) return;
      requestEpoch.current++;
      setStatus(nextStatus);
      setStatusError(null);
    } catch (cause) {
      if (!mounted.current) return;
      requestEpoch.current++;
      setStatus((current) => statusWithPhase(current, "idle"));
      setActionError(errorMessage(cause, i18n.t("settings:useAppUpdate.couldNotCheckForUpdates")));
      await refreshStatus();
    } finally {
      acting.current = false;
      if (mounted.current) setActionPhase(null);
    }
  }, [bridge, refreshStatus]);

  const installUpdate = useCallback(
    async (request: UpdateInstallRequest) => {
      if (acting.current) return;
      acting.current = true;
      requestEpoch.current++;
      setActionPhase("confirming");
      setStatus((current) => statusWithPhase(current, "confirming"));
      setActionError(null);
      try {
        const result = await bridge.updateInstall(request);
        if (!mounted.current) return;
        requestEpoch.current++;
        setStatus((current) =>
          current
            ? {
                ...current,
                phase: "idle",
                blockers: result.blockers,
                error: null,
              }
            : current,
        );
        setStatusError(null);
      } catch (cause) {
        if (!mounted.current) return;
        requestEpoch.current++;
        setStatus((current) => statusWithPhase(current, "idle"));
        setActionError(errorMessage(cause, i18n.t("settings:useAppUpdate.couldNotInstallTheUpdate")));
        await refreshStatus();
      } finally {
        acting.current = false;
        if (mounted.current) setActionPhase(null);
      }
    },
    [bridge, refreshStatus],
  );

  useEffect(() => {
    mounted.current = true;
    void refreshStatus();
    return () => { mounted.current = false; requestEpoch.current++; };
  }, [refreshStatus]);

  const phase = status?.phase && status.phase !== "idle" ? status.phase : actionPhase ?? "idle";
  useEffect(() => {
    const busy = phase !== "idle";
    const interval = window.setInterval(
      () => void refreshStatus(),
      busy ? 250 : 15_000,
    );
    return () => window.clearInterval(interval);
  }, [refreshStatus, phase]);

  return {
    status,
    phase,
    error: actionError ?? statusError ?? status?.error ?? null,
    checkForUpdates,
    installUpdate,
  };
}
