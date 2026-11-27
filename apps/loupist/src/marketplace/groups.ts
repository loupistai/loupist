import i18n from "../i18n";
import directory from "../../src-tauri/src/finance_data/openfigi/equity-exchanges.json";

export type ConnectionGroup<T> = {
  id: string;
  label: string;
  entries: T[];
};

type Groupable = {
  plugin: string;
  name?: string;
  free_state?: string;
  capabilities?: readonly string[];
  markets?: readonly string[];
  featured?: boolean;
};

const FILING_OR_MARKET = /^(?:filing\.|company\.facts$|market\.(?:quote|history|ohlcv)$|macro\.series$)/u;
const exchanges: Readonly<Record<string, (string | null)[][]>> = directory.exchanges;
const englishRegions = new Intl.DisplayNames("en", { type: "region", fallback: "none" });

/** English region name for a followed asset's listing venue. "Global" is not a market. */
export function followedMarket(exchange: string | null | undefined): string | null {
  const rows = exchanges[exchange?.trim() ?? ""];
  const code = rows?.length === 1 ? rows[0]?.[1] : null;
  if (!code || !/^[A-Z]{2}$/u.test(code)) return null;
  const region = englishRegions.of(code);
  return region && region !== "Global" ? region : null;
}

function groupId(entry: Groupable): "keyless" | "filings" | "more" {
  if (entry.free_state === "keyless") return "keyless";
  const capabilities = entry.capabilities ?? [];
  if (entry.free_state === "free_account" && capabilities.some(capability => FILING_OR_MARKET.test(capability))) return "filings";
  return "more";
}

function capabilityRank(entry: Groupable) {
  const capabilities = entry.capabilities ?? [];
  if (capabilities.some(capability => capability.startsWith("filing.") || capability === "company.facts")) return 0;
  if (capabilities.some(capability => capability.startsWith("market."))) return 1;
  if (capabilities.some(capability => capability.startsWith("macro."))) return 2;
  return 3;
}

/**
 * Three fixed groups. Inside a group: followed markets (more assets first; Global alone
 * does not match), then featured, then filings, market data and macro, then name.
 * Readiness never moves a row. No followed assets skips the market rule.
 */
export function connectionGroups<T extends Groupable>(
  _plugins: ReadonlyArray<{ name: string; interface: { displayName: string }; category?: string }>,
  entries: readonly T[],
  followedMarkets: ReadonlyMap<string, number> = new Map(),
): ConnectionGroup<T>[] {
  const language = i18n.language;
  const collator = new Intl.Collator(language);
  const labels = {
    keyless: i18n.t("connections:catalog.worksWithoutKey"),
    filings: i18n.t("connections:catalog.filingsAndMarket"),
    more: i18n.t("connections:catalog.moreProviders"),
  };
  const groups: ConnectionGroup<T>[] = (["keyless", "filings", "more"] as const).map(id => ({ id, label: labels[id], entries: [] }));
  for (const entry of entries) groups.find(group => group.id === groupId(entry))!.entries.push(entry);
  for (const group of groups) {
    group.entries.sort((left, right) => {
      const weight = (entry: Groupable) => (entry.markets ?? []).reduce((sum, market) => market === "Global" ? sum : sum + (followedMarkets.get(market) ?? 0), 0);
      return (weight(right) - weight(left))
        || Number(!left.featured) - Number(!right.featured)
        || capabilityRank(left) - capabilityRank(right)
        || collator.compare(left.name ?? left.plugin, right.name ?? right.plugin);
    });
  }
  return groups.filter(group => group.entries.length > 0);
}
