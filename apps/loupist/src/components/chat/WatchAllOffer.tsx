import i18n, { Trans, useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useEffect, useState } from "react";
import { Button } from "../ui/button";
import type { LoupistBridge } from "../../types";
import type { WatchDefaultsApplyResult, WatchSettings } from "../../contracts/watch";
import { watchKindColumn } from "../assets/watchText";

const t = i18n.getFixedT(null, "chat");
const tb = i18n.getFixedT(null, "briefings");

/**
 * "Watch all n" (product direction F5): on a new install following watches nothing,
 * so the first follow receipt and the Following page offer to watch every followed
 * asset with its class defaults in one press (`watch_defaults_apply`). Hidden once
 * something is watched, or when nothing followed can be watched. A kind that needs a
 * data connection says so after the press.
 */
export function WatchAllOffer({ bridge, loupistId, refreshKey, onOpenFollowing, onOpenConnections }: {
  bridge?: LoupistBridge; loupistId: string; refreshKey?: unknown; onOpenFollowing?: () => void; onOpenConnections?: () => void;
}) {
  useTranslation(["chat", "briefings"]);
  const [settings, setSettings] = useState<WatchSettings | null>(null);
  const [state, setState] = useState<"idle" | "busy" | "done" | "failed">("idle");
  const [result, setResult] = useState<WatchDefaultsApplyResult | null>(null);
  useEffect(() => {
    if (!bridge) return;
    let current = true;
    void bridge.watchSettingsList({ loupistId }).then(next => { if (current) setSettings(next); }).catch(() => {});
    return () => { current = false; };
  }, [bridge, loupistId, refreshKey]);
  if (!bridge || !settings) return null;
  if (state === "done" && result) {
    const watched = result.watchedAssets;
    return <div className="watch-all-offer" data-watch-all="done">
      {watched > 0 ? <p role="status">
        <Trans ns="chat" i18nKey="watchAll.done" values={{ count: watched, formattedCount: formatNumber(watched) }}
          components={[onOpenFollowing ? <button type="button" className="morning-link" onClick={onOpenFollowing} /> : <span />]} />
      </p> : null}
      {result.needsConnections.length ? <ul className="watch-all-needs" data-watch-needs>{result.needsConnections.map(item => <li key={`${item.kind}:${item.connection.id}`}>
        {tb("following.needsConnection", { kind: watchKindColumn(item.kind), connection: item.connection.name })}
        {onOpenConnections ? <> <button type="button" className="morning-link" onClick={onOpenConnections}>{tb("following.openConnections")}</button></> : null}
      </li>)}</ul> : null}
    </div>;
  }
  const count = settings.notWatched.length;
  if (settings.watched.length > 0 || count === 0) return null;
  const run = async () => {
    setState("busy");
    try { setResult(await bridge.watchDefaultsApply({ loupistId })); setState("done"); }
    catch { setState("failed"); }
  };
  return <div className="watch-all-offer follow-watch-offer" data-watch-all="offer">
    <p>{t("watchAll.note")}</p>
    <Button type="button" size="sm" disabled={state === "busy"} aria-busy={state === "busy" || undefined} onClick={() => void run()}>
      {t("watchAll.action", { count, formattedCount: formatNumber(count) })}
    </Button>
    {state === "failed" ? <p className="inline-error" role="alert">{t("watchAll.failed")}</p> : null}
  </div>;
}
