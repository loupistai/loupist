import { formatPrice } from "../../i18n/format";

type Props = {
  value: number;
  /** ISO 4217 code; the price is written as the interface locale writes that currency. */
  currency?: string;
  /** Decimals to show in place of the currency's own, or "auto" for a saved price in its stated currency or bounded decimal precision. */
  digits?: number | "auto";
  /** body: a row's price; title: a card's headline; figure: the 38 px headline price of an asset. */
  size?: "body" | "title" | "figure";
  className?: string;
};

/** A price with tabular figures, formatted through `Intl` for the interface locale. */
export function PriceText({ value, currency, digits, size = "body", className }: Props) {
  return (
    <span className={className ? `market-price gg-num ${className}` : "market-price gg-num"} data-size={size}>
      {formatPrice(value, currency, { digits })}
    </span>
  );
}
