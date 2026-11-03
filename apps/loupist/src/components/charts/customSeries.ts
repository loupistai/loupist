import { registerIndicator, type IndicatorCreate } from "klinecharts";
import type { ChartCustomSeries, ChartDataset } from "../../types";
import { datasetRows, numericValue, timestampValue } from "./dataset";

const NAME = "LOUPIST_SAVED_SERIES";
let registered = false;
export function customSeriesConfig(series: ChartCustomSeries, dataset: ChartDataset, timeColumn: string): IndicatorCreate {
  if (!registered) {
    registerIndicator<{ value?: number }, number, Record<string, number>>({
      name: NAME,
      figures: [{ key: "value", title: "", type: "line" }],
      calc: (bars, indicator) => bars.map((bar) => ({ value: indicator.extendData[String(bar.timestamp)] })),
    });
    registered = true;
  }
  const values: Record<string, number> = {};
  for (const row of datasetRows(dataset)) {
    const timestamp = timestampValue(row[timeColumn]);
    const value = numericValue(row[series.column]);
    if (timestamp !== undefined && value !== undefined) values[String(timestamp)] = value;
  }
  return {
    id: `loupist-series:${series.id}`, name: NAME, shortName: series.label,
    paneId: series.pane ?? "candle_pane", extendData: values,
    ...(series.color ? { styles: { lines: [{ color: series.color }] } } : {}),
  };
}
