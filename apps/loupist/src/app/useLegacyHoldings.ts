import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import type { LegacyHoldingsStatus } from "../contracts/legacyHoldings";
import type { LoupistBridge } from "../types";

/**
 * The host's holdings export status, for the shell notice and the Settings row.
 * `null` until the first read returns; a read that throws leaves it `null` until
 * the next change event. The status is read only once the change listener is
 * attached (the native one attaches asynchronously and does not replay), and an
 * event wins over a read that started before it, so a run that ends during
 * startup is not missed.
 */
export function useLegacyHoldingsStatus(bridge: LoupistBridge) {
  const [status, setStatus] = useState<LegacyHoldingsStatus | null>(null);
  useEffect(() => {
    let live = true, heard = false;
    const read = () => {
      if (live) bridge.legacyHoldingsStatus().then(next => { if (live && !heard) setStatus(next); }, () => undefined);
    };
    const stop = bridge.onLegacyHoldingsChanged(next => { heard = true; if (live) setStatus(next); }, read);
    return () => { live = false; stop(); };
  }, [bridge]);
  return [status, setStatus] as const;
}

export type LegacyHoldingsFileError = "show" | "save";

/**
 * One file action at a time across every surface: the shell line and the
 * Settings row can be on screen together, and the host reads the file before
 * its save panel opens, so a second click elsewhere could open a second panel.
 */
let fileBusy = false;
const fileBusyListeners = new Set<() => void>();
const setFileBusy = (value: boolean) => { fileBusy = value; for (const listener of fileBusyListeners) listener(); };
const subscribeFileBusy = (listener: () => void) => { fileBusyListeners.add(listener); return () => { fileBusyListeners.delete(listener); }; };
const readFileBusy = () => fileBusy;

/**
 * Show file and Save a copy, shared by the notice and the Settings row. A
 * cancelled save panel shows nothing; a failure names what could not be done
 * on the surface that asked and leaves the button usable. While one call runs,
 * `busy` is true on every surface and further calls are dropped.
 */
export function useLegacyHoldingsFile(bridge: LoupistBridge) {
  const [error, setError] = useState<LegacyHoldingsFileError | null>(null);
  const busy = useSyncExternalStore(subscribeFileBusy, readFileBusy);
  const run = useCallback(async (kind: LegacyHoldingsFileError) => {
    if (fileBusy) return;
    setFileBusy(true);
    setError(null);
    try {
      if (kind === "show") await bridge.legacyHoldingsReveal();
      else await bridge.legacyHoldingsSaveCopy();
    } catch {
      setError(kind);
    } finally {
      setFileBusy(false);
    }
  }, [bridge]);
  return { error, busy, show: () => void run("show"), saveCopy: () => void run("save") };
}
