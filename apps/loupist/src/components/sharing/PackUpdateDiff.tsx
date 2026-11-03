import { useId, useMemo, useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatNumber } from "@/i18n/format";
import type { LoupistBridge } from "../../types";
import type { PackFlag, PackImportView, PackItemView, PackUpdate, PackUpdateChangedItem, PackUpdateRemovedItem } from "../../contracts/sharing";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Spinner } from "../ui/spinner";
import { PackMarkdown } from "./PackMarkdown";
import "@/styles/sharing.css";

type Viewing = { title: string; then: string | null; now: PackItemView; edited: boolean; key: string; flags: PackFlag[] };

const flagText = (flags: readonly PackFlag[]) => [...new Set(flags)].map(flag => i18n.t(`sharing:flag.${flag}`)).join(" · ");

/**
 * "Update available" under a shared Loupist's row (W19.5). Nothing changes
 * until Apply selected: new items become suggestions, chosen changes replace
 * the text here (one with a warning waits in the suggestions), and the map
 * gains the author's new relationships while the ones they withdrew are marked.
 */
export function PackUpdateDiff({ bridge, loupistId, update, onApplied, onDismiss, onArchived }: {
  bridge: LoupistBridge; loupistId: string; update: PackUpdate;
  onApplied: (imports: PackImportView[]) => void; onDismiss: () => void; onArchived: () => void;
}) {
  const { t } = useTranslation("sharing");
  const ids = { new: useId(), changed: useId(), removed: useId() };
  // Clean items start chosen; anything with a warning, or changed here since, starts unchosen.
  const [chosen, setChosen] = useState<Set<string>>(() => new Set([
    ...update.newItems.filter(item => !item.flags.length).map(item => item.key),
    ...update.changed.filter(item => !item.flags.length && !item.edited).map(item => item.key),
  ]));
  const [map, setMap] = useState(Boolean(update.map?.added));
  const [archived, setArchived] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewing, setViewing] = useState<Viewing | null>(null);
  const toggle = (key: string, on: boolean) => setChosen(current => { const next = new Set(current); if (on) next.add(key); else next.delete(key); return next; });
  const date = Number.isNaN(Date.parse(update.createdAt)) ? null : formatDate(Date.parse(update.createdAt), { year: undefined, month: "short", day: "numeric" });
  const removed = useMemo(() => update.removed.filter(item => !archived.has(item.key)), [update.removed, archived]);

  const apply = async () => {
    setBusy("apply"); setError(null);
    try { onApplied(await bridge.packApplyUpdate({ loupistId, importId: update.importId, stagingId: update.stagingId, itemKeys: [...chosen], applyMap: map && Boolean(update.map) })); }
    catch (cause) { setError(errorMessage(cause, t("addFailed"))); }
    finally { setBusy(null); }
  };
  const archive = async (item: PackUpdateRemovedItem) => {
    setBusy(item.key); setError(null);
    try {
      if (item.kind === "principle") await bridge.principleArchive({ loupistId, recordId: item.lensRecordId, archived: true });
      else await bridge.knowledgeLensArchive({ loupistId, recordId: item.lensRecordId, archived: true });
      setArchived(current => new Set(current).add(item.key));
      onArchived();
    } catch (cause) { setError(errorMessage(cause, t("addFailed"))); }
    finally { setBusy(null); }
  };

  return <div className="pack-update" data-pack-update>
    <p className="pack-update-title">{[t("updateAvailable", { from: formatNumber(update.fromVersion), to: formatNumber(update.toVersion) }), date].filter(Boolean).join(" · ")}</p>
    {update.blocked ? <p className="shared-pack-banner" role="status">{t("safetyBlocked", { reason: t(`reason.${update.blocked}`) })}</p> : null}
    {update.newItems.length ? <section className="pack-update-group" aria-labelledby={ids.new}>
      <h3 id={ids.new} className="share-label">{t("diffNew")} <span className="gg-num">({formatNumber(update.newItems.length)})</span></h3>
      <ul>{update.newItems.map(item => <li key={item.key} className="pack-update-row">
        <label className="pack-update-choice"><input type="checkbox" checked={chosen.has(item.key)} disabled={busy !== null || Boolean(update.blocked)} onChange={event => toggle(item.key, event.target.checked)} />
          <span className="pack-update-text"><span>{item.title}</span>{item.flags.length ? <span className="pack-suggestion-warning">{flagText(item.flags)}</span> : null}</span></label>
        <Button variant="ghost" size="sm" onClick={() => setViewing({ title: item.title, then: null, now: item.item, edited: false, key: item.key, flags: item.flags })}>{t("view")}</Button>
      </li>)}</ul>
    </section> : null}
    {update.changed.length ? <section className="pack-update-group" aria-labelledby={ids.changed}>
      <h3 id={ids.changed} className="share-label">{t("diffChanged")} <span className="gg-num">({formatNumber(update.changed.length)})</span></h3>
      <ul>{update.changed.map((item: PackUpdateChangedItem) => <li key={item.key} className="pack-update-row">
        <label className="pack-update-choice"><input type="checkbox" checked={chosen.has(item.key)} disabled={busy !== null || Boolean(update.blocked)} onChange={event => toggle(item.key, event.target.checked)} />
          <span className="pack-update-text"><span>{item.title}</span>
            {item.edited ? <span className="pack-update-note">{t("youEdited")}</span> : null}
            {item.flags.length ? <span className="pack-suggestion-warning">{flagText(item.flags)}</span> : null}</span></label>
        <Button variant="ghost" size="sm" onClick={() => setViewing({ title: item.title, then: item.then, now: item.now, edited: item.edited, key: item.key, flags: item.flags })}>{t("thenNow")}</Button>
      </li>)}</ul>
    </section> : null}
    {removed.length ? <section className="pack-update-group" aria-labelledby={ids.removed}>
      <h3 id={ids.removed} className="share-label">{t("diffRemoved")}</h3>
      <ul>{removed.map(item => <li key={item.key} className="pack-update-row">
        <span className="pack-update-text"><span>{item.title} <span className="pack-update-note">· {t("keptHere")}</span></span></span>
        <Button variant="ghost" size="sm" disabled={busy !== null} onClick={() => void archive(item)}>{busy === item.key ? <Spinner /> : null}{t("archive")}</Button>
      </li>)}</ul>
    </section> : null}
    {update.map ? <label className="pack-update-choice pack-update-map"><input type="checkbox" checked={map} disabled={busy !== null || Boolean(update.blocked)} onChange={event => setMap(event.target.checked)} />
      <span className="gg-num">{t("mapChanges", { names: formatNumber(update.map.names), links: formatNumber(update.map.links), withdrawn: formatNumber(update.map.withdrawn) })}</span></label> : null}
    {error ? <p className="share-error" role="alert">{error}</p> : null}
    <div className="pack-update-actions">
      <Button variant="outline" size="sm" disabled={busy !== null || Boolean(update.blocked)} data-pack-update-apply onClick={() => void apply()}>{busy === "apply" ? <Spinner /> : null}{t("applySelected")}</Button>
      <Button variant="ghost" size="sm" disabled={busy !== null} onClick={onDismiss}>{t("notNow")}</Button>
    </div>
    <ThenNowDialog viewing={viewing} chosen={viewing ? chosen.has(viewing.key) : false} onClose={() => setViewing(null)}
      onChoose={on => { if (viewing) toggle(viewing.key, on); setViewing(null); }} />
  </div>;
}

/** Yours beside theirs; a new item shows theirs alone. Keep mine / Use theirs set the choice. */
function ThenNowDialog({ viewing, chosen, onClose, onChoose }: {
  viewing: Viewing | null; chosen: boolean; onClose: () => void; onChoose: (on: boolean) => void;
}) {
  const { t } = useTranslation("sharing");
  const highlight = useMemo(() => new Set(viewing?.now.findings.flatMap(finding => finding.lines) ?? []), [viewing]);
  return <Dialog open={viewing !== null} onOpenChange={open => { if (!open) onClose(); }}>
    <DialogContent className="pack-then-now-dialog" data-pack-then-now>
      <DialogHeader>
        <DialogTitle>{viewing?.title}</DialogTitle>
        {viewing?.then !== null && viewing ? <DialogDescription>{viewing.edited ? t("youEdited") : t("thenNow")}</DialogDescription> : null}
      </DialogHeader>
      <DialogBody>
        {viewing ? <div className={viewing.then !== null ? "pack-then-now" : "pack-then-now is-single"}>
          {viewing.then !== null ? <section aria-label={t("yours")}><h3 className="share-label">{t("yours")}</h3><PackMarkdown markdown={viewing.then} /></section> : null}
          <section aria-label={t("theirs")}>
            {viewing.then !== null ? <h3 className="share-label">{t("theirs")}</h3> : null}
            {viewing.flags.length ? <ul className="pack-flags">{[...new Set(viewing.flags)].map(flag => <li key={flag}>{t(`flag.${flag}`)}</li>)}</ul> : null}
            <PackMarkdown markdown={viewing.now.markdown} highlight={highlight} />
          </section>
        </div> : null}
      </DialogBody>
      <DialogFooter className="share-footer">
        {viewing?.flags.length ? <p className="pack-suggestion-note">{t("acceptFlaggedNote")}</p> : null}
        {viewing?.edited ? <div className="share-outputs">
          <Button variant="ghost" aria-pressed={!chosen} onClick={() => onChoose(false)}>{t("keepMine")}</Button>
          <Button variant="outline" aria-pressed={chosen} onClick={() => onChoose(true)}>{t("useTheirs")}</Button>
        </div> : <Button variant="outline" onClick={onClose}>{i18n.t("common:dialog.close")}</Button>}
      </DialogFooter>
    </DialogContent>
  </Dialog>;
}
