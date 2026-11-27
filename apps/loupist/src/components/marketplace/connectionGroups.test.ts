import i18n from "../../i18n";
import { connectionGroups, followedMarket } from "../../marketplace/groups";

afterEach(async () => { await i18n.changeLanguage("en"); });

const entry = (patch: Partial<{ id: string; name: string; free_state: string; capabilities: string[]; markets: string[]; featured: boolean }>) => ({
  plugin: patch.id ?? patch.name ?? "provider",
  name: patch.name ?? patch.id ?? "Provider",
  free_state: patch.free_state ?? "paid",
  capabilities: patch.capabilities ?? [],
  markets: patch.markets ?? [],
  featured: patch.featured ?? false,
});

it("groups providers by key, filings and market data, then the rest, with a count", async () => {
  const entries = [
    entry({ name: "Zeta", free_state: "keyless" }),
    entry({ name: "Alpha", free_state: "free_account", capabilities: ["filing.submissions"] }),
    entry({ name: "Beta", free_state: "paid", capabilities: ["news.articles"] }),
  ];
  expect(connectionGroups([], entries).map(group => group.id)).toEqual(["keyless", "filings", "more"]);
  expect(connectionGroups([], entries).map(group => group.label)).toEqual(["Works without a key", "Filings and market data", "More providers"]);
  await i18n.changeLanguage("ko");
  expect(connectionGroups([], entries).map(group => group.label)).toEqual(["키 없이 사용", "공시와 시장 데이터", "추가 제공자"]);
});

it("orders a group by followed markets, featured, then filings, market data and name", () => {
  const markets = new Map([["United States", 3], ["South Korea", 1]]);
  const entries = [
    entry({ name: "Zulu", free_state: "free_account", capabilities: ["macro.series"], markets: ["Global"] }),
    entry({ name: "Alpha", free_state: "free_account", capabilities: ["market.quote"], featured: true }),
    entry({ name: "Korea filings", free_state: "free_account", capabilities: ["filing.submissions"], markets: ["South Korea"] }),
    entry({ name: "US quotes", free_state: "free_account", capabilities: ["market.quote"], markets: ["United States"] }),
  ];
  expect(connectionGroups([], entries, markets)[0]?.entries.map(item => item.name)).toEqual(["US quotes", "Korea filings", "Alpha", "Zulu"]);
});

it("reads an asset market from its exchange's English region and ignores a blank exchange", () => {
  expect(followedMarket("US")).toBe("United States");
  expect(followedMarket("KS")).toBe("South Korea");
  expect(followedMarket("")).toBeNull();
});
