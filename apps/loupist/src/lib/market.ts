/** The stored market of an asset followed without one (Rust `UNKNOWN_MARKET`). */
const UNKNOWN_MARKET = "UNKNOWN";

/**
 * The market to show beside a ticker, or null when none is known. An asset
 * followed by ticker alone ("NVDA") is stored under `UNKNOWN`; that word is a
 * storage key, never a label, so the ticker stands alone.
 */
export function shownMarket(market: string | null | undefined): string | null {
  const trimmed = market?.trim() ?? "";
  return trimmed && trimmed.toUpperCase() !== UNKNOWN_MARKET ? trimmed : null;
}

/** "NVDA · NASDAQ", or "NVDA" when the market is not known. */
export function tickerLabel(symbol: string, market: string | null | undefined): string {
  const shown = shownMarket(market);
  return shown ? `${symbol} · ${shown}` : symbol;
}
