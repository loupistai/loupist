//! Price data: retained numeric market-data results of the price kind. World
//! building looks for names, facts and relationships, and a price table holds
//! none of them, so such a Source is kept, searchable and citable like any other
//! but is never queued for extraction (`docs/knowledge/worlds.md#price-data`).
//!
//! The producing tool decides first: a retained dataset whose recorded producer
//! and request name a price route below, with the recorded `trust: numeric`. The
//! result's shape decides the rest: a table whose columns are only dates,
//! instrument identifiers and price fields, with numbers in every price cell, is
//! price data whatever produced it. That rule covers a table the model composed
//! from such results and saved from the workspace. The title never counts.
//!
//! Company statements and fundamentals stay on the World side: their figures can
//! be facts worth keeping.
use serde_json::Value;

use crate::broker::ToolMethod;
use crate::custom_connections::taint::TRUST_NUMERIC;
use crate::datasets::saved_rows::SavedRows;

/// Bundled market-data routes whose rows are prices: price history and bars
/// at any interval, quotes and snapshots, exchange rates, index and commodity
/// levels, and yield and rate series. Each is on the reviewed numeric-only
/// list (`NUMERIC_ONLY_OPENBB_TOOLS`).
pub(crate) const PRICE_OPENBB_TOOLS: &[&str] = &[
    // Price history and bars.
    "crypto_price_historical",
    "currency_price_historical",
    "derivatives_futures_historical",
    "equity_historical_market_cap",
    "equity_price_historical",
    "etf_historical",
    "index_price_historical",
    // Quotes, snapshots and returns computed from prices.
    "derivatives_futures_curve",
    "derivatives_options_chains",
    "derivatives_options_snapshots",
    "equity_market_snapshots",
    "equity_price_performance",
    "equity_price_quote",
    "etf_price_performance",
    // Exchange rates.
    "currency_reference_rates",
    "currency_snapshots",
    // Index and commodity levels.
    "commodity_price_spot",
    "economy_share_price_index",
    "index_snapshots",
    // Yield and rate series.
    "economy_interest_rates",
    "fixedincome_corporate_bond_prices",
    "fixedincome_corporate_hqm",
    "fixedincome_corporate_spot_rates",
    "fixedincome_government_svensson_yield_curve",
    "fixedincome_government_tips_yields",
    "fixedincome_government_treasury_prices",
    "fixedincome_government_treasury_rates",
    "fixedincome_government_yield_curve",
    "fixedincome_mortgage_indices",
    "fixedincome_rate_dpcredit",
    "fixedincome_rate_ecb",
    "fixedincome_rate_effr",
    "fixedincome_rate_effr_forecast",
    "fixedincome_rate_estr",
    "fixedincome_rate_iorb",
    "fixedincome_rate_overnight_bank_funding",
    "fixedincome_rate_sofr",
    "fixedincome_rate_sonia",
    "fixedincome_spreads_tcm",
    "fixedincome_spreads_tcm_effr",
    "fixedincome_spreads_treasury_effr",
];

/// Bundled KIS operations (`marketplace/kis-read-api-v1.json`) that return
/// current prices, order books, trades, bars or index levels. Rankings,
/// investor and member flows, product information, news and calendars stay
/// on the World side.
const PRICE_KIS_OPERATIONS: &[&str] = &[
    "domestic_bond.avg_unit",
    "domestic_bond.inquire_asking_price",
    "domestic_bond.inquire_ccnl",
    "domestic_bond.inquire_daily_price",
    "domestic_bond.inquire_price",
    "domestic_futureoption.display_board_top",
    "domestic_futureoption.exp_price_trend",
    "domestic_futureoption.inquire_asking_price",
    "domestic_futureoption.inquire_daily_fuopchartprice",
    "domestic_futureoption.inquire_price",
    "domestic_futureoption.inquire_time_fuopchartprice",
    "domestic_stock.inquire_asking_price_exp_ccn",
    "domestic_stock.inquire_ccnl",
    "domestic_stock.inquire_daily_indexchartprice",
    "domestic_stock.inquire_daily_itemchartprice",
    "domestic_stock.inquire_daily_overtimeprice",
    "domestic_stock.inquire_daily_price",
    "domestic_stock.inquire_elw_price",
    "domestic_stock.inquire_index_daily_price",
    "domestic_stock.inquire_index_price",
    "domestic_stock.inquire_overtime_asking_price",
    "domestic_stock.inquire_overtime_price",
    "domestic_stock.inquire_price",
    "domestic_stock.inquire_price_2",
    "domestic_stock.inquire_time_dailychartprice",
    "domestic_stock.inquire_time_indexchartprice",
    "domestic_stock.inquire_time_itemchartprice",
    "domestic_stock.inquire_time_itemconclusion",
    "domestic_stock.inquire_time_overtimeconclusion",
    "etfetn.inquire_price",
    "etfetn.nav_comparison_trend",
    "overseas_futureoption.daily_ccnl",
    "overseas_futureoption.inquire_asking_price",
    "overseas_futureoption.inquire_price",
    "overseas_futureoption.inquire_time_futurechartprice",
    "overseas_futureoption.opt_asking_price",
    "overseas_futureoption.opt_price",
    "overseas_stock.dailyprice",
    "overseas_stock.inquire_asking_price",
    "overseas_stock.inquire_daily_chartprice",
    "overseas_stock.inquire_time_indexchartprice",
    "overseas_stock.inquire_time_itemchartprice",
    "overseas_stock.price",
    "overseas_stock.price_detail",
    "overseas_stock.quot_inquire_ccnl",
];

/// Whether a retained dataset's recorded producer, request and trust name a
/// price read (`retain_dataset` records them with every native and bundled read).
pub(crate) fn price_dataset(metadata: &Value) -> bool {
    if metadata.get("trust").and_then(Value::as_str) != Some(TRUST_NUMERIC) {
        return false;
    }
    let request = &metadata["request"];
    match metadata.get("producer").and_then(Value::as_str) {
        Some("mcp") => {
            request["server_id"] == "openbb"
                && request["tool_name"]
                    .as_str()
                    .is_some_and(|tool| PRICE_OPENBB_TOOLS.contains(&tool))
        }
        Some(producer) if producer == native_producer(ToolMethod::KrxDailyQuote) => true,
        Some(producer) if producer == native_producer(ToolMethod::KisRead) => request
            ["operation_id"]
            .as_str()
            .is_some_and(|operation| PRICE_KIS_OPERATIONS.contains(&operation)),
        _ => false,
    }
}

/// How `retain_dataset` names a native producer.
fn native_producer(method: ToolMethod) -> String {
    format!("{method:?}")
}

/// Whether a retained tool result is price data: its dataset names a price
/// read, or its rows are a price table. A result without a retained dataset
/// (a web page, a search) never is.
pub(crate) fn price_payload(payload: &Value, metadata: Option<&Value>) -> bool {
    payload["dataset_ref"].is_object()
        && (metadata.is_some_and(price_dataset)
            || SavedRows::parse(payload).is_some_and(|rows| price_rows(rows.rows)))
}

/// Whether a workspace file is a price table: CSV, TSV, a Markdown pipe table
/// or JSON rows whose columns pass [`price_columns`].
pub(crate) fn price_file(media_type: &str, text: &str) -> bool {
    if text.len() > MAX_TABLE_BYTES {
        return false;
    }
    let media_type = media_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if media_type == "application/json" || media_type.ends_with("+json") {
        return serde_json::from_str::<Value>(text)
            .ok()
            .as_ref()
            .and_then(json_rows)
            .is_some_and(price_rows);
    }
    if !media_type.starts_with("text/") {
        return false;
    }
    if let Some((header, rows)) = markdown_table(text) {
        return price_text_table(&header, &rows);
    }
    let delimiter = if media_type == "text/tab-separated-values" {
        '\t'
    } else {
        ','
    };
    delimited_table(text, delimiter).is_some_and(|(header, rows)| price_text_table(&header, &rows))
}

/// World building reads at most this much text from a Source span.
const MAX_TABLE_BYTES: usize = 1_000_000;

/// Columns naming when a row was observed.
const DATE_COLUMNS: &[&str] = &[
    "date",
    "datetime",
    "time",
    "timestamp",
    "tradedate",
    "tradingdate",
];
/// Columns naming which instrument a row prices.
const IDENTIFIER_COLUMNS: &[&str] = &[
    "symbol",
    "ticker",
    "pair",
    "currency",
    "basecurrency",
    "quotecurrency",
];
/// Price fields: a table needs at least one.
const PRICE_COLUMNS: &[&str] = &[
    "open",
    "high",
    "low",
    "close",
    "openprice",
    "highprice",
    "lowprice",
    "closeprice",
    "adjclose",
    "adjustedclose",
    "price",
    "lastprice",
    "last",
    "bid",
    "ask",
    "mid",
    "vwap",
    "settle",
    "rate",
    "exchangerate",
    "yield",
];
/// Numeric fields a bar table carries beside its prices.
const BAR_COLUMNS: &[&str] = &[
    "volume",
    "change",
    "changepercent",
    "percentchange",
    "pctchange",
    "openinterest",
    "dividend",
    "dividends",
    "splitratio",
    "stocksplits",
    "transactions",
];

#[derive(Clone, Copy, PartialEq)]
enum Column {
    Date,
    Identifier,
    Numeric,
}

fn fold(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The role of every column, or none when one column is not a date, an
/// identifier or a price field, or when no column is a price and nothing
/// says which instrument or day a price belongs to.
fn price_columns<'a>(names: impl IntoIterator<Item = &'a str>) -> Option<Vec<Column>> {
    let mut roles = Vec::new();
    let mut price = false;
    for name in names {
        let folded = fold(name);
        let role = if DATE_COLUMNS.contains(&folded.as_str()) {
            Column::Date
        } else if IDENTIFIER_COLUMNS.contains(&folded.as_str()) {
            Column::Identifier
        } else if PRICE_COLUMNS.contains(&folded.as_str()) {
            price = true;
            Column::Numeric
        } else if BAR_COLUMNS.contains(&folded.as_str()) {
            Column::Numeric
        } else {
            return None;
        };
        roles.push(role);
    }
    (price && roles.iter().any(|role| *role != Column::Numeric)).then_some(roles)
}

fn numeric_text(cell: &str) -> bool {
    let cell = cell.trim();
    if matches!(cell, "" | "-" | "—" | "null" | "NaN" | "nan" | "N/A") {
        return true;
    }
    let cell = cell
        .strip_suffix('%')
        .unwrap_or(cell)
        .trim_start_matches(['+', '$'])
        .replace(',', "");
    cell.parse::<f64>().is_ok_and(f64::is_finite)
}

fn numeric_cell(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Number(_) => true,
        Value::String(text) => numeric_text(text),
        _ => false,
    }
}

/// Rows of objects whose keys each pass [`price_columns`] and whose price and
/// bar cells are numbers. A provider may leave a field out of some rows.
fn price_rows(rows: &[Value]) -> bool {
    !rows.is_empty()
        && rows.iter().all(|row| {
            let Some(row) = row.as_object() else {
                return false;
            };
            let Some(roles) = price_columns(row.keys().map(String::as_str)) else {
                return false;
            };
            row.values()
                .zip(&roles)
                .all(|(cell, role)| *role != Column::Numeric || numeric_cell(cell))
        })
}

fn price_text_table(header: &[String], rows: &[Vec<String>]) -> bool {
    let Some(roles) = price_columns(header.iter().map(String::as_str)) else {
        return false;
    };
    !rows.is_empty()
        && rows.iter().all(|row| {
            row.len() == roles.len()
                && row
                    .iter()
                    .zip(&roles)
                    .all(|(cell, role)| *role != Column::Numeric || numeric_text(cell))
        })
}

/// The rows of a JSON file: a top-level array of objects, or one under a
/// conventional key.
fn json_rows(value: &Value) -> Option<&[Value]> {
    let rows = match value {
        Value::Array(rows) => rows,
        Value::Object(_) => ["/rows", "/data", "/data/rows", "/results"]
            .into_iter()
            .find_map(|pointer| value.pointer(pointer)?.as_array())?,
        _ => return None,
    };
    (!rows.is_empty() && rows.iter().all(Value::is_object)).then_some(rows.as_slice())
}

/// A CSV or TSV file without quoted cells: a header line and rows of the same width.
fn delimited_table(text: &str, delimiter: char) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    if text.contains('"') {
        return None;
    }
    let mut lines = text
        .lines()
        .map(|line| line.trim_end_matches('\r'))
        .filter(|line| !line.trim().is_empty());
    let split = |line: &str| -> Vec<String> {
        line.split(delimiter)
            .map(|cell| cell.trim().to_owned())
            .collect()
    };
    let header = split(lines.next()?);
    if header.len() < 2 {
        return None;
    }
    Some((header, lines.map(split).collect()))
}

/// A Markdown document that is one pipe table, optionally under headings.
fn markdown_table(text: &str) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let mut table = Vec::new();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || (line.starts_with('#') && table.is_empty()) {
            continue;
        }
        if !line.starts_with('|') {
            return None;
        }
        let cells: Vec<String> = line
            .trim_matches('|')
            .split('|')
            .map(|cell| cell.trim().to_owned())
            .collect();
        let separator = cells
            .iter()
            .all(|cell| !cell.is_empty() && cell.chars().all(|c| matches!(c, '-' | ':')));
        if separator {
            if table.len() != 1 {
                return None;
            }
            continue;
        }
        table.push(cells);
    }
    let mut rows = table.into_iter();
    let header = rows.next()?;
    (header.len() >= 2).then(|| (header, rows.collect()))
}

/// Whether a stored Source span is price data, read from its stored bytes and,
/// for a tool result, the dataset its payload names in this database.
pub(super) fn stored_price_data(
    tx: &rusqlite::Transaction<'_>,
    loupist_id: &str,
    material: &super::source::SpanMaterial,
) -> super::KnowledgeResult<bool> {
    use rusqlite::OptionalExtension;
    let Ok(text) = std::str::from_utf8(&material.bytes) else {
        return Ok(false);
    };
    match material.source_class {
        super::SourceClass::ToolResult => {
            let Ok(payload) = serde_json::from_str::<Value>(text) else {
                return Ok(false);
            };
            let Some(dataset_id) = payload
                .pointer("/dataset_ref/dataset_id")
                .and_then(Value::as_str)
            else {
                return Ok(false);
            };
            let metadata: Option<String> = tx
                .query_row(
                    "SELECT metadata_json FROM datasets WHERE loupist_id = ?1 AND id = ?2",
                    rusqlite::params![loupist_id, dataset_id],
                    |row| row.get(0),
                )
                .optional()?;
            let metadata = metadata.and_then(|json| serde_json::from_str::<Value>(&json).ok());
            Ok(price_payload(&payload, metadata.as_ref()))
        }
        super::SourceClass::UserFile => Ok(price_file(&material.media_type, text)),
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests;
