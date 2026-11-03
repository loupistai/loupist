import { mockAssetId } from "../../bridge/mock/assets";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { previewCommodityWatchEvents, previewCommodityWatchSettings, previewCryptoWatchEvents, previewCryptoWatchSettings, previewWatchEvents, previewWatchSettings } from "../../preview/finance";
import type { LoupistBridge, LoupistSummary, WatchSettings } from "../../types";
import { WatchingReader } from "./Watching";

function Reader({ bridge, loupist, initial }: { bridge: LoupistBridge; loupist: LoupistSummary; initial: WatchSettings }) {
  const [settings, setSettings] = useState(initial);
  return <WatchingReader bridge={bridge} loupist={loupist} settings={settings} onSettings={setSettings} onBack={vi.fn()} />;
}

async function reader(options: { paused?: boolean; events?: boolean } = {}) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  bridge.watch.install(loupist!.id, previewWatchSettings({ paused: options.paused }), options.events === false ? [] : previewWatchEvents());
  const settings = await bridge.watchSettingsList({ loupistId: loupist!.id });
  render(<Reader bridge={bridge} loupist={loupist!} initial={settings} />);
  const article = screen.getByRole("article", { name: "Watching" });
  return { bridge, loupist: loupist!, article, user: userEvent.setup() };
}

it("shows per-kind checks as a read-only table and keeps holdings private", async () => {
  const { bridge, article } = await reader();
  expect(within(article).getByText("Your quantities and values never leave this computer. Checks ask data sources only about the names you follow.")).toBeVisible();
  const matrix = within(article).getByRole("table", {name:"Watching"});
  expect(within(matrix).getAllByRole("columnheader").map(header => header.textContent)).toEqual(["Asset", "Filings", "Results", "Insiders", "5% holders", "Flows", "Dividends", "Last alert"]);
  expect(within(matrix).queryByRole("checkbox")).toBeNull();
  expect(bridge.watch.setCalls).toHaveLength(0);
});

it("lists recent alerts with their sources and marks them seen when Watching opens", async () => {
  const { bridge, article } = await reader();
  const recent = await within(article).findByRole("region", { name: "Recent alerts" });
  await waitFor(() => expect(within(recent).getAllByRole("listitem")).toHaveLength(6));
  expect(within(recent).getByText(/Morrow Components: 3 insiders bought shares between/)).toBeVisible();
  for (const item of within(recent).getAllByRole("listitem")) expect(item.querySelector(".loupist-citation")).not.toBeNull();
  await waitFor(() => expect(bridge.watch.seenCalls.flat()).toHaveLength(6));
});

it("shows pause state and the first-check explanation without offering manual controls", async () => {
  const { article } = await reader({paused:true,events:false});
  expect(await within(article).findByText("No alerts yet. The first check sets a starting point.")).toBeVisible();
  expect(within(article).getByText("Paused")).toBeVisible();
  expect(within(article).queryByRole("button", {name:"Check now"})).toBeNull();
  expect(within(article).queryByRole("button", {name:"Resume"})).toBeNull();
});

it("gives each asset class its own table, says why a class kind does not apply, and words class alerts", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  const stocks = previewWatchSettings();
  const coins = previewCryptoWatchSettings();
  const futures = previewCommodityWatchSettings();
  const coin = mockAssetId("NOVA", "CRYPTO");
  bridge.watch.install(loupist!.id, {
    ...stocks,
    watched: [...stocks.watched!, ...coins.watched!, ...futures.watched!],
    notWatched: [{ assetId: coin, symbol: "NOVA", market: "CRYPTO", name: "Nova Coin", revision: 1, kinds: [{ kind: "cryptoMove", enabled: false }, { kind: "drawdown", enabled: false }] }],
  }, [...previewCommodityWatchEvents(), ...previewCryptoWatchEvents()]);
  const settings = await bridge.watchSettingsList({ loupistId: loupist!.id });
  render(<Reader bridge={bridge} loupist={loupist!} initial={settings} />);
  const article = screen.getByRole("article", { name: "Watching" });
  const crypto = within(article).getByRole("table", { name: "Watching · Crypto" });
  expect(within(crypto).getAllByRole("columnheader").map(header => header.textContent)).toEqual(["Asset", "Price move", "From high", "Peg", "Value locked", "Last alert"]);
  const commodities = within(article).getByRole("table", { name: "Watching · Commodities" });
  expect(within(commodities).getAllByRole("columnheader").map(header => header.textContent)).toEqual(["Asset", "Price move", "Positioning", "Inventories", "Last alert"]);
  expect(within(article).getByRole("table", { name: "Watching · Stocks" })).toBeVisible();
  expect(within(crypto).queryByRole("checkbox")).toBeNull();
  // Class alerts in plain words, each with its source.
  const recent = await within(article).findByRole("region", { name: "Recent alerts" });
  await waitFor(() => expect(within(recent).getAllByRole("listitem")).toHaveLength(4));
  expect(within(recent).getByText(/^Gold: money managers turned net short \(CFTC, as of /)).toBeVisible();
  expect(within(recent).getByText(/^Copper rose 6\.2% over 7 days \(to /)).toBeVisible();
  expect(within(recent).getByText(/^WTI crude fell 5\.4% over 7 days/)).toBeVisible();
  expect(within(recent).getByText(/^Corvid Token closed 21\.4% below its 90-day high/)).toBeVisible();
  // "Watch" on a coin watched nowhere here turns on the class kinds that apply.
});
