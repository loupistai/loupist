import { formatNumber } from "@/i18n/format";
import i18n from "@/i18n";
import type { AxisTick, IndicatorCreate, OverlayCreate, Period, Point } from "klinecharts";
import type { ChartDrawing, ChartStudy } from "../../types";
import { timestampValue } from "./dataset";

const t = i18n.getFixedT(null, "chat");

const PRICE_STUDIES = new Set<ChartStudy["module_id"]>([
  "AVP",
  "BBI",
  "MA",
  "EMA",
  "SMA",
  "BOLL",
  "SAR",
]);

const OVERLAY_NAMES: Record<ChartDrawing["kind"], string> = {
  segment: "segment",
  ray: "rayLine",
  line: "straightLine",
  horizontal_line: "horizontalStraightLine",
  vertical_line: "verticalStraightLine",
  price_line: "priceLine",
  fibonacci: "fibonacciLine",
  horizontal_segment: "horizontalSegment",
  horizontal_ray: "horizontalRayLine",
  vertical_segment: "verticalSegment",
  vertical_ray: "verticalRayLine",
  parallel_line: "parallelStraightLine",
  price_channel: "priceChannelLine",
  annotation: "simpleAnnotation",
  rectangle: "rect",
  arrow: "arrow",
  measure: "measure",
  fibonacci_extension: "fibonacciExtension",
  long_position: "longPosition",
  short_position: "shortPosition",
};

export const periodFor = (interval: string): Period => {
  const match = /^([1-9][0-9]{0,3})(m|h|d|wk|mo)$/u.exec(interval);
  if (!match) throw new Error(t("financialAdapter.unsupportedFinancialChartInterval"));
  const span = Number(match[1]);
  const unit = match[2];
  if (unit === "m") return { type: "minute", span };
  if (unit === "h") return { type: "hour", span };
  if (unit === "wk") return { type: "week", span };
  if (unit === "mo") return { type: "month", span };
  return { type: "day", span };
};

export const indicatorConfig = (
  study: ChartStudy,
): { value: IndicatorCreate; isStack: boolean } => {
  const isPriceStudy = PRICE_STUDIES.has(study.module_id);
  return {
    value: {
      name: study.module_id,
      ...(isPriceStudy ? { paneId: "candle_pane" } : {}),
      ...(study.calc_params.length > 0 ? { calcParams: study.calc_params } : {}),
    },
    isStack: isPriceStudy,
  };
};

const overlayColor = (hex: string, alpha: number): string => {
  const body = hex.startsWith("#") ? hex.slice(1) : hex;
  const red = Number.parseInt(body.slice(0, 2), 16);
  const green = Number.parseInt(body.slice(2, 4), 16);
  const blue = Number.parseInt(body.slice(4, 6), 16);
  return `rgba(${red}, ${green}, ${blue}, ${alpha})`; // token-exception: user-authored chart drawing color as a canvas alpha fill.
};

const overlayPoints = (drawing: ChartDrawing): Array<{ timestamp: number; value: number }> =>
  drawing.points.map((point) => {
    const timestamp = timestampValue(point.timestamp);
    if (timestamp === undefined) throw new Error(t("financialAdapter.unsupportedFinancialChartDrawingTimestamp"));
    return { timestamp, value: point.value };
  });

const overlayStyles = (drawing: ChartDrawing): OverlayCreate["styles"] => {
  const line = {
    ...(drawing.color ? { color: drawing.color } : {}),
    ...(drawing.line_width ? { size: drawing.line_width } : {}),
    ...(drawing.line_style ? { style: drawing.line_style } : {}),
  };
  return {
    ...(Object.keys(line).length > 0 ? { line } : {}),
    ...(drawing.kind === "rectangle" && drawing.color
      ? { polygon: { color: overlayColor(drawing.color, 0.15) } }
      : {}),
  };
};

const measureLabels = (points: Array<Partial<Point>>): string[] => {
  const start = points[0];
  const end = points[1];
  if (
    start?.value === undefined ||
    end?.value === undefined ||
    start.timestamp === undefined ||
    end.timestamp === undefined
  ) {
    return [];
  }
  const change = end.value - start.value;
  const percent = start.value === 0 ? 0 : (change / start.value) * 100;
  const durationMs = Math.abs(end.timestamp - start.timestamp);
  const day = 86_400_000;
  const hour = 3_600_000;
  const minute = 60_000;
  const duration =
    durationMs >= day
      ? t("financialAdapter.durationDays", { value1: Math.round(durationMs / day) })
      : durationMs >= hour
        ? t("financialAdapter.durationHours", { value1: Math.round(durationMs / hour) })
        : t("financialAdapter.durationMinutes", { value1: Math.round(durationMs / minute) });
  const signed = (value: number, suffix = "") =>
    `${formatNumber(value, { signDisplay: "always", minimumFractionDigits: 2, maximumFractionDigits: 2 }) }${suffix}`;
  return [signed(change), signed(percent, "%"), duration];
};

const drawingConfig = (drawing: ChartDrawing): OverlayCreate => {
  const points = overlayPoints(drawing);
  const styles = overlayStyles(drawing);
  return {
    name: OVERLAY_NAMES[drawing.kind],
    points,
    lock: true,
    ...(Object.keys(styles ?? {}).length > 0 ? { styles } : {}),
    ...(drawing.kind === "annotation" ? { extendData: drawing.label ?? "" } : {}),
    ...(drawing.kind === "measure" ? { extendData: measureLabels } : {}),
  };
};

export const drawingOverlays = (drawing: ChartDrawing): OverlayCreate[] => {
  const overlay = drawingConfig(drawing);
  if (drawing.kind === "annotation" || !drawing.label) return [overlay];
  const anchor = overlay.points?.at(-1);
  if (!anchor) return [overlay];
  return [
    overlay,
    {
      name: "simpleAnnotation",
      points: [anchor],
      lock: true,
      extendData: drawing.label,
    },
  ];
};

/** The chart's own price text: the same fraction digits the price axis prints, in the interface locale. */
export function formatChartPrice(value: number, precision: number): string {
  if (!Number.isFinite(value)) return String(value);
  const digits = Number.isInteger(precision) && precision >= 0 && precision <= 12 ? precision : 2;
  return formatNumber(value, { minimumFractionDigits: digits, maximumFractionDigits: digits });
}

/** Air beyond half a center-aligned axis label so the rightmost date is not clipped. */
const AXIS_LABEL_AIR_PX = 4;
/** Widest bar the chart will draw; a shorter range stays readable instead of stretching. */
const BAR_SPACE_MAX = 36;
/** Narrowest bar. A year of daily bars in a docked panel is thinner than the library's default of 10. */
const BAR_SPACE_MIN = 0.05;

/**
 * Bar width and right inset that show every bar of the selected range, with the last bar
 * far enough from the right edge that a center-aligned axis label fits.
 * `toIndex` is the last selected bar; `dataCount` is the whole series.
 */
export function fittedRange(plotWidth: number, barCount: number, dataCount: number, toIndex: number, labelWidth: number): { barSpace: number; offsetRight: number } | null {
  if (!(plotWidth > 0) || barCount <= 0 || dataCount <= 0) return null;
  const gutter = Math.max(0, labelWidth) / 2 + AXIS_LABEL_AIR_PX;
  const usable = Math.max(1, plotWidth - Math.min(gutter * 2, plotWidth - 1));
  const barSpace = Math.min(BAR_SPACE_MAX, Math.max(BAR_SPACE_MIN, usable / barCount));
  const offsetRight = gutter + (toIndex + 0.5 - dataCount) * barSpace;
  return { barSpace, offsetRight };
}

export type PlotTick = { coord: number; value: number | string; text: string };

/**
 * Center-aligned axis labels, shifted in from either edge so none is clipped,
 * and the rightmost label kept even when that crowds the one before it.
 */
export function ticksThatFit(ticks: readonly PlotTick[], width: number, textWidth: (text: string) => number): PlotTick[] {
  if (!(width > 0)) return [...ticks];
  const place = (tick: PlotTick): PlotTick | null => {
    const half = Math.max(0, textWidth(tick.text) / 2);
    if (half * 2 > width) return null;
    const coord = Math.min(width - half, Math.max(half, tick.coord));
    return coord === tick.coord ? tick : { ...tick, coord };
  };
  const overlaps = (left: PlotTick, right: PlotTick) =>
    right.coord - textWidth(right.text) / 2 < left.coord + textWidth(left.text) / 2 + AXIS_LABEL_AIR_PX;
  const fitted: PlotTick[] = [];
  for (const tick of ticks) {
    const next = place(tick);
    if (!next) continue;
    const previous = fitted.at(-1);
    if (previous && overlaps(previous, next)) continue;
    fitted.push(next);
  }
  const last = ticks.at(-1);
  const placed = last ? place(last) : null;
  if (!placed) return fitted;
  while (fitted.length > 0 && overlaps(fitted.at(-1)!, placed) && fitted.at(-1) !== placed) fitted.pop();
  if (fitted.at(-1) !== placed) fitted.push(placed);
  return fitted;
}

/** Half the library's last-price tag (12 px text, 4 px padding) plus half an axis label, with a little air. */
const LAST_PRICE_CLEARANCE_PX = 18;

/**
 * The price axis labels without those the last-price tag would cover: the library draws the tag over
 * the axis, and the tag already names that price. Ticks are linear in price, so two of them place it.
 * The library gives a tick's value as its label text ("104.00").
 */
export function ticksClearOfLastPrice(ticks: readonly AxisTick[], lastPrice: number | undefined): AxisTick[] {
  const priced = ticks.map((tick) => ({ coord: tick.coord, value: Number(tick.value) })).filter((tick) => Number.isFinite(tick.value));
  const first = priced[0], last = priced.at(-1);
  if (lastPrice === undefined || !first || !last || first.value === last.value) return [...ticks];
  const coord = first.coord + (lastPrice - first.value) * (last.coord - first.coord) / (last.value - first.value);
  return ticks.filter((tick) => Math.abs(tick.coord - coord) >= LAST_PRICE_CLEARANCE_PX);
}
