import { useEffect, useRef, useState } from "react";
import type { LoupistBridge } from "../types";
import type { BriefingResidentSettings } from "../contracts/scheduled";

/** The OS may change the login item while System Settings has focus. */
export function useResidentSettings(bridge: LoupistBridge) {
  const [resident, setResident] = useState<BriefingResidentSettings | null>(null);
  const [error, setFailedOperation] = useState<"load" | "save" | null>(null);
  const [busy, setBusy] = useState(false);
  const writing = useRef<Promise<BriefingResidentSettings> | null>(null);
  const generation = useRef(0);

  useEffect(() => {
    let live = true;
    const refresh = async () => {
      // A focus change during registration still gets a fresh OS read after it settles.
      if (writing.current) await writing.current.catch(() => undefined);
      if (!live) return;
      const read = ++generation.current;
      try {
        const result = await bridge.briefingResidentSettingsGet();
        if (live && read === generation.current) { setResident(result); setFailedOperation(null); }
      } catch {
        if (live && read === generation.current) { setResident(null); setFailedOperation("load"); }
      }
    };
    void refresh();
    window.addEventListener("focus", refresh);
    return () => { live = false; window.removeEventListener("focus", refresh); };
  }, [bridge]);

  const change = async (operation: () => Promise<BriefingResidentSettings>) => {
    if (writing.current) return;
    const pending = Promise.resolve().then(operation);
    writing.current = pending;
    generation.current++;
    setBusy(true);
    setFailedOperation(null);
    try {
      const result = await pending;
      setResident(result);
      return result;
    } catch { setFailedOperation("save"); }
    finally { writing.current = null; setBusy(false); }
  };
  return { resident, busy, error, change };
}
