import i18n, { Trans, useTranslation } from "@/i18n";
import { formatNumber } from "@/i18n/format";
import { useEffect, useState } from "react";
import { Button } from "../ui/button";
import type { LoupistBridge } from "../../types";
import type { WatchSettings } from "../../contracts/watch";

const t = i18n.getFixedT(null, "chat");

/**
 * "Watch all n" (product direction F5): on a new install following watches nothing,
 * so the first follow receipt and the Following page offer to watch every followed
 * asset with its class defaults in one press. Hidden until the host command exists,
 * once something is watched, or when nothing followed can be watched.
 */
export function WatchAllOffer({ bridge, loupistId, refreshKey, onOpenFollowing }: { bridge?: LoupistBridge; loupistId: string; refreshKey?: unknown; onOpenFollowing?: () => void }) {
  useTranslation("chat");
  const apply = bridge?.watchDefaultsApply?.bind(bridge);
  const [settings, setSettings] = useState<WatchSettings | null>(null);
  const [state, setState] = useState<"idle" | "busy" | "done" | "failed">("idle");
  const [watched, setWatched] = useState(0);
  useEffect(() => {
    if (!bridge || !apply) return;
    let current = true;
    void bridge.watchSettingsList({ loupistId }).then(next => { if (current) setSettings(next); }).catch(() => {});
    return () => { current = false; };
  }, [bridge, apply, loupistId, refreshKey]);
  if (!apply || !settings) return null;
  if (state === "done") return <p className="watch-all-offer" role="status" data-watch-all="done">
    <Trans ns="chat" i18nKey="watchAll.done" values={{ count: watched, formattedCount: formatNumber(watched) }}
      components={[onOpenFollowing ? <button type="button" className="morning-link" onClick={onOpenFollowing} /> : <span />]} />
  </p>;
  const count = settings.notWatched.length;
  if (settings.watched.length > 0 || count === 0) return null;
  const run = async () => {
    setState("busy");
    try { setWatched((await apply({ loupistId })).watchedAssets); setState("done"); }
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
