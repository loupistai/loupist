import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import type { LoupistSummary } from "../../types";
import { FollowingPage, parseFollow } from "./FollowingPage";

async function fixture(setup?: (bridge: MockLoupistBridge) => Promise<void>) {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const [loupist] = await bridge.loupistList() as LoupistSummary[];
  await setup?.(bridge);
  render(<FollowingPage bridge={bridge} loupist={loupist!} />);
  return { bridge, loupist: loupist!, user: userEvent.setup() };
}

it("says what Following is and what to add when nothing is followed", async () => {
  await fixture(async bridge => { for (const entry of await bridge.watchlistList()) await bridge.watchlistRemove({ symbol: entry.symbol, market: entry.market }); });
  expect(screen.getByRole("heading", { name: "Following" })).toBeVisible();
  expect(screen.getByText("What you follow is shared by all your Loupists. Each Loupist chooses what to watch for.")).toBeVisible();
  expect(await screen.findByText("Add a company, fund or coin to follow. Following is not a holding or a trade.")).toBeVisible();
  expect(screen.getByText("You can also tell your Loupist “follow NVDA” in Chat.")).toBeVisible();
});

it("follows a bare six-digit code on KRX and notes the Map is still adding it", async () => {
  const { bridge, user } = await fixture(async bridge => { vi.spyOn(bridge, "knowledgeFollowedStatus").mockResolvedValue({ state: "queued", assets: 1 }); });
  const add = vi.spyOn(bridge, "watchlistAdd");
  await user.type(screen.getByRole("textbox", { name: "Add something to follow" }), "005930");
  await user.click(screen.getByRole("button", { name: "+ Add" }));
  expect(add).toHaveBeenCalledWith(expect.objectContaining({ symbol: "005930", market: "KRX" }));
  expect(await screen.findByText("Adding to the map…")).toBeVisible();
});

it("parses what to follow without guessing a market for other symbols", () => {
  expect(parseFollow(" aapl ")).toEqual({ symbol: "AAPL", market: "UNKNOWN" });
  expect(parseFollow("005930")).toEqual({ symbol: "005930", market: "KRX" });
  expect(parseFollow("005930:kosdaq")).toEqual({ symbol: "005930", market: "KOSDAQ" });
});

it("lists follows and holdings as counts only, removes a follow, and imports holdings", async () => {
  const { bridge, user } = await fixture(async bridge => {
    await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
    const preview = await bridge.holdingsImportCsv({ path: "/tmp/holdings.csv" });
    await bridge.holdingsCommitCsv({ previewId: preview.previewId });
  });
  const remove = vi.spyOn(bridge, "watchlistRemove");
  expect(await screen.findByText("Apple · AAPL")).toBeVisible();
  expect(await screen.findByText(/^Stocks and funds: 1 asset · .* · Crypto: none/)).toBeVisible();
  // Quantities and values never reach this page.
  expect(document.body).not.toHaveTextContent("2.5");
  expect(document.body).not.toHaveTextContent("500");
  await user.click(screen.getByRole("button", { name: "Stop following Apple" }));
  expect(remove).toHaveBeenCalledWith({ symbol: "AAPL", market: "NASDAQ" });
  await user.click(screen.getByRole("button", { name: "Import holdings…" }));
  expect(await screen.findByRole("dialog")).toBeVisible();
});

it("watches every followed asset with the class defaults in one step", async () => {
  const { bridge, loupist, user } = await fixture(async bridge => { await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" }); });
  const apply = vi.spyOn(bridge, "watchDefaultsApply");
  const button = await screen.findByRole("button", { name: /^Watch all \d+$/ });
  expect(screen.getByText(/No AI usage\. Only names are sent to data sources\./)).toBeVisible();
  await user.click(button);
  expect(apply).toHaveBeenCalledWith({ loupistId: loupist.id });
  await waitFor(() => expect(screen.queryByRole("button", { name: /^Watch all/ })).toBeNull());
  const watching = screen.getByRole("article", { name: "Watching" });
  expect(within(watching).getAllByRole("table").length).toBeGreaterThan(0);
});
