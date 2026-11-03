import { connectionText } from "./connectionCopy";
import i18n, { useTranslation } from "@/i18n";
import { ChevronRightIcon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { MarketplaceConnectorStatus, MarketplaceEntry, MarketplaceFreeState } from "../../marketplace/types";
import { CONNECTOR_READINESS_LABELS } from "../../marketplace/readiness";

export const FREE_STATE_LABELS: Record<MarketplaceFreeState, string> = {
  get keyless() { return i18n.t("connections:marketplaceCard.noApiKeyRequired"); },
  get free_account() { return i18n.t("connections:marketplaceCard.freeAccountRequired"); },
  get paid() { return i18n.t("connections:marketplaceCard.paid"); },
};

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

export function MarketplaceCard({ entry, connector, onSetup }: {
  entry: MarketplaceEntry;
  connector?: MarketplaceConnectorStatus;
  onSetup?: (entry: MarketplaceEntry) => void;
}) {
  useTranslation();
  const action = setupActionLabel(entry, connector);
  const readiness = connector?.readiness ?? "needs_configuration";
  return (
    <article className="marketplace-card" aria-labelledby={`connection-${entry.id}`}>
      <div className="marketplace-entry-copy">
        <div className="marketplace-entry-title">
          <h3 id={`connection-${entry.id}`}>{connectionText(entry.name)}</h3>
          {entry.release_stage === "preview" ? <Badge variant="outline">{i18n.t("connections:marketplaceCard.preview")}</Badge> : null}
        </div>
        <p>{connectionText(entry.summary)}</p>
        <div className="marketplace-entry-meta">
          <span>{FREE_STATE_LABELS[entry.free_state]}</span>
          {entry.markets.length ? <span>{entry.markets.map(connectionText).join(" · ")}</span> : null}
        </div>
      </div>
      <div className="marketplace-entry-actions">
        <span className="connection-status" data-readiness={readiness}>
          <span aria-hidden="true" />{connectionText(CONNECTOR_READINESS_LABELS[readiness])}
        </span>
        {onSetup ? <Button type="button" size="sm" variant="ghost" aria-label={setupEntryLabel(entry, connector)} onClick={() => onSetup(entry)}>{action}<ChevronRightIcon /></Button> : null}
      </div>
    </article>
  );
}
