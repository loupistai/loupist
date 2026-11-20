import i18n, { useTranslation } from "@/i18n";
import { formatDate } from "@/i18n/format";
import { useLayoutEffect, useRef, useState } from "react";
import type { ChatMessage, LoupistBridge } from "../../types";
import { Button } from "../ui/button";
import { commandErrorCode } from "@/lib/errors";
import { useGo } from "../navigation/Open";
import { hostSourceDestination, savedAssetTarget } from "../../chat/navigation";
import { citationNumbers, shortDay, SourceChips, verdictLabel } from "../assets/assetText";
import { useAssetNames } from "../assets/assetName";
import { BotPostBody } from "./BotPost";
import "@/styles/bot-cards.css";

const t = i18n.getFixedT(null, "chat");

/**
 * A crossed red line, posted once into the home Thread by Rust: the asset, the
 * user's own line and the observed value with its source. Review opens the
 * asset page; Undo restores the previous read while it is the latest change.
 */
export function ThreadAlert({ message, bridge, loupistId, loupistName }: {
  message: ChatMessage;
  bridge?: LoupistBridge;
  loupistId: string;
  loupistName: string;
}) {
  useTranslation(["chat", "assets"]);
  const alert = message.alert!;
  const go = useGo();
  const name = useAssetNames(bridge, loupistId)({ name: alert.name, assetId: alert.asset_id, symbol: alert.symbol, market: alert.market });
  const [undone, setUndone] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const at = Date.parse(message.created_at);
  // Undo goes away once it has run or can no longer run: focus moves to what happened.
  const outcomeRef = useRef<HTMLParagraphElement>(null);
  const errorRef = useRef<HTMLParagraphElement>(null);
  const actionsRef = useRef<HTMLDivElement>(null);
  const undoPressed = useRef(false);
  useLayoutEffect(() => {
    if (undone === null || !undoPressed.current) return;
    undoPressed.current = false;
    const focused = document.activeElement;
    if (focused && focused !== document.body) return;
    (outcomeRef.current ?? errorRef.current ?? actionsRef.current?.querySelector("button"))?.focus({ preventScroll: true });
  }, [undone]);
  // Rust undoes only this move while it is still the latest one.
  const undo = async () => {
    if (!bridge || busy) return;
    undoPressed.current = true;
    setBusy(true);
    setError(null);
    try {
      const restored = await bridge.assetVerdictUndo({ loupistId, assetId: alert.asset_id, moveId: alert.move_id });
      setUndone(restored.read ? verdictLabel(restored.read.verdict) : "");
    } catch (cause) {
      const stale = commandErrorCode(cause) === "conflict";
      setError(i18n.t(stale ? "assets:error.undoStale" : "assets:error.undo"));
      if (stale) setUndone("");
    } finally {
      setBusy(false);
    }
  };
  // The alert's page by its id, else by the ticker it retained.
  const page = savedAssetTarget(loupistId, { assetId: alert.asset_id, ticker: alert.symbol, name: alert.name });
  const evidence = [{ tool: "source", title: alert.source_title || name, url: alert.source_url, date: alert.fact_date }];
  return <article className="message assistant thread-alert" data-bot-post="" aria-label={t("redLine.label", { name })}>
    <BotPostBody loupistName={loupistName} at={at}>
    <div className="bot-card" data-tone="alert">
    <div className="bot-card-kicker"><span className="bot-card-tag">{t("botCards.lineCrossed")}</span><span>{name}</span></div>
    <p className="bot-card-body">
      {t("redLine.message", { name, date: formatDate(alert.set_at_ms, { year: undefined, month: "short", day: "numeric" }), claim: alert.claim, value: alert.value, factDate: shortDay(alert.fact_date) })}{" "}
      <SourceChips evidence={evidence} numbers={citationNumbers()} onOpen={(_source, citation) => go?.(hostSourceDestination(citation, page))} />
    </p>
    {undone ? <p ref={outcomeRef} tabIndex={-1} className="thread-delivery-source" role="status">{i18n.t("assets:verdict.undone", { verdict: undone, date: shortDay(alert.fact_date) })}</p> : null}
    {error ? <p ref={errorRef} tabIndex={-1} className="inline-error" role="alert">{error}</p> : null}
    <div ref={actionsRef} className="thread-alert-actions bot-card-actions">
      {go && page ? <Button type="button" variant="outline" size="sm" onClick={() => go({ kind: "asset", target: page })}>{t("redLine.review")}</Button> : null}
      {/* Waiting keeps the button focusable, so a failed Undo leaves the keyboard where it was. */}
      {bridge && undone === null ? <Button type="button" variant="ghost" size="sm" aria-disabled={busy || undefined} onClick={() => void undo()}>{i18n.t("assets:verdict.undo")}</Button> : null}
    </div>
    </div>
    </BotPostBody>
  </article>;
}
