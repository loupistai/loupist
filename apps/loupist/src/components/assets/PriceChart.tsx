import i18n, { useTranslation } from "@/i18n";
import { formatNumber, formatPercent, formatPrice } from "@/i18n/format";
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import type { AssetPricePoint } from "../../contracts/assets";
import { DIRECTION_GLYPH, priceDirection } from "../market/direction";
import { shortDay } from "./assetText";

const t = i18n.getFixedT(null, "assets");
const tDynamic = t as unknown as (key: string, options?: Record<string, unknown>) => string;

export const PRICE_RANGES = ["1W", "1M", "3M", "6M", "1Y"] as const;
export type PriceRange = (typeof PRICE_RANGES)[number];
/** How far back each range reaches from the last point, in calendar days. */
const RANGE_DAYS: Record<PriceRange, number> = { "1W": 7, "1M": 31, "3M": 92, "6M": 183, "1Y": 366 };
/** A range counts as filled when the points start within this many days of its start. */
const FILL_SLACK_DAYS = 7;
const HEIGHT = 230;
const PAD = { left: 4, right: 52, top: 28, bottom: 24 };
const DAY_MS = 86_400_000;

const dayMs = (date: string) => Date.parse(`${date.slice(0, 10)}T00:00:00Z`);

/** Points with a readable day and a finite value, oldest first, one per day (the last one stated for a day wins). */
export function chartPoints(points: readonly AssetPricePoint[]): AssetPricePoint[] {
  const byDay = new Map<string, AssetPricePoint>();
  for (const point of points) if (Number.isFinite(point.value) && Number.isFinite(dayMs(point.date))) byDay.set(point.date.slice(0, 10), point);
  return [...byDay.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([, point]) => point);
}

/**
 * The ranges the points can fill, shortest first: each range whose start they reach,
 * and the first one beyond (it shows all of them). Nothing with fewer than two points.
 */
export function offeredRanges(points: readonly AssetPricePoint[]): PriceRange[] {
  if (points.length < 2) return [];
  const depth = (dayMs(points.at(-1)!.date) - dayMs(points[0]!.date)) / DAY_MS;
  const offered: PriceRange[] = PRICE_RANGES.filter(range => depth + FILL_SLACK_DAYS >= RANGE_DAYS[range]);
  const next = PRICE_RANGES[offered.length];
  if (next && depth > RANGE_DAYS[offered.at(-1) ?? "1W"]) offered.push(next);
  return offered;
}

/** The points of one range, counted back from the last point. */
export function rangePoints(points: readonly AssetPricePoint[], range: PriceRange) {
  const end = points.at(-1);
  if (!end) return [];
  const start = dayMs(end.date) - RANGE_DAYS[range] * DAY_MS;
  return points.filter(point => dayMs(point.date) >= start);
}

/** Round steps for the price axis: about four lines across the visible span. */
function ticks(low: number, high: number) {
  const raw = (high - low) / 4;
  if (!(raw > 0)) return [];
  const magnitude = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map(multiple => multiple * magnitude).find(size => size >= raw) ?? raw;
  const values: number[] = [];
  for (let value = Math.ceil(low / step) * step; value <= high + step * 1e-9; value += step) values.push(Number(value.toPrecision(12)));
  return values;
}

/** Measures the chart's box so the line is drawn at its real width, not stretched. */
function useWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(640);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => { if (element.clientWidth > 0) setWidth(element.clientWidth); };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

type Props = {
  /** The range's points, oldest first (`rangePoints` of `chartPoints`). */
  points: readonly AssetPricePoint[];
  /** The stored price's currency; none when the result stated none. */
  currency?: string;
  range: PriceRange;
  ranges: readonly PriceRange[];
  onRange: (range: PriceRange) => void;
  /** The asset's name in the interface language. */
  name: string;
  busy?: boolean;
};

/**
 * The asset page's price line, from the points the host's replays stored or its
 * history read returned: a range switch, a recessive grid with the price axis on the
 * right, a dashed line at the range's first point, and a crosshair (pointer or arrow
 * keys) that names each point's date and value. Up and down colours follow the
 * price-colour setting and the sign always shows beside them.
 */
export function PriceChart({ points, currency, range, ranges, onRange, name, busy }: Props) {
  useTranslation("assets");
  const [box, width] = useWidth();
  const [active, setActive] = useState<number | null>(null);
  const hintId = useId();
  const count = points.length;
  useEffect(() => { setActive(null); }, [range, count]);

  const first = points[0]?.value ?? 0;
  const last = points.at(-1)?.value ?? 0;
  const change = first ? (last - first) / Math.abs(first) : 0;
  const direction = priceDirection(change, 4);
  const percent = (value: number) => formatPercent(value, { minimumFractionDigits: 2, maximumFractionDigits: 2, signDisplay: "exceptZero" });

  const values = points.map(point => point.value);
  const min = Math.min(...values), max = Math.max(...values);
  const margin = (max - min) * 0.1 || Math.abs(max) * 0.01 || 1;
  const low = min - margin, high = max + margin;
  const plotWidth = Math.max(1, width - PAD.left - PAD.right);
  const x = (index: number) => PAD.left + (count > 1 ? plotWidth * index / (count - 1) : plotWidth);
  const y = (value: number) => PAD.top + (HEIGHT - PAD.top - PAD.bottom) * (1 - (value - low) / (high - low));
  const line = points.map((point, index) => `${index ? "L" : "M"}${x(index).toFixed(1)} ${y(point.value).toFixed(1)}`).join("");
  const area = count > 1 ? `${line}L${x(count - 1).toFixed(1)} ${HEIGHT - PAD.bottom}L${PAD.left} ${HEIGHT - PAD.bottom}Z` : "";
  const axis = ticks(low, high);
  const axisDigits = axis.length > 1 && Math.abs(axis[1]! - axis[0]!) < 1 ? 2 : 0;
  const dateLabels = count > 1 ? [...new Set([0, Math.round((count - 1) / 3), Math.round((count - 1) * 2 / 3), count - 1])] : [];

  const pointDay = (index: number) => shortDay(points[index]!.date.slice(0, 10));
  const pointPrice = (value: number) => formatPrice(value, currency, { digits: "auto" });
  const pointText = (index: number) => {
    const point = points[index]!;
    return t("assetPage.chart.point", { date: pointDay(index), value: pointPrice(point.value), change: percent(first ? (point.value - first) / Math.abs(first) : 0) });
  };
  const summary = count > 1 ? t("assetPage.chart.summary", {
    name, range: tDynamic(`assetPage.chart.rangeLong.${range}`), from: pointDay(0), to: pointDay(count - 1),
    first: pointPrice(first), last: pointPrice(last), change: percent(change),
    low: pointPrice(min), high: pointPrice(max),
  }) : "";

  const indexAt = (clientX: number, element: Element) => {
    const left = element.getBoundingClientRect().left;
    return Math.max(0, Math.min(count - 1, Math.round((clientX - left - PAD.left) / plotWidth * (count - 1))));
  };
  const keys = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!count) return;
    const current = active ?? count - 1;
    const step = event.shiftKey ? 5 : 1;
    const next = event.key === "ArrowLeft" ? Math.max(0, current - step)
      : event.key === "ArrowRight" ? Math.min(count - 1, current + step)
        : event.key === "Home" ? 0 : event.key === "End" ? count - 1 : undefined;
    if (next !== undefined) { event.preventDefault(); setActive(next); return; }
    if (event.key === "Escape") setActive(null);
  };
  const rangeKeys = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!["ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    const index = ranges.indexOf(range);
    const next = ranges[(index + (event.key === "ArrowRight" ? 1 : ranges.length - 1)) % ranges.length]!;
    onRange(next);
    event.currentTarget.querySelector<HTMLButtonElement>(`[data-range="${next}"]`)?.focus();
  };
  const tipLeft = active === null ? 0 : Math.max(72, Math.min(width - 72, x(active)));
  const activePoint = active === null ? undefined : points[active];

  return <section className="price-chart" aria-label={t("assetPage.chart.label")} aria-busy={busy || undefined}>
    <div className="price-chart-head">
      {count > 0 ? <span className="market-price gg-num">{pointPrice(last)}</span> : null}
      <span className="price-chart-label gg-num">
        {tDynamic(`assetPage.chart.rangeShort.${range}`)}{" "}
        <span className="price-chart-change" data-direction={direction}><span aria-hidden="true">{DIRECTION_GLYPH[direction]} </span>{percent(change)}</span>
      </span>
      {ranges.length > 1 ? <div className="price-chart-ranges" role="group" aria-label={t("assetPage.chart.range")} onKeyDown={rangeKeys}>
        {ranges.map(choice => <button key={choice} type="button" data-range={choice} aria-pressed={choice === range} tabIndex={choice === range ? 0 : -1}
          aria-label={tDynamic(`assetPage.chart.rangeLong.${choice}`)} onClick={() => onRange(choice)}>{tDynamic(`assetPage.chart.rangeShort.${choice}`)}</button>)}
      </div> : null}
    </div>
    <div ref={box} className="price-chart-plot" data-direction={direction} tabIndex={0} aria-describedby={hintId}
      aria-label={summary} role="group"
      onKeyDown={keys} onBlur={() => setActive(null)}
      onPointerMove={(event: PointerEvent<HTMLDivElement>) => setActive(indexAt(event.clientX, event.currentTarget))}
      onPointerLeave={() => setActive(null)}>
      <svg width={width} height={HEIGHT} viewBox={`0 0 ${width} ${HEIGHT}`} aria-hidden="true" focusable="false">
        {axis.map(value => <g key={value}>
          <line className="price-chart-grid" x1={PAD.left} x2={width - PAD.right} y1={y(value).toFixed(1)} y2={y(value).toFixed(1)} />
          <text x={width - PAD.right + 8} y={(y(value) + 4).toFixed(1)}>{formatNumber(value, { minimumFractionDigits: axisDigits, maximumFractionDigits: axisDigits })}</text>
        </g>)}
        {count > 1 ? <line className="price-chart-base" x1={PAD.left} x2={width - PAD.right} y1={y(first).toFixed(1)} y2={y(first).toFixed(1)} /> : null}
        <path className="price-chart-area" d={area} />
        <path className="price-chart-line" d={line} />
        {dateLabels.map((index, at) => <text key={index} x={x(index).toFixed(1)} y={HEIGHT - 6} textAnchor={at === 0 ? "start" : at === dateLabels.length - 1 ? "end" : "middle"}>{pointDay(index)}</text>)}
        {activePoint ? <>
          <line className="price-chart-cross" x1={x(active!)} x2={x(active!)} y1={PAD.top} y2={HEIGHT - PAD.bottom} />
          <circle className="price-chart-dot" r="4.5" cx={x(active!)} cy={y(activePoint.value)} />
        </> : count ? <circle className="price-chart-dot" r="4.5" cx={x(count - 1)} cy={y(last)} /> : null}
      </svg>
      {activePoint ? <div className="price-chart-tip gg-num" style={{ left: tipLeft }} aria-hidden="true">{pointText(active!)}</div> : null}
    </div>
    <p id={hintId} className="sr-only">{t("assetPage.chart.keys")}</p>
    <p className="sr-only" aria-live="polite">{active === null ? "" : pointText(active)}</p>
    {count < 20 && ranges.length <= 2 ? <p className="price-chart-note">{t("assetPage.chart.short", { number: formatNumber(count) })}</p> : null}
  </section>;
}
