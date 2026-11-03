import { useCallback, useEffect, useRef, useState } from "react";
import type { LoupistBridge, ModelCatalog, ProviderConnectFailure } from "@/types";

/** "Browser didn't come back?" appears this long after the browser opened (spec providers §3.2). */
export const signInTiming = { pasteRevealMs: 20_000 };

export type SignInDevice = { userCode: string; verificationUri: string };

export type SignInState =
  | { phase: "idle" }
  | {
      phase: "waiting";
      provider: string;
      /** False when Rust could not open the browser; `url` is then the link to copy. */
      opened: boolean;
      url: string | null;
      /** Pi is ready for the address the browser shows after signing in. */
      manual: boolean;
      device: SignInDevice | null;
      pasteAvailable: boolean;
    }
  /** `reason` is Rust's typed failure; `message` is a host-composed fallback for untyped errors. */
  | { phase: "error"; provider: string; reason: ProviderConnectFailure | null; message: string | null };

export type SignInBridge = Pick<LoupistBridge, "providerConnect" | "providerConnectCancel" | "providerConnectOpenBrowser" | "providerConnectSubmitCode">;

const FAILURES: ReadonlySet<string> = new Set<ProviderConnectFailure>(["plan_permission_declined", "plan_not_eligible", "port_in_use", "timed_out", "browser_failed", "offline", "denied", "other"]);

const errorCode = (cause: unknown) => cause && typeof cause === "object" && "code" in cause && typeof cause.code === "string" ? cause.code : null;
const errorText = (cause: unknown) => cause instanceof Error ? cause.message
  : cause && typeof cause === "object" && "message" in cause && typeof cause.message === "string" ? cause.message
    : typeof cause === "string" ? cause : null;

/**
 * One browser or device sign-in at a time (Rust admits one OAuth lease). The
 * flow ignores results that arrive after Cancel or after a newer attempt, and
 * cancels a still-running sign-in when the owner unmounts.
 */
export function useSignIn(bridge: SignInBridge, onConnected: (provider: string, catalog: ModelCatalog) => void) {
  const [state, setState] = useState<SignInState>({ phase: "idle" });
  const operation = useRef(0);
  const active = useRef(false);
  const cancelled = useRef(false);
  const revealTimer = useRef<number | undefined>(undefined);
  const bridgeRef = useRef(bridge);
  bridgeRef.current = bridge;
  const connectedRef = useRef(onConnected);
  connectedRef.current = onConnected;

  const clearReveal = () => { if (revealTimer.current !== undefined) window.clearTimeout(revealTimer.current); revealTimer.current = undefined; };
  useEffect(() => () => {
    operation.current++;
    clearReveal();
    if (active.current) void bridgeRef.current.providerConnectCancel().catch(() => {});
  }, []);

  const update = (id: number, change: (current: SignInState) => SignInState) => {
    if (id === operation.current) setState(current => change(current));
  };

  const start = useCallback(async (provider: string) => {
    if (active.current) return;
    const id = ++operation.current;
    active.current = true;
    cancelled.current = false;
    clearReveal();
    let failure: ProviderConnectFailure | null = null;
    setState({ phase: "waiting", provider, opened: true, url: null, manual: false, device: null, pasteAvailable: false });
    try {
      const catalog = await bridgeRef.current.providerConnect(provider, event => {
        if (id !== operation.current) return;
        if (event.type === "opening_browser") {
          update(id, current => current.phase === "waiting" ? { ...current, opened: event.opened, url: event.url, pasteAvailable: current.pasteAvailable || !event.opened } : current);
          clearReveal();
          if (event.opened) {
            revealTimer.current = window.setTimeout(() => update(id, current => current.phase === "waiting" ? { ...current, pasteAvailable: true } : current), signInTiming.pasteRevealMs);
          }
        } else if (event.type === "device_code") {
          update(id, current => current.phase === "waiting" ? { ...current, device: { userCode: event.user_code, verificationUri: event.verification_uri } } : current);
        } else if (event.type === "manual_entry") {
          update(id, current => current.phase === "waiting" ? { ...current, manual: true } : current);
        } else if (event.type === "failed") {
          failure = event.reason;
        }
      });
      if (id !== operation.current) return;
      setState({ phase: "idle" });
      connectedRef.current(provider, catalog);
    } catch (cause) {
      if (id !== operation.current) return;
      if (cancelled.current || errorCode(cause) === "cancelled") { setState({ phase: "idle" }); return; }
      const coded = errorCode(cause) === "provider_connect_failed" ? errorText(cause) : null;
      const reason = failure ?? (coded && FAILURES.has(coded) ? coded as ProviderConnectFailure : null);
      // Opening the browser is never an error of its own: the card offers the link instead.
      setState({ phase: "error", provider, reason: reason === "browser_failed" ? "other" : reason, message: reason ? null : errorText(cause) });
    } finally {
      if (id === operation.current) { active.current = false; clearReveal(); }
    }
  }, []);

  /** Cancel or Escape: the card returns to the button with no error row. */
  const cancel = useCallback(async () => {
    if (!active.current) { setState({ phase: "idle" }); return; }
    cancelled.current = true;
    operation.current++;
    active.current = false;
    clearReveal();
    setState({ phase: "idle" });
    await bridgeRef.current.providerConnectCancel().catch(() => {});
  }, []);

  const dismiss = useCallback(() => { if (!active.current) setState({ phase: "idle" }); }, []);
  const openAgain = useCallback(() => bridgeRef.current.providerConnectOpenBrowser(), []);
  /** Rust validates the pasted value; it rejects with code `paste_invalid` and forwards nothing. */
  const submitCode = useCallback(async (value: string): Promise<"ok" | "invalid" | "failed"> => {
    try { await bridgeRef.current.providerConnectSubmitCode({ value: value.trim() }); return "ok"; }
    catch (cause) { return errorCode(cause) === "paste_invalid" ? "invalid" : "failed"; }
  }, []);

  return { state, start, cancel, dismiss, openAgain, submitCode, busy: state.phase === "waiting" };
}

export type SignInController = ReturnType<typeof useSignIn>;
