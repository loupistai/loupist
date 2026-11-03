import i18n from "@/i18n";
import type {
  MarketplaceConnectorStatus,
} from "./types";

export const CONNECTOR_READINESS_LABELS: Record<
  MarketplaceConnectorStatus["readiness"],
  string
> = {
  get ready() { return i18n.t("connections:readiness.ready"); },
  get needs_configuration() { return i18n.t("connections:readiness.needs_configuration"); },
  get runtime_unavailable() { return i18n.t("connections:readiness.runtime_unavailable"); },
};

export function unavailableCapabilityNote(
  connector?: MarketplaceConnectorStatus,
): string {
  if (connector?.readiness === "runtime_unavailable") {
    return i18n.t("connections:readiness.unavailableNote");
  }
  return i18n.t("connections:readiness.setupNote");
}
