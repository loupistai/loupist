import { forwardRef, useEffect, useId, useRef, useState } from "react";
import { ExternalLinkIcon } from "lucide-react";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import type { LoupistBridge } from "../../types";
import type { PackImportView, PackUpdate, PackUpdateCheck } from "../../contracts/sharing";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { Spinner } from "../ui/spinner";
import { PackUpdateDiff } from "./PackUpdateDiff";
import { errorCode } from "./sharingText";
import "@/styles/sharing.css";

/** The "What {Loupist} knows" marker that opens the shared Loupists section. */
export const SHARED_SECTION_ITEM = "shared-loupists";

const shortSource = (url: string) => url.replace(/^https:\/\//, "");

type CheckResult = PackUpdateCheck | { kind: "rateLimited" } | { kind: "failed"; message: string };
type Check = { busy: true } | { busy: false; result: CheckResult };

/**
 * "Shared Loupists you added" (W19.4, W19.5, W19.7): where each came from,
 * Check for updates (or Choose a newer copy for a ZIP or folder) with the
 * update diff, Remove what it added, and Report when this build has a
 * community index. A pack the index removed shows a banner.
 */
export const SharedPacks = forwardRef<HTMLElement, { bridge: LoupistBridge; loupistId: string; imports: readonly PackImportView[]; indexConfigured: boolean; onChanged: (imports: PackImportView[]) => void }>(
  function SharedPacks({ bridge, loupistId, imports, indexConfigured, onChanged }, ref) {
    const { t } = useTranslation("sharing");
    const headingId = useId();
    const [removing, setRemoving] = useState<PackImportView | null>(null);
    const [done, setDone] = useState(false);
    const [checks, setChecks] = useState<Record<string, Check>>({});
    const automatic = useRef(new Set<string>());
    const checkText = (result: CheckResult) => {
      switch (result.kind) {
        case "upToDate": return t("upToDate");
        case "originGone": return t("originGone");
        case "differentPack": return t("differentPack");
        case "offline": return t("publishOffline");
        case "rateLimited": return t("rateLimited");
        case "rejected": return t(`invalid.${result.reason}`);
        case "failed": return result.message;
        default: return null;
      }
    };
    const run = async (entry: PackImportView, request: () => Promise<PackUpdateCheck | null>) => {
      setChecks(current => ({ ...current, [entry.importId]: { busy: true } }));
      let result: CheckResult | null;
      try { result = await request(); }
      catch (cause) { result = errorCode(cause) === "rate_limited" ? { kind: "rateLimited" } : { kind: "failed", message: errorMessage(cause, t("publishOffline")) }; }
      setChecks(current => {
        const next = { ...current };
        if (result && result.kind !== "notChecked") next[entry.importId] = { busy: false, result };
        else delete next[entry.importId];
        return next;
      });
    };
    const check = (entry: PackImportView, isAutomatic = false) => run(entry, () => bridge.packCheckUpdate({ loupistId, importId: entry.importId, automatic: isAutomatic }));
    const chooseNewer = (entry: PackImportView, source: { zip: true } | { folder: true }) => run(entry, () => bridge.packChooseNewer({ loupistId, importId: entry.importId, source }));
    // At most once a day when the section opens (Rust keeps the date); never in the background.
    useEffect(() => {
      for (const entry of imports) {
        if (!entry.sourceUrl || automatic.current.has(entry.importId)) continue;
        automatic.current.add(entry.importId);
        void check(entry, true);
      }
      // eslint-disable-next-line react-hooks/exhaustive-deps -- each pack is checked once per opening
    }, [imports]);
    if (!imports.length && !done) return null;
    return <section ref={ref} className="memory-card shared-packs" aria-labelledby={headingId} data-shared-packs tabIndex={-1}>
      <h2 id={headingId}>{t("sharedTitle")}</h2>
      {done ? <p className="share-saved" role="status">{t("removeDone")}</p> : null}
      <ul>{imports.map(entry => {
        const state = checks[entry.importId];
        const update: PackUpdate | null = state && !state.busy && state.result.kind === "available" ? state.result.update : null;
        // A pack the index already removed, or one added from a file, has nothing to report.
        const reportable = indexConfigured && Boolean(entry.sourceUrl) && !entry.blockedReason;
        return <li key={entry.importId} className="shared-pack-row" data-blocked={entry.blockedReason ?? undefined}>
          {entry.blockedReason ? <p className="shared-pack-banner" role="status" data-pack-removed-banner>
            <span>{t("removedBanner", { reason: t(`reason.${entry.blockedReason}`) })}</span>
            <Button variant="outline" size="sm" onClick={() => { setDone(false); setRemoving(entry); }}>{t("remove")}</Button>
          </p> : null}
          <div className="shared-pack-text">
            <strong>{entry.name}</strong>
            <span className="shared-pack-meta">{t("addedOn", { date: formatDate(entry.addedAtMs), version: formatNumber(entry.version) })}</span>
            {entry.sourceUrl ? <Button variant="link" size="sm" className="pack-review-link" onClick={() => void bridge.openExternalUrl(entry.sourceUrl!)}>{shortSource(entry.sourceUrl)}<ExternalLinkIcon aria-hidden="true" /></Button> : null}
          </div>
          <div className="shared-pack-actions">
            {entry.sourceUrl ? <Button variant="outline" size="sm" disabled={state?.busy} data-pack-check onClick={() => void check(entry)}>{state?.busy ? <Spinner /> : null}{t("checkUpdates")}</Button>
              : <DropdownMenu>
                <DropdownMenuTrigger asChild><Button variant="outline" size="sm" disabled={state?.busy} data-pack-choose-newer>{state?.busy ? <Spinner /> : null}{t("chooseNewer")}</Button></DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-auto">
                  <DropdownMenuItem onSelect={() => void chooseNewer(entry, { zip: true })}>{t("chooseZip")}</DropdownMenuItem>
                  <DropdownMenuItem onSelect={() => void chooseNewer(entry, { folder: true })}>{t("chooseFolder")}</DropdownMenuItem>
                </DropdownMenuContent>
              </DropdownMenu>}
            {entry.blockedReason ? null : <Button variant="ghost" size="sm" onClick={() => { setDone(false); setRemoving(entry); }}>{t("remove")}</Button>}
            {reportable ? <Button variant="ghost" size="sm" aria-describedby={`${headingId}-${entry.importId}-report`} onClick={() => void bridge.packReport({ loupistId, importId: entry.importId })}>{t("report")}</Button> : null}
          </div>
          {reportable ? <p id={`${headingId}-${entry.importId}-report`} className="shared-pack-meta">{t("reportNote")}</p> : null}
          {state && !state.busy && !update ? <p className="shared-pack-status" role="status" data-pack-check-result={state.result.kind}>{checkText(state.result)}</p> : null}
          {update ? <PackUpdateDiff key={update.stagingId} bridge={bridge} loupistId={loupistId} update={update}
            onDismiss={() => setChecks(current => { const next = { ...current }; delete next[entry.importId]; return next; })}
            onArchived={() => onChanged([...imports])}
            onApplied={next => { setChecks(current => { const rest = { ...current }; delete rest[entry.importId]; return rest; }); onChanged(next); }} /> : null}
        </li>;
      })}</ul>
      <RemoveDialog bridge={bridge} loupistId={loupistId} target={removing} onClose={() => setRemoving(null)} onRemoved={next => { setRemoving(null); setDone(true); onChanged(next); }} />
    </section>;
  },
);

function RemoveDialog({ bridge, loupistId, target, onClose, onRemoved }: {
  bridge: LoupistBridge; loupistId: string; target: PackImportView | null; onClose: () => void; onRemoved: (imports: PackImportView[]) => void;
}) {
  const { t } = useTranslation("sharing");
  const [archive, setArchive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { setArchive(false); setError(null); }, [target?.importId]);
  const remove = async () => {
    if (!target) return;
    setBusy(true); setError(null);
    try { onRemoved(await bridge.packRemove({ loupistId, importId: target.importId, archiveAccepted: archive })); }
    catch (cause) { setError(errorMessage(cause, t("addFailed"))); }
    finally { setBusy(false); }
  };
  return <Dialog open={target !== null} onOpenChange={open => { if (!open && !busy) onClose(); }}>
    <DialogContent size="sm" className="pack-remove-dialog" data-pack-remove-dialog>
      <DialogHeader>
        <DialogTitle>{t("remove")}</DialogTitle>
        {target ? <DialogDescription>{t("removeConfirm", { names: formatNumber(target.removable.names), links: formatNumber(target.removable.links), pack: target.name })}</DialogDescription> : null}
      </DialogHeader>
      {target?.removable.accepted ? <label className="pack-remove-archive"><input type="checkbox" checked={archive} onChange={event => setArchive(event.target.checked)} />{t("removeArchive", { formattedCount: formatNumber(target.removable.accepted) })}</label> : null}
      {error ? <p className="share-error" role="alert">{error}</p> : null}
      <DialogFooter>
        <Button variant="outline" disabled={busy} onClick={onClose}>{i18n.t("onboarding:appDialogs.cancel")}</Button>
        <Button variant="destructive" className="pack-remove-confirm" disabled={busy} onClick={() => void remove()}>{t("remove")}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>;
}
