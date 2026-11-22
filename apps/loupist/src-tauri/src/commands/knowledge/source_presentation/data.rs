use crate::datasets::saved_rows::{visible_field, SavedRows};
use serde::Serialize;
use serde_json::Value;

const MAX_ROWS: usize = 40;
const MAX_COLUMNS: usize = 24;
const MAX_CELL_CHARS: usize = 256;

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SourceDataTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<SourceDataCell>>,
    pub row_offset: usize,
    pub total_rows: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub focused_cell: Option<SourceDataCellPosition>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SourceDataCellPosition {
    /// Both indexes are zero-based and relative to the returned table.
    pub row: usize,
    pub column: usize,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SourceDataCell {
    Text(String),
    /// Exact decimal spelling, so large integers survive JSON and JavaScript.
    Number(String),
    Boolean(bool),
    Empty,
}

fn cell(value: &Value, key: &str, observation: bool) -> SourceDataCell {
    match value {
        Value::Number(value) if matches!(key, "date" | "period" | "year" | "fiscal_year") => {
            SourceDataCell::Text(value.to_string())
        }
        Value::Number(value) => SourceDataCell::Number(value.to_string()),
        Value::Bool(value) => SourceDataCell::Boolean(*value),
        Value::String(value)
            if observation
                && value.len() <= MAX_CELL_CHARS
                && value.parse::<f64>().is_ok_and(f64::is_finite) =>
        {
            SourceDataCell::Number(value.clone())
        }
        Value::String(value) => SourceDataCell::Text(crate::util::shorten(value, MAX_CELL_CHARS)),
        _ => SourceDataCell::Empty,
    }
}

pub(super) fn table(data: &SavedRows<'_>, focus: Option<&str>) -> Option<SourceDataTable> {
    let focused = focus.and_then(|pointer| data.cell(pointer));
    let series = data.series();
    let mut keys: Vec<&str> = if series.is_some() {
        let period = if data.rows.iter().any(|row| row.get("period").is_some()) {
            "period"
        } else {
            "date"
        };
        ["economy_name", period, "value"]
            .into_iter()
            .filter(|key| {
                data.rows.iter().any(|row| row.get(key).is_some())
                    || series.is_some_and(|series| series.get(key).is_some())
            })
            .collect()
    } else {
        let mut keys = Vec::new();
        'rows: for row in data.rows {
            for (key, value) in row.as_object()? {
                if visible_field(key)
                    && !value.is_object()
                    && !value.is_array()
                    && !keys.contains(&key.as_str())
                {
                    keys.push(key);
                    if keys.len() == MAX_COLUMNS {
                        break 'rows;
                    }
                }
            }
        }
        keys
    };
    if keys.is_empty() {
        return None;
    }
    if let Some((row, key)) = &focused {
        // A cited value can be in a column beyond the ordinary display cap.
        if series.is_none() && !keys.contains(&key.as_str()) {
            keys.truncate(MAX_COLUMNS - 1);
            keys.push(data.rows[*row].as_object()?.get_key_value(key)?.0);
        }
    }
    let row_offset = focused.as_ref().map_or(0, |(row, _)| {
        if *row < MAX_ROWS {
            0
        } else {
            (*row).min(data.rows.len().saturating_sub(MAX_ROWS))
        }
    });
    let columns = keys
        .iter()
        .map(|key| crate::util::shorten(data.column_label(key).unwrap_or(key), MAX_CELL_CHARS))
        .collect();
    let rows = data
        .rows
        .iter()
        .skip(row_offset)
        .take(MAX_ROWS)
        .map(|row| {
            keys.iter()
                .map(|&key| {
                    let value = row
                        .get(key)
                        .or_else(|| series?.get(key))
                        .unwrap_or(&Value::Null);
                    cell(value, key, series.is_some() && key == "value")
                })
                .collect()
        })
        .collect();
    let focused_cell = focused.and_then(|(row, key)| {
        Some(SourceDataCellPosition {
            row: row - row_offset,
            column: keys.iter().position(|&column| column == key)?,
        })
    });
    Some(SourceDataTable {
        columns,
        rows,
        row_offset,
        total_rows: data.rows.len(),
        focused_cell,
    })
}
