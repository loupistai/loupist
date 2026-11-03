import { useEffect, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { AssetEvidence } from "../../contracts/assets";
import type { EarningsSnapshot } from "../../contracts/earnings";
import { useAssetPageOpener } from "../assets/assetOpener";
import { useAssetNames } from "../assets/assetName";
import { EarningsCard } from "../assets/EarningsCard";

/**
 * The compact results card in an answer, in place of a paragraph that only
 * linked to the snapshot (finance-earnings-snapshot.md §2.2). It renders
 * inline, never in the panel; Rust's current snapshot fills it.
 */
export function EarningsChatCard({ bridge, loupistId, loupistName, assetId, eventId, onOpenLink }: {
  bridge: LoupistBridge;
  loupistId: string;
  loupistName: string;
  assetId: string;
  eventId: string;
  onOpenLink?: (url: string) => void;
}) {
  const [snapshot, setSnapshot] = useState<EarningsSnapshot | null>(null);
  const openAsset = useAssetPageOpener();
  const names = useAssetNames(bridge, loupistId);
  useEffect(() => {
    let live = true;
    void bridge.earningsSnapshotGet?.({ loupistId, assetId, eventId }).then(next => { if (live) setSnapshot(next); }).catch(() => undefined);
    return () => { live = false; };
  }, [bridge, loupistId, assetId, eventId]);
  if (!snapshot) return null;
  const name = names(snapshot);
  const open = (evidence: AssetEvidence) => {
    if (evidence.url && onOpenLink) onOpenLink(evidence.url);
    else openAsset?.({ loupistId, assetId, name, section: "results", eventId });
  };
  return <div className="earnings-chat-card" data-earnings-card={eventId}>
    <EarningsCard bridge={bridge} loupistId={loupistId} loupistName={loupistName} snapshot={snapshot} name={name} variant="compact" onOpenEvidence={open} />
  </div>;
}
