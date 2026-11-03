import { formatNumber } from "@/i18n/format";
import i18n, { useTranslation } from "@/i18n";
import { lazy, Suspense, useId } from "react";
import type { ChartDataset, ChartDocument } from "../../types";
import { Spinner } from "../ui/spinner";
import { datasetRows } from "../charts/dataset";

const t = i18n.getFixedT(null, "chat");

const FinancialChart = lazy(() => import("../charts/FinancialChart"));
const AnalyticChart = lazy(() => import("../charts/AnalyticChart"));

type Props = {
  chart: ChartDocument;
  dataset: ChartDataset;
  theme: "light" | "dark";
  onAskRange?: (range: { start: string; end: string }) => void;
  /** Inline answer cards carry their own title row, so the chart drops its header. */
  compact?: boolean;
};

export default function ArtifactChart({ chart, dataset, theme, onAskRange, compact = false }: Props) {
  useTranslation("chat");
  const { view } = chart;
  const financial = view.kind === "candlestick";
  const descriptionId = useId();
  if (!["candlestick", "line", "area", "bar", "scatter"].includes(view.kind)) {
    return <div className="artifact-panel-state" role="alert">{t("artifactChart.unsupportedFormat")}</div>;
  }
  const rows = financial ? datasetRows(dataset) : [];
  const first = rows[0];
  const latest = rows.at(-1);
  const financialDescription = financial
    ? [
        t("artifactChart.priceDescription", { symbol: view.symbol, interval: view.interval, count: rows.length, formattedCount: formatNumber(rows.length) }),
        first && latest
          ? t("artifactChart.dateRange", { value1: String(first[view.time]), value2: String(latest[view.time]) })
          : null,
        latest ? t("artifactChart.latestClose", { value1: String(latest[view.close]) }) : null,
        chart.studies.length > 0
          ? t("artifactChart.indicators", { value1: chart.studies.map((study) => study.module_id).join(", ") })
          : null,
      ].filter(Boolean).join(" ")
    : undefined;
  return (
    <figure
      className="artifact-chart-shell"
      data-compact={compact || undefined}
      aria-label={financial ? t("artifactChart.financialChart") : t("artifactChart.analyticChart")}
      aria-describedby={financial ? descriptionId : undefined}
    >
      {financialDescription ? (
        <p id={descriptionId} className="sr-only">{financialDescription}</p>
      ) : null}
      {compact ? null : <header className="chart-engine-header">
        <div className="chart-engine-identity">
          <span>
            <strong>{financial ? view.symbol : view.title ?? t("artifactChart.dataAnalysis")}</strong>
            <small>
              {financial
                ? t("artifactChart.intervalBars", { interval: view.interval, count: dataset.rows.length, formattedCount: formatNumber(dataset.rows.length) })
                : t("artifactChart.rows", { count: dataset.rows.length, formattedCount: formatNumber(dataset.rows.length) })}
            </small>
          </span>
        </div>
        {chart.studies.length > 0 ? (
          <div className="chart-study-pills" aria-label={t("artifactChart.activeIndicators")}>
            {chart.studies.map((study) => <span key={study.module_id}>{study.module_id}</span>)}
          </div>
        ) : null}
      </header>}
      <div className="chart-engine-surface">
        <Suspense fallback={<div className="artifact-panel-state" role="status"><Spinner /><span>{t("artifactChart.loadingChart")}</span></div>}>
          {view.kind === "candlestick" ? (
            <FinancialChart chart={chart} dataset={dataset} theme={theme} onAskRange={onAskRange} compact={compact} />
          ) : (
            <AnalyticChart chart={chart} dataset={dataset} theme={theme} />
          )}
        </Suspense>
      </div>
      {chart.note ? <figcaption className="artifact-chart-note">{chart.note}</figcaption> : null}
    </figure>
  );
}
