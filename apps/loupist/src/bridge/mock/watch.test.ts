import type { WatchAsset } from "../../contracts/watch";
import { mockWatchKinds, MockWatch } from "./watch";

const equity = (assetId: string, name: string): WatchAsset => ({ assetId, symbol: assetId.toUpperCase(), market: "NASDAQ", name, revision: 1, kinds: mockWatchKinds("NASDAQ") });
const kis = { id: "koreainvestment.market-data", name: "Korea Investment" };

it("applies Watch all like the host: undecided assets get the class defaults even without their connection, decided ones keep their switches", async () => {
  const watch = new MockWatch(async () => []);
  const decided = equity("aaa", "Already decided");
  const fresh = { ...equity("bbb", "Fresh"), kinds: mockWatchKinds("NASDAQ").map(kind => kind.kind === "dividends" ? { ...kind, needsConnection: kis } : kind) };
  watch.install("loupist", { consentOff: true, notWatched: [decided, fresh] });
  // The user looked at the first asset's filings and left them off: that is a decision.
  await watch.watchSettingSet({ loupistId: "loupist", assetId: "aaa", kind: "filings", enabled: false, expectedRevision: 1 });
  // Both are listed as not watched, but only the untouched one is undecided.
  expect((await watch.watchSettingsList({ loupistId: "loupist" })).undecidedAssetIds).toEqual(["bbb"]);

  const result = await watch.watchDefaultsApply({ loupistId: "loupist" });
  const settings = await watch.watchSettingsList({ loupistId: "loupist" });
  expect(settings.consentOff).toBe(false);
  expect(settings.undecidedAssetIds).toEqual([]);
  expect(settings.notWatched.map(asset => asset.assetId)).toEqual(["aaa"]);
  expect(settings.notWatched[0]!.kinds.every(kind => !kind.enabled)).toBe(true);
  const watched = settings.watched.find(asset => asset.assetId === "bbb")!;
  expect(watched.kinds.filter(kind => kind.enabled).map(kind => kind.kind)).toEqual(["filings", "results", "dividends"]);
  expect(watched.revision).toBe(2);
  expect(result).toEqual({ watchedAssets: 1, needsConnections: [{ kind: "dividends", connection: kis, assetIds: ["bbb"] }] });
  // A second Watch all finds nothing undecided and changes nothing.
  await watch.watchDefaultsApply({ loupistId: "loupist" });
  expect((await watch.watchSettingsList({ loupistId: "loupist" })).notWatched.map(asset => asset.assetId)).toEqual(["aaa"]);
});

it("lists what is followed now, each undecided until chosen, as the host does", async () => {
  let followed = [{ symbol: "AAPL", market: "NASDAQ", displayName: "Apple", addedAtMs: 1 }, { symbol: "MSFT", market: "NASDAQ", displayName: "Microsoft", addedAtMs: 2 }];
  const watch = new MockWatch(async () => followed);
  const first = await watch.watchSettingsList({ loupistId: "loupist" });
  expect(first.notWatched.map(asset => asset.symbol)).toEqual(["AAPL", "MSFT"]);
  expect(first.undecidedAssetIds).toEqual(first.notWatched.map(asset => asset.assetId));
  const apple = first.notWatched[0]!;
  await watch.watchSettingSet({ loupistId: "loupist", assetId: apple.assetId, kind: "filings", enabled: false, expectedRevision: 1 });
  followed = followed.slice(1);
  const after = await watch.watchSettingsList({ loupistId: "loupist" });
  expect(after.notWatched.map(asset => asset.symbol)).toEqual(["MSFT"]);
  expect(after.undecidedAssetIds).toEqual([after.notWatched[0]!.assetId]);
});
