import { connectionText } from "@/lib/connectionCopy";
import { keychainErrorMessage } from "@/lib/errors";
import i18n, { useTranslation } from "@/i18n";
import { ChevronRightIcon } from "lucide-react";
import { useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { MarketplaceConnectorStatus, MarketplaceEntry, MarketplaceFreeState } from "../../marketplace/types";
import { CONNECTION_STATES, connectionState, needsSavedKeyPermission } from "../../marketplace/readiness";

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

export function MarketplaceCard({ entry, connector, offFor, onSetup, onUnlock, permissionBatchActive, headingLevel = 4 }: {
  entry: MarketplaceEntry;
  connector?: MarketplaceConnectorStatus;
  /** The current Loupist's name when this ready connection is switched off for it. */
  offFor?: string;
  onSetup?: (entry: MarketplaceEntry) => void;
  onUnlock?: (entry: MarketplaceEntry, presenceOnly: boolean) => Promise<void>;
  permissionBatchActive?: boolean;
  headingLevel?: 3 | 4;
}) {
  const Title = `h${headingLevel}` as const;
  useTranslation();
  const [unlocking, setUnlocking] = useState(false);
  const [unlockError, setUnlockError] = useState<string | null>(null);
  const action = setupActionLabel(entry, connector);
  const locked = connectionState(connector) === "locked";
  const savedKeyNeedsPermission = needsSavedKeyPermission(connector);
  const savedKeyDenied = connector?.credentials.some(({ last_error }) => last_error === "credential_saved_denied");
  const presenceUnavailable = !locked && connector?.credentials.some(({ last_error }) => last_error === "credential_presence_unavailable");
  const savedKeyUnknown = connector?.credentials.some(({ last_error }) => last_error === "credential_status_unknown");
  const showRetry = locked || presenceUnavailable || savedKeyDenied;
  const checkDetail = locked
    ? i18n.t("connections:marketplaceCard.lockedExplanation")
    : presenceUnavailable
      ? i18n.t("connections:marketplaceCard.couldNotCheckSavedKey")
      : savedKeyDenied
        ? i18n.t("connections:savedKeyPermission.denied")
        : savedKeyNeedsPermission
          ? i18n.t("connections:marketplaceCard.checkSavedKeyDetail")
          : i18n.t("connections:marketplaceCard.checkSavedKeyUnknownDetail");
  const state = CONNECTION_STATES[connectionState(connector)];
  const tryAgain = async () => {
    if (!onUnlock || unlocking) return;
    setUnlocking(true);
    setUnlockError(null);
    try {
      await onUnlock(entry, Boolean(presenceUnavailable));
    } catch (cause) {
      setUnlockError(keychainErrorMessage(cause) ?? i18n.t("connections:marketplaceView.couldNotRefreshConnections"));
    } finally {
      setUnlocking(false);
    }
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
        {showRetry || savedKeyNeedsPermission || savedKeyUnknown ? <div className="marketplace-locked" data-connection-locked={locked || undefined}>
          <p>{checkDetail}</p>
          {unlockError ? <p role="status">{unlockError}</p> : null}
          {onUnlock ? <Button type="button" size="sm" variant="outline" disabled={unlocking || permissionBatchActive} aria-label={i18n.t(showRetry ? "connections:marketplaceCard.tryAgainEntry" : "connections:marketplaceCard.checkSavedKeyEntry", { name: connectionText(entry.name) })} onClick={() => void tryAgain()}>{i18n.t(showRetry ? "connections:marketplaceCard.tryAgain" : "connections:marketplaceCard.checkSavedKey")}</Button> : null}
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
        {onSetup && !savedKeyNeedsPermission && !presenceUnavailable ? <Button type="button" size="sm" variant="ghost" aria-label={setupEntryLabel(entry, connector)} onClick={() => onSetup(entry)}>{action}<ChevronRightIcon /></Button> : null}
      </div>
    </article>
  );
}
