import english from "@/i18n/locales/en/connections.json";
import korean from "@/i18n/locales/ko/connections.json";
import i18n from "@/i18n";
import { connectionText } from "./connectionCopy";

type Connector = { id: string; name: string; summary: string };
const connectors = Object.values(
  import.meta.glob<Connector>("../../../marketplace/plugins/*/connectors/*.json", { eager: true, import: "default" }),
);
const englishCatalog: Record<string, string> = english.catalog;
const koreanCatalog: Record<string, string> = korean.catalog;

afterEach(async () => { await i18n.changeLanguage("en"); });

describe("bundled connector copy", () => {
  it("has an English catalog entry for every connector summary, so editing the JSON cannot silently lose a translation", () => {
    const known = new Set(Object.values(englishCatalog));
    expect(connectors.filter((connector) => !known.has(connector.summary)).map((connector) => connector.id)).toEqual([]);
  });

  it("keeps catalog names in step with the connector JSON", () => {
    const names = new Set(connectors.map((connector) => connector.name));
    const stale = Object.entries(englishCatalog).filter(([key, value]) => key.endsWith("_name") && !names.has(value)).map(([key]) => key);
    expect(stale).toEqual([]);
  });

  it("translates every summary into Korean", async () => {
    await i18n.changeLanguage("ko");
    for (const connector of connectors) {
      const key = Object.keys(englishCatalog).find((candidate) => englishCatalog[candidate] === connector.summary)!;
      expect(koreanCatalog[key], connector.id).toBeTruthy();
      expect(connectionText(connector.summary), connector.id).toBe(koreanCatalog[key]);
    }
  });

  it("names no implementation in user-visible English or Korean copy", () => {
    const visible = [...Object.values(englishCatalog), ...Object.values(koreanCatalog)];
    expect(visible.filter((text) => /openbb|runtime|endpoint|mcp\b/i.test(text))).toEqual([]);
  });
});
