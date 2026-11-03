import { formatNumber } from "@/i18n/format";
import i18n from "@/i18n";
import { registerOverlay, type OverlayFigure, type OverlayTemplate } from "klinecharts";
import { readChartTokens, chartColorWithOpacity } from "./chartTokens";
import { arrow, fibonacciExtension, measure, rect } from "@klinecharts/extension";

const t = i18n.getFixedT(null, "chat");

const formatPrice = (value: number): string => {
  const digits = Math.abs(value) >= 1 ? 2 : 4;
  return formatNumber(value, { minimumFractionDigits: digits, maximumFractionDigits: digits, useGrouping: false });
};

const positionOverlay = (name: "longPosition" | "shortPosition"): OverlayTemplate => ({
  name,
  totalStep: 4,
  needDefaultPointFigure: true,
  needDefaultXAxisFigure: true,
  needDefaultYAxisFigure: true,
  createPointFigures: ({ coordinates, overlay }) => {
    if (coordinates.length < 3) return [];
    const [entry, stop, target] = coordinates;
    const entryValue = overlay.points[0]?.value;
    const stopValue = overlay.points[1]?.value;
    const targetValue = overlay.points[2]?.value;
    if (
      entryValue === undefined ||
      stopValue === undefined ||
      targetValue === undefined ||
      !Number.isFinite(entryValue) ||
      !Number.isFinite(stopValue) ||
      !Number.isFinite(targetValue)
    ) {
      return [];
    }
    const colors = readChartTokens();
    const stopColor = stopValue >= entryValue ? colors.up : colors.down;
    const targetColor = targetValue >= entryValue ? colors.up : colors.down;
    const risk = Math.abs(entryValue - stopValue);
    const reward = Math.abs(targetValue - entryValue);
    const ratio = risk === 0 ? 0 : reward / risk;
    const xLeft = Math.min(entry.x, stop.x, target.x);
    const xRight = Math.max(entry.x, stop.x, target.x);
    const width = Math.max(xRight - xLeft, 8);
    const textX = xRight + 6;
    const zone = (fromY: number, toY: number, color: string): OverlayFigure => ({
      type: "rect",
      attrs: {
        x: xLeft,
        y: Math.min(fromY, toY),
        width,
        height: Math.max(Math.abs(toY - fromY), 1),
      },
      styles: { style: "fill", color },
      ignoreEvent: true,
    });
    const line = (y: number, color: string): OverlayFigure => ({
      type: "line",
      attrs: { coordinates: [{ x: xLeft, y }, { x: xRight, y }] },
      styles: { color },
    });
    const caption = (y: number, text: string): OverlayFigure => ({
      type: "text",
      ignoreEvent: true,
      attrs: { x: textX, y, text, align: "left", baseline: "middle" },
    });
    return [
      zone(entry.y, stop.y, chartColorWithOpacity(stopColor, 0.18)),
      zone(entry.y, target.y, chartColorWithOpacity(targetColor, 0.18)),
      line(entry.y, colors.secondary),
      line(stop.y, stopColor),
      line(target.y, targetColor),
      caption(entry.y, t("financialOverlays.entryPrice", { value1: formatPrice(entryValue) })),
      caption(stop.y, t("financialOverlays.stopPrice", { value1: formatPrice(stopValue) })),
      caption(target.y, t("financialOverlays.targetPriceRatio", { value1: formatPrice(targetValue), value2: formatNumber(ratio, { minimumFractionDigits: 2, maximumFractionDigits: 2 }) })),
    ];
  },
});

let registered = false;

export const registerReviewedOverlays = (): void => {
  if (registered) return;
  registerOverlay(rect);
  registerOverlay(arrow);
  registerOverlay(measure);
  registerOverlay(fibonacciExtension);
  registerOverlay(positionOverlay("longPosition"));
  registerOverlay(positionOverlay("shortPosition"));
  registered = true;
};
