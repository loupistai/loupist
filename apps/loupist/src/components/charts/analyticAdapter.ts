import type { ChartAssemblyInput } from "flint-chart/core";
import { readChartTokens } from "./chartTokens";
import { assembleVegaLite } from "flint-chart/vegalite";
import type { ChartColumn, ChartDocument } from "../../types";

type AnalyticView = Exclude<ChartDocument["view"], { kind: "candlestick" }>;
type VegaNode = Record<string, unknown>;

const flintType = (chartType: AnalyticView["kind"]) => ({
  line: "Line Chart",
  area: "Area Chart",
  bar: "Bar Chart",
  scatter: "Scatter Plot",
})[chartType];

const analyticTheme = (theme: "light" | "dark"): ChartAssemblyInput["theme_spec"] => {
  const colors = readChartTokens();
  return {
    extends: theme === "dark" ? "powerbi" : "powerbi-light",
    ink: {
      surface: { source: "house", canvas: colors.background, plot: colors.background, panel: colors.raised },
      text: { primary: colors.text, secondary: colors.secondary, muted: colors.secondary },
      structure: { axis: colors.border, grid: colors.border, frame: colors.border, rule: colors.border },
      series: {
        single: colors.series[0], categorical: colors.series, categoricalExtended: colors.series, overflow: colors.secondary,
        status: { positive: colors.up, negative: colors.down, neutral: colors.secondary },
        sequential: { stops: [colors.background, colors.series[0]] },
        diverging: { stops: [colors.down, colors.background, colors.up], neutral: colors.background },
      },
      accent: colors.info,
    },
  };
};

const fitAnalyticFrame = (width: number, height: number) => {
  const measuredWidth = Number.isFinite(width) ? Math.floor(width) : 0;
  const measuredHeight = Number.isFinite(height) ? Math.floor(height) : 0;
  return {
    width: measuredWidth > 0 ? Math.max(280, measuredWidth) : 640,
    height: measuredHeight > 0 ? Math.max(200, measuredHeight) : 400,
  };
};

const humanizeFieldLabel = (name: string) => {
  const stripped = name
    .replace(/_(krw|usd|eur|jpy|cny)(_(trillion|billion|million|thousand|mn|bn|tn))?$/i, "")
    .replace(/_/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!stripped) return name;
  return stripped.replace(/\b([a-z])/g, (letter) => letter.toUpperCase());
};

const visitVega = (node: unknown, visit: (value: VegaNode) => void) => {
  if (!node || typeof node !== "object") return;
  const value = node as VegaNode;
  visit(value);
  for (const key of ["layer", "hconcat", "vconcat", "concat"]) {
    const children = value[key];
    if (Array.isArray(children)) children.forEach((child) => visitVega(child, visit));
  }
  visitVega(value.spec, visit);
};

const uniqueXCount = (view: AnalyticView, rows: Array<Record<string, unknown>>) =>
  new Set(rows.map((row) => row[view.x]).filter((value) => value != null)).size;

const finalizeAnalyticSpec = (
  spec: VegaNode,
  frame: { width: number; height: number },
  view: AnalyticView,
  rows: Array<Record<string, unknown>>,
  columns: ChartColumn[],
) => {
  spec.width = frame.width;
  spec.height = frame.height;
  spec.autosize = { type: "fit", contains: "padding" };

  const config = spec.config;
  if (config && typeof config === "object") {
    const viewConfig = (config as VegaNode).view;
    if (viewConfig && typeof viewConfig === "object") {
      delete (viewConfig as VegaNode).continuousWidth;
      delete (viewConfig as VegaNode).continuousHeight;
    }
  }

  const labels = new Map(columns.map((column) => [column.id, column.label]));
  const seriesLabels = Object.fromEntries(view.y.map((field) => [
    field, labels.get(field) || humanizeFieldLabel(field),
  ]));

  const showEveryPoint =
    (view.kind === "line" || view.kind === "area") &&
    uniqueXCount(view, rows) <= 16;

  visitVega(spec, (node) => {
    const encoding = node.encoding;
    if (encoding && typeof encoding === "object") {
      const channels = encoding as VegaNode;
      for (const name of ["x", "y", "color"]) {
        const channel = channels[name];
        if (!channel || typeof channel !== "object" || Array.isArray(channel)) continue;
        const field = (channel as VegaNode).field;
        if (typeof field === "string" && labels.has(field)) {
          (channel as VegaNode).title = labels.get(field);
        }
      }
      const color = channels.color as VegaNode | undefined;
      if (color && typeof color === "object" && color.legend !== null) {
        color.legend = {
          ...(color.legend as VegaNode | undefined),
          orient: "bottom",
          direction: "horizontal",
          columns: { expr: "max(1, floor(width / 260))" },
          labelLimit: { expr: "max(80, min(240, width - 24))" },
          ...(color.field === "__flint_series_key" ? {
            // Labels are presentation only: equal labels must not merge series.
            labelExpr: `(${JSON.stringify(seriesLabels)})[datum.label] || datum.label`,
          } : {}),
        };
      }
      const y = channels.y;
      if (y && typeof y === "object" && !Array.isArray(y)) {
        const yEnc = y as VegaNode;
        if (yEnc.field === "__flint_series_value") yEnc.title = null;
        if (yEnc.type === "quantitative") {
          yEnc.scale = { ...(yEnc.scale as VegaNode | undefined), zero: true };
        }
        const axis = yEnc.axis;
        if (axis && typeof axis === "object") {
          const yAxis = axis as VegaNode;
          if (yAxis.format === ",.12~g") {
            // Vega chooses tick precision from the scale. Fixed decimal places
            // can label distinct small values as zero; semantic formats remain.
            delete yAxis.format;
          }
        }
      }
      const x = channels.x;
      if (x && typeof x === "object" && !Array.isArray(x)) {
        const xEnc = x as VegaNode;
        if (xEnc.axis !== null) {
          const axis =
            xEnc.axis && typeof xEnc.axis === "object"
              ? (xEnc.axis as VegaNode)
              : {};
          axis.labelAngle = 0;
          axis.labelOverlap = false;
          axis.labelLimit = 160;
          xEnc.axis = axis;
        }
      }
    }

    if (!showEveryPoint) return;
    const mark = node.mark;
    const markType =
      typeof mark === "string"
        ? mark
        : mark && typeof mark === "object"
          ? (mark as VegaNode).type
          : undefined;
    if (markType !== "point" || !Array.isArray(node.transform)) return;
    node.transform = node.transform.filter((step) => {
      if (!step || typeof step !== "object") return true;
      return (step as VegaNode).filter !== "datum.__peLast === 1";
    });
  });

  return spec;
};

export const assembleAnalyticSpec = (
  view: AnalyticView,
  rows: Array<Record<string, unknown>>,
  theme: "light" | "dark",
  width: number,
  height: number,
  columns: ChartColumn[] = [],
): Record<string, unknown> => {
  const frame = fitAnalyticFrame(width, height);
  const encodings: ChartAssemblyInput["chart_spec"]["encodings"] = {
    x: { field: view.x },
    y: view.y.length === 1
      ? { field: view.y[0] }
      : view.y.map((field) => ({ field })),
  };
  if (view.color) encodings.color = { field: view.color };
  const input: ChartAssemblyInput = {
    data: { values: rows },
    semantic_types: view.semantic_types,
    chart_spec: {
      chartType: flintType(view.kind),
      encodings,
      baseSize: frame,
      canvasSize: frame,
    },
    theme_spec: analyticTheme(theme),
  };
  return finalizeAnalyticSpec(assembleVegaLite(input) as VegaNode, frame, view, rows, columns);
};
