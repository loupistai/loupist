import type { ChartAssemblyInput } from "flint-chart/core";
import { readChartTokens } from "./chartTokens";
import { assembleVegaLite } from "flint-chart/vegalite";
import type { ChartColumn, ChartDocument } from "../../types";
import i18n from "@/i18n";
import { numericValue } from "./dataset";
import { columnUnit, isYear, unitText, type DateGrain } from "./chartFormat";

type AnalyticView = Exclude<ChartDocument["view"], { kind: "candlestick" }>;
type VegaNode = Record<string, unknown>;

const t = i18n.getFixedT(null, "chat");

// Semantic types (flint's, or the model's own words) whose values are never scaled to 조 or bn: rates, prices, scores.
const UNSCALED = /percent|ratio|rate|price|temperature|sentiment|correlation|score|rank|latitude|longitude|duration|multiple/iu;
// Semantic types that name a period, a date, a category or an identifier: printed as given, never as a quantity.
const LABEL_TYPES = /date|time|year|quarter|month|week|day|hour|decade|period|fiscal|^id$|identifier|code|category|name|rank/iu;
// Semantic types that say a value is a quantity, so even integers in 1800–2200 are counted, not read as years.
const QUANTITY_TYPES = /amount|price|profit|quantity|count|percent|revenue|sales|income|value|volume/iu;

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

  // Custom format types (`loupistExact`, `chartFormat.ts`) are opt-in in Vega-Lite.
  spec.config = { ...(spec.config as VegaNode | undefined), customFormatTypes: true };
  const semantic = (field: string) => view.semantic_types[field] ?? "";
  const kinds = new Map(columns.map((column) => [column.id, column.kind]));
  const abbreviates = (fields: readonly string[]) => fields.every((field) => !UNSCALED.test(semantic(field)) && !/%|percent|퍼센트|비율/iu.test(labels.get(field) ?? ""));
  const extentOf = (fields: readonly string[], zero: boolean, sums = false) => {
    let [lo, hi] = zero ? [0, 0] : [Infinity, -Infinity];
    const stacks = new Map<unknown, [number, number]>();
    for (const row of rows) for (const field of fields) {
      const value = numericValue(row[field]);
      if (value === undefined) continue;
      [lo, hi] = [Math.min(lo, value), Math.max(hi, value)];
      if (!sums) continue;
      const stack = stacks.get(row[view.x]) ?? [0, 0];
      stack[value < 0 ? 0 : 1] += value;
      stacks.set(row[view.x], stack);
    }
    for (const [down, up] of stacks.values()) [lo, hi] = [Math.min(lo, down), Math.max(hi, up)];
    return Number.isFinite(lo) && Number.isFinite(hi) ? [lo, hi] as const : [0, 0] as const;
  };
  // Stacked bars and areas reach the sum of a category's values, which sets the axis's unit.
  const valueExtent = (channels: VegaNode, zero: boolean) => {
    const yEnc = channels.y as VegaNode;
    const stacked = (view.kind === "bar" || view.kind === "area") && Boolean(channels.color) && !channels.xOffset && yEnc.stack !== null && yEnc.stack !== false;
    return extentOf(view.y, zero, stacked);
  };
  // The value axis says the unit once: the column's label with its currency, or the shared unit of folded series.
  const units = view.y.map((field) => columnUnit(field, labels.get(field) ?? ""));
  const sharedUnit = units.every((unit) => unitText(unit) === unitText(units[0]!)) ? unitText(units[0]!) : "";
  const onlyLabel = labels.get(view.y[0]!) ?? "";
  const valueTitle = view.y.length === 1
    ? { axis: !sharedUnit ? undefined : units[0]!.named ? onlyLabel : t("chartAxis.titleWithUnit", { label: onlyLabel || humanizeFieldLabel(view.y[0]!), unit: sharedUnit }), tooltip: onlyLabel || humanizeFieldLabel(view.y[0]!) }
    : { axis: sharedUnit && !units.every((unit) => unit.named) ? t("chartAxis.unitOnly", { unit: sharedUnit }) : null, tooltip: t("chartAxis.value") };
  const currency = units.every((unit) => unit.currency === units[0]!.currency && !unit.scale) ? units[0]!.currency ?? "" : "";
  const xValues = rows.map((row) => row[view.x]).filter((value) => value != null);
  // How the chart knows the x axis holds periods or identifiers: its semantic type or column kind says so, or,
  // with no type saying it is a quantity, every value is an integer in 1800–2200 (a year).
  const xIsLabel = xValues.length > 0 && xValues.every((value) => Number.isInteger(numericValue(value)))
    && (LABEL_TYPES.test(semantic(view.x)) || ["string", "date", "datetime"].includes(kinds.get(view.x) ?? "")
      || (!QUANTITY_TYPES.test(semantic(view.x)) && xValues.every((value) => isYear(numericValue(value)))));

  const formatValueAxis = (channel: VegaNode, [lo, hi]: readonly [number, number], side: "width" | "height", abbreviate: boolean) => {
    if (!channel.axis || typeof channel.axis !== "object") return;
    const axis = channel.axis as VegaNode;
    // A semantic format flint chose (a percent, say) stays; the generic one gives way.
    if (axis.labelExpr || (axis.format && axis.format !== ",.12~g")) return;
    delete axis.format;
    const count = typeof axis.tickCount === "number" ? String(axis.tickCount) : `ceil(${side} / 40)`;
    axis.labelExpr = `loupistTick(datum.value, ${lo}, ${hi}, ${count}, ${abbreviate})`;
  };
  const dateGrain = (format: unknown): DateGrain => typeof format !== "string" ? "day" : /%-?[de]/u.test(format) ? "day" : /%[bBm]/u.test(format) ? "month" : "year";
  // The grain flint chose for a time axis (its first layer carries the format), used by every layer and the tooltip.
  let timeFormat: unknown;
  let utc = false;
  visitVega(spec, (node) => {
    const x = (node.encoding as VegaNode | undefined)?.x as VegaNode | undefined;
    if (timeFormat !== undefined || x?.type !== "temporal") return;
    timeFormat = (x.axis as VegaNode | undefined)?.format ?? null;
    utc = (x.scale as VegaNode | undefined)?.type === "utc";
  });
  const grain = dateGrain(timeFormat);
  const formatDateAxis = (channel: VegaNode) => {
    if (!channel.axis || typeof channel.axis !== "object") return;
    const axis = channel.axis as VegaNode;
    if (axis.labelExpr) return;
    axis.labelExpr = `loupistDate(datum.value, ${JSON.stringify({ grain, utc })})`;
    delete axis.format;
  };
  // A value printed on its bar: the answer's amount, not an SI suffix ("302조", not "302T").
  const formatDataLabel = (node: VegaNode, channels: VegaNode) => {
    const text = channels.text as VegaNode | undefined;
    if (!text || typeof text !== "object" || text.type !== "quantitative" || typeof text.field !== "string") return;
    const field = text.field;
    node.transform = [...(Array.isArray(node.transform) ? node.transform : []),
      { calculate: `loupistLabel(datum[${JSON.stringify(field)}], ${abbreviates(view.y)})`, as: "__loupist_label" }];
    channels.text = { field: "__loupist_label", type: "nominal" };
  };
  // Hovering a mark gives its period as given and the value exactly as stored ("302조 1,234억 원").
  const addTooltip = (node: VegaNode, channels: VegaNode) => {
    if (node.__themeSynthetic || channels.text) return;
    const x = channels.x as VegaNode | undefined;
    const y = channels.y as VegaNode | undefined;
    if (!x || !y || typeof x.field !== "string" || typeof y.field !== "string") return;
    const xTitle = labels.get(view.x) || humanizeFieldLabel(view.x);
    const xTip: VegaNode = x.type === "temporal"
      ? { field: x.field, type: "temporal", title: xTitle, format: { grain, utc }, formatType: "loupistDate" }
      : x.type === "quantitative" && !xIsLabel ? { field: x.field, type: "quantitative", title: xTitle, format: "", formatType: "loupistExact" }
      : { field: x.field, type: x.type, title: xTitle };
    const color = channels.color as VegaNode | undefined;
    const series: VegaNode[] = !color || typeof color.field !== "string" ? []
      : color.field === "__flint_series_key" ? [{ field: color.field, type: "nominal", title: t("chartAxis.series"), format: seriesLabels, formatType: "loupistSeries" }]
      : [{ field: color.field, type: "nominal", title: labels.get(color.field) || humanizeFieldLabel(color.field) }];
    channels.tooltip = [xTip, ...series, { field: y.field, type: "quantitative", title: valueTitle.tooltip, format: currency, formatType: "loupistExact" }];
  };

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
        if (yEnc.field === "__flint_series_value") yEnc.title = valueTitle.axis;
        else if (typeof yEnc.field === "string" && labels.has(yEnc.field)) yEnc.title = valueTitle.axis ?? yEnc.title;
        // A unit the dataset names is shown even where the theme would leave the value axis untitled.
        if (valueTitle.axis && yEnc.axis && typeof yEnc.axis === "object" && (yEnc.axis as VegaNode).title === null) delete (yEnc.axis as VegaNode).title;
        if (yEnc.type === "quantitative") {
          yEnc.scale = { ...(yEnc.scale as VegaNode | undefined), zero: true };
          formatValueAxis(yEnc, valueExtent(channels, true), "height", abbreviates(view.y));
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
        if (xEnc.type === "temporal") formatDateAxis(xEnc);
        else if (xEnc.type === "quantitative" && xIsLabel) {
          // A year, a period or an identifier is a category: one band each, in order, printed as given
          // ("2023", never "2,023"), and never a quantity whose bars squeeze against a zero-based domain.
          xEnc.type = "ordinal";
          delete xEnc.scale;
          if (xEnc.axis) for (const key of ["format", "labelExpr", "values", "tickMinStep"]) delete (xEnc.axis as VegaNode)[key];
        } else if (xEnc.type === "quantitative") {
          formatValueAxis(xEnc, extentOf([view.x], false), "width", abbreviates([view.x]));
        }
      }
      formatDataLabel(node, channels);
      addTooltip(node, channels);
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
