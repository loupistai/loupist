import i18n from "@/i18n";
import type { AssetStoredPrice } from "@/contracts/assets";
import { formatDay, formatTime } from "@/i18n/format";

const t = i18n.getFixedT(null, "common");
const DAY = /^\d{4}-\d{2}-\d{2}$/u;
const DATE_TIME = /^\d{4}-\d{2}-\d{2}T/u;

/**
 * The provider's stated date or time: a calendar day reads "Nov 18" without a time-zone
 * shift, a time adds the local clock, and anything else stays as given.
 */
export function statedDate(date: string) {
  const text = date.trim();
  if (DAY.test(text)) return formatDay(text);
  const ms = DATE_TIME.test(text) ? Date.parse(text) : Number.NaN;
  return Number.isFinite(ms) ? `${formatDay(text.slice(0, 10))} ${formatTime(ms)}` : text;
}

/** A stated price kind precedes its date. Older or generic price reads use "As of". */
export function priceAsOf(date: string, kind?: AssetStoredPrice["kind"]) {
  const when = statedDate(date);
  return kind ? `${i18n.t(`assets:price.kind.${kind}`)} · ${when}` : t("market.asOf", { date: when });
}
