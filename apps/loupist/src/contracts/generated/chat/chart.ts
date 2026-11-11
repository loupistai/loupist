// Generated from Rust by ts-rs; do not edit. Regenerate with:
// LOUPIST_WRITE_CONTRACTS=1 cargo test --manifest-path apps/loupist/src-tauri/Cargo.toml --lib bridge_contract

import type { JsonValue } from "../external";

export type AnalyticChartType = "line" | "area" | "bar" | "scatter";

export type ChartColumn = { id: string, label: string, kind: ChartColumnKind };

export type ChartColumnKind = "string" | "number" | "boolean" | "date" | "datetime";

export type ChartCustomSeries = {
  id: string,
  label: string,
  column: string,
  pane: string | null,
  color: string | null,
};

export type ChartDataset = {
  id: string,
  columns: Array<ChartColumn>,
  rows: Array<Array<JsonValue>>,
  lineage: ChartDatasetLineage,
  digest: string,
};

export type ChartDatasetLineage =
  | {
    "kind": "from_dataset",
    dataset_id: string,
    revision: number,
    digest: string,
    rows_pointer: string,
    columns: Array<ChartResultColumnSelection>,
  }
  | {
    "kind": "from_result",
    receipt: ChartResultReceipt,
    rows_pointer: string,
    columns: Array<ChartResultColumnSelection>,
  }
  | { "kind": "agent_authored", upstream_receipts: Array<ChartResultReceipt> };

export type ChartDocument = {
  custom_series: Array<ChartCustomSeries>,
  dataset_id: string,
  dataset_digest: string,
  view: ChartView,
  studies: Array<ChartStudy>,
  drawings: Array<ChartDrawing>,
  note: string | null,
};

export type ChartDrawing = {
  kind: ChartDrawingKind,
  points: Array<ChartDrawingPoint>,
  color: string | null,
  line_width: number | null,
  line_style: ChartDrawingLineStyle | null,
  label: string | null,
};

export type ChartDrawingKind =
  | "segment"
  | "ray"
  | "line"
  | "horizontal_line"
  | "vertical_line"
  | "price_line"
  | "fibonacci"
  | "horizontal_segment"
  | "horizontal_ray"
  | "vertical_segment"
  | "vertical_ray"
  | "parallel_line"
  | "price_channel"
  | "annotation"
  | "rectangle"
  | "arrow"
  | "measure"
  | "fibonacci_extension"
  | "long_position"
  | "short_position";

export type ChartDrawingLineStyle = "solid" | "dashed";

export type ChartDrawingPoint = {
  /** A date string or a Unix timestamp; validation refuses any other value. */
  timestamp: string | number,
  value: number,
};

export type ChartResultColumnSelection = {
  id: string,
  label: string,
  kind: ChartColumnKind,
  pointer: string,
};

export type ChartResultReceipt = {
  result_ref: string,
  runtime_id: string,
  tool_name: string,
  provider: string | null,
  request_digest: string,
  response_digest: string,
  retrieved_at: string,
  warnings: Array<string>,
  upstream_result_refs: Array<string>,
};

export type ChartStudy = { module_id: ChartStudyModule, calc_params: Array<number> };

export type ChartStudyModule =
  | "AVP"
  | "AO"
  | "BIAS"
  | "BRAR"
  | "BBI"
  | "VOL"
  | "MA"
  | "EMA"
  | "SMA"
  | "BOLL"
  | "CCI"
  | "CR"
  | "DMA"
  | "DMI"
  | "EMV"
  | "MACD"
  | "MTM"
  | "OBV"
  | "PVT"
  | "PSY"
  | "ROC"
  | "RSI"
  | "KDJ"
  | "SAR"
  | "TRIX"
  | "VR"
  | "WR";

export type ChartView =
  | {
    "kind": "candlestick",
    symbol: string,
    interval: string,
    time: string,
    open: string,
    high: string,
    low: string,
    close: string,
    volume: string | null,
    turnover: string | null,
    price_precision: number | null,
  }
  | {
    kind: AnalyticChartType,
    x: string,
    y: Array<string>,
    color: string | null,
    semantic_types: { [key in string]: string },
    title: string | null,
    subtitle: string | null,
  };
