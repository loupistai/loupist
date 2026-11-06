import { useEffect, useState } from "react";
import i18n, { useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import type { LoupistBridge } from "../../types";
import type { PackItemView, PackSuggestion, PackSuggestionGroup } from "../../contracts/sharing";
import { errorMessage } from "../../lib/errors";
import { Button } from "../ui/button";
import { Dialog, DialogBody, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "../ui/dialog";
import { Spinner } from "../ui/spinner";
import { PackMarkdown } from "./PackMarkdown";
import "@/styles/sharing.css";

const kindLabel = (kind: PackSuggestion["kind"]) => i18n.t(kind === "perspective" ? "knowledge:pageReader.kind.perspective" : kind === "principle" ? "knowledge:pageReader.kind.principle" : "knowledge:pageReader.kind.method");

/**
 * Suggestions from shared Loupists (W19.4, W19.5): nothing reaches Lens until the
 * user accepts it, and a flagged item is accepted on its own. An author's newer
 * text for an item already accepted ("Changed") waits here the same way.
 */
export function PackSuggestions({ bridge, loupistId, groups, onChanged }: {
  bridge: LoupistBridge; loupistId: string; groups: readonly PackSuggestionGroup[]; onChanged: (groups: PackSuggestionGroup[]) => void;
}) {
  const { t } = useTranslation("sharing");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewing, setViewing] = useState<{ importId: string; suggestion: PackSuggestion } | null>(null);
  const decide = async (importId: string, itemKeys: string[], decision: "accept" | "skip") => {
    setBusy(`${importId}:${itemKeys.join(",")}`); setError(null);
    try { onChanged(await bridge.packDecide({ loupistId, importId, itemKeys, decision })); setViewing(null); }
    catch (cause) { setError(errorMessage(cause, t("addFailed"))); }
    finally { setBusy(null); }
  };
  const pendingGroups = groups.map(group => ({ ...group, items: group.items.filter(item => item.state === "pending") })).filter(group => group.items.length);
  if (!pendingGroups.length) return null;
  return <div className="pack-suggestions" data-pack-suggestions>
    {pendingGroups.map(group => {
      const safe = group.items.filter(item => !item.flags.length);
      return <section key={group.importId} className="pack-suggestion-group" aria-label={t("suggestionsTitle", { pack: group.packName, formattedCount: formatNumber(group.items.length) })}>
        <div className="pack-suggestion-head">
          <h3 className="share-label">{t("suggestionsTitle", { pack: group.packName, formattedCount: formatNumber(group.items.length) })}</h3>
          {safe.length ? <Button variant="outline" size="sm" disabled={busy !== null} data-accept-safe onClick={() => void decide(group.importId, safe.map(item => item.key), "accept")}>{t("acceptSafe", { formattedCount: formatNumber(safe.length) })}</Button> : null}
        </div>
        <ul>{group.items.map(item => <li key={item.key} className="pack-suggestion" data-flagged={item.flags.length ? true : undefined}>
          <div className="pack-suggestion-text">
            <strong>{item.title}</strong>
            <span className="pack-suggestion-kind">{item.update ? `${kindLabel(item.kind)} · ${t("diffChanged")}` : kindLabel(item.kind)}</span>
            {item.flags.length ? <span className="pack-suggestion-warning">{item.flags.map(flag => t(`flag.${flag}`)).join(" · ")}</span> : null}
          </div>
          {item.flags.length ? <p className="pack-suggestion-note">{t("acceptFlaggedNote")}</p> : null}
          <div className="pack-suggestion-actions">
            <Button variant="ghost" size="sm" onClick={() => setViewing({ importId: group.importId, suggestion: item })}>{t("view")}</Button>
            <Button variant="outline" size="sm" disabled={busy !== null} onClick={() => void decide(group.importId, [item.key], "accept")}>{t(item.flags.length ? "acceptAnyway" : "accept")}</Button>
            <Button variant="ghost" size="sm" disabled={busy !== null} onClick={() => void decide(group.importId, [item.key], "skip")}>{t("skip")}</Button>
          </div>
        </li>)}</ul>
      </section>;
    })}
    {error ? <p className="share-error" role="alert">{error}</p> : null}
    <SuggestionReader bridge={bridge} loupistId={loupistId} viewing={viewing} busy={busy !== null} onClose={() => setViewing(null)}
      onDecide={(decision) => viewing && void decide(viewing.importId, [viewing.suggestion.key], decision)} />
  </div>;
}

/** The read-only proposal view: the shared text exactly as validated, with its warnings. */
function SuggestionReader({ bridge, loupistId, viewing, busy, onClose, onDecide }: {
  bridge: LoupistBridge; loupistId: string; viewing: { importId: string; suggestion: PackSuggestion } | null; busy: boolean;
  onClose: () => void; onDecide: (decision: "accept" | "skip") => void;
}) {
  const { t } = useTranslation("sharing");
  const [item, setItem] = useState<PackItemView | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    if (!viewing) return;
    let live = true;
    setItem(null); setError(null);
    void bridge.packItemRead({ loupistId, importId: viewing.importId, key: viewing.suggestion.key })
      .then(value => { if (live) setItem(value); })
      .catch(cause => { if (live) setError(errorMessage(cause, t("addFailed"))); });
    return () => { live = false; };
  }, [bridge, loupistId, viewing, t]);
  const flagged = Boolean(viewing?.suggestion.flags.length);
  return <Dialog open={viewing !== null} onOpenChange={open => { if (!open) onClose(); }}>
    <DialogContent className="pack-suggestion-reader" data-pack-suggestion-reader>
      <DialogHeader><DialogTitle>{viewing?.suggestion.title}</DialogTitle></DialogHeader>
      <DialogBody>
        {viewing ? <p className="pack-suggestion-kind">{kindLabel(viewing.suggestion.kind)}</p> : null}
        {item?.findings.length ? <ul className="pack-flags">{[...new Set(item.findings.map(finding => finding.flag))].map(flag => <li key={flag}>{t(`flag.${flag}`)}</li>)}</ul> : null}
        {item ? <PackMarkdown markdown={item.markdown} highlight={new Set(item.findings.flatMap(finding => finding.lines))} /> : error ? <p className="share-error" role="alert">{error}</p> : <Spinner />}
      </DialogBody>
      <DialogFooter className="share-footer">
        {flagged ? <p className="pack-suggestion-note">{t("acceptFlaggedNote")}</p> : null}
        <div className="share-outputs">
          <Button variant="outline" disabled={busy || !item} onClick={() => onDecide("accept")}>{t(flagged ? "acceptAnyway" : "accept")}</Button>
          <Button variant="ghost" disabled={busy} onClick={() => onDecide("skip")}>{t("skip")}</Button>
        </div>
      </DialogFooter>
    </DialogContent>
  </Dialog>;
}
