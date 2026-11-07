import i18n from "@/i18n";
import type { MarketplaceConnectorStatus, MarketplaceEntry } from "./types";

export type ConnectionState = "ready" | "not_connected" | "needs_attention" | "unavailable";
export type ConnectionTone = "neutral" | "positive" | "warning" | "negative";

/** What a user sees for each state. Only `needs_attention` is a problem to fix. */
export const CONNECTION_STATES: Record<
  ConnectionState,
  { label: string; tone: ConnectionTone; needsAttention: boolean }
> = {
  ready: { get label() { return i18n.t("connections:state.ready"); }, tone: "positive", needsAttention: false },
  not_connected: { get label() { return i18n.t("connections:state.notConnected"); }, tone: "neutral", needsAttention: false },
  needs_attention: { get label() { return i18n.t("connections:state.needsAttention"); }, tone: "warning", needsAttention: true },
  unavailable: { get label() { return i18n.t("connections:state.unavailable"); }, tone: "negative", needsAttention: false },
};

/**
 * The host reports `needs_configuration` both for an entry nobody set up and
 * for one whose key was rejected. Only the second is a problem: it has a stored
 * key, an error, or a saved setting that no longer validates.
 */
export function connectionState(connector?: MarketplaceConnectorStatus): ConnectionState {
  if (!connector) return "not_connected";
  if (connector.readiness === "runtime_unavailable") return "unavailable";
  if (connector.readiness === "ready") return "ready";
  const savedConfig = Object.values(connector.config).some((value) => value.trim() !== "");
  const failing =
    connector.credentials.some((credential) => credential.stored || credential.last_error) ||
    (connector.config_state === "missing" && savedConfig);
  return failing ? "needs_attention" : "not_connected";
}

/** Ready entries first, entries that need no setup ahead of the rest, then by display name. */
export function sortConnectionEntries<T extends MarketplaceEntry>(
  entries: readonly T[],
  connectors: ReadonlyMap<string, MarketplaceConnectorStatus>,
  displayName: (entry: T) => string,
): T[] {
  const collator = new Intl.Collator(i18n.language, { numeric: true, sensitivity: "base" });
  const rank = (entry: T) => (connectors.get(entry.id)?.readiness === "ready" ? 0 : 1);
  return [...entries].sort(
    (left, right) =>
      rank(left) - rank(right) ||
      Number(Boolean(left.setup)) - Number(Boolean(right.setup)) ||
      collator.compare(displayName(left), displayName(right)),
  );
}

export function unavailableCapabilityNote(
  connector?: MarketplaceConnectorStatus,
): string {
  if (connector?.readiness === "runtime_unavailable") {
    return i18n.t("connections:readiness.unavailableNote");
  }
  return i18n.t("connections:readiness.setupNote");
}
