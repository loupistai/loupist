import { formatDate, formatDateTime, formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { useEffect, useRef, useState } from "react";
import { dispose, init, registerLocale, type KLineData, type Chart, type OverlayCreate, type Styles, type DeepPartial, type IndicatorCreate } from "klinecharts";
import { ChevronDownIcon } from "lucide-react";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import type { ChartDataset, ChartDocument } from "../../types";
import { datasetRows, numericValue, timestampValue } from "./dataset";
import { drawingOverlays, indicatorConfig, periodFor, ticksClearOfLastPrice } from "./financialAdapter";
import { customSeriesConfig } from "./customSeries";
import { useChartTokens } from "./chartTokens";
import { registerReviewedOverlays } from "./financialOverlays";
import { useChartSnapshotSlot } from "./chartSnapshot";

const t = i18n.getFixedT(null, "chat");

registerReviewedOverlays();

const chartLocale = () => {
  const locale = `loupist-${i18n.language}`;
  registerLocale(locale, {
    time: t("financialChart.time"), open: t("financialChart.open"), high: t("financialChart.high"), low: t("financialChart.low"),
    close: t("financialChart.close"), volume: t("financialChart.volume"), change: t("financialChart.change"), turnover: t("financialChart.turnover"),
    second: t("financialChart.second"), minute: t("financialChart.minute"), hour: t("financialChart.hour"), day: t("financialChart.day"),
    week: t("financialChart.week"), month: t("financialChart.month"), year: t("financialChart.year"),
  });
  return locale;
};

const rangeDateLabel = (value: unknown) => {
  const timestamp = timestampValue(value);
  if (timestamp === undefined) return String(value);
  return typeof value === "string" && /^\d{4}-\d{2}-\d{2}$/.test(value)
    ? formatDate(timestamp, { timeZone: "UTC" }) : formatDateTime(timestamp, { timeZone: "UTC" });
};

/** Indicator pane height inside an answer card, in canvas pixels. */
const COMPACT_PANE_HEIGHT = 48;
/** In the panel a study pane takes this share of the chart's height, from the answer-card height up to the library's 100 px. */
const STUDY_PANE_SHARE = 0.25;
const STUDY_PANE_MAX = 100;
/** The candle legend takes about two lines at the top of the price pane; bars stay below it. */
const LEGEND_CLEARANCE_PX = 56;
/** Below this width the candle legend wraps; it then shows on hover instead of over the bars. */
const NARROW_CHART_PX = 560;
/** A panel chart shorter than this (the default bottom dock) shows the legend on hover; its header names the symbol. */
const SHORT_CHART_PX = 260;
const VOLUME_AVERAGES = [5, 10, 20];

type ColorTokens = ReturnType<typeof useChartTokens>;

/**
 * The volume study with localized legend titles. The library's own VOL template
 * prints "VOLUME:" and "MA5:" in English; keys and calculation stay the library's.
 */
function localizedVolume(indicator: IndicatorCreate, params: readonly number[], currentColors: () => ColorTokens): IndicatorCreate {
  const averages = params.length ? params : VOLUME_AVERAGES;
  return {
    ...indicator,
    shortName: t("financialChart.volumeIndicator"),
    figures: [
      ...averages.map((days, index) => ({ key: `ma${index + 1}`, title: t("financialChart.movingAverage", { days }), type: "line" })),
      {
        key: "volume", title: t("financialChart.volume"), type: "bar", baseValue: 0,
        styles: ({ data }: { data: { current?: Partial<KLineData> | null } }) => {
          const bar = data.current;
          // Read the tokens at draw time so a price-color swap recolors volume bars too.
          const colors = currentColors();
          const color = !bar || bar.close === undefined || bar.open === undefined ? colors.secondary
            : bar.close > bar.open ? colors.up : bar.close < bar.open ? colors.down : colors.secondary;
          return { color };
        },
      },
    ] as IndicatorCreate["figures"],
  };
}

type Props = {
  chart: ChartDocument;
  dataset: ChartDataset;
  theme: "light" | "dark";
  onAskRange?: (range: { start: string; end: string }) => void;
  /** Inline answer cards show the chart alone; drawing and range tools stay in the panel. */
  compact?: boolean;
};

export default function FinancialChart({ chart, dataset, theme, onAskRange, compact = false }: Props) {
  useTranslation("chat");
  const colors = useChartTokens();
  const colorsRef = useRef(colors);
  colorsRef.current = colors;
  const containerRef = useRef<HTMLDivElement>(null);
  const instanceRef = useRef<Chart | null>(null);
  // "Save as PNG…" takes the library's own picture of every pane, with drawings, at the screen's pixel ratio.
  const snapshotSlot = useChartSnapshotSlot();
  useEffect(() => {
    if (!snapshotSlot) return undefined;
    const take = async () => {
      const instance = instanceRef.current;
      if (!instance) throw new Error("Chart is not drawn yet");
      const picture = new Image();
      picture.src = instance.getConvertPictureUrl(true, "png", colorsRef.current.background);
      await picture.decode();
      const canvas = document.createElement("canvas");
      canvas.width = picture.naturalWidth;
      canvas.height = picture.naturalHeight;
      canvas.getContext("2d")?.drawImage(picture, 0, 0);
      const width = containerRef.current?.clientWidth || picture.naturalWidth;
      return { canvas, scale: picture.naturalWidth / width };
    };
    snapshotSlot.current = take;
    return () => { if (snapshotSlot.current === take) snapshotSlot.current = null; };
  }, [snapshotSlot]);
  const history = useRef<OverlayCreate[][]>([]);
  const [historySize, setHistorySize] = useState(0);
  const [editable, setEditable] = useState(false);
  const [color, setColor] = useState(() => colors.info);
  const [rangeStart, setRangeStart] = useState(0);
  const [rangeEnd, setRangeEnd] = useState(Math.max(0, dataset.rows.length - 1));
  const chartStyles = (): DeepPartial<Styles> => ({
        grid: {
          horizontal: { color: colors.border, style: "dashed" },
          vertical: { color: colors.border, style: "dashed" },
        },
        candle: {
          priceMark: {
            high: { color: colors.secondary }, low: { color: colors.secondary },
            last: { upColor: colors.up, downColor: colors.down, noChangeColor: colors.secondary },
          },
          tooltip: { rect: { color: colors.raised, borderColor: colors.border }, legend: { defaultValue: "–" } },
          bar: {
            noChangeColor: colors.secondary,
            noChangeBorderColor: colors.secondary,
            noChangeWickColor: colors.secondary,
            upColor: colors.up,
            downColor: colors.down,
            upBorderColor: colors.up,
            downBorderColor: colors.down,
            upWickColor: colors.up,
            downWickColor: colors.down,
          },
        },
        indicator: {
          ohlc: { upColor: colors.up, downColor: colors.down, noChangeColor: colors.secondary },
          bars: [{ upColor: colors.up, downColor: colors.down, noChangeColor: colors.secondary }],
          lines: colors.series.map((color) => ({ color })),
          // Study values show while the pointer is on the chart; at rest the panes stay clear of
          // half-empty rows (moving averages have no value for the first bars).
          tooltip: { showRule: "follow_cross", legend: { defaultValue: "–" } },
        },
        xAxis: { axisLine: { color: colors.border }, tickLine: { color: colors.border }, tickText: { color: colors.secondary, family: colors.font } },
        yAxis: { axisLine: { color: colors.border }, tickLine: { color: colors.border }, tickText: { color: colors.secondary, family: colors.font } },
        separator: { color: colors.border },
        crosshair: {
          horizontal: { line: { color: colors.secondary }, text: { color: colors.text, backgroundColor: colors.raised, borderColor: colors.border } },
          vertical: { line: { color: colors.secondary }, text: { color: colors.text, backgroundColor: colors.raised, borderColor: colors.border } },
        },
      });
  const rows = datasetRows(dataset);
  const timeColumn = chart.view.kind === "candlestick" ? chart.view.time : "";
  const saveDrawings = () => {
    const current = instanceRef.current;
    if (!current) return;
    const overlays = current.getOverlays().filter((overlay) => overlay.currentStep === -1).map((overlay) => ({
      id: overlay.id, name: overlay.name, points: overlay.points.map((point) => ({ ...point })),
      lock: overlay.lock, styles: overlay.styles, extendData: overlay.extendData,
    }));
    history.current.push(overlays);
    setHistorySize(history.current.length);
  };
  const editCallbacks = { onDrawEnd: saveDrawings, onPressedMoveEnd: saveDrawings };
  const draw = (name: string) => instanceRef.current?.createOverlay({ name, lock: false, styles: { line: { color } }, ...editCallbacks });
  const undo = () => {
    const current = instanceRef.current;
    if (!current || history.current.length < 2) return;
    history.current.pop();
    current.removeOverlay();
    for (const overlay of history.current.at(-1) ?? []) current.createOverlay({ ...overlay, ...editCallbacks });
    setHistorySize(history.current.length);
  };

  useEffect(() => {
    const container = containerRef.current;
    if (!container || chart.view.kind !== "candlestick") return;
    const view = chart.view;
    const bars = datasetRows(dataset).flatMap<KLineData>((row) => {
      const timestamp = timestampValue(row[view.time]);
      const open = numericValue(row[view.open]);
      const high = numericValue(row[view.high]);
      const low = numericValue(row[view.low]);
      const close = numericValue(row[view.close]);
      const volume = view.volume ? numericValue(row[view.volume]) : undefined;
      const turnover = view.turnover ? numericValue(row[view.turnover]) : undefined;
      return timestamp === undefined || open === undefined || high === undefined || low === undefined || close === undefined
        ? []
        : [{ timestamp, open, high, low, close, volume, turnover }];
    });
    // The library tags the latest close on the price axis; labels under that tag are dropped.
    const lastClose = bars.at(-1)?.close;
    const instance = init(container, {
      locale: chartLocale(),
      formatter: {
        formatBigNumber: (value) => Number.isFinite(Number(value)) ? formatNumber(Number(value), { notation: "compact", maximumFractionDigits: 2 }) : String(value),
        formatDate: ({ timestamp, template }) => formatDateTime(timestamp, {
          timeZone: "UTC",
          year: template.includes("YYYY") ? "numeric" : undefined,
          month: template.includes("MM") ? "short" : undefined,
          day: template.includes("DD") ? "numeric" : undefined,
          hour: template.includes("HH") ? "numeric" : undefined,
          minute: template.includes("mm") ? "2-digit" : undefined,
          second: template.includes("ss") ? "2-digit" : undefined,
        }),
      },
      layout: {
        barSpaceLimit: { min: 2, max: 36 },
      },
      styles: chartStyles(),
    });
    if (!instance) return;
    instanceRef.current = instance;
    instance.setDataLoader({
      getBars: ({ type, callback }) => callback(type === "init" ? bars : [], false),
    });
    instance.setSymbol({
      ticker: view.symbol,
      pricePrecision: view.price_precision ?? 2,
      volumePrecision: 0,
    });
    instance.setPeriod(periodFor(view.interval));
    instance.setTimezone("UTC");
    // Each study below the price pane gets a named pane, sized to the chart's height (`fitPanes`).
    const studyPanes: string[] = [];
    for (const study of chart.studies) {
      const indicator = indicatorConfig(study);
      const value = study.module_id === "VOL" ? localizedVolume(indicator.value, study.calc_params.map(Number).filter(Number.isFinite), () => colorsRef.current) : indicator.value;
      const paneId = indicator.isStack ? undefined : `study-${study.module_id}`;
      instance.createIndicator(paneId ? { ...value, paneId } : value, indicator.isStack);
      if (paneId && !studyPanes.includes(paneId)) studyPanes.push(paneId);
    }
    for (const series of chart.custom_series ?? []) {
      instance.createIndicator(customSeriesConfig(series, dataset, view.time), true);
    }
    history.current = [];
    for (const drawing of chart.drawings) {
      for (const overlay of drawingOverlays(drawing)) {
        instance.createOverlay({ ...overlay, ...editCallbacks });
      }
    }
    saveDrawings();
    setEditable(false);
    // Short panes (a docked panel, a narrow window) would put the legend on top of the bars:
    // reserve the legend's height as a top gap that grows as the pane shrinks.
    const fitPanes = () => {
      const height = container.clientHeight;
      // An answer card keeps its study panes small so the price pane stays readable; a panel chart gives each
      // a quarter of its height, so a short dock keeps room for the bars.
      const studyHeight = compact ? COMPACT_PANE_HEIGHT : Math.round(Math.min(STUDY_PANE_MAX, Math.max(COMPACT_PANE_HEIGHT, height * STUDY_PANE_SHARE)));
      for (const id of studyPanes) instance.setPaneOptions({ id, height: studyHeight });
      // A narrow chart wraps the legend over most of a short price pane, and a short panel chart has no room
      // above its bars: there the legend appears on hover only.
      const hoverLegend = container.clientWidth < NARROW_CHART_PX || (!compact && height < SHORT_CHART_PX);
      instance.setStyles?.({ candle: { tooltip: { showRule: hoverLegend ? "follow_cross" : "always" } } });
      const paneHeight = Math.max(1, height * 0.65);
      const top = hoverLegend ? 0.2 : Math.min(0.45, Math.max(0.2, LEGEND_CLEARANCE_PX / paneHeight));
      // A short price pane (an answer card, a hover-legend chart) keeps room under the lowest bar for its low-price mark.
      instance.overrideYAxis?.({ paneId: "candle_pane", gap: { top, bottom: hoverLegend || compact ? 0.18 : 0.08 }, createTicks: ({ defaultTicks }) => ticksClearOfLastPrice(defaultTicks, lastClose) });
    };
    fitPanes();
    instance.resize();
    const observer = new ResizeObserver(() => { fitPanes(); instance.resize(); });
    observer.observe(container);
    return () => {
      observer.disconnect();
      instanceRef.current = null;
      dispose(instance);
    };
  }, [chart, dataset, theme, compact]);

  useEffect(() => {
    instanceRef.current?.setStyles(chartStyles());
  }, [colors]);

  useEffect(() => {
    instanceRef.current?.setLocale(chartLocale());
  }, [i18n.language]);

  return (
    <div className="chart-financial-stage gg-num">
      {/* One row of chrome so the plot keeps its height in a docked or short panel: drawing tools
          sit behind "Draw", then zoom, then the range to ask about. */}
      {compact ? null : <div className="chart-local-toolbar" role="toolbar" aria-label={t("financialChart.chartControls")}>
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button type="button" className="chart-draw-trigger" aria-pressed={editable || undefined}>{t("financialChart.draw")}<ChevronDownIcon aria-hidden="true" /></button>
          </DropdownMenuTrigger>
          <DropdownMenuContent align="start">
            <DropdownMenuItem onSelect={() => draw("segment")}>{t("financialChart.trend")}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => draw("horizontalStraightLine")}>{t("financialChart.level")}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => draw("rect")}>{t("financialChart.rectangle")}</DropdownMenuItem>
            <DropdownMenuItem onSelect={() => draw("measure")}>{t("financialChart.measure")}</DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onSelect={() => {
              const next = !editable; setEditable(next);
              instanceRef.current?.overrideOverlay({ lock: !next });
            }}>{editable ? t("financialChart.stopEditingDrawings") : t("financialChart.editDrawings")}</DropdownMenuItem>
            <DropdownMenuItem disabled={historySize < 2} onSelect={undo}>{t("financialChart.undoDrawing")}</DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
        <input type="color" aria-label={t("financialChart.drawingColor")} value={color} onChange={(event) => setColor(event.target.value)} />
        <span className="chart-toolbar-gap" aria-hidden="true" />
        <button type="button" onClick={() => instanceRef.current?.zoomAtCoordinate(1.2)}>{t("financialChart.zoomIn")}</button>
        <button type="button" onClick={() => instanceRef.current?.zoomAtCoordinate(.8)}>{t("financialChart.zoomOut")}</button>
        <button type="button" onClick={() => instanceRef.current?.scrollToRealTime()}>{t("financialChart.latest")}</button>
        <span className="chart-toolbar-gap" aria-hidden="true" />
        <label>{t("financialChart.from")}<select aria-label={t("financialChart.rangeStart")} value={rangeStart} onChange={(event) => setRangeStart(Number(event.target.value))}>{rows.map((row, index) => <option key={index} value={index}>{rangeDateLabel(row[timeColumn])}</option>)}</select></label>
        <label>{t("financialChart.to")}<select aria-label={t("financialChart.rangeEnd")} value={rangeEnd} onChange={(event) => setRangeEnd(Number(event.target.value))}>{rows.map((row, index) => <option key={index} value={index}>{rangeDateLabel(row[timeColumn])}</option>)}</select></label>
        {onAskRange && <button type="button" disabled={rangeStart > rangeEnd || !rows.length} onClick={() => onAskRange({ start: String(rows[rangeStart]?.[timeColumn]), end: String(rows[rangeEnd]?.[timeColumn]) })}>{t("financialChart.askAboutRange")}</button>}
        <output>{rangeStart <= rangeEnd ? t("financialChart.selectedBars", { count: rangeEnd - rangeStart + 1, formattedCount: formatNumber(rangeEnd - rangeStart + 1) }) : t("financialChart.chooseAStartBeforeTheEnd")}</output>
      </div>}
      <div ref={containerRef} className="chart-financial-canvas" aria-label={t("financialChart.interactiveFinancialChart")} />
    </div>
  );
}
