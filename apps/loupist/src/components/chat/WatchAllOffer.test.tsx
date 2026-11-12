import { render, screen } from "@testing-library/react";
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

it("stays hidden when the host cannot apply watch defaults", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const loupistId = (await bridge.loupistList())[0]!.id;
  await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
  const { container } = render(<WatchAllOffer bridge={Object.assign(Object.create(bridge), { watchDefaultsApply: undefined })} loupistId={loupistId} />);
  await new Promise((resolve) => setTimeout(resolve, 20));
  expect(container).toBeEmptyDOMElement();
});
