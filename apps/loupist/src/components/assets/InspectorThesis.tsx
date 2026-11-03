import i18n, { useTranslation } from "@/i18n";
import { useEffect, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { AssetDelta } from "../../contracts/assets";
import { Button } from "../ui/button";
import { useAssetPageOpener } from "./assetOpener";
import { crossedReason, midSentence, shortDay, verdictLabel } from "./assetText";
import { VerdictChip } from "./VerdictChip";

/**
 * Top of the Map inspector's Facts for an entity matched to an asset page with
 * a thesis: the read, its latest sentence and the page. Omitted otherwise.
 */
export function InspectorThesis({ bridge, loupistId, loupistName, entityId }: { bridge: LoupistBridge; loupistId: string; loupistName?: string; entityId: string }) {
  useTranslation(["assets", "knowledge"]);
  const open = useAssetPageOpener();
  const [page, setPage] = useState<AssetDelta | null>(null);
  useEffect(() => {
    let live = true;
    setPage(null);
    // Read-only: every page's read; matching is Rust's (`entityId`).
    void (bridge.assetDelta?.({ loupistId, sinceMs: Date.now() }) ?? Promise.resolve([]))
      .then(deltas => { if (live) setPage(deltas.find(delta => delta.entityId === entityId && delta.verdict) ?? null); })
      .catch(() => undefined);
    return () => { live = false; };
  }, [bridge, loupistId, entityId]);
  if (!page?.verdict) return null;
  const move = page.latestMove;
  const sentence = move
    ? `${i18n.t("assets:verdict.since", { verdict: verdictLabel(page.verdict), date: shortDay(page.verdictSince) })} — ${move.line?.crossed ? crossedReason(move.line, move.factDate) : midSentence(move.reason)}`
    : i18n.t("assets:verdict.started", { date: shortDay(page.verdictSince) });
  return <section className="asset-thesis-summary" aria-label={i18n.t("knowledge:map.thesis")}>
    <div className="asset-thesis-summary-head">
      {/* Styled like the inspector's other headings ("Key facts"). */}
      <h3>{i18n.t("knowledge:map.thesis")}</h3>
      <VerdictChip verdict={page.verdict} loupist={loupistName} />
    </div>
    <p>{sentence}</p>
    {open ? <Button type="button" variant="link" size="sm" onClick={() => open({ loupistId, assetId: page.assetId, name: page.name })}>{i18n.t("knowledge:map.openAssetPage")}</Button> : null}
  </section>;
}
