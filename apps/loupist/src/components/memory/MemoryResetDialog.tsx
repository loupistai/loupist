import { useEffect, useId, useState } from "react";
import { useTranslation } from "../../i18n";
import { formatNumber, subjectName } from "../../i18n/format";
import type { LoupistBridge } from "../../types";
import type { MemoryResetCounts } from "../../contracts/knowledge";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
// The counts table is styled here too: Loupist settings › Learning opens this dialog without the Lens page.
import "@/styles/memory-page.css";

export function MemoryResetDialog({ bridge, loupistId, name, open, onOpenChange, onChanged }: {
  bridge: LoupistBridge; loupistId: string; name: string; open: boolean; onOpenChange: (open: boolean) => void; onChanged: () => void;
}) {
  const { t, i18n } = useTranslation("knowledge");
  const [counts, setCounts] = useState<MemoryResetCounts | null>(null);
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retry, setRetry] = useState(0);
  const inputId = useId();
  const bodyId = useId();
  useEffect(() => {
    if (!open) return;
    let live = true;
    setCounts(null); setConfirmation(""); setError(null);
    void bridge.knowledgeMemoryResetPreview({ loupistId }).then(value => { if (live) setCounts(value); })
      .catch(cause => { if (live) setError(errorMessage(cause, t("memoryPage.actionError"))); });
    return () => { live = false; };
  }, [bridge, loupistId, open, retry, t]);
  const reset = async () => {
    if (!counts || busy || confirmation !== t("memoryPage.reset.confirmWord")) return;
    setBusy(true); setError(null);
    try { await bridge.knowledgeMemoryReset({ loupistId, confirm: true, expected: counts }); onChanged(); onOpenChange(false); }
    catch (cause) { setError(errorMessage(cause, t("memoryPage.actionError"))); }
    finally { setBusy(false); }
  };
  return <Dialog open={open} onOpenChange={value => { if (!busy) onOpenChange(value); }}>
    <DialogContent size="sm" className="memory-reset-dialog" showCloseButton={!busy}>
      <DialogHeader><DialogTitle>{t("memoryPage.reset.title", { loupistSubject: subjectName(name, i18n.language) })}</DialogTitle>
        <DialogDescription id={bodyId}>{t("memoryPage.reset.body")}</DialogDescription></DialogHeader>
      <DialogBody>
        {counts ? <dl className="memory-reset-counts">{(["principles", "pages", "sections", "names", "relationships", "sources"] as const).map(kind => <div key={kind}><dt>{t(`memoryPage.reset.counts.${kind}`)}</dt><dd>{formatNumber(counts[kind])}</dd></div>)}</dl> : !error ? <p role="status">{t("memoryPage.loading")}</p> : null}
        <p className="memory-meta">{t("memoryPage.reset.keeps")}</p>
        <label className="memory-reset-confirm" htmlFor={inputId}>{t("memoryPage.reset.confirmLabel")}
          <Input id={inputId} value={confirmation} disabled={busy || !counts} onChange={event => setConfirmation(event.target.value)} aria-describedby={bodyId} autoComplete="off" />
        </label>
        {error ? <p role="alert">{error}</p> : null}
        {error ? <Button size="sm" variant="ghost" disabled={busy} onClick={() => setRetry(value => value + 1)}>{t("memoryPage.retry")}</Button> : null}
      </DialogBody>
      <DialogFooter><Button variant="ghost" disabled={busy} onClick={() => onOpenChange(false)}>{t("memoryPage.cancel")}</Button>
        <Button variant="destructive" disabled={!counts || busy || confirmation !== t("memoryPage.reset.confirmWord")} aria-describedby={bodyId} onClick={() => void reset()}>{t("memoryPage.reset.action")}</Button></DialogFooter>
    </DialogContent>
  </Dialog>;
}
