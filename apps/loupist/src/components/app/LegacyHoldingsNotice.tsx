import { useEffect, useRef, useState } from "react";
import { useTranslation } from "@/i18n";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { useLegacyHoldingsFile, useLegacyHoldingsStatus } from "../../app/useLegacyHoldings";
import type { LoupistBridge } from "../../types";

/**
 * The one-time shell line after holdings left Loupist: the export succeeded (Show
 * file, Save a copy…, Dismiss) or has not yet (Try again, Dismiss). Nothing shows
 * while the export is pending, after a dismissal or while `hidden` (Following). It
 * never names a path, a file name or a count. `onGone` receives focus when the
 * line leaves while focus was inside it.
 */
export function LegacyHoldingsNotice({ bridge, hidden, onGone }: { bridge: LoupistBridge; hidden: boolean; onGone: () => void }) {
  const { t } = useTranslation("app");
  const [status, setStatus] = useLegacyHoldingsStatus(bridge);
  const file = useLegacyHoldingsFile(bridge);
  const [retrying, setRetrying] = useState(false);
  const section = useRef<HTMLElement>(null);
  const restoreFocus = useRef(false);
  const focusInside = () => Boolean(section.current?.contains(document.activeElement));

  // A disabled Try again drops focus; once the run ends, give it back in the page.
  useEffect(() => {
    if (retrying || !restoreFocus.current) return;
    restoreFocus.current = false;
    if (document.activeElement && document.activeElement !== document.body && !focusInside()) return;
    const target = section.current?.querySelector<HTMLButtonElement>("button:not(:disabled)");
    if (target) target.focus(); else onGone();
  });

  if (hidden || !status || status.dismissed || (status.state !== "exported" && status.state !== "failed")) return null;
  const exported = status.state === "exported";

  const dismiss = () => {
    const hadFocus = focusInside();
    setStatus({ ...status, dismissed: true });
    if (hadFocus) onGone();
    bridge.legacyHoldingsDismiss().then(setStatus, () => undefined);
  };
  const retry = async () => {
    if (retrying) return;
    restoreFocus.current = focusInside();
    setRetrying(true);
    try { setStatus(await bridge.legacyHoldingsRetry()); }
    catch { /* The rows are still there; the failed line stays. */ }
    finally { setRetrying(false); }
  };

  return <section ref={section} className="app-selection-error legacy-holdings-notice" role="status" aria-label={t("app.legacyHoldings.label")} data-legacy-holdings={status.state}>
    <p>{t(exported ? "app.legacyHoldings.exported" : "app.legacyHoldings.failed")}</p>
    {exported ? <>
      {/* aria-disabled, not disabled: focus stays on the button while the panel is open. */}
      <button type="button" className="ghost-button legacy-holdings-action" aria-disabled={file.busy || undefined} onClick={file.show}>{t("app.legacyHoldings.showFile")}</button>
      <button type="button" className="ghost-button legacy-holdings-action" aria-disabled={file.busy || undefined} onClick={file.saveCopy}>{t("app.legacyHoldings.saveCopy")}</button>
    </> : <button type="button" className="ghost-button" disabled={retrying} onClick={() => void retry()}>{retrying ? <Spinner /> : null}{t("app.tryAgain")}</button>}
    <button type="button" className="ghost-button" onClick={dismiss}>{t("app.dismiss")}</button>
    {exported && file.error ? <p className="legacy-holdings-notice-error" role="alert">
      {t(file.error === "show" ? "app.legacyHoldings.showFailed" : "app.legacyHoldings.saveFailed")}
      {file.error === "save" ? <> <Button type="button" variant="link" size="sm" className="legacy-holdings-action" aria-disabled={file.busy || undefined} onClick={file.saveCopy}>{t("app.tryAgain")}</Button></> : null}
    </p> : null}
  </section>;
}
