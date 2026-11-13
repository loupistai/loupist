import i18n, { useTranslation } from "@/i18n";
import { useEffect, useMemo, useRef, useState } from "react";
import vegaEmbed, { type Result as VegaResult } from "vega-embed";
import type { ChartDataset, ChartDocument } from "../../types";
import { assembleAnalyticSpec } from "./analyticAdapter";
import { useChartTokens } from "./chartTokens";
import { datasetRows } from "./dataset";
import { useChartSnapshotSlot } from "./chartSnapshot";

const t = i18n.getFixedT(null, "chat");

type Props = {
  chart: ChartDocument;
  dataset: ChartDataset;
  theme: "light" | "dark";
};

const VEGA_OPTIONS = {
  actions: false,
  ast: true,
  renderer: "svg",
} as const;

const syncVegaSize = (result: VegaResult, container: HTMLElement) => {
  const width = container.clientWidth;
  const height = container.clientHeight;
  if (width < 2 || height < 2) return;
  void result.view.width(width).height(height).resize().runAsync();
};

export default function AnalyticChart({ chart, dataset, theme }: Props) {
  // Axis, label and tooltip text follow the interface language, so a language change draws the chart again.
  const { i18n: { language } } = useTranslation("chat");
  const colors = useChartTokens();
  const containerRef = useRef<HTMLDivElement>(null);
  const resultRef = useRef<VegaResult | null>(null);
  const [error, setError] = useState<string>();
  const rows = useMemo(() => datasetRows(dataset), [dataset]);
  // "Save as PNG…" draws the view again onto a canvas, with the app's fonts.
  const snapshotSlot = useChartSnapshotSlot();
  useEffect(() => {
    if (!snapshotSlot) return undefined;
    const take = async (scale: number) => {
      const result = resultRef.current;
      if (!result) throw new Error("Chart is not drawn yet");
      return { canvas: await result.view.toCanvas(scale), scale };
    };
    snapshotSlot.current = take;
    return () => { if (snapshotSlot.current === take) snapshotSlot.current = null; };
  }, [snapshotSlot]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || chart.view.kind === "candlestick") return;
    let cancelled = false;
    let rendering: VegaResult | undefined;
    setError(undefined);
    try {
      const spec = assembleAnalyticSpec(
        chart.view,
        rows,
        theme,
        container.clientWidth,
        container.clientHeight,
        dataset.columns,
      );
      // Resolve the app palette at render time; Vega cannot consume CSS variables.
      const config = (spec.config ?? {}) as Record<string, unknown>;
      spec.background = colors.background;
      spec.config = {
        ...config,
        font: colors.font,
        background: colors.background,
        view: { ...(config.view as object), stroke: colors.border },
        axis: { ...(config.axis as object), labelColor: colors.secondary, titleColor: colors.text, gridColor: colors.border, domainColor: colors.border, tickColor: colors.border },
        legend: { ...(config.legend as object), labelColor: colors.secondary, titleColor: colors.text },
        title: { ...(config.title as object), color: colors.text },
        range: { ...(config.range as object), category: colors.series },
      };
      void vegaEmbed(container, spec, { ...VEGA_OPTIONS, tooltip: { theme: theme === "dark" ? "dark" : "light" } })
        .then((result) => {
          if (cancelled) {
            result.finalize();
            return;
          }
          rendering = result;
          resultRef.current = result;
          syncVegaSize(result, container);
        })
        .catch((cause: unknown) => {
          if (!cancelled) {
            setError(
              cause instanceof Error
                ? cause.message
                : t("analyticChart.couldNotRenderThisAnalyticChart"),
            );
          }
        });
    } catch (cause) {
      setError(
        cause instanceof Error
          ? cause.message
          : t("analyticChart.couldNotCompileThisAnalyticChart"),
      );
    }
    return () => {
      cancelled = true;
      resultRef.current = null;
      rendering?.finalize();
    };
  }, [chart, rows, dataset.columns, theme, colors, language]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const observer = new ResizeObserver(() => {
      const result = resultRef.current;
      if (result) syncVegaSize(result, container);
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  return (
    <div className="chart-analytic-stage gg-num">
      <div
        ref={containerRef}
        className="chart-analytic-canvas"
        aria-label={t("analyticChart.interactiveAnalyticChart")}
      />
      {error ? (
        <div className="artifact-panel-state" role="alert">
          <strong>{t("analyticChart.chartCouldNotBeRendered")}</strong>
          <p>{error}</p>
        </div>
      ) : null}
    </div>
  );
}
