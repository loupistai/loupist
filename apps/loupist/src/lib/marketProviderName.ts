import { connectionText } from "@/lib/connectionCopy";

/**
 * Market-data providers by the id the host records ("yfinance"), as their owners write
 * them. Brand names are not translated. A connection name the host already resolved
 * ("KRX", "OpenDART") is not in this list and passes through `connectionText`.
 */
const PROVIDERS: Record<string, string> = {
  yfinance: "Yahoo Finance",
  fmp: "Financial Modeling Prep",
  polygon: "Polygon.io",
  intrinio: "Intrinio",
  tiingo: "Tiingo",
  alpha_vantage: "Alpha Vantage",
  benzinga: "Benzinga",
  cboe: "Cboe",
  nasdaq: "Nasdaq",
  tmx: "TMX",
  tradier: "Tradier",
  sec: "SEC EDGAR",
  fred: "FRED",
  federal_reserve: "Federal Reserve",
  ecb: "European Central Bank",
  oecd: "OECD",
  imf: "IMF",
  eia: "EIA",
  cftc: "CFTC",
  finra: "FINRA",
  deribit: "Deribit",
  defillama: "DefiLlama",
  coingecko: "CoinGecko",
  coinbase: "Coinbase",
  binance: "Binance",
  kraken: "Kraken",
  upbit: "Upbit",
  bithumb: "Bithumb",
  krx: "KRX",
  opendart: "OpenDART",
};

/** A bare runtime id ("yfinance", "newsfeed_v2", "openbb.platform"), as opposed to a name a person wrote. */
const RUNTIME_ID = /^[a-z0-9]+(?:[._-][a-z0-9]+)*$/u;

/**
 * A provider's display name for a source line: a known id by its owner's name, a name the
 * host already wrote as given. A runtime id nobody named is undefined: it never reaches the screen.
 */
export function providerName(source: string | undefined): string | undefined {
  const id = source?.trim();
  if (!id) return undefined;
  const named = PROVIDERS[id.toLowerCase()] ?? connectionText(id);
  return named === id && RUNTIME_ID.test(id) ? undefined : named;
}
