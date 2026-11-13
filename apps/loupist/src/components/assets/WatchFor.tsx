import i18n, { useTranslation } from "@/i18n";
import { formatTime } from "@/i18n/format";
import { useCallback, useEffect, useId, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { WatchKind, WatchKindState, WatchSettings } from "../../contracts/watch";
import { Switch } from "../ui/switch";
import { connectionText } from "../marketplace/connectionCopy";
import { watchKindLabel } from "./watchText";
import "@/styles/watch.css";

const t = i18n.getFixedT(null, "assets");

/**
 * The Loupist's watch settings, kept current from `watch-changed`. `failed` says the
 * last read failed; a page that depends on them offers `reload` as its retry.
 */
export function useWatchSettings(bridge: LoupistBridge | undefined, loupistId: string) {
  const [settings, setSettings] = useState<WatchSettings | null>(null);
  const [failed, setFailed] = useState(false);
  const load = useCallback(async () => {
    if (!bridge?.watchSettingsList) return;
    try { setSettings(await bridge.watchSettingsList({ loupistId })); setFailed(false); } catch { setFailed(true); }
  }, [bridge, loupistId]);
  useEffect(() => {
    void load();
    const off = bridge?.onWatchChanged?.(event => { if (event.loupistId === loupistId) void load(); });
    return () => off?.();
  }, [bridge, loupistId, load]);
  return { settings, setSettings, reload: load, failed };
}

/**
 * "Watch for" on the asset page (finance-watchers.md §2.2): one switch per kind
 * that applies to this asset. A kind whose connection is off stays focusable
 * and says which connection it needs; switches only control alerts.
 */
export function WatchFor({ bridge, loupistId, loupistName, assetId, name, onOpenConnections, onKinds }: {
  bridge: LoupistBridge;
  loupistId: string;
  loupistName: string;
  assetId: string;
  name: string;
  onOpenConnections: () => void;
  /** The asset's switches whenever they change, so the page knows what the watchers already check. */
  onKinds?: (kinds: readonly WatchKindState[]) => void;
}) {
  useTranslation("assets");
  const titleId = useId();
  const { settings, setSettings, reload } = useWatchSettings(bridge, loupistId);
  const [busy, setBusy] = useState<WatchKind | null>(null);
  const asset = settings ? [...settings.watched, ...settings.notWatched].find(item => item.assetId === assetId) : undefined;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- reports the kinds when the settings or the asset change; `onKinds` is the parent's inline callback.
  useEffect(() => { onKinds?.(asset?.kinds ?? []); }, [settings, assetId]);
  if (!settings || !asset || !asset.kinds.length) return null;
  const toggle = async (kind: WatchKind, enabled: boolean) => {
    if (busy) return;
    setBusy(kind);
    try { setSettings(await bridge.watchSettingSet({ loupistId, assetId, kind, enabled, expectedRevision: asset.revision })); }
    catch { await reload(); }
    finally { setBusy(null); }
  };
  const failed = asset.kinds.filter(state => state.failed);
  return <section className="watch-for" aria-labelledby={titleId}>
    <h2 id={titleId} className="asset-section-label">{t("watch.title")}</h2>
    <ul className="watch-for-kinds">
      {asset.kinds.map(state => {
        const reasonId = `${titleId}-${state.kind}`;
        const needs = state.needsConnection;
        return <li key={state.kind} className="watch-for-row" data-disabled={needs ? true : undefined}>
          <span className="watch-for-label">{watchKindLabel(state.kind)}</span>
          {/* A switch whose connection is off stays focusable; its reason is its description. */}
          <Switch size="sm" className="watch-switch" checked={state.enabled} data-kind={state.kind}
            aria-label={t("watch.switchLabel", { kind: watchKindLabel(state.kind), name })}
            aria-disabled={needs || busy === state.kind ? true : undefined} aria-describedby={needs ? reasonId : undefined}
            onCheckedChange={checked => { if (!needs) void toggle(state.kind, checked); }} />
          {needs ? <p id={reasonId} className="watch-for-reason">{t("watch.needsConnection", { connection: connectionText(needs.name) })} <button type="button" className="asset-inline-link" onClick={onOpenConnections}>{t("watch.turnOn")}</button></p> : null}
        </li>;
      })}
    </ul>
    {failed.map(state => <p key={state.kind} className="asset-check-problem">{t("watch.failed", { kind: watchKindLabel(state.kind), connection: connectionText(state.failed!.connection) })}</p>)}
    <p className="asset-caption">{settings.checking ? t("watch.checking")
      : settings.lastCheckedMs ? t("watch.footer", { time: formatTime(settings.lastCheckedMs), loupist: loupistName })
        : t("watch.notChecked", { loupist: loupistName })}</p>
  </section>;
}
