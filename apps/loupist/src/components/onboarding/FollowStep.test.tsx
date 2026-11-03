import { useState } from "react";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "@/bridge/mockBridge";
import type { WatchlistAddRequest } from "@/contracts/userContext";
import { FollowStep } from "./FollowStep";

const idle = () => {};
function Harness({ bridge, next = idle }: { bridge: MockLoupistBridge; next?: () => void }) {
  const [chips, setChips] = useState<WatchlistAddRequest[]>([]);
  return <FollowStep bridge={bridge} chips={chips} onChipsChange={setChips} onContinue={next} onBusyChange={idle} />;
}

it("respects composition, uppercases tickers and enforces the 30-chip limit", async () => {
  const user = userEvent.setup(); const bridge = new MockLoupistBridge({ delay_ms: 0 });
  render(<Harness bridge={bridge} />);
  const input = screen.getByRole("textbox", { name: "Companies or tickers" });
  fireEvent.change(input, { target: { value: "aapl:nasdaq" } });
  fireEvent.keyDown(input, { key: "Enter", isComposing: true });
  expect(screen.queryByRole("button", { name: "Remove AAPL" })).toBeNull();
  fireEvent.keyDown(input, { key: "Enter" });
  expect(screen.getByRole("button", { name: "Remove AAPL" })).toBeVisible();
  fireEvent.change(input, { target: { value: Array.from({ length: 30 }, (_, index) => `s${index}`).join(",") } });
  fireEvent.keyDown(input, { key: "Enter" });
  expect(screen.getByRole("alert")).toHaveTextContent("up to 30");
  expect(screen.getAllByRole("button", { name: /^Remove / })).toHaveLength(30);
  await user.click(screen.getByRole("button", { name: "Remove S0" }));
  await waitFor(() => expect(screen.getAllByRole("button", { name: /^Remove / })).toHaveLength(29));
});

it("does not import when the native chooser is cancelled and retains a failed preview for retry", async () => {
  const user = userEvent.setup(); const bridge = new MockLoupistBridge({ delay_ms: 0 });
  vi.spyOn(bridge, "pickFile").mockResolvedValueOnce(null).mockResolvedValueOnce("/mock/holdings.csv");
  const read = vi.spyOn(bridge, "holdingsImportCsv");
  const commit = vi.spyOn(bridge, "holdingsCommitCsv").mockRejectedValueOnce(new Error("Preview expired"));
  render(<Harness bridge={bridge} />);
  await user.click(screen.getByText("Import holdings"));
  await user.click(screen.getByRole("button", { name: "From a CSV file" }));
  expect(read).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "From a CSV file" }));
  await screen.findByRole("table");
  await user.click(screen.getByRole("button", { name: "Confirm import" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Preview expired");
  expect(screen.getByRole("table")).toBeVisible();
  expect(commit).toHaveBeenCalledTimes(1);
  expect(await bridge.watchlistList()).toEqual([]);
});

it("offers Korea Investment only for a ready connector and follows its imported positions", async () => {
  const user = userEvent.setup(); const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const snapshot = await bridge.marketplaceSnapshot();
  snapshot.connectors = [{ entry_id: "koreainvestment.market-data", config: {}, config_state: "valid", credentials: [], readiness: "ready" }];
  vi.spyOn(bridge, "marketplaceSnapshot").mockResolvedValue(snapshot);
  const importKis = vi.spyOn(bridge, "holdingsImportKis").mockResolvedValue({ snapshotId: "kis-test", source: "kis", importedAtMs: 0, currency: "KRW", note: null,
    positions: [{ symbol: "005930", market: "KRX", displayName: "Samsung", quantity: "10", costBasis: null, marketValue: null, currency: "KRW" }] });
  render(<Harness bridge={bridge} />);
  await user.click(screen.getByText("Import holdings"));
  await user.click(await screen.findByRole("button", { name: "From Korea Investment" }));
  await screen.findByText("Holdings imported");
  expect(importKis).toHaveBeenCalledTimes(1);
  await waitFor(async () => expect(await bridge.watchlistList()).toEqual([expect.objectContaining({ symbol: "005930", market: "KRX" })]));

  await user.click(screen.getByRole("button", { name: "Remove 005930" }));
  await waitFor(async () => expect(await bridge.watchlistList()).toEqual([]));
  expect(screen.queryByRole("button", { name: "Remove 005930" })).toBeNull();
});
