import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { AssetPage } from "../../contracts/assets";
import { Button } from "../ui/button";
import { useAssetPageOpener } from "./assetOpener";
import { readSentence } from "./assetText";
import { VerdictChip } from "./VerdictChip";
import { listingExchange } from "./listingExchange";

/**
 * Top of the Map inspector's Facts for an entity matched to an asset page with
 * a thesis: the read, its latest sentence and the page. A followed listing can
 * also show its country on its own.
 */
export function InspectorThesis({ bridge, loupistId, loupistName, entityId }: { bridge: LoupistBridge; loupistId: string; loupistName?: string; entityId: string }) {
  useTranslation(["assets", "knowledge"]);
  const open = useAssetPageOpener();
  const [page, setPage] = useState<AssetPage | null>(null);
  useEffect(() => {
    let live = true;
    setPage(null);
    const refresh = () => {
      void bridge.assetPageGet({ loupistId, entityId })
        .then(next => { if (live) setPage(next); })
        .catch(() => { if (live) setPage(null); });
    };
    refresh();
    const unsubscribe = bridge.onAssetsChanged(refresh);
    return () => { live = false; unsubscribe(); };
  }, [bridge, loupistId, entityId]);
  const country = page?.asset.followedAtMs != null ? listingExchange(page.asset.reference, i18n.language).country : null;
  const listing = country ? <p className="map-muted">{country}</p> : null;
  if (!page?.read) return listing;
  // The page's own sentence, so an undone move reads the same here as on the page.
  const sentence = readSentence(page.read).text;
  return <section className="asset-thesis-summary" aria-label={i18n.t("knowledge:map.thesis")}>
    <div className="asset-thesis-summary-head">
      {/* Styled like the inspector's other headings ("Key facts"). */}
      <h3>{i18n.t("knowledge:map.thesis")}</h3>
      {page.read.latestMove ? <VerdictChip verdict={page.read.verdict} loupist={loupistName} /> : null}
    </div>
    {listing}
    <p>{sentence}</p>
    {open ? <Button type="button" variant="link" size="sm" onClick={() => open({ loupistId, assetId: page.asset.assetId, name: page.asset.name })}>{i18n.t("knowledge:map.openAssetPage")}</Button> : null}
  </section>;
}
