import i18n, { useTranslation } from "../../i18n";
import { useCallback, useEffect, useRef, useState } from "react";
import { commandErrorCode } from "../../lib/errors";
import type {
  LoupistBridge,
  UpdateInstallRequest,
  UpdateState,
} from "../../types";

/** The native install confirmation's words in the interface language; the host adds the versions line itself. */
export function updateConfirmation(currentVersion: string, version: string): UpdateInstallRequest["confirmation"] {
  return {
    title: i18n.t("settings:useAppUpdate.confirmTitle"),
    message: i18n.t("settings:useAppUpdate.confirmMessage", { current: currentVersion, version }),
    install: i18n.t("settings:useAppUpdate.confirmInstall"),
    cancel: i18n.t("settings:useAppUpdate.confirmCancel"),
  };
}

const statusWithPhase = (
  status: UpdateState | null,
  phase: UpdateState["phase"],
): UpdateState | null => (status ? { ...status, phase, error: null } : status);

type UpdateErrorKey = "couldNotReadUpdateStatus" | "couldNotCheckForUpdates" |
  "couldNotInstallTheUpdate" | "backgroundCheckFailed" | "couldNotSaveUpdateSchedule" |
  "couldNotVerifyTheUpdate" | "couldNotDownloadTheUpdate" | "updateChanged" |
  "updatesUnavailable" | "updateBusy";

// UpdateState predates coded errors. Keep its host diagnostics at the bridge
// boundary; only translated keys reach the settings card, including unknown errors.
function statusErrorKey(message: string): UpdateErrorKey {
  switch (message) {
    case "Could not check for updates. Loupist will retry.": return "backgroundCheckFailed";
    case "Could not save update scheduling state.": return "couldNotSaveUpdateSchedule";
    case "Could not verify the update. Try again.": return "couldNotVerifyTheUpdate";
    case "Could not download the update. Try again.":
    case "The update download timed out. Try again.": return "couldNotDownloadTheUpdate";
    case "Could not install the update. Try again.": return "couldNotInstallTheUpdate";
    case "The available update changed. Check again before installing.": return "updateChanged";
    case "Automatic updates are unavailable in this build.": return "updatesUnavailable";
    default: return "couldNotReadUpdateStatus";
  }
}

// A localization key, not text: the card translates it when it renders.
const STATUS_READ_FAILED: UpdateErrorKey = "couldNotReadUpdateStatus";

function actionErrorKey(cause: unknown, fallback: UpdateErrorKey): UpdateErrorKey {
  switch (commandErrorCode(cause)) {
    case "update_busy": return "updateBusy";
    case "conflict":
    case "not_found":
    case "update_changed": return "updateChanged";
    case "updater_unavailable": return "updatesUnavailable";
    case "update_check_failed": return fallback === "couldNotInstallTheUpdate" ? "couldNotVerifyTheUpdate" : "couldNotCheckForUpdates";
    // The updater verifies the package signature during download. A refused
    // signature is the same host error as a failed download, never install success.
    case "update_download_failed": return "couldNotDownloadTheUpdate";
    case "update_install_failed": return "couldNotInstallTheUpdate";
    default: return fallback;
  }
}

export function useAppUpdate(bridge: LoupistBridge) {
  useTranslation("settings");
  const [status, setStatus] = useState<UpdateState | null>(null);
  const [statusError, setStatusError] = useState<UpdateErrorKey | null>(null);
  const [actionError, setActionError] = useState<UpdateErrorKey | null>(null);
  const [actionPhase, setActionPhase] = useState<UpdateState["phase"] | null>(null);
  const requestEpoch = useRef(0);
  const polling = useRef(false);
  const acting = useRef(false);
  const mounted = useRef(true);
  const latestStatus = useRef<UpdateState | null>(null);

  const refreshStatus = useCallback(async (preserveActionError = false) => {
    if (polling.current) return;
    polling.current = true;
    const epoch = requestEpoch.current;
    try {
      const next = await bridge.updateStatus();
      if (!mounted.current || epoch !== requestEpoch.current) return;
      setStatus(current => JSON.stringify(current) === JSON.stringify(next) ? current : next);
      setStatusError(null);
      if (!next.error && !preserveActionError) setActionError(null);
    } catch {
      if (!mounted.current || epoch !== requestEpoch.current) return;
      setStatusError(STATUS_READ_FAILED);
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
      setActionError(actionErrorKey(cause, "couldNotCheckForUpdates"));
      await refreshStatus(true);
    } finally {
      acting.current = false;
      if (mounted.current) setActionPhase(null);
    }
  }, [bridge, refreshStatus]);

  const installUpdate = useCallback(
    async (offerId: string) => {
      if (acting.current) return;
      const offered = latestStatus.current?.offer;
      const request: UpdateInstallRequest = {
        offer_id: offerId,
        confirmation: updateConfirmation(latestStatus.current?.current_version ?? "", offered?.offer_id === offerId ? offered.version : ""),
      };
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
        setActionError(actionErrorKey(cause, "couldNotInstallTheUpdate"));
        await refreshStatus(true);
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
    // eslint-disable-next-line react-hooks/exhaustive-deps -- a request counter, not a DOM node: cleanup invalidates the request that is in flight.
    return () => { mounted.current = false; requestEpoch.current++; };
  }, [refreshStatus]);

  useEffect(() => { latestStatus.current = status; }, [status]);

  const phase = status?.phase && status.phase !== "idle" ? status.phase : actionPhase ?? "idle";
  useEffect(() => {
    const busy = phase !== "idle";
    const interval = window.setInterval(
      () => void refreshStatus(),
      busy ? 250 : 15_000,
    );
    return () => window.clearInterval(interval);
  }, [refreshStatus, phase]);

  const errorKey = actionError ?? statusError ?? (status?.error ? statusErrorKey(status.error) : null);
  return {
    status,
    phase,
    error: errorKey ? i18n.t(`settings:useAppUpdate.${errorKey}`) : null,
    checkForUpdates,
    installUpdate,
  };
}
