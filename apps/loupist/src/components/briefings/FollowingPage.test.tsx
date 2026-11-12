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
  expect(screen.getByText("Or tell your Loupist in Home: “follow NVDA”.")).toBeVisible();
});

it("follows a bare six-digit code on KRX and notes the Map is still adding it", async () => {
  const { bridge, user } = await fixture(async bridge => { vi.spyOn(bridge, "knowledgeFollowedStatus").mockResolvedValue({ state: "queued", assets: 1 }); });
  const add = vi.spyOn(bridge, "watchlistAdd");
  await user.type(screen.getByRole("textbox", { name: "Add something to follow" }), "005930");
  await user.click(screen.getByRole("button", { name: "Follow" }));
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
  // With holdings imported, the action reads Update….
  await user.click(screen.getByRole("button", { name: "Update…" }));
  expect(await screen.findByRole("dialog")).toBeVisible();
});

it("lists every followed asset with a Watch-for box per kind that saves on change", async () => {
  const { bridge, loupist, user } = await fixture(async bridge => { await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" }); });
  const set = vi.spyOn(bridge, "watchSettingSet");
  const box = await screen.findByRole("checkbox", { name: "Filings, Apple" });
  expect(box).not.toBeChecked();
  await user.click(box);
  expect(set).toHaveBeenCalledWith(expect.objectContaining({ loupistId: loupist.id, kind: "filings", enabled: true }));
  await waitFor(() => expect(screen.getByRole("checkbox", { name: "Filings, Apple" })).toBeChecked());
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

it("says when Watching settings could not load and loads them again on retry", async () => {
  const { user } = await fixture(async bridge => {
    await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
    vi.spyOn(bridge, "watchSettingsList").mockRejectedValueOnce(new Error("store busy"));
  });
  const alert = await screen.findByRole("alert");
  expect(alert).toHaveTextContent("Couldn't load Watching settings.");
  await user.click(within(alert).getByRole("button", { name: "Try again" }));
  expect(await screen.findByRole("checkbox", { name: "Filings, Apple" })).toBeVisible();
  expect(screen.queryByRole("alert")).toBeNull();
});

it("drops the Adding to the map note once the Map has read what was followed", async () => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  try {
    const status = vi.fn().mockResolvedValueOnce({ state: "queued", assets: 1 }).mockResolvedValueOnce({ state: "running", assets: 1 }).mockResolvedValue({ state: "done", assets: 1 });
    const { user } = await fixture(async bridge => { vi.spyOn(bridge, "knowledgeFollowedStatus").mockImplementation(status); });
    await user.type(screen.getByRole("textbox", { name: "Add something to follow" }), "005930");
    await user.click(screen.getByRole("button", { name: "Follow" }));
    expect(await screen.findByText("Adding to the map…")).toBeVisible();
    await vi.advanceTimersByTimeAsync(3_000);
    expect(screen.getByText("Adding to the map…")).toBeVisible();
    await vi.advanceTimersByTimeAsync(3_000);
    await waitFor(() => expect(screen.queryByText("Adding to the map…")).toBeNull());
    expect(status).toHaveBeenCalledTimes(3);
  } finally {
    vi.useRealTimers();
  }
});
