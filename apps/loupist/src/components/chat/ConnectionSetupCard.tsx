import i18n, { useTranslation } from "@/i18n";
import { useCallback, useEffect, useState } from "react";
import type { LoupistBridge } from "../../types";
import type { MarketplaceConnectorStatus, MarketplaceEntry } from "../../marketplace/types";
import { Button } from "../ui/button";
import { MarketplaceSetupDialog } from "../marketplace/MarketplaceSetupDialog";
import { connectionText } from "../marketplace/connectionCopy";

const t = i18n.getFixedT(null, "chat");

type Live = { entry: MarketplaceEntry; connector: MarketplaceConnectorStatus };

/**
 * What the card can show, read live from the host every time it refreshes.
 * `unavailable` covers an entry that left the catalog and a runtime that is
 * missing from this build; the other three mirror the host's `state`.
 */
type CardState =
  | { status: "loading" }
  | { status: "needs_setup"; live: Live }
  | { status: "off_here"; live: Live }
  | { status: "ready"; live: Live }
  | { status: "unavailable"; name?: string };

async function readState(bridge: LoupistBridge, loupistId: string, entryId: string): Promise<CardState> {
  const [snapshot, bindings] = await Promise.all([bridge.marketplaceSnapshot(), bridge.loupistCapabilityList(loupistId)]);
  const entry = snapshot.catalog.entries.find((candidate) => candidate.id === entryId);
  const connector = snapshot.connectors.find((candidate) => candidate.entry_id === entryId);
  if (!entry || !connector || connector.readiness === "runtime_unavailable") return { status: "unavailable", name: entry?.name };
  const live = { entry, connector };
  if (connector.readiness !== "ready") return { status: "needs_setup", live };
  const enabled = bindings.find((binding) => binding.entry_id === entryId)?.enabled === true;
  return enabled ? { status: "ready", live } : { status: "off_here", live };
}

/**
 * The card a `connection_setup` step leaves in the answer: it reads the
 * connection's live state and offers the one action that state needs. Setup
 * happens in the Connections dialog, so no key passes through Chat, and the
 * connection works from the next message because a turn's authority is sealed
 * when it starts.
 */
export function ConnectionSetupCard({ bridge, loupistId, entryId, onAskAgain }: {
  bridge: LoupistBridge;
  loupistId: string;
  entryId: string;
  /** Refills the composer with the question the user asked before setting up. */
  onAskAgain?: () => void;
}) {
  useTranslation("chat");
  const [state, setState] = useState<CardState>({ status: "loading" });
  const [dialogOpen, setDialogOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const refresh = useCallback(async () => {
    try {
      setState(await readState(bridge, loupistId, entryId));
    } catch {
      setState({ status: "unavailable" });
    }
  }, [bridge, loupistId, entryId]);
  useEffect(() => {
    let current = true;
    void readState(bridge, loupistId, entryId).then((next) => { if (current) setState(next); }, () => { if (current) setState({ status: "unavailable" }); });
    return () => { current = false; };
  }, [bridge, loupistId, entryId]);

  const enable = async () => {
    setBusy(true);
    setFailed(false);
    try {
      await bridge.loupistCapabilityEnable({ loupist_id: loupistId, entry_id: entryId });
      await refresh();
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };
  const live = "live" in state ? state.live : undefined;
  const name = live ? connectionText(live.entry.name) : state.status === "unavailable" ? state.name ?? entryId : entryId;
  const closeDialog = () => {
    setDialogOpen(false);
    void refresh();
  };
  return <section className="connection-setup-card" data-state={state.status} data-entry-id={entryId} aria-label={t("connectionSetupCard.label", { name })}>
    <header><strong>{t("connectionSetupCard.title", { name })}</strong></header>
    {state.status === "loading" ? <p className="connection-setup-card-note">{t("connectionSetupCard.loading", { name })}</p> : null}
    {state.status === "unavailable" ? <p>{t("connectionSetupCard.unavailable", { name })}</p> : null}
    {state.status === "needs_setup" ? <>
      <p>{t("connectionSetupCard.needsSetup", { name })}</p>
      <p className="connection-setup-card-note">{t("connectionSetupCard.keyNote")}</p>
      <div className="connection-setup-card-actions">
        <Button type="button" onClick={() => setDialogOpen(true)}>{t("connectionSetupCard.setUp")}</Button>
      </div>
    </> : null}
    {state.status === "off_here" ? <>
      <p>{t("connectionSetupCard.offHere", { name })}</p>
      <div className="connection-setup-card-actions">
        <Button type="button" disabled={busy} onClick={() => void enable()}>{t("connectionSetupCard.useHere")}</Button>
      </div>
    </> : null}
    {state.status === "ready" ? <>
      <p>{t("connectionSetupCard.ready", { name })}</p>
      {onAskAgain ? <div className="connection-setup-card-actions">
        <Button type="button" variant="outline" onClick={onAskAgain}>{t("connectionSetupCard.askAgain")}</Button>
      </div> : null}
    </> : null}
    {failed ? <p role="alert" className="connection-setup-card-error">{t("connectionSetupCard.error")}</p> : null}
    {/* The dialog outlives the state it opened from, so a verified key shows its result before the card settles. */}
    {dialogOpen && live ? <MarketplaceSetupDialog bridge={bridge} entry={live.entry} connector={live.connector} onClose={closeDialog} onChanged={refresh} /> : null}
  </section>;
}
