import i18n from "@/i18n";
import english from "@/i18n/locales/en/connections.json";
import { connectionText } from "@/lib/connectionCopy";

/** The bundled market-data runtime by its English name or by the id the host may report ("openbb.platform"). */
const MARKET_DATA = new Set([english.catalog.openbb_platform_name.toLowerCase(), "openbb.platform", "openbb_platform", "openbb"]);
export const isMarketData = (name: string) => MARKET_DATA.has(name.trim().toLowerCase());

/**
 * A connection a check names, as the Connections tab names it. The bundled market-data
 * runtime is listed there under "Market data", the plain name asset surfaces use.
 */
export const connectionLabel = (name: string) => isMarketData(name) ? i18n.t("connections:catalog.MarketData") : connectionText(name);
