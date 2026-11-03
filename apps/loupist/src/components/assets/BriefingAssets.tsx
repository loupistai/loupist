import i18n, { useTranslation } from "@/i18n";
import type { LoupistBridge } from "../../types";
import type { BriefingAsset } from "../../contracts/scheduled";
import type { AssetEvidence } from "../../contracts/assets";
import { Button } from "../ui/button";
import { useAssetPageOpener } from "./assetOpener";
import { useAssetNames } from "./assetName";
import { citationNumbers, datedSentence, SourceChips } from "./assetText";
import { VerdictChip } from "./VerdictChip";

/** Rows per asset in the briefing reader. */
const ROWS = 2;

/**
 * The briefing's "What you follow": each followed asset with a delta Rust
 * computed at run start, its read, up to two dated rows with sources (numbered
 * once across the briefing), and its page.
 */
export function BriefingAssets({ assets, bridge, loupistId, loupistName, onOpenLink }: { assets: readonly BriefingAsset[]; bridge?: LoupistBridge; loupistId: string; loupistName: string; onOpenLink: (url: string) => void }) {
  useTranslation(["assets", "chat"]);
  const open = useAssetPageOpener();
  const names = useAssetNames(bridge, loupistId);
  const numbers = citationNumbers(assets.flatMap(asset => asset.rows.slice(0, ROWS).map(row => row.evidence)));
  const openEvidence = (asset: BriefingAsset, name: string) => (evidence: AssetEvidence) => {
    if (evidence.url) onOpenLink(evidence.url);
    else open?.({ loupistId, assetId: asset.assetId, name });
  };
  return <ul className="briefing-assets">
    {assets.map(asset => {
      const name = names(asset);
      return <li key={asset.assetId}>
        <div className="briefing-asset-head">
          <strong>{name}</strong>
          {asset.verdict ? <VerdictChip verdict={asset.verdict} loupist={loupistName} /> : null}
          {open ? <Button type="button" variant="link" size="sm" data-print="omit" onClick={() => open({ loupistId, assetId: asset.assetId, name })}>{i18n.t("chat:assetCard.openPage")}</Button> : null}
        </div>
        <ul className="briefing-asset-rows">
          {asset.rows.slice(0, ROWS).map(row => <li key={row.id}>{datedSentence(row)} <SourceChips evidence={row.evidence} numbers={numbers} onOpen={openEvidence(asset, name)} /></li>)}
        </ul>
      </li>;
    })}
  </ul>;
}
