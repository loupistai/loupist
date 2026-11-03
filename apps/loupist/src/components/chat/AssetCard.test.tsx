import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MockLoupistBridge } from "../../bridge/mockBridge";
import { ChatExecutionProvider } from "./ChatExecutionContext";
import { AssetCard, resetQuoteRequests } from "./AssetCard";

function renderCard(bridge: MockLoupistBridge, asset: { symbol: string; market?: string }, asOf?: string) {
  return render(<ChatExecutionProvider value={{ bridge, loupistId: "loupist-quality", threadId: "chat", theme: "light", active: true }}>
    <AssetCard asset={asset} loupistId="loupist-quality" onOpenMapNode={vi.fn()} asOf={asOf} />
  </ChatExecutionProvider>);
}

beforeEach(() => resetQuoteRequests());

it("shows the last close Rust reports, with its source and time, and asks once per asset", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
  bridge.assets.installQuote("loupist-quality", "AAPL", "NASDAQ", {
    value: 187.42, currency: "USD", date: "2026-10-28", retrievedAt: "2026-10-29T00:12:00Z", fresh: false,
    evidence: { tool: "equity_price_historical", provider: "yfinance", title: "Yahoo Finance", date: "2026-10-28" },
  });
  const quote = vi.spyOn(bridge, "assetQuote");
  renderCard(bridge, { symbol: "AAPL", market: "NASDAQ" });
  renderCard(bridge, { symbol: "AAPL", market: "NASDAQ" });
  const [card] = await screen.findAllByRole("region", { name: "About Apple" });
  expect(await within(card!).findByText("$187.42")).toBeVisible();
  expect(card).toHaveTextContent("Oct 28 close");
  // The source reads like a citation: its name in the chip, the time it was read in its label.
  const source = within(card!).getByRole("button", { name: /^Price from Yahoo Finance, Oct 29, 2026/u });
  expect(source).toHaveTextContent("Yahoo Finance");
  expect(quote).toHaveBeenCalledTimes(1);
  expect(quote).toHaveBeenCalledWith({ loupistId: "loupist-quality", symbol: "AAPL", market: "NASDAQ" });
});

it("leaves the price out without a quote, and in an as-of answer", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
  const quote = vi.spyOn(bridge, "assetQuote");
  const first = renderCard(bridge, { symbol: "AAPL", market: "NASDAQ" });
  const card = await screen.findByRole("region", { name: "About Apple" });
  await vi.waitFor(() => expect(quote).toHaveBeenCalledTimes(1));
  expect(card.querySelector(".asset-card-price")).toBeNull();
  first.unmount();
  resetQuoteRequests();
  bridge.assets.installQuote("loupist-quality", "AAPL", "NASDAQ", {
    value: 187.42, currency: "USD", date: "2026-10-28", fresh: true, evidence: { tool: "equity_price_historical", title: "Yahoo Finance" },
  });
  renderCard(bridge, { symbol: "AAPL", market: "NASDAQ" }, "2026-06-30");
  const dated = await screen.findByRole("region", { name: "About Apple" });
  expect(dated).toHaveTextContent("as of");
  expect(quote).toHaveBeenCalledTimes(1);
  expect(dated.querySelector(".asset-card-price")).toBeNull();
});

it("shows what this computer knows about the named asset and never a quantity", async () => {
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  await bridge.watchlistAdd({ symbol: "AAPL", market: "NASDAQ", displayName: "Apple" });
  renderCard(bridge, { symbol: "AAPL", market: "NASDAQ" });
  const card = await screen.findByRole("region", { name: "About Apple" });
  expect(card).toHaveTextContent("AAPL · NASDAQ");
  expect(card).toHaveTextContent("Watching");
  expect(card.textContent).not.toMatch(/\d+(\.\d+)? (shares|units)/u);
  expect(screen.queryByRole("button", { name: "Add to what you follow" })).not.toBeInTheDocument();
});

it("names the class for crypto and offers to follow an asset the user does not watch", async () => {
  const user = userEvent.setup();
  const bridge = new MockLoupistBridge({ delay_ms: 0 });
  const add = vi.spyOn(bridge, "watchlistAdd");
  renderCard(bridge, { symbol: "BTC", market: "UPBIT" });
  const card = await screen.findByRole("region", { name: "About BTC" });
  // With no name of its own, the symbol is the title and shows once, followed by the class.
  const title = card.querySelector(".asset-card-title")!.textContent ?? "";
  expect(title.match(/BTC/gu)).toHaveLength(1);
  expect(title).toContain("Crypto");
  expect(card).toHaveTextContent("Not on your map yet");
  await user.click(screen.getByRole("button", { name: "Add to what you follow" }));
  // The open Loupist watches what the user follows from here with the default kinds.
  expect(add).toHaveBeenCalledWith({ symbol: "BTC", market: "UPBIT", displayName: null, loupistId: "loupist-quality" });
  expect(await screen.findByText(/Watching/)).toBeVisible();
});
