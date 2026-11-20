import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import type { FxDisplay, LoupistBridge } from "../types";
import type { useConfiguration } from "./useConfiguration";

/** A host reply that names a currency: everything but `unset`. */
export type FxDisplayReply = Exclude<FxDisplay, { state: "unset" }>;

/**
 * What the shell knows about the display currency for the selected Loupist.
 * `off`: no display currency. `no_loupist`: one is set, but no Loupist is open to
 * read rates for. `reading`: the first read for this Loupist and currency is in
 * flight. `failed`: the read itself failed, so nothing is known about the rates.
 * `read`: the host's reply, as Rust worded it.
 */
export type DisplayFx =
  | { status: "off" }
  | { status: "no_loupist"; currency: string }
  | { status: "reading"; currency: string }
  | { status: "failed"; currency: string }
  | { status: "read"; currency: string; display: FxDisplayReply };

export type DisplayFxValue = DisplayFx & { retry: () => void };

type Session = { generation: number; loupistId: string | null; currency?: string; busy: boolean; again: boolean; reconciles: number };
type Result = { key: string; display?: FxDisplayReply; failed?: true; expired?: true };

/** Reads repeated after a reload confirmed this window's currency, before the query reports a failure. */
const MAX_RECONCILES = 2;

/** setTimeout's longest delay; a later deadline is reached in more than one step. */
const MAX_TIMER_MS = 2_147_483_647;

/**
 * The instant the host says a ready table stops being usable (`usable_until`, RFC 3339),
 * in epoch milliseconds; undefined when the host sends none or it does not parse.
 */
function usableUntil(display: FxDisplayReply | undefined) {
  if (display?.state !== "ready" || !display.usable_until) return undefined;
  const at = Date.parse(display.usable_until);
  return Number.isFinite(at) ? at : undefined;
}

const sessionKey = (loupistId: string | null, currency?: string) => loupistId && currency ? `${loupistId}\n${currency}` : null;

/**
 * The one display-currency query of a shell window. It reads `fx_display` for the
 * selected Loupist and the global preference on mount, when either changes, and
 * on every app-wide `fx-changed` hint. Hints that arrive during a read are
 * joined into one more read after it, so none is lost. A reply for an earlier
 * Loupist, preference or mount is dropped; a reply whose currency differs from
 * this window's configuration reloads the configuration instead of painting, then
 * reads again (a bounded number of times) or reports a retryable failure.
 */
export function useDisplayFx(bridge: LoupistBridge, loupistId: string | null, configuration: ReturnType<typeof useConfiguration>): DisplayFxValue {
  const currency = configuration.snapshot?.effective.appearance.display_currency || undefined;
  const key = sessionKey(loupistId, currency);
  const [result, setResult] = useState<Result | null>(null);
  const session = useRef<Session>({ generation: 0, loupistId: null, busy: false, again: false, reconciles: 0 });
  const listening = useRef(false);
  const reloadConfiguration = useRef(configuration.reload);
  reloadConfiguration.current = configuration.reload;

  const read = useCallback(function read() {
    const current = session.current;
    const requestKey = sessionKey(current.loupistId, current.currency);
    if (!requestKey || !current.loupistId) return;
    // Until the hint listener is attached a read could miss the completion it starts.
    if (!listening.current) return;
    if (current.busy) { current.again = true; return; }
    current.busy = true;
    current.again = false;
    const { generation, currency: expected } = current;
    const settle = (next: Result | null) => {
      if (session.current.generation !== generation) return;
      if (next) setResult(next);
      session.current.busy = false;
      if (session.current.again) read();
    };
    bridge.fxDisplay({ loupistId: current.loupistId }).then(async (display) => {
      if (session.current.generation !== generation) return;
      if (display.state !== "unset" && display.currency === expected) {
        session.current.reconciles = 0;
        settle({ key: requestKey, display });
        return;
      }
      // Rust read another global preference than this window holds. What this window
      // showed for its own preference is no longer the host's: take the committed
      // configuration before painting anything.
      setResult(null);
      const committed = await reloadConfiguration.current();
      if (session.current.generation !== generation) return;
      const preference = committed?.effective.appearance.display_currency || undefined;
      // A different committed preference starts its own session and read.
      if (committed && preference !== expected) { settle(null); return; }
      // The same preference: the reply was from a moment between two writes; read once more.
      if (committed && session.current.reconciles < MAX_RECONCILES) {
        session.current.reconciles += 1;
        session.current.again = true;
        settle(null);
        return;
      }
      // The reload failed, or the host keeps answering for another currency: a retryable failure.
      settle({ key: requestKey, failed: true });
    }, () => settle({ key: requestKey, failed: true }));
  }, [bridge]);

  // The first read waits for the listener to attach, so a cold read's completion is not
  // missed; a listener that attaches after cleanup does nothing.
  useEffect(() => {
    let disposed = false;
    listening.current = false;
    const hint = () => {
      // A window left on As listed learns of a currency chosen in another window only from the
      // committed configuration: reload it, and the new preference starts the query.
      if (session.current.loupistId && !session.current.currency) { void reloadConfiguration.current(); return; }
      read();
    };
    const stop = bridge.onFxChanged(hint, () => {
      if (disposed) return;
      listening.current = true;
      read();
    });
    return () => { disposed = true; listening.current = false; stop(); };
  }, [bridge, read]);

  // A new Loupist, preference or mount is a new session: replies to an earlier one,
  // including the same pair chosen again (A, B, then A), are dropped by generation.
  useEffect(() => {
    session.current = { generation: session.current.generation + 1, loupistId, currency, busy: false, again: false, reconciles: 0 };
    setResult(null);
    read();
    return () => { session.current = { generation: session.current.generation + 1, loupistId: null, busy: false, again: false, reconciles: 0 }; };
  }, [loupistId, currency, read]);

  // The host's freshness deadline for a usable table: at that instant the figure is hidden
  // and the table reread once; a new reply or unmount clears the timer. No polling.
  const rereadFor = useRef<string | null>(null);
  // Keyed by the reply (each reply is a new Result), so a reread that returns the same table
  // is checked against its deadline again.
  const expire = useCallback((reply: Result) => {
    setResult(previous => previous === reply ? { ...reply, expired: true } : previous);
    // Once per table and deadline: a reread that returns the same expired table stays hidden.
    const display = reply.display!;
    const table = `${display.currency}|${display.state === "ready" ? display.revision : ""}|${usableUntil(display)}`;
    if (rereadFor.current === table) return;
    rereadFor.current = table;
    read();
  }, [read]);
  const current = result && result.key === key && !result.expired ? result : null;
  useEffect(() => {
    const until = usableUntil(current?.display);
    if (!current || until === undefined) return;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const check = () => {
      const left = until - Date.now();
      if (left <= 0) expire(current);
      else timer = setTimeout(check, Math.min(left, MAX_TIMER_MS));
    };
    check();
    return () => clearTimeout(timer);
  }, [current, expire]);

  // A window that becomes visible again (after sleep, say) rereads, and hides at once a
  // table whose deadline passed while its timer could not run.
  useEffect(() => {
    const visible = () => {
      if (document.visibilityState !== "visible") return;
      const until = usableUntil(current?.display);
      if (current && until !== undefined && until <= Date.now()) expire(current);
      else read();
    };
    document.addEventListener("visibilitychange", visible);
    return () => document.removeEventListener("visibilitychange", visible);
  }, [current, expire, read]);

  const retry = useCallback(() => { session.current.reconciles = 0; read(); }, [read]);
  return useMemo((): DisplayFxValue => {
    if (!currency) return { status: "off", retry };
    if (!loupistId) return { status: "no_loupist", currency, retry };
    if (current?.display) return { status: "read", currency, display: current.display, retry };
    return { status: current?.failed ? "failed" : "reading", currency, retry };
  }, [currency, loupistId, current, retry]);
}

const DisplayFxContext = createContext<DisplayFxValue>({ status: "off", retry: () => {} });

/** Shares the shell's one query with Settings and every page that shows a converted amount. */
export const DisplayFxProvider = DisplayFxContext.Provider;
export const useSharedDisplayFx = () => useContext(DisplayFxContext);
