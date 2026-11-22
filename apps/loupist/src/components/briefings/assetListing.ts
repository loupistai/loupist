import i18n, { type resources } from "@/i18n";
import type { AssetReferenceFields } from "../../contracts/assets";
import { listingExchange } from "../assets/listingExchange";

type SecurityType = keyof typeof resources.en.briefings.following.securityType;
const types = new Map<string, SecurityType>([
  ["common stock", "commonStock"],
  ["preferred stock", "preferredStock"], ["preference", "preferredStock"],
  ["etf", "etf"], ["exchange traded fund", "etf"], ["exchange traded funds", "etf"],
  ["etp", "etp"], ["etf/etp", "etp"], ["exchange traded product", "etp"],
  ["etn", "etn"],
  ["adr", "americanDepositaryReceipt"], ["gdr", "globalDepositaryReceipt"],
  ["depositary receipt", "depositaryReceipt"], ["depositary receipts", "depositaryReceipt"],
  ["canadian dr", "canadianDepositaryReceipt"], ["cedear", "cedear"],
  ["reit", "reit"], ["fund", "fund"], ["mutual fund", "mutualFund"],
  ["index", "index"], ["equity index", "equityIndex"],
  ["bond", "bond"], ["bond/note", "bond"], ["note", "note"],
  ["priv placement", "privatePlacement"],
  ["warrant", "warrant"], ["right", "right"], ["unit", "unit"],
  ["unit investment trust", "unitInvestmentTrust"], ["partnership shares", "partnershipShares"],
  ["future", "future"], ["single stock future", "singleStockFuture"],
  ["option", "option"], ["equity option", "equityOption"],
  ["us domestic", "usDomesticBond"], ["canadian", "canadianBond"],
]);

/** Format generic listing labels with the interface language, keeping the provider's reference fields intact. */
export function listingLabel(language: string) {
  const t = i18n.getFixedT(language, "briefings");
  return (fields: AssetReferenceFields) => {
    const { exchange } = listingExchange(fields, language);
    const type = fields.securityType?.trim();
    const key = type && types.get(type.toLowerCase());
    return [fields.ticker?.trim(), exchange, key ? t(`following.securityType.${key}`) : type].filter(Boolean).join(" · ") || null;
  };
}
