import i18n, { useTranslation } from "@/i18n";
import { formatDate, formatTime } from "@/i18n/format";
import { useState } from "react";
import type { ChatMessage, LoupistBridge } from "../../types";
import { Button } from "../ui/button";
import { useAssetPageOpener } from "../assets/assetOpener";
import { citationNumbers, shortDay, SourceChips, verdictLabel } from "../assets/assetText";
import { useAssetNames } from "../assets/assetName";

const t = i18n.getFixedT(null, "chat");

/**
 * A crossed red line, posted once into the home Thread by Rust: the asset, the
 * user's own line and the observed value with its source. Review opens the
 * asset page; Undo restores the previous read while it is the latest change.
 */
export function ThreadAlert({ message, bridge, loupistId, loupistName, onOpenLink }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
  onOpenLink?: (url: string) => void;
}) {
  useTranslation(["chat", "assets"]);
  const alert = message.alert!;
  const openAsset = useAssetPageOpener();
  const name = useAssetNames(bridge, loupistId)({ name: alert.name, assetId: alert.asset_id, symbol: alert.symbol, market: alert.market });
  const [undone, setUndone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const at = Date.parse(message.created_at);
  // Rust undoes only this move while it is still the latest one.
  const undo = async () => {
    if (!bridge || busy) return;
    setBusy(true);
    setError(null);
    try {
      const restored = await bridge.assetVerdictUndo({ loupistId, assetId: alert.asset_id, moveId: alert.move_id });
      setUndone(restored.read ? verdictLabel(restored.read.verdict) : "");
    } catch (cause) {
      const stale = (cause as { code?: unknown } | null)?.code === "conflict";
      setError(i18n.t(stale ? "assets:error.undoStale" : "assets:error.undo"));
      if (stale) setUndone("");
    } finally {
      setBusy(false);
    }
  };
  const evidence = [{ tool: "source", title: alert.source_title || name, url: alert.source_url, date: alert.fact_date }];
  return <article className="message assistant thread-alert" aria-label={name}>
    <div className="message-meta">
      <span>{loupistName}</span>
      {Number.isFinite(at) ? <time dateTime={new Date(at).toISOString()}>{formatTime(at)}</time> : null}
    </div>
    <p>
      {t("redLine.message", { name, date: formatDate(alert.set_at_ms, { year: undefined, month: "short", day: "numeric" }), claim: alert.claim, value: alert.value, factDate: shortDay(alert.fact_date) })}{" "}
      <SourceChips evidence={evidence} numbers={citationNumbers()} onOpen={item => { if (item.url && onOpenLink) onOpenLink(item.url); else openAsset?.({ loupistId, assetId: alert.asset_id, name }); }} />
    </p>
    {undone ? <p className="thread-delivery-source">{i18n.t("assets:verdict.undone", { verdict: undone, date: shortDay(alert.fact_date) })}</p> : null}
    {error ? <p className="inline-error" role="alert">{error}</p> : null}
    <div className="thread-alert-actions">
      {openAsset ? <Button type="button" variant="outline" size="sm" onClick={() => openAsset({ loupistId, assetId: alert.asset_id, name })}>{t("redLine.review")}</Button> : null}
      {bridge && undone === null ? <Button type="button" variant="ghost" size="sm" disabled={busy} onClick={() => void undo()}>{i18n.t("assets:verdict.undo")}</Button> : null}
    </div>
  </article>;
}
