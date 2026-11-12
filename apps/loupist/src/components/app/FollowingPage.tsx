import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import { Button } from "../ui/button";
import { PageHeader } from "../ui/page-header";
import { useAssetPageOpener } from "../assets/assetOpener";
import { WatchAllOffer } from "../chat/WatchAllOffer";
import type { LoupistBridge, LoupistSummary } from "../../types";
import type { WatchAsset } from "../../contracts/watch";

/** Props of the Following page, the `following` page in `src/app/navigation.ts`. */
export type FollowingPageProps = {
  bridge: LoupistBridge;
  loupist: LoupistSummary;
  /** Opens the holdings import sheet the shell owns (also the palette's Import holdings…). */
  onImportHoldings: () => void;
};

/**
 * The Following page's route body: what you follow, shared by every Loupist. This is the
 * shell's minimal page; the full page (Watch-for toggles, add field, recent alerts)
 * replaces this body with the same props.
 */
export function FollowingPage({ bridge, loupist, onImportHoldings }: FollowingPageProps) {
  useTranslation("app");
  const openAsset = useAssetPageOpener();
  const [assets, setAssets] = useState<WatchAsset[] | null>(null);
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let current = true;
    void bridge.watchSettingsList({ loupistId: loupist.id })
      .then(settings => { if (current) setAssets([...settings.watched, ...settings.notWatched]); })
      .catch(() => { if (current) setAssets([]); });
    return () => { current = false; };
  }, [bridge, loupist.id, revision]);
  useEffect(() => bridge.onWatchChanged((event) => { if (event.loupistId === loupist.id) setRevision(value => value + 1); }), [bridge, loupist.id]);
  return <section className="following-page" data-following-page aria-labelledby="following-title">
    <PageHeader title={i18n.t("app:shell.tabs.following")} titleId="following-title" description={i18n.t("app:following.intro")}
      actions={<Button type="button" size="sm" variant="outline" onClick={onImportHoldings}>{i18n.t("app:palette.actions.importHoldings")}</Button>} />
    {assets === null ? null : assets.length ? <ul className="following-list">
      {assets.map(asset => <li key={asset.assetId}>
        {openAsset ? <button type="button" className="following-row" onClick={() => openAsset({ loupistId: loupist.id, assetId: asset.assetId, name: asset.name })}>
          <strong>{asset.name}</strong><span>{asset.symbol}</span>
        </button> : <span className="following-row"><strong>{asset.name}</strong><span>{asset.symbol}</span></span>}
      </li>)}
    </ul> : <p className="following-empty">{i18n.t("app:following.empty")}</p>}
    <WatchAllOffer bridge={bridge} loupistId={loupist.id} refreshKey={revision} />
  </section>;
}
