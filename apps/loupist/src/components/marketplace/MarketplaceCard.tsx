import { connectionText } from "./connectionCopy";
import i18n, { useTranslation } from "@/i18n";
import { ChevronRightIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { MarketplaceConnectorStatus, MarketplaceEntry, MarketplaceFreeState } from "../../marketplace/types";
import { CONNECTION_STATES, connectionState } from "../../marketplace/readiness";

const FREE_STATE_LABELS: Record<MarketplaceFreeState, string> = {
  get keyless() { return i18n.t("connections:marketplaceCard.noApiKeyRequired"); },
  get free_account() { return i18n.t("connections:marketplaceCard.freeAccountRequired"); },
  get paid() { return i18n.t("connections:marketplaceCard.paid"); },
};

/** A keyless source that still asks for a contact email says so instead of "No API key required". */
export function accessLabel(entry: MarketplaceEntry): string {
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

export function MarketplaceCard({ entry, connector, onSetup, headingLevel = 4 }: {
  entry: MarketplaceEntry;
  connector?: MarketplaceConnectorStatus;
  onSetup?: (entry: MarketplaceEntry) => void;
  headingLevel?: 3 | 4;
}) {
  const Title = `h${headingLevel}` as const;
  useTranslation();
  const action = setupActionLabel(entry, connector);
  const state = CONNECTION_STATES[connectionState(connector)];
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
      </div>
      <div className="marketplace-entry-actions">
        <span className="connection-status" data-tone={state.tone}>
          <span aria-hidden="true" />{state.label}
        </span>
        {onSetup ? <Button type="button" size="sm" variant="ghost" aria-label={setupEntryLabel(entry, connector)} onClick={() => onSetup(entry)}>{action}<ChevronRightIcon /></Button> : null}
      </div>
    </article>
  );
}
