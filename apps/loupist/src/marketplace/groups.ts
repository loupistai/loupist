import i18n from "../i18n";

export type ConnectionGroup<T> = {
  id: string;
  label: string;
  entries: T[];
};

function connectionGroupLabel(name: string): string {
  const keys: Record<string, "MarketData" | "News" | "Filings" | "Research" | "Korea" | "WebResearch" | "WorldBank"> = { "Market data": "MarketData", News: "News", Filings: "Filings", Research: "Research", Korea: "Korea", "Web Research": "WebResearch", "World Bank": "WorldBank" };
  return keys[name] ? i18n.t(`connections:catalog.${keys[name]}`) : name;
}

export function connectionGroups<T extends { plugin: string }>(
  plugins: ReadonlyArray<{ name: string; interface: { displayName: string }; category?: string }>,
  entries: readonly T[],
): ConnectionGroup<T>[] {
  const regional = new Set(plugins.filter(plugin => plugin.category === "Korea").map(plugin => plugin.name));
  const groups = plugins
    .filter(plugin => !regional.has(plugin.name))
    .map((plugin) => ({
      id: plugin.name,
      label: connectionGroupLabel(plugin.interface.displayName),
      entries: entries.filter((entry) => entry.plugin === plugin.name),
    }))
    .filter((group) => group.entries.length > 0);
  const korea = entries.filter(entry => regional.has(entry.plugin));
  if (korea.length) groups.push({ id: "korea", label: i18n.t("connections:catalog.Korea"), entries: korea });
  return groups;
}
