import { connectionText } from "@/lib/connectionCopy";
import i18n, { useTranslation } from "@/i18n";
import { ChevronRightIcon } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { MarketplaceConnectorStatus, MarketplaceEntry, MarketplaceFreeState } from "../../marketplace/types";
import { CONNECTION_STATES, connectionState } from "../../marketplace/readiness";

const FREE_STATE_LABELS: Record<MarketplaceFreeState, string> = {
  get keyless() { return i18n.t("connections:marketplaceCard.noApiKeyRequired"); },
  get free_account() { return i18n.t("connections:marketplaceCard.freeAccountRequired"); },
  get paid() { return i18n.t("connections:marketplaceCard.paid"); },
};

export function accessLabel(entry: MarketplaceEntry): string {
  if (entry.id === "koreainvestment.market-data" || entry.id === "tradier.market-data") return i18n.t("connections:marketplaceCard.brokerageRequired");
  if (entry.id === "biztoc.news") return i18n.t("connections:marketplaceCard.freeTierRequiresCard");
  const setup = entry.setup;
  const emailOnly = entry.free_state === "keyless" && setup && !setup.credential_fields.length
    && setup.config_fields.some((field) => field.kind === "email" && field.required);
  return emailOnly ? i18n.t("connections:marketplaceCard.contactEmailOnly") : FREE_STATE_LABELS[entry.free_state];
}

function setupActionLabel(entry: MarketplaceEntry, connector?: MarketplaceConnectorStatus) {
  if (!entry.setup || connector?.readiness === "runtime_unavailable") return i18n.t("connections:marketplaceCard.details");
  if (entry.id === "community.web-research") return i18n.t("connections:marketplaceCard.settings");
  if (connector?.readiness === "ready") return i18n.t("connections:marketplaceCard.manage");
  if (connector?.config_state === "valid" || connector?.credentials.some((credential) => credential.stored)) return i18n.t("connections:marketplaceCard.continueSetup");
  return i18n.t("connections:marketplaceCard.setUp");
}

export function setupEntryLabel(entry: MarketplaceEntry, connector?: MarketplaceConnectorStatus) {
  const name = connectionText(entry.name);
  if (!entry.setup || connector?.readiness === "runtime_unavailable") return i18n.t("connections:marketplaceCard.detailsEntry", { name });
  if (entry.id === "community.web-research") return i18n.t("connections:marketplaceCard.settingsEntry", { name });
  if (connector?.readiness === "ready") return i18n.t("connections:marketplaceCard.manageEntry", { name });
  if (connector?.config_state === "valid" || connector?.credentials.some((credential) => credential.stored)) return i18n.t("connections:marketplaceCard.continueEntry", { name });
  return i18n.t("connections:marketplaceCard.setupEntry", { name });
}

export function MarketplaceCard({ entry, connector, offFor, onSetup, onUnlock, headingLevel = 4 }: {
  entry: MarketplaceEntry;
  connector?: MarketplaceConnectorStatus;
  /** The current Loupist's name when this ready connection is switched off for it. */
  offFor?: string;
  onSetup?: (entry: MarketplaceEntry) => void;
  /** Try again on a key macOS refused; resolves false when it is still locked. */
  onUnlock?: (entry: MarketplaceEntry) => Promise<boolean>;
  headingLevel?: 3 | 4;
}) {
  const Title = `h${headingLevel}` as const;
  useTranslation();
  const [unlocking, setUnlocking] = useState(false);
  const [stillLocked, setStillLocked] = useState(false);
  const action = setupActionLabel(entry, connector);
  const locked = connectionState(connector) === "locked";
  const state = CONNECTION_STATES[connectionState(connector)];
  const tryAgain = async () => {
    if (!onUnlock || unlocking) return;
    setUnlocking(true);
    try { setStillLocked(!(await onUnlock(entry))); } catch { setStillLocked(true); } finally { setUnlocking(false); }
  };
  return (
    <article className="marketplace-card" data-testid="marketplace-card" aria-labelledby={`connection-${entry.id}`}>
      <div className="marketplace-entry-copy">
        <div className="marketplace-entry-title">
          <Title id={`connection-${entry.id}`}>{connectionText(entry.name)}</Title>
          {entry.release_stage === "preview" ? <Badge variant="outline">{i18n.t("connections:marketplaceCard.preview")}</Badge> : null}
        </div>
        <p>{connectionText(entry.summary)}</p>
        <div className="marketplace-entry-meta">
          <span>{accessLabel(entry)}</span>
          {entry.markets.length ? <span>{entry.markets.map(connectionText).join(" · ")}</span> : null}
        </div>
        {/* Said before Try again, the read that makes macOS ask for the login password. */}
        {locked ? <div className="marketplace-locked" data-connection-locked>
          <p>{i18n.t("connections:marketplaceCard.lockedExplanation")}</p>
          {stillLocked ? <p role="status">{i18n.t("connections:marketplaceCard.stillLocked")}</p> : null}
          {onUnlock ? <Button type="button" size="sm" variant="outline" disabled={unlocking} aria-label={i18n.t("connections:marketplaceCard.tryAgainEntry", { name: connectionText(entry.name) })} onClick={() => void tryAgain()}>{i18n.t("connections:marketplaceCard.tryAgain")}</Button> : null}
        </div> : null}
      </div>
      <div className="marketplace-entry-actions">
        {/* Twenty grey "Not connected" labels say nothing: only Ready and what needs attention are marked. */}
        {state.tone === "neutral" ? null : <span className="marketplace-entry-state">
          <span className="connection-status" data-tone={state.tone}>
            <span aria-hidden="true" />{state.label}
          </span>
          {/* Ready is set up for every Loupist; this one may still have it off, which is why a page sent the user here. */}
          {offFor && connectionState(connector) === "ready" ? <span className="marketplace-off-here">{i18n.t("connections:marketplaceCard.offFor", { loupist: offFor })}</span> : null}
        </span>}
        {onSetup ? <Button type="button" size="sm" variant="ghost" aria-label={setupEntryLabel(entry, connector)} onClick={() => onSetup(entry)}>{action}<ChevronRightIcon /></Button> : null}
      </div>
    </article>
  );
}
