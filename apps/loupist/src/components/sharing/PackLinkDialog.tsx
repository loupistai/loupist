import { useEffect, useId, useRef, useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { PackOpenResult, PackSource } from "../../contracts/sharing";
import { commandErrorCode, errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Input } from "../ui/input";
import { Spinner } from "../ui/spinner";
import "@/styles/sharing.css";

/**
 * "Open a shared Loupist" (W19.4): a GitHub link, a ZIP or a folder. Rust
 * downloads, validates and stages the pack; nothing is added here.
 */
export function PackLinkDialog({ bridge, open, onOpenChange, onOpened }: {
  bridge: LoupistBridge; open: boolean; onOpenChange: (open: boolean) => void; onOpened: (result: PackOpenResult) => void;
}) {
  const { t } = useTranslation("sharing");
  const fieldId = useId();
  const [link, setLink] = useState("");
  const [busy, setBusy] = useState<"url" | "zip" | "folder" | null>(null);
  const [fieldError, setFieldError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const request = useRef(0);
  useEffect(() => { if (open) { request.current += 1; setLink(""); setBusy(null); setFieldError(null); setError(null); } }, [open]);

  const openSource = async (source: PackSource, kind: "url" | "zip" | "folder") => {
    const current = ++request.current;
    setBusy(kind); setFieldError(null); setError(null);
    try {
      const result = await bridge.packOpen({ source });
      if (current !== request.current) return;
      if (result) onOpened(result);
    } catch (cause) {
      if (current !== request.current) return;
      const code = commandErrorCode(cause);
      if (code === "link_invalid") setFieldError(t("linkInvalid"));
      else if (code === "link_not_found") setFieldError(t("linkNotFound"));
      else if (code === "offline") setError(t("publishOffline"));
      else setError(errorMessage(cause, t("linkNotFound")));
    } finally {
      if (current === request.current) setBusy(null);
    }
  };

  return <Dialog open={open} onOpenChange={onOpenChange}>
    <DialogContent size="sm" className="pack-link-dialog" data-pack-link-dialog>
      <DialogHeader><DialogTitle>{t("linkTitle")}</DialogTitle></DialogHeader>
      <form onSubmit={event => { event.preventDefault(); if (link.trim() && !busy) void openSource({ url: link.trim() }, "url"); }}>
        <DialogBody className="pack-link-body">
          <label className="share-field" htmlFor={fieldId}><span>{t("linkLabel")}</span>
            <Input id={fieldId} value={link} inputMode="url" autoComplete="off" spellCheck={false} aria-invalid={Boolean(fieldError) || undefined}
              aria-describedby={`${fieldId}-hint${fieldError ? ` ${fieldId}-error` : ""}`} onChange={event => { setLink(event.target.value); setFieldError(null); }} autoFocus />
            <span id={`${fieldId}-hint`} className="share-section-note">{t("linkHint")}</span>
            {fieldError ? <span id={`${fieldId}-error`} className="share-field-error" role="alert">{fieldError}</span> : null}
          </label>
          {busy === "url" ? <p className="share-status" role="status"><Spinner className="share-spinner" />{t("fetching")}</p> : null}
          <Button type="submit" className="share-primary" disabled={!link.trim() || busy !== null}>{t("preview")}</Button>
          <p className="pack-link-or" role="separator" aria-label={t("or")}><span aria-hidden="true">{t("or")}</span></p>
          <div className="share-outputs">
            <Button type="button" variant="outline" disabled={busy !== null} onClick={() => void openSource({ zip: true }, "zip")}>{busy === "zip" ? <Spinner /> : null}{t("chooseZip")}</Button>
            <Button type="button" variant="outline" disabled={busy !== null} onClick={() => void openSource({ folder: true }, "folder")}>{busy === "folder" ? <Spinner /> : null}{t("chooseFolder")}</Button>
          </div>
          {busy === "zip" || busy === "folder" ? <p className="share-status" role="status"><Spinner className="share-spinner" />{t("fetching")}</p> : null}
          {error ? <p className="share-error" role="alert">{error}</p> : null}
        </DialogBody>
      </form>
      <DialogFooter><Button type="button" variant="ghost" onClick={() => onOpenChange(false)}>{i18n.t("onboarding:appDialogs.cancel")}</Button></DialogFooter>
    </DialogContent>
  </Dialog>;
}
