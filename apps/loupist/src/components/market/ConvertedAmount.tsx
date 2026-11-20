import { useTranslation } from "@/i18n";
import { currencyDigits, currencyName, formatDay, formatNumber } from "@/i18n/format";
import { useSharedDisplayFx, type DisplayFx } from "../../app/useDisplayFx";

const ISO_CODE = /^[A-Z]{3}$/u;

/** A finite number as the decimal it prints as: digits × 10^-scale ("0.7" → 7, 1; "6.14e-5" → 614, 7). */
function decimal(value: number) {
  const [mantissa = "0", exponent = "0"] = String(Math.abs(value)).split("e");
  const [whole = "0", fraction = ""] = mantissa.split(".");
  let digits = BigInt(`${whole}${fraction}`);
  let scale = fraction.length - Number(exponent);
  if (scale < 0) { digits *= 10n ** BigInt(-scale); scale = 0; }
  return { digits: value < 0 ? -digits : digits, scale };
}

/**
 * price × rate on their decimal representations, rounded once (half away from zero) to
 * `places` decimals. Binary multiplication would put an exact half just below it
 * (0.7 × 1355 = 948.4999…), so the product is never rounded from a float.
 */
function multiplyRounded(price: number, rate: number, places: number) {
  const a = decimal(price), b = decimal(rate);
  let digits = a.digits * b.digits;
  const scale = a.scale + b.scale;
  if (scale > places) {
    const divisor = 10n ** BigInt(scale - places);
    const remainder = digits % divisor;
    digits /= divisor;
    const twice = (remainder < 0n ? -remainder : remainder) * 2n;
    if (twice >= divisor) digits += remainder < 0n ? -1n : 1n;
  } else {
    digits *= 10n ** BigInt(places - scale);
  }
  const sign = digits < 0n ? "-" : "";
  const text = (digits < 0n ? -digits : digits).toString().padStart(places + 1, "0");
  return Number(places ? `${sign}${text.slice(0, -places)}.${text.slice(-places)}` : `${sign}${text}`);
}

/**
 * The listing price in the display currency, or undefined when there is nothing to add:
 * no ready table, no listing currency, the same currency, or no positive finite rate for it.
 * Rust's rate is target units per one listing unit; it is applied once to the unrounded price.
 */
export function convertListingPrice(value: number, currency: string | undefined, fx: DisplayFx) {
  if (fx.status !== "read" || fx.display.state !== "ready") return undefined;
  const table = fx.display;
  if (!currency || !ISO_CODE.test(currency) || currency === table.currency || !Number.isFinite(value)) return undefined;
  const rate = table.rates[currency];
  if (typeof rate !== "number" || !Number.isFinite(rate) || rate <= 0) return undefined;
  if (!Number.isFinite(value * rate)) return undefined;
  // Rounded once, to the target's minor units, from the exact decimal product.
  const amount = multiplyRounded(value, rate, currencyDigits(table.currency));
  return { amount, rate, from: currency, to: table.currency, source: table.source, date: table.date };
}

type Props = {
  /** The listing's own price, unrounded. */
  value: number;
  /** The currency of that exact price or of its selected series; never a market value's. */
  currency?: string;
  /** The shell's shared query unless given. */
  fx?: DisplayFx;
};

/**
 * "≈ KRW 217,384 · 1 USD = 1,334.87 KRW · European Central Bank …, Nov 16": a quieter
 * second line under a price. The amount names its currency by code (or the locale's own
 * word for it) and never by symbol; the rate names both codes with the digits Rust sent,
 * and the rate's own date stays beside its source.
 */
export function ConvertedAmount({ value, currency, fx }: Props) {
  const shared = useSharedDisplayFx();
  const { t } = useTranslation("assets");
  const converted = convertListingPrice(value, currency, fx ?? shared);
  if (!converted) return null;
  const digits = currencyDigits(converted.to);
  const number = formatNumber(converted.amount, { minimumFractionDigits: digits, maximumFractionDigits: digits });
  const word = currencyName(converted.to);
  const amount = word === converted.to
    ? t("assetPage.converted.figureCode", { code: converted.to, number })
    : t("assetPage.converted.figureWord", { word, number });
  // Every digit Rust kept (six significant), with no second rounding to money digits.
  const rate = t("assetPage.converted.rate", { from: converted.from, to: converted.to, rate: formatNumber(converted.rate, { maximumSignificantDigits: 15 }) });
  // The rate's date never breaks across lines ("Oct 27", "10월 27일").
  const source = t("assetPage.converted.source", { source: converted.source, date: formatDay(converted.date).replace(/ /gu, " ") });
  return <small className="asset-quote-meta asset-converted" data-converted-amount={converted.to}>
    <span aria-hidden="true">{t("assetPage.converted.approximate", { amount })} · {rate} · {source}</span>
    <span className="sr-only">{t("assetPage.converted.spoken", { amount, rate, source })}</span>
  </small>;
}
