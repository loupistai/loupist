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
