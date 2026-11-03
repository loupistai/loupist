use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::chat::artifacts::ChatArtifactError;

pub const CHART_SCHEMA: &str = "loupist-chart/1";
const MAX_COLUMNS: usize = 32;
const MAX_ROWS: usize = 10_000;
const MAX_CELL_TEXT_BYTES: usize = 8 * 1024;
const MAX_DATASET_BYTES: usize = 16 * 1024 * 1024;
const MAX_COLUMN_LABEL_BYTES: usize = 160;
const MAX_JSON_POINTER_BYTES: usize = 2 * 1024;
const MAX_LINEAGE_RESULTS: usize = 32;
const MAX_LINEAGE_WARNINGS: usize = 32;
const MAX_LINEAGE_WARNING_BYTES: usize = 2 * 1024;
const MAX_STUDIES: usize = 12;
const MAX_DRAWINGS: usize = 32;
const MAX_DRAWING_LABEL_BYTES: usize = 80;
const MAX_CHART_INTERVAL_SPAN: u16 = 9_999;
const UNIX_SECONDS_CUTOFF: f64 = 10_000_000_000.0;
const JAVASCRIPT_TIMESTAMP_LIMIT_MS: f64 = 8_640_000_000_000_000.0;

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_:/".contains(&byte) || byte == b'.')
}

fn valid_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.contains('\0')
}

fn valid_chart_interval(value: &str) -> bool {
    ["wk", "mo", "m", "h", "d"].into_iter().any(|suffix| {
        value.strip_suffix(suffix).is_some_and(|span| {
            !span.is_empty()
                && !span.starts_with('0')
                && span.bytes().all(|byte| byte.is_ascii_digit())
                && span
                    .parse::<u16>()
                    .is_ok_and(|span| (1..=MAX_CHART_INTERVAL_SPAN).contains(&span))
        })
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartColumnKind {
    String,
    Number,
    Boolean,
    Date,
    Datetime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartColumn {
    pub id: String,
    pub label: String,
    pub kind: ChartColumnKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartResultReceipt {
    pub result_ref: String,
    pub runtime_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub provider: Option<String>,
    pub request_digest: String,
    pub response_digest: String,
    pub retrieved_at: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub upstream_result_refs: Vec<String>,
}

impl ChartResultReceipt {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        if !valid_identifier(&self.result_ref)
            || !valid_identifier(&self.runtime_id)
            || !valid_identifier(&self.tool_name)
            || self
                .provider
                .as_deref()
                .is_some_and(|provider| !valid_identifier(provider))
            || !valid_digest(&self.request_digest)
            || !valid_digest(&self.response_digest)
            || !valid_text(&self.retrieved_at, 80)
            || self.warnings.len() > MAX_LINEAGE_WARNINGS
            || self
                .warnings
                .iter()
                .any(|warning| !valid_text(warning, MAX_LINEAGE_WARNING_BYTES))
            || self.upstream_result_refs.len() > MAX_LINEAGE_RESULTS
            || self
                .upstream_result_refs
                .iter()
                .any(|result_ref| !valid_identifier(result_ref))
        {
            return Err(ChatArtifactError::Invalid("chart result receipt"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartResultColumnSelection {
    pub id: String,
    pub label: String,
    pub kind: ChartColumnKind,
    pub pointer: String,
}

impl ChartResultColumnSelection {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        validate_column(&ChartColumn {
            id: self.id.clone(),
            label: self.label.clone(),
            kind: self.kind.clone(),
        })?;
        validate_json_pointer(&self.pointer)
    }

    fn column(&self) -> ChartColumn {
        ChartColumn {
            id: self.id.clone(),
            label: self.label.clone(),
            kind: self.kind.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChartDatasetLineage {
    FromDataset {
        dataset_id: String,
        revision: u32,
        digest: String,
        rows_pointer: String,
        columns: Vec<ChartResultColumnSelection>,
    },
    FromResult {
        receipt: Box<ChartResultReceipt>,
        rows_pointer: String,
        columns: Vec<ChartResultColumnSelection>,
    },
    AgentAuthored {
        #[serde(default)]
        upstream_receipts: Vec<ChartResultReceipt>,
    },
}

impl ChartDatasetLineage {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        match self {
            Self::FromDataset {
                dataset_id,
                revision,
                digest,
                rows_pointer,
                columns,
            } => {
                if !valid_identifier(dataset_id)
                    || *revision == 0
                    || digest.len() != 64
                    || !digest.bytes().all(|byte| byte.is_ascii_hexdigit())
                {
                    return Err(ChatArtifactError::Invalid("chart dataset revision binding"));
                }
                validate_json_pointer(rows_pointer)?;
                if columns.is_empty() || columns.len() > MAX_COLUMNS {
                    return Err(ChatArtifactError::Invalid("chart dataset selection"));
                }
                let mut ids = BTreeSet::new();
                for column in columns {
                    column.validate()?;
                    if !ids.insert(&column.id) {
                        return Err(ChatArtifactError::Invalid("chart dataset selection"));
                    }
                }
            }
            Self::FromResult {
                receipt,
                rows_pointer,
                columns,
            } => {
                receipt.validate()?;
                validate_json_pointer(rows_pointer)?;
                if columns.is_empty() || columns.len() > MAX_COLUMNS {
                    return Err(ChatArtifactError::Invalid("chart result selection"));
                }
                let mut ids = BTreeSet::new();
                for column in columns {
                    column.validate()?;
                    if !ids.insert(column.id.as_str()) {
                        return Err(ChatArtifactError::Invalid("chart result selection"));
                    }
                }
            }
            Self::AgentAuthored { upstream_receipts } => {
                if upstream_receipts.len() > MAX_LINEAGE_RESULTS {
                    return Err(ChatArtifactError::Invalid("chart dataset lineage"));
                }
                let mut result_refs = BTreeSet::new();
                for receipt in upstream_receipts {
                    receipt.validate()?;
                    if !result_refs.insert(receipt.result_ref.as_str()) {
                        return Err(ChatArtifactError::Invalid("chart dataset lineage"));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartDataset {
    pub id: String,
    pub columns: Vec<ChartColumn>,
    pub rows: Vec<Vec<Value>>,
    pub lineage: ChartDatasetLineage,
    pub digest: String,
}

impl ChartDataset {
    pub fn new(
        id: String,
        columns: Vec<ChartColumn>,
        rows: Vec<Vec<Value>>,
        lineage: ChartDatasetLineage,
    ) -> Result<Self, ChatArtifactError> {
        let digest = dataset_digest(&columns, &rows, &lineage)?;
        let dataset = Self {
            id,
            columns,
            rows,
            lineage,
            digest,
        };
        dataset.validate()?;
        Ok(dataset)
    }

    pub fn validate(&self) -> Result<(), ChatArtifactError> {
        if !valid_identifier(&self.id)
            || self.columns.is_empty()
            || self.columns.len() > MAX_COLUMNS
            || self.rows.is_empty()
            || self.rows.len() > MAX_ROWS
        {
            return Err(ChatArtifactError::Invalid("chart dataset shape"));
        }
        let mut names = BTreeSet::new();
        for column in &self.columns {
            validate_column(column)?;
            if !names.insert(column.id.as_str()) {
                return Err(ChatArtifactError::Invalid("chart dataset column"));
            }
        }
        for row in &self.rows {
            if row.len() != self.columns.len()
                || row
                    .iter()
                    .zip(&self.columns)
                    .any(|(cell, column)| !valid_typed_cell(cell, &column.kind))
            {
                return Err(ChatArtifactError::Invalid("chart dataset row"));
            }
        }
        self.lineage.validate()?;
        let encoded = serde_json::to_vec(&(&self.columns, &self.rows, &self.lineage))
            .map_err(|_| ChatArtifactError::Digest)?;
        if encoded.len() > MAX_DATASET_BYTES || self.digest != hex::encode(Sha256::digest(&encoded))
        {
            return Err(ChatArtifactError::Invalid("chart dataset digest"));
        }
        Ok(())
    }

    pub fn summary(&self) -> ChartDatasetSummary {
        ChartDatasetSummary {
            dataset_id: self.id.clone(),
            columns: self.columns.clone(),
            row_count: self.rows.len(),
            lineage: self.lineage.clone(),
            digest: self.digest.clone(),
        }
    }
}

fn validate_column(column: &ChartColumn) -> Result<(), ChatArtifactError> {
    if !valid_identifier(&column.id) || !valid_text(&column.label, MAX_COLUMN_LABEL_BYTES) {
        return Err(ChatArtifactError::Invalid("chart dataset column"));
    }
    Ok(())
}

fn valid_typed_cell(value: &Value, kind: &ChartColumnKind) -> bool {
    if value.is_null() {
        return true;
    }
    if !valid_cell(value) {
        return false;
    }
    match kind {
        ChartColumnKind::String => value.is_string(),
        ChartColumnKind::Number => chart_number(value).is_some(),
        ChartColumnKind::Boolean => value.is_boolean(),
        ChartColumnKind::Date => value
            .as_str()
            .is_some_and(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()),
        ChartColumnKind::Datetime => chart_timestamp(value).is_some(),
    }
}

fn valid_cell(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(value) => value.len() <= MAX_CELL_TEXT_BYTES && !value.contains('\0'),
        Value::Array(_) | Value::Object(_) => false,
    }
}

fn dataset_digest(
    columns: &[ChartColumn],
    rows: &[Vec<Value>],
    lineage: &ChartDatasetLineage,
) -> Result<String, ChatArtifactError> {
    let bytes =
        serde_json::to_vec(&(columns, rows, lineage)).map_err(|_| ChatArtifactError::Digest)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn validate_json_pointer(pointer: &str) -> Result<(), ChatArtifactError> {
    if !crate::util::json_pointer::valid_json_pointer(pointer, MAX_JSON_POINTER_BYTES, true) {
        return Err(ChatArtifactError::Invalid("chart JSON pointer"));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ChartDatasetSummary {
    pub dataset_id: String,
    pub columns: Vec<ChartColumn>,
    pub row_count: usize,
    pub lineage: ChartDatasetLineage,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartDocument {
    #[serde(default)]
    pub custom_series: Vec<ChartCustomSeries>,
    pub dataset_id: String,
    pub dataset_digest: String,
    pub view: ChartView,
    #[serde(default)]
    pub studies: Vec<ChartStudy>,
    #[serde(default)]
    pub drawings: Vec<ChartDrawing>,
    #[serde(default)]
    pub note: Option<String>,
}

impl ChartDocument {
    pub fn validate(&self) -> Result<(), ChatArtifactError> {
        if !valid_identifier(&self.dataset_id)
            || self.dataset_digest.len() != 64
            || !self
                .dataset_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || self.custom_series.len() > MAX_STUDIES
            || self.studies.len() > MAX_STUDIES
            || self.drawings.len() > MAX_DRAWINGS
            || self
                .note
                .as_deref()
                .is_some_and(|value| value.len() > 2_000 || value.contains('\0'))
        {
            return Err(ChatArtifactError::Invalid("chart document"));
        }
        self.view.validate()?;
        if matches!(self.view, ChartView::Analytic { .. })
            && (!self.studies.is_empty()
                || !self.drawings.is_empty()
                || !self.custom_series.is_empty())
        {
            return Err(ChatArtifactError::Invalid(
                "analytic chart study or drawing",
            ));
        }
        let mut series_ids = BTreeSet::new();
        for series in &self.custom_series {
            series.validate()?;
            if !series_ids.insert(&series.id) {
                return Err(ChatArtifactError::Invalid("duplicate custom chart series"));
            }
        }
        let mut module_ids = BTreeSet::new();
        for study in &self.studies {
            study.validate()?;
            if !module_ids.insert(study.module_id) {
                return Err(ChatArtifactError::Invalid("duplicate chart study"));
            }
        }
        for drawing in &self.drawings {
            drawing.validate()?;
        }
        Ok(())
    }

    pub fn validate_dataset(&self, dataset: &ChartDataset) -> Result<(), ChatArtifactError> {
        dataset.validate()?;
        if dataset.id != self.dataset_id || dataset.digest != self.dataset_digest {
            return Err(ChatArtifactError::Invalid("chart dataset binding"));
        }
        let fields = dataset
            .columns
            .iter()
            .enumerate()
            .map(|(index, column)| (column.id.as_str(), index))
            .collect::<BTreeMap<_, _>>();
        if self
            .view
            .referenced_fields()
            .iter()
            .any(|field| !fields.contains_key(field))
        {
            return Err(ChatArtifactError::Invalid("view column mappings: every column id must exist in dataset.columns; use the returned column ids"));
        }
        for series in &self.custom_series {
            let Some(index) = fields.get(series.column.as_str()) else {
                return Err(ChatArtifactError::Invalid(
                    "custom chart series field binding",
                ));
            };
            if dataset.columns[*index].kind != ChartColumnKind::Number {
                return Err(ChatArtifactError::Invalid(
                    "custom chart series must be numeric",
                ));
            }
        }
        if let ChartView::Financial {
            time,
            open,
            high,
            low,
            close,
            volume,
            turnover,
            ..
        } = &self.view
        {
            if self
                .studies
                .iter()
                .any(|study| study.module_id.requires_volume())
                && volume.is_none()
            {
                return Err(ChatArtifactError::Invalid(
                    "financial chart study requires volume",
                ));
            }
            if self
                .studies
                .iter()
                .any(|study| study.module_id.requires_turnover())
                && turnover.is_none()
            {
                return Err(ChatArtifactError::Invalid(
                    "financial chart study requires turnover",
                ));
            }
            validate_financial_rows(
                dataset,
                FinancialFieldIndices {
                    time: fields[time.as_str()],
                    open: fields[open.as_str()],
                    high: fields[high.as_str()],
                    low: fields[low.as_str()],
                    close: fields[close.as_str()],
                    volume: volume.as_deref().map(|field| fields[field]),
                    turnover: turnover.as_deref().map(|field| fields[field]),
                },
            )?;
            validate_financial_drawing_bounds(dataset, fields[time.as_str()], &self.drawings)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartCustomSeries {
    pub id: String,
    pub label: String,
    pub column: String,
    #[serde(default)]
    pub pane: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

impl ChartCustomSeries {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        if !valid_identifier(&self.id)
            || !valid_identifier(&self.column)
            || self.label.trim().is_empty()
            || self.label.len() > 120
            || self.label.contains('\0')
            || self
                .pane
                .as_deref()
                .is_some_and(|pane| !valid_identifier(pane))
            || self.color.as_deref().is_some_and(|color| {
                color.len() != 7
                    || !color.starts_with('#')
                    || !color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(ChatArtifactError::Invalid("custom chart series"));
        }
        Ok(())
    }
}

fn validate_financial_drawing_bounds(
    dataset: &ChartDataset,
    time: usize,
    drawings: &[ChartDrawing],
) -> Result<(), ChatArtifactError> {
    let first = chart_timestamp(&dataset.rows[0][time])
        .ok_or(ChatArtifactError::Invalid("financial chart timestamp"))?;
    let last = chart_timestamp(&dataset.rows[dataset.rows.len() - 1][time])
        .ok_or(ChatArtifactError::Invalid("financial chart timestamp"))?;
    if drawings
        .iter()
        .flat_map(|drawing| &drawing.points)
        .any(|point| {
            chart_timestamp(&point.timestamp)
                .is_none_or(|timestamp| timestamp < first || timestamp > last)
        })
    {
        return Err(ChatArtifactError::Invalid("financial chart drawing range"));
    }
    Ok(())
}

struct FinancialFieldIndices {
    time: usize,
    open: usize,
    high: usize,
    low: usize,
    close: usize,
    volume: Option<usize>,
    turnover: Option<usize>,
}

fn validate_financial_rows(
    dataset: &ChartDataset,
    fields: FinancialFieldIndices,
) -> Result<(), ChatArtifactError> {
    let mut previous_time = None;
    for row in &dataset.rows {
        let timestamp = chart_timestamp(&row[fields.time])
            .ok_or(ChatArtifactError::Invalid("financial chart timestamp"))?;
        if previous_time.is_some_and(|previous| timestamp <= previous) {
            return Err(ChatArtifactError::Invalid(
                "financial chart timestamp order",
            ));
        }
        previous_time = Some(timestamp);
        let open = chart_number(&row[fields.open])
            .ok_or(ChatArtifactError::Invalid("financial chart OHLC"))?;
        let high = chart_number(&row[fields.high])
            .ok_or(ChatArtifactError::Invalid("financial chart OHLC"))?;
        let low = chart_number(&row[fields.low])
            .ok_or(ChatArtifactError::Invalid("financial chart OHLC"))?;
        let close = chart_number(&row[fields.close])
            .ok_or(ChatArtifactError::Invalid("financial chart OHLC"))?;
        if high < open.max(close) || low > open.min(close) || high < low {
            return Err(ChatArtifactError::Invalid("financial chart OHLC range"));
        }
        if fields
            .volume
            .is_some_and(|index| chart_number(&row[index]).is_none_or(|value| value < 0.0))
        {
            return Err(ChatArtifactError::Invalid("financial chart volume"));
        }
        if fields
            .turnover
            .is_some_and(|index| chart_number(&row[index]).is_none_or(|value| value < 0.0))
        {
            return Err(ChatArtifactError::Invalid("financial chart turnover"));
        }
    }
    Ok(())
}

fn chart_number(value: &Value) -> Option<f64> {
    let value = match value {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    value.is_finite().then_some(value)
}

fn chart_timestamp(value: &Value) -> Option<f64> {
    match value {
        // Numeric timestamps are unambiguous only for the Unix era. Values
        // below the cutoff are seconds; larger values are milliseconds.
        // Pre-1970 dates remain available through explicit ISO strings.
        Value::Number(_) => chart_number(value).and_then(|timestamp| {
            if timestamp < 0.0 {
                return None;
            }
            let timestamp_ms = if timestamp < UNIX_SECONDS_CUTOFF {
                timestamp * 1_000.0
            } else {
                timestamp
            };
            (timestamp_ms <= JAVASCRIPT_TIMESTAMP_LIMIT_MS).then_some(timestamp_ms)
        }),
        Value::String(value) => chrono::DateTime::parse_from_rfc3339(value)
            .map(|timestamp| timestamp.timestamp_millis() as f64)
            .or_else(|_| {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").map(|date| {
                    date.and_hms_opt(0, 0, 0)
                        .unwrap()
                        .and_utc()
                        .timestamp_millis() as f64
                })
            })
            .ok()
            .filter(|timestamp| timestamp.abs() <= JAVASCRIPT_TIMESTAMP_LIMIT_MS),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ChartView {
    #[serde(rename = "candlestick")]
    Financial {
        symbol: String,
        interval: String,
        time: String,
        open: String,
        high: String,
        low: String,
        close: String,
        #[serde(default)]
        volume: Option<String>,
        #[serde(default)]
        turnover: Option<String>,
        #[serde(default)]
        price_precision: Option<u8>,
    },
    #[serde(untagged)]
    Analytic {
        #[serde(rename = "kind")]
        chart_type: AnalyticChartType,
        x: String,
        #[serde(deserialize_with = "deserialize_y_columns")]
        y: Vec<String>,
        #[serde(default)]
        color: Option<String>,
        #[serde(default)]
        semantic_types: BTreeMap<String, String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        subtitle: Option<String>,
    },
}

fn deserialize_y_columns<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<String>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Columns {
        One(String),
        Many(Vec<String>),
    }
    Ok(match Columns::deserialize(deserializer)? {
        Columns::One(column) => vec![column],
        Columns::Many(columns) => columns,
    })
}

impl ChartView {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        match self {
            Self::Financial {
                symbol,
                interval,
                time,
                open,
                high,
                low,
                close,
                volume,
                turnover,
                price_precision,
            } => {
                if !valid_text(symbol, 120) {
                    return Err(ChatArtifactError::Invalid(
                        "view.symbol: expected nonblank text up to 120 bytes",
                    ));
                }
                if !valid_chart_interval(interval) {
                    return Err(ChatArtifactError::Invalid("view.interval: expected a positive span with m, h, d, wk, or mo (for example 1d)"));
                }
                for (message, value) in [
                    ("view.time: expected a dataset column id", time),
                    ("view.open: expected a dataset column id", open),
                    ("view.high: expected a dataset column id", high),
                    ("view.low: expected a dataset column id", low),
                    ("view.close: expected a dataset column id", close),
                ] {
                    if !valid_identifier(value) {
                        return Err(ChatArtifactError::Invalid(message));
                    }
                }
                for (message, value) in [
                    ("view.volume: expected a dataset column id", volume),
                    ("view.turnover: expected a dataset column id", turnover),
                ] {
                    if value
                        .as_deref()
                        .is_some_and(|value| !valid_identifier(value))
                    {
                        return Err(ChatArtifactError::Invalid(message));
                    }
                }
                if price_precision.is_some_and(|value| value > 12) {
                    return Err(ChatArtifactError::Invalid(
                        "view.price_precision: expected an integer from 0 to 12",
                    ));
                }
            }
            Self::Analytic {
                x,
                y,
                color,
                semantic_types,
                title,
                subtitle,
                ..
            } => {
                let mut referenced_fields = BTreeSet::from([x.as_str()]);
                referenced_fields.extend(y.iter().map(String::as_str));
                referenced_fields.extend(color.as_deref());
                if !valid_identifier(x) {
                    return Err(ChatArtifactError::Invalid(
                        "view.x: expected a dataset column id",
                    ));
                }
                if y.is_empty() || y.len() > 8 || y.iter().any(|field| !valid_identifier(field)) {
                    return Err(ChatArtifactError::Invalid(
                        "view.y: expected a column id or an array of 1–8 column ids",
                    ));
                }
                if color.is_some() && y.len() > 1 {
                    return Err(ChatArtifactError::Invalid("view.color: grouping requires one y column; omit color for multiple y columns"));
                }
                if referenced_fields.len() != 1 + y.len() + usize::from(color.is_some()) {
                    return Err(ChatArtifactError::Invalid(
                        "view.x/y/color: use distinct column ids; remove duplicates",
                    ));
                }
                if color
                    .as_deref()
                    .is_some_and(|field| !valid_identifier(field))
                {
                    return Err(ChatArtifactError::Invalid(
                        "view.color: expected a dataset column id",
                    ));
                }
                if semantic_types.len() > MAX_COLUMNS
                    || semantic_types.iter().any(|(field, semantic)| {
                        !referenced_fields.contains(field.as_str()) || !valid_text(semantic, 64)
                    })
                {
                    return Err(ChatArtifactError::Invalid("view.semantic_types: map only x/y/color column ids to nonblank type names up to 64 bytes"));
                }
                if title
                    .as_deref()
                    .is_some_and(|value| !valid_text(value, 200))
                {
                    return Err(ChatArtifactError::Invalid(
                        "view.title: expected nonblank text up to 200 bytes",
                    ));
                }
                if subtitle
                    .as_deref()
                    .is_some_and(|value| !valid_text(value, 400))
                {
                    return Err(ChatArtifactError::Invalid(
                        "view.subtitle: expected nonblank text up to 400 bytes",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn missing_column_mappings(&self, dataset: &ChartDataset) -> Vec<String> {
        let columns = dataset
            .columns
            .iter()
            .map(|column| column.id.as_str())
            .collect::<BTreeSet<_>>();
        let mappings: Vec<(&str, &str)> = match self {
            Self::Financial {
                time,
                open,
                high,
                low,
                close,
                volume,
                turnover,
                ..
            } => {
                let mut mappings = vec![
                    ("time", time.as_str()),
                    ("open", open),
                    ("high", high),
                    ("low", low),
                    ("close", close),
                ];
                if let Some(value) = volume {
                    mappings.push(("volume", value));
                }
                if let Some(value) = turnover {
                    mappings.push(("turnover", value));
                }
                mappings
            }
            Self::Analytic { x, y, color, .. } => {
                let mut mappings = vec![("x", x.as_str())];
                mappings.extend(y.iter().map(|value| ("y", value.as_str())));
                if let Some(value) = color {
                    mappings.push(("color", value));
                }
                mappings
            }
        };
        mappings
            .into_iter()
            .filter(|(_, id)| !columns.contains(id))
            .map(|(field, id)| format!("view.{field}: unknown column id {id}"))
            .collect()
    }

    pub fn referenced_fields(&self) -> Vec<&str> {
        match self {
            Self::Financial {
                time,
                open,
                high,
                low,
                close,
                volume,
                turnover,
                ..
            } => {
                let mut fields = vec![time.as_str(), open, high, low, close];
                fields.extend(volume.as_deref());
                fields.extend(turnover.as_deref());
                fields
            }
            Self::Analytic { x, y, color, .. } => {
                let mut fields = vec![x.as_str()];
                fields.extend(y.iter().map(String::as_str));
                fields.extend(color.as_deref());
                fields
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticChartType {
    Line,
    Area,
    Bar,
    Scatter,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartDrawing {
    pub kind: ChartDrawingKind,
    pub points: Vec<ChartDrawingPoint>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub line_width: Option<u8>,
    #[serde(default)]
    pub line_style: Option<ChartDrawingLineStyle>,
    #[serde(default)]
    pub label: Option<String>,
}

impl ChartDrawing {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        let label_ok = match (&self.kind, self.label.as_deref()) {
            (ChartDrawingKind::Annotation, None) => false,
            (_, None) => true,
            (_, Some(label)) => valid_drawing_label(label),
        };
        if self.points.len() != self.kind.point_count()
            || !label_ok
            || self
                .color
                .as_deref()
                .is_some_and(|color| !valid_chart_color(color))
            || self
                .line_width
                .is_some_and(|line_width| !(1..=8).contains(&line_width))
            || self.points.iter().any(|point| {
                chart_timestamp(&point.timestamp).is_none() || !point.value.is_finite()
            })
        {
            return Err(ChatArtifactError::Invalid("financial chart drawing"));
        }
        Ok(())
    }
}

fn valid_drawing_label(label: &str) -> bool {
    !label.trim().is_empty()
        && label.len() <= MAX_DRAWING_LABEL_BYTES
        && !label.contains('\0')
        && !label.contains('\n')
        && !label.contains('\r')
}

fn valid_chart_color(color: &str) -> bool {
    matches!(color.len(), 7 | 9)
        && color.starts_with('#')
        && color[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartDrawingPoint {
    pub timestamp: Value,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartDrawingKind {
    Segment,
    Ray,
    Line,
    HorizontalLine,
    VerticalLine,
    PriceLine,
    Fibonacci,
    HorizontalSegment,
    HorizontalRay,
    VerticalSegment,
    VerticalRay,
    ParallelLine,
    PriceChannel,
    Annotation,
    Rectangle,
    Arrow,
    Measure,
    FibonacciExtension,
    LongPosition,
    ShortPosition,
}

impl ChartDrawingKind {
    fn point_count(self) -> usize {
        match self {
            Self::HorizontalLine | Self::VerticalLine | Self::PriceLine | Self::Annotation => 1,
            Self::ParallelLine
            | Self::PriceChannel
            | Self::FibonacciExtension
            | Self::LongPosition
            | Self::ShortPosition => 3,
            _ => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartDrawingLineStyle {
    Solid,
    Dashed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartStudy {
    pub module_id: ChartStudyModule,
    #[serde(default)]
    pub calc_params: Vec<f64>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChartStudyModule {
    Avp,
    Ao,
    Bias,
    Brar,
    Bbi,
    Vol,
    Ma,
    Ema,
    Sma,
    Boll,
    Cci,
    Cr,
    Dma,
    Dmi,
    Emv,
    Macd,
    Mtm,
    Obv,
    Pvt,
    Psy,
    Roc,
    Rsi,
    Kdj,
    Sar,
    Trix,
    Vr,
    Wr,
}

impl ChartStudyModule {
    fn requires_volume(self) -> bool {
        matches!(
            self,
            Self::Avp | Self::Emv | Self::Obv | Self::Pvt | Self::Vol | Self::Vr
        )
    }

    fn requires_turnover(self) -> bool {
        matches!(self, Self::Avp)
    }
}

impl ChartStudy {
    fn validate(&self) -> Result<(), ChatArtifactError> {
        let valid_params = self.calc_params.is_empty()
            || match self.module_id {
                ChartStudyModule::Vol
                | ChartStudyModule::Ma
                | ChartStudyModule::Ema
                | ChartStudyModule::Rsi
                | ChartStudyModule::Bias
                | ChartStudyModule::Wr => {
                    self.calc_params.len() <= 16
                        && self.calc_params.iter().all(|value| valid_period(*value))
                }
                ChartStudyModule::Avp | ChartStudyModule::Pvt => false,
                ChartStudyModule::Ao => {
                    valid_period_shape(&self.calc_params, 2)
                        && self.calc_params[0] < self.calc_params[1]
                }
                ChartStudyModule::Boll => {
                    self.calc_params.len() == 2
                        && valid_period(self.calc_params[0])
                        && valid_positive(self.calc_params[1])
                }
                ChartStudyModule::Sma => {
                    self.calc_params.len() == 2
                        && self.calc_params.iter().all(|value| valid_period(*value))
                        && self.calc_params[1] <= self.calc_params[0]
                }
                ChartStudyModule::Macd => {
                    valid_period_shape(&self.calc_params, 3)
                        && self.calc_params[0] < self.calc_params[1]
                }
                ChartStudyModule::Kdj => valid_period_shape(&self.calc_params, 3),
                ChartStudyModule::Brar | ChartStudyModule::Cci | ChartStudyModule::Obv => {
                    valid_period_shape(&self.calc_params, 1)
                }
                ChartStudyModule::Bbi => valid_period_shape(&self.calc_params, 4),
                ChartStudyModule::Cr => valid_period_shape(&self.calc_params, 5),
                ChartStudyModule::Dma => {
                    valid_period_shape(&self.calc_params, 3)
                        && self.calc_params[0] < self.calc_params[1]
                }
                ChartStudyModule::Dmi
                | ChartStudyModule::Emv
                | ChartStudyModule::Mtm
                | ChartStudyModule::Psy
                | ChartStudyModule::Roc
                | ChartStudyModule::Trix
                | ChartStudyModule::Vr => valid_period_shape(&self.calc_params, 2),
                ChartStudyModule::Sar => {
                    self.calc_params.len() == 3
                        && self.calc_params.iter().all(|value| valid_positive(*value))
                        && self.calc_params[0] <= self.calc_params[2]
                        && self.calc_params[1] <= self.calc_params[2]
                }
            };
        if !valid_params {
            return Err(ChatArtifactError::Invalid("chart study"));
        }
        Ok(())
    }
}

fn valid_period_shape(values: &[f64], length: usize) -> bool {
    values.len() == length && values.iter().all(|value| valid_period(*value))
}

fn valid_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value <= 10_000.0
}

fn valid_period(value: f64) -> bool {
    valid_positive(value) && value.fract() == 0.0
}

#[derive(Clone, Debug)]
pub enum ChartDatasetInput {
    FromDataset(ChartFromDatasetEnvelope),
    FromResult(ChartFromResultEnvelope),
    Inline(ChartInlineEnvelope),
}

impl<'de> Deserialize<'de> for ChartDatasetInput {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .filter(|object| object.len() == 1)
            .ok_or_else(|| {
                D::Error::custom(
                    "dataset: supply exactly one of from_dataset, from_result, or inline",
                )
            })?;
        match object.keys().next().map(String::as_str) {
            Some("from_dataset") => serde_json::from_value(value)
                .map(Self::FromDataset)
                .map_err(|_| D::Error::custom("dataset.from_dataset: expected {dataset_id: string, revision: integer, rows_pointer?: string, columns: array} matching the tool schema")),
            Some("from_result") => serde_json::from_value(value)
                .map(Self::FromResult)
                .map_err(|_| D::Error::custom("dataset.from_result: expected {result_ref: string, rows_pointer: string, columns: array} matching the tool schema")),
            Some("inline") => serde_json::from_value(value)
                .map(Self::Inline)
                .map_err(|_| D::Error::custom("dataset.inline: expected {columns: array, rows: array} matching the tool schema")),
            _ => Err(D::Error::custom(
                "dataset: supply from_dataset, from_result, or inline",
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartFromDatasetEnvelope {
    pub from_dataset: ChartFromDatasetInput,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartFromDatasetInput {
    pub dataset_id: String,
    pub revision: u32,
    #[serde(default, alias = "pointer")]
    pub rows_pointer: String,
    pub columns: Vec<ChartResultColumnSelection>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartFromResultEnvelope {
    pub from_result: ChartFromResultInput,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartInlineEnvelope {
    pub inline: ChartInlineInput,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartFromResultInput {
    pub result_ref: String,
    pub rows_pointer: String,
    pub columns: Vec<ChartResultColumnSelection>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChartInlineInput {
    pub columns: Vec<ChartColumn>,
    pub rows: Vec<Vec<Value>>,
    #[serde(default)]
    pub upstream_result_refs: Vec<String>,
}

pub fn dataset_from_result_selection(
    id: String,
    input: ChartFromResultInput,
    payload: &Value,
    receipt: ChartResultReceipt,
) -> Result<ChartDataset, ChatArtifactError> {
    if input.result_ref != receipt.result_ref {
        return Err(ChatArtifactError::Invalid("chart result receipt binding"));
    }
    validate_json_pointer(&input.rows_pointer)?;
    if input.columns.is_empty() || input.columns.len() > MAX_COLUMNS {
        return Err(ChatArtifactError::Invalid("chart result selection"));
    }
    let selected_rows = payload
        .pointer(&input.rows_pointer)
        .and_then(Value::as_array)
        .ok_or(ChatArtifactError::Invalid("chart rows pointer"))?;
    if selected_rows.is_empty() || selected_rows.len() > MAX_ROWS {
        return Err(ChatArtifactError::Invalid("chart dataset shape"));
    }
    for column in &input.columns {
        column.validate()?;
    }
    let rows = selected_rows
        .iter()
        .map(|row| {
            input
                .columns
                .iter()
                .map(|column| {
                    row.pointer(&column.pointer)
                        .cloned()
                        .ok_or(ChatArtifactError::Invalid("chart column pointer"))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let columns = input
        .columns
        .iter()
        .map(ChartResultColumnSelection::column)
        .collect::<Vec<_>>();
    ChartDataset::new(
        id,
        columns,
        rows,
        ChartDatasetLineage::FromResult {
            receipt: Box::new(receipt),
            rows_pointer: input.rows_pointer,
            columns: input.columns,
        },
    )
}

pub fn dataset_from_dataset_selection(
    id: String,
    input: ChartFromDatasetInput,
    payload: &Value,
    source_digest: String,
) -> Result<ChartDataset, ChatArtifactError> {
    validate_json_pointer(&input.rows_pointer)?;
    if input.columns.is_empty() || input.columns.len() > MAX_COLUMNS {
        return Err(ChatArtifactError::Invalid("chart dataset selection"));
    }
    for column in &input.columns {
        column.validate()?;
    }
    let selected = payload
        .pointer(&input.rows_pointer)
        .and_then(Value::as_array)
        .ok_or(ChatArtifactError::Invalid("chart rows pointer"))?;
    if selected.is_empty() || selected.len() > MAX_ROWS {
        return Err(ChatArtifactError::Invalid("chart dataset shape"));
    }
    let rows = selected
        .iter()
        .map(|row| {
            input
                .columns
                .iter()
                .map(|column| {
                    row.pointer(&column.pointer)
                        .cloned()
                        .ok_or(ChatArtifactError::Invalid("chart column pointer"))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect::<Result<Vec<_>, _>>()?;
    let columns = input
        .columns
        .iter()
        .map(ChartResultColumnSelection::column)
        .collect();
    ChartDataset::new(
        id,
        columns,
        rows,
        ChartDatasetLineage::FromDataset {
            dataset_id: input.dataset_id,
            revision: input.revision,
            digest: source_digest,
            rows_pointer: input.rows_pointer,
            columns: input.columns,
        },
    )
}

pub fn dataset_from_inline(
    id: String,
    input: ChartInlineInput,
    upstream_receipts: Vec<ChartResultReceipt>,
) -> Result<ChartDataset, ChatArtifactError> {
    if input.upstream_result_refs.len() != upstream_receipts.len()
        || input
            .upstream_result_refs
            .iter()
            .zip(&upstream_receipts)
            .any(|(expected, receipt)| expected != &receipt.result_ref)
    {
        return Err(ChatArtifactError::Invalid("chart upstream receipt binding"));
    }
    ChartDataset::new(
        id,
        input.columns,
        input.rows,
        ChartDatasetLineage::AgentAuthored { upstream_receipts },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chart_dataset_shape_errors_do_not_echo_values() {
        let secret = "sk-private-value-must-not-appear";
        for (input, field) in [
            (
                json!({"from_dataset":{"dataset_id":"ds","revision":secret,"columns":[]}}),
                "dataset.from_dataset",
            ),
            (
                json!({"from_result":{"result_ref":[secret],"rows_pointer":"","columns":[]}}),
                "dataset.from_result",
            ),
            (
                json!({"inline":{"columns":secret,"rows":[]}}),
                "dataset.inline",
            ),
        ] {
            let error = serde_json::from_value::<ChartDatasetInput>(input)
                .unwrap_err()
                .to_string();
            assert!(error.contains(field));
            assert!(!error.contains(secret));
        }
    }

    #[test]
    fn chart_view_accepts_natural_types_and_normalizes_single_y() {
        for kind in ["line", "area", "bar", "scatter"] {
            let view: ChartView =
                serde_json::from_value(json!({"kind": kind, "x": "quarter", "y": "revenue"}))
                    .unwrap();
            view.validate().unwrap();
            let stored = serde_json::to_value(&view).unwrap();
            assert_eq!(stored["y"], json!(["revenue"]));
            assert_eq!(serde_json::from_value::<ChartView>(stored).unwrap(), view);
        }
        let candle = json!({"kind":"candlestick","symbol":"AAPL","interval":"1d","time":"date","open":"open","high":"high","low":"low","close":"close"});
        serde_json::from_value::<ChartView>(candle)
            .unwrap()
            .validate()
            .unwrap();
        let duplicate_series = ChartView::Analytic {
            chart_type: AnalyticChartType::Line,
            x: "date".into(),
            y: vec!["close".into(), "close".into()],
            color: None,
            semantic_types: BTreeMap::new(),
            title: None,
            subtitle: None,
        };
        assert!(duplicate_series.validate().is_err());
    }

    fn receipt(result_ref: &str) -> ChartResultReceipt {
        ChartResultReceipt {
            result_ref: result_ref.into(),
            runtime_id: "openbb".into(),
            tool_name: "equity_price_historical".into(),
            provider: Some("yfinance".into()),
            request_digest: "a".repeat(64),
            response_digest: "b".repeat(64),
            retrieved_at: "2026-08-24T12:00:00Z".into(),
            warnings: vec!["Delayed market data".into()],
            upstream_result_refs: vec![],
        }
    }

    fn select(
        rows_pointer: &str,
        pointer: &str,
        payload: &Value,
    ) -> Result<ChartDataset, ChatArtifactError> {
        dataset_from_result_selection(
            "dataset-1".into(),
            ChartFromResultInput {
                result_ref: "result:1".into(),
                rows_pointer: rows_pointer.into(),
                columns: vec![
                    ChartResultColumnSelection {
                        id: "period".into(),
                        label: "Period".into(),
                        kind: ChartColumnKind::Date,
                        pointer: "/period".into(),
                    },
                    ChartResultColumnSelection {
                        id: "close".into(),
                        label: "Close".into(),
                        kind: ChartColumnKind::Number,
                        pointer: pointer.into(),
                    },
                ],
            },
            payload,
            receipt("result:1"),
        )
    }

    #[test]
    fn explicit_result_selection_becomes_a_digest_bound_dataset() {
        let payload = json!({"data":{"rows":[
            {"period":"2026-08-01","open":"10","close":"11","nested":{"close":"11"}},
            {"period":"2026-08-02","open":"11","close":"12","nested":{"close":"12"}}
        ]}});
        let dataset = select("/data/rows", "/close", &payload).unwrap();
        assert_eq!(dataset.rows[0], vec![json!("2026-08-01"), json!("11")]);
        let mut tampered = dataset.clone();
        let ChartDatasetLineage::FromResult { receipt, .. } = &mut tampered.lineage else {
            unreachable!();
        };
        receipt.response_digest = "c".repeat(64);
        assert!(tampered.validate().is_err());

        for pointer in ["/missing", "/nested", "/bad~2", "/trailing~"] {
            assert!(
                select("/data/rows", pointer, &payload).is_err(),
                "{pointer}"
            );
        }
    }

    #[test]
    fn inline_dataset_is_marked_agent_authored_and_binds_upstream_receipts() {
        let dataset = dataset_from_inline(
            "dataset-1".into(),
            ChartInlineInput {
                columns: vec![
                    ChartColumn {
                        id: "date".into(),
                        label: "Date".into(),
                        kind: ChartColumnKind::Date,
                    },
                    ChartColumn {
                        id: "return".into(),
                        label: "Return".into(),
                        kind: ChartColumnKind::Number,
                    },
                ],
                rows: vec![vec![json!("2026-08-01"), json!(0.15)]],
                upstream_result_refs: vec!["result:1".into()],
            },
            vec![receipt("result:1")],
        )
        .unwrap();
        assert!(matches!(
            &dataset.lineage,
            ChartDatasetLineage::AgentAuthored { upstream_receipts }
                if upstream_receipts[0].result_ref == "result:1"
        ));
        assert!(dataset.validate().is_ok());
    }

    #[test]
    fn studies_reject_duplicates_and_unsafe_parameters() {
        let study = |module_id, calc_params: &[f64]| ChartStudy {
            module_id,
            calc_params: calc_params.to_vec(),
        };
        for valid in [
            study(ChartStudyModule::Macd, &[]),
            study(ChartStudyModule::Macd, &[12.0, 26.0, 9.0]),
            study(ChartStudyModule::Boll, &[20.0, 2.5]),
        ] {
            assert!(valid.validate().is_ok(), "{valid:?}");
        }
        for invalid in [
            study(ChartStudyModule::Macd, &[12.0, 26.0]),
            study(ChartStudyModule::Macd, &[12.0, 0.0, 9.0]),
            study(ChartStudyModule::Ma, &[0.5]),
            study(ChartStudyModule::Ema, &[f64::INFINITY]),
            study(ChartStudyModule::Pvt, &[10.0]),
        ] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }

        let ema = study(ChartStudyModule::Ema, &[20.0]);
        let document = ChartDocument {
            custom_series: vec![],
            dataset_id: "dataset-1".into(),
            dataset_digest: "a".repeat(64),
            view: financial_view(),
            studies: vec![ema.clone(), ema],
            drawings: vec![],
            note: None,
        };
        assert!(document.validate().is_err());
    }

    fn financial_view() -> ChartView {
        ChartView::Financial {
            symbol: "TEST".into(),
            interval: "1d".into(),
            time: "date".into(),
            open: "open".into(),
            high: "high".into(),
            low: "low".into(),
            close: "close".into(),
            volume: None,
            turnover: None,
            price_precision: None,
        }
    }

    #[test]
    fn financial_drawings_have_bounded_typed_geometry() {
        let valid = ChartDrawing {
            kind: ChartDrawingKind::Segment,
            points: vec![
                ChartDrawingPoint {
                    timestamp: json!("2026-08-01"),
                    value: 10.0,
                },
                ChartDrawingPoint {
                    timestamp: json!("2026-08-02"),
                    value: 12.0,
                },
            ],
            color: Some("#2563EB".into()),
            line_width: Some(2),
            line_style: Some(ChartDrawingLineStyle::Dashed),
            label: Some("breakout".into()),
        };
        assert!(valid.validate().is_ok());

        let mut wrong_arity = valid.clone();
        wrong_arity.kind = ChartDrawingKind::LongPosition;
        let mut bad_color = valid.clone();
        bad_color.color = Some("red".into());
        let mut multiline_label = valid.clone();
        multiline_label.label = Some("earnings\ncall".into());
        for invalid in [wrong_arity, bad_color, multiline_label] {
            assert!(invalid.validate().is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn financial_time_contract_rejects_ambiguous_or_unrenderable_values() {
        assert!(valid_chart_interval("15m"));
        for interval in ["quarterly", "0d", "01d", "1D"] {
            assert!(!valid_chart_interval(interval), "{interval}");
        }
        assert_eq!(
            chart_timestamp(&json!(1_786_492_800)),
            Some(1_786_492_800_000.0)
        );
        assert!(chart_timestamp(&json!(1.0e20)).is_none());
    }

    #[test]
    fn financial_document_requires_ordered_ohlc_rows_and_in_range_drawings() {
        let dataset = ChartDataset::new(
            "dataset-1".into(),
            ["date", "open", "high", "low", "close"]
                .into_iter()
                .map(|id| ChartColumn {
                    id: id.into(),
                    label: id.into(),
                    kind: if id == "date" {
                        ChartColumnKind::Date
                    } else {
                        ChartColumnKind::Number
                    },
                })
                .collect(),
            vec![
                vec![
                    json!("2026-08-01"),
                    json!(10),
                    json!(12),
                    json!(9),
                    json!(11),
                ],
                vec![
                    json!("2026-08-02"),
                    json!(11),
                    json!(13),
                    json!(10),
                    json!(12),
                ],
            ],
            ChartDatasetLineage::AgentAuthored {
                upstream_receipts: vec![],
            },
        )
        .unwrap();
        let point = |timestamp: &str| ChartDrawingPoint {
            timestamp: json!(timestamp),
            value: 10.0,
        };
        let document = ChartDocument {
            custom_series: vec![],
            dataset_id: dataset.id.clone(),
            dataset_digest: dataset.digest.clone(),
            view: financial_view(),
            studies: Vec::new(),
            drawings: vec![ChartDrawing {
                kind: ChartDrawingKind::Segment,
                points: vec![point("2026-08-01"), point("2026-08-02")],
                color: None,
                line_width: None,
                line_style: None,
                label: None,
            }],
            note: None,
        };
        document.validate_dataset(&dataset).unwrap();

        let mut custom = document.clone();
        custom.custom_series.push(ChartCustomSeries {
            id: "saved-close".into(),
            label: "Saved close".into(),
            column: "missing".into(),
            pane: None,
            color: None,
        });
        assert!(custom.validate_dataset(&dataset).is_err());

        let mut drawing_outside_dataset = document.clone();
        drawing_outside_dataset.drawings[0].points[1] = point("2026-08-03");
        assert!(drawing_outside_dataset.validate_dataset(&dataset).is_err());

        // A high below the open is not a renderable candle.
        let mut invalid = dataset.clone();
        invalid.rows[1][2] = json!(10);
        invalid.digest = dataset_digest(&invalid.columns, &invalid.rows, &invalid.lineage).unwrap();
        let mut invalid_document = document;
        invalid_document.dataset_digest = invalid.digest.clone();
        assert!(invalid_document.validate_dataset(&invalid).is_err());
    }
}
