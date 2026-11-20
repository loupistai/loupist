import { useTranslation } from "../../i18n";
import { formatPercent, formatPrice } from "../../i18n/format";
import { priceDirection } from "./direction";

type Props = {
  /** Closes, oldest first. Fewer than two finite values draws nothing. */
  closes: readonly number[];
  /** Formats the first and last close in the spoken summary. */
  currency?: string;
  width?: number;
  height?: number;
  /** A dot on the last close. */
  showEnd?: boolean;
};

const INSET = 2;

/** A small trend line from a series of closes, in the up or down colour of first to last. */
export function Sparkline({ closes, currency, width = 96, height = 28, showEnd = true }: Props) {
  const { t } = useTranslation("common");
  const values = closes.filter(Number.isFinite);
  if (values.length < 2) return null;
  const first = values[0];
  const last = values[values.length - 1];
  const low = Math.min(...values);
  const span = Math.max(...values) - low;
  const x = (index: number) => INSET + (width - INSET * 2 - 2) * index / (values.length - 1);
  // A level series sits on the middle line rather than the floor.
  const y = (value: number) => span ? INSET + 1 + (height - INSET * 2 - 2) * (1 - (value - low) / span) : height / 2;
  const line = values.map((value, index) => `${index ? "L" : "M"}${x(index).toFixed(1)} ${y(value).toFixed(1)}`).join("");
  const area = `${line}L${x(values.length - 1).toFixed(1)} ${height}L${INSET} ${height}Z`;
  const change = first ? (last - first) / Math.abs(first) : 0;
  // The summary gives the percent to one decimal (three of the fraction); below that it is level.
  const direction = priceDirection(change, 3);
  const summary = t(`market.trend.${direction}`, {
    first: formatPrice(first, currency),
    last: formatPrice(last, currency),
    percent: formatPercent(Math.abs(change), { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
  });
  return (
    <svg className="market-sparkline" data-direction={direction} width={width} height={height} viewBox={`0 0 ${width} ${height}`} role="img" aria-label={summary}>
      <path className="market-sparkline-area" d={area} />
      <path className="market-sparkline-line" d={line} />
      {showEnd ? <circle className="market-sparkline-end" cx={x(values.length - 1).toFixed(1)} cy={y(last).toFixed(1)} r="2.2" /> : null}
    </svg>
  );
}
