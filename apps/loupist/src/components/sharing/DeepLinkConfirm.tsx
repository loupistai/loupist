import { useCallback, useEffect, useRef, useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { DeepLinkEvent, PackOpenResult } from "../../contracts/sharing";
import { errorMessage } from "../../lib/errors";
import { sharingReleaseVisible } from "../../sharing/releaseVisibility";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Spinner } from "../ui/spinner";
import "@/styles/sharing.css";

/**
 * `loupist://import` links (W19.8). Rust validates and holds the link; the
 * renderer asks once onboarding is done. Nothing is fetched before Preview.
 */
export function useDeepLinks(bridge: LoupistBridge, ready: boolean, onBad: (message: string) => void) {
  const present = sharingReleaseVisible();
  const [link, setLink] = useState<string | null>(null);
  const held = useRef<DeepLinkEvent | null>(null);
  const asked = useRef(false);
  const readyRef = useRef(ready);
  readyRef.current = ready;
  const onBadRef = useRef(onBad);
  onBadRef.current = onBad;
  useEffect(() => bridge.onDeepLinkImport(event => {
    // This release dismisses the link. Parsing stays in Rust; nothing opens here.
    if (!present) { void bridge.deepLinkDismiss(); return; }
    if (!readyRef.current) { held.current = event; return; }
    if (event.status === "bad") onBadRef.current(i18n.t("sharing:linkBad"));
    else setLink(event.source);
  }), [bridge, present]);
  useEffect(() => {
    if (!ready) return;
    if (!present) {
      held.current = null;
      void bridge.deepLinkPending().then(pending => { if (pending) void bridge.deepLinkDismiss(); }).catch(() => undefined);
      return;
    }
    const event = held.current;
    held.current = null;
    if (event?.status === "bad") onBadRef.current(i18n.t("sharing:linkBad"));
    else if (event?.status === "ok") setLink(event.source);
    if (asked.current) return;
    asked.current = true;
    void bridge.deepLinkPending().then(pending => {
      if (pending?.status === "ok") setLink(pending.source);
    }).catch(() => undefined);
  }, [bridge, ready, present]);
  const clear = useCallback(() => setLink(null), []);
  return { link, clear };
}

export function DeepLinkConfirm({ bridge, source, onClose, onOpened }: { bridge: LoupistBridge; source: string | null; onClose: () => void; onOpened: (result: PackOpenResult) => void }) {
  const { t } = useTranslation("sharing");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { setBusy(false); setError(null); }, [source]);
  const cancel = () => { void bridge.deepLinkDismiss(); onClose(); };
  const preview = async () => {
    setBusy(true); setError(null);
    try {
      const result = await bridge.packOpen({ source: { pending: true } });
      if (result) onOpened(result); else onClose();
    } catch (cause) { setError(errorMessage(cause, t("linkBad"))); }
    finally { setBusy(false); }
  };
  return <Dialog open={source !== null} onOpenChange={open => { if (!open && !busy) cancel(); }}>
    <DialogContent size="sm" className="deep-link-dialog" data-deep-link-dialog>
      <DialogHeader><DialogTitle>{t("deepTitle")}</DialogTitle><DialogDescription className="deep-link-source">{source}</DialogDescription></DialogHeader>
      <DialogBody>
        <p>{t("deepBody")}</p>
        {busy ? <p className="share-status" role="status"><Spinner className="share-spinner" />{t("fetching")}</p> : null}
        {error ? <p className="share-error" role="alert">{error}</p> : null}
      </DialogBody>
      <DialogFooter>
        <Button variant="outline" disabled={busy} onClick={cancel}>{i18n.t("onboarding:appDialogs.cancel")}</Button>
        <Button disabled={busy} onClick={() => void preview()}>{t("preview")}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>;
}
