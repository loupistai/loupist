import { useTranslation } from "../../i18n";
import { currencyDigits, formatNumber, formatPercent } from "../../i18n/format";
import { DIRECTION_GLYPH, priceDirection } from "./direction";

type Props = {
  /** The change as a fraction of the previous price: 0.0126 is +1.26%. */
  percent: number;
  /** The change in price, shown before the percent when given. */
  change?: number;
  /** Sets the decimals of `change` (2 for USD, 0 for KRW). */
  currency?: string;
  size?: "sm" | "md" | "lg";
};

/**
 * A change in the up or down colour (appearance.price_colors). The direction glyph is the
 * sign: the figures after it are magnitudes ("▼ 3.34 · 2.01%"), never an arrow and a minus
 * together. A spoken direction precedes them, so colour is never the only signal.
 */
export function ChangePill({ percent, change, currency, size = "md" }: Props) {
  const { t } = useTranslation("common");
  const digits = currencyDigits(currency);
  // The percent shows two decimals of a percent, four of the fraction; a change too small
  // for that still takes its direction from the price change.
  const byPercent = priceDirection(percent, 4);
  const direction = byPercent === "flat" && change !== undefined ? priceDirection(change, digits) : byPercent;
  const percentText = formatPercent(Math.abs(percent), { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  const text = change === undefined
    ? percentText
    : t("market.changeWithPercent", { change: formatNumber(Math.abs(change), { minimumFractionDigits: digits, maximumFractionDigits: digits }), percent: percentText });
  return (
    <span className="market-change gg-num" data-direction={direction} data-size={size}>
      <span className="market-change-glyph" aria-hidden="true">{DIRECTION_GLYPH[direction]}</span>
      <span className="sr-only">{t(`market.direction.${direction}`)} </span>
      <span>{text}</span>
    </span>
  );
}
