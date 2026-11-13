import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { WatchAllOffer } from "./WatchAllOffer";

it("watches everything followed in one press, then links to Following", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0]!.id;
  for (const entry of [{ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" }, { symbol: "MSFT", market: "NASDAQ", displayName: "Microsoft" }]) await bridge.watchlistAdd(entry);
  const apply = vi.spyOn(bridge, "watchDefaultsApply");
  const onOpenFollowing = vi.fn();
  render(<WatchAllOffer bridge={bridge} loupistId={loupistId} onOpenFollowing={onOpenFollowing} />);
  await user.click(await screen.findByRole("button", { name: "Watch all 2" }));
  expect(apply).toHaveBeenCalledWith({ loupistId });
  expect(await screen.findByRole("status")).toHaveTextContent("Watching 2 names. Change it in Following.");
  await user.click(screen.getByRole("button", { name: "Following" }));
  expect(onOpenFollowing).toHaveBeenCalledOnce();
});

it("says which kinds still need a data connection after Watch all", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0]!.id;
  bridge.watch.install(loupistId, { notWatched: [{
    assetId: "asset-samsung", symbol: "005930", market: "KRX", name: "Samsung Electronics", revision: 1,
    kinds: [{ kind: "filings", enabled: false, needsConnection: { id: "dart", name: "OpenDART" } }, { kind: "results", enabled: false }],
  }] });
  const onOpenConnections = vi.fn();
  render(<WatchAllOffer bridge={bridge} loupistId={loupistId} onOpenConnections={onOpenConnections} />);
  await user.click(await screen.findByRole("button", { name: "Watch Samsung Electronics" }));
  expect(await screen.findByText(/Filings: needs OpenDART/)).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Data connections" }));
  expect(onOpenConnections).toHaveBeenCalledOnce();
});

it("offers only followed assets with no decision yet and hides when every one is decided", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0]!.id;
  const asset = (assetId: string, name: string) => ({ assetId, symbol: assetId.toUpperCase(), market: "NASDAQ", name, revision: 2, kinds: [{ kind: "filings" as const, enabled: false }] });
  // Nothing is watched, but the user switched one of the three fully off.
  bridge.watch.install(loupistId, { notWatched: [asset("aaa", "Off on purpose"), asset("bbb", "New"), asset("ccc", "Newer")], undecidedAssetIds: ["bbb", "ccc"] });
  const { unmount } = render(<WatchAllOffer bridge={bridge} loupistId={loupistId} />);
  expect(await screen.findByRole("button", { name: "Watch all 2" })).toBeVisible();
  unmount();
  bridge.watch.install(loupistId, { notWatched: [asset("aaa", "Off on purpose")], undecidedAssetIds: [] });
  const list = vi.spyOn(bridge, "watchSettingsList");
  render(<WatchAllOffer bridge={bridge} loupistId={loupistId} />);
  await vi.waitFor(() => expect(list).toHaveBeenCalled());
  await act(async () => { await list.mock.results[0]!.value; });
  expect(screen.queryByRole("button", { name: /^Watch/ })).toBeNull();
});
