import { createContext, useContext } from "react";
import { pngBytes } from "../map/export";

/** The chart as drawn now: its canvas and how many canvas pixels make one CSS pixel. */
export type ChartPicture = { canvas: HTMLCanvasElement; scale: number };
/** Draws the chart onto a canvas at about `scale` pixels per CSS pixel. */
export type ChartSnapshot = (scale: number) => Promise<ChartPicture>;
/** Where a mounted chart engine leaves its snapshot for the card or panel around it. */
export type ChartSnapshotSlot = { current: ChartSnapshot | null };

export const ChartSnapshotContext = createContext<ChartSnapshotSlot | null>(null);
export const useChartSnapshotSlot = () => useContext(ChartSnapshotContext);

/** The scale of a saved chart image (shell-briefings §2.6: PNG at 2×). */
export const EXPORT_SCALE = 2;
const FOOTER_HEIGHT = 56;

export type ChartExportFooter = { title: string; date: string; disclosure: string };

/**
 * A chart image with the export footer band (map.md §7): the chart's title,
 * the date and Loupist, and the AI notice. Only the chart bitmap and these
 * display fields cross into the file.
 */
export function composeChartExport(picture: ChartPicture, footer: ChartExportFooter, output = document.createElement("canvas")) {
  const width = picture.canvas.width / picture.scale;
  const height = picture.canvas.height / picture.scale;
  output.width = Math.round(width * EXPORT_SCALE);
  output.height = Math.round((height + FOOTER_HEIGHT) * EXPORT_SCALE);
  const ctx = output.getContext("2d");
  if (!ctx) throw new Error("Canvas unavailable");
  const css = getComputedStyle(document.documentElement);
  const token = (name: string) => css.getPropertyValue(`--gg-color-${name}`).trim();
  ctx.setTransform(EXPORT_SCALE, 0, 0, EXPORT_SCALE, 0, 0);
  ctx.fillStyle = token("bg-app");
  ctx.fillRect(0, 0, width, height);
  ctx.drawImage(picture.canvas, 0, 0, width, height);
  ctx.fillStyle = token("bg-raised");
  ctx.fillRect(0, height, width, FOOTER_HEIGHT);
  ctx.fillStyle = token("border-subtle");
  ctx.fillRect(0, height, width, 1);
  const font = css.getPropertyValue("--gg-font-ui").trim() || "sans-serif";
  const fit = (text: string, max: number) => {
    let value = text;
    while (value.length > 1 && ctx.measureText(value).width > max) value = `${value.slice(0, -2)}…`;
    return value;
  };
  ctx.textBaseline = "middle";
  ctx.font = `500 12px ${font}`;
  ctx.fillStyle = token("text-primary");
  ctx.textAlign = "left";
  const stamp = `${footer.date} · Loupist`;
  ctx.fillText(fit(footer.title, width * 0.6 - 24), 16, height + 18);
  ctx.font = `12px ${font}`;
  ctx.fillStyle = token("text-secondary");
  ctx.textAlign = "right";
  ctx.fillText(fit(stamp, width * 0.4 - 24), width - 16, height + 18);
  ctx.font = `11px ${font}`;
  ctx.textAlign = "left";
  ctx.fillText(fit(footer.disclosure, width - 32), 16, height + 40);
  return output;
}

/** PNG bytes of the chart with its footer, ready for Rust `export_save`. */
export async function chartPngBytes(snapshot: ChartSnapshot, footer: ChartExportFooter) {
  return Array.from(pngBytes(composeChartExport(await snapshot(EXPORT_SCALE), footer)));
}
