//! Exchange balance files: format detection, Kraken code normalization, the
//! column-mapping step and the trades-only refusal. The preview shows
//! quantities so the user can check the file; after import they stay here.
use super::{
    csv_records, decimal, decimal_sum, identity, invalid, CsvValidationError, HoldingsPosition,
    MAX_CSV_BYTES,
};
use crate::store::StoreResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CRYPTO_MARKET: &str = "CRYPTO";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CryptoFileFormat {
    KrakenLedger,
    Balances,
    /// The user chose the asset and balance columns.
    Mapped,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CryptoPreviewRow {
    pub symbol: String,
    pub quantity: String,
    /// The file's own codes that merged into this asset (XETH, ETH2.S).
    pub from: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum CryptoSkipReason {
    Cash,
    Zero,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CryptoSkipped {
    pub symbol: String,
    pub reason: CryptoSkipReason,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CryptoFilePreview {
    pub preview_id: String,
    pub format: Option<CryptoFileFormat>,
    pub row_count: usize,
    /// The latest ledger time, when the file has one.
    pub as_of_ms: Option<i64>,
    pub rows: Vec<CryptoPreviewRow>,
    pub skipped: Vec<CryptoSkipped>,
    pub errors: Vec<CsvValidationError>,
    /// Present until the user picks the asset and balance columns.
    pub mapping_headers: Option<Vec<String>>,
    /// A trade history without balances; refused.
    pub trades_only: bool,
}

/// What a crypto preview holds while it waits for mapping or commit.
pub(crate) struct CryptoPending {
    pub records: Vec<Vec<String>>,
    pub positions: Vec<HoldingsPosition>,
}

const ASSET_HEADERS: &[&str] = &["asset", "coin", "symbol", "currency", "코인", "자산"];
/// In priority order: an explicit balance beats a generic amount.
const BALANCE_HEADERS: &[&str] = &["balance", "total", "보유수량", "amount", "quantity", "수량"];
const EXPLICIT_BALANCE: &[&str] = &["balance", "total", "보유수량"];
const TRADE_HEADERS: &[&str] = &[
    "side",
    "price",
    "pair",
    "ordertxid",
    "order id",
    "order_id",
    "trade id",
    "trade_id",
    "tradeid",
    "trade type",
    "transaction type",
    "executed",
    "filled",
    "매수/매도",
    "거래유형",
    "거래구분",
    "체결가격",
    "체결가",
    "체결수량",
];
const STAKING_SUFFIXES: &[&str] = &["S", "M", "F", "B", "HOLD"];
const KRAKEN_CODES: &[(&str, &str)] = &[
    ("XXBT", "BTC"),
    ("XBT", "BTC"),
    ("XETH", "ETH"),
    ("ETH2", "ETH"),
    ("XXDG", "DOGE"),
    ("XDG", "DOGE"),
    ("XETC", "ETC"),
    ("XLTC", "LTC"),
    ("XXRP", "XRP"),
    ("XXLM", "XLM"),
    ("XXMR", "XMR"),
    ("XZEC", "ZEC"),
    ("XMLN", "MLN"),
    ("XREP", "REP"),
    ("ZUSD", "USD"),
    ("ZEUR", "EUR"),
    ("ZGBP", "GBP"),
    ("ZJPY", "JPY"),
    ("ZCAD", "CAD"),
    ("ZAUD", "AUD"),
    ("ZCHF", "CHF"),
];
/// ISO 4217 cash rows are skipped; stablecoins are kept.
const FIAT: &[&str] = &[
    "AED", "ARS", "AUD", "BRL", "CAD", "CHF", "CLP", "CNY", "COP", "CZK", "DKK", "EGP", "EUR",
    "GBP", "HKD", "HUF", "IDR", "ILS", "INR", "JPY", "KES", "KRW", "MXN", "MYR", "NGN", "NOK",
    "NZD", "PEN", "PHP", "PKR", "PLN", "RON", "RUB", "SAR", "SEK", "SGD", "THB", "TRY", "TWD",
    "UAH", "USD", "VND", "ZAR",
];

/// Kraken's legacy codes and staking variants merge into the base asset.
pub fn normalize_code(code: &str) -> String {
    let mut code = code.trim().to_uppercase();
    if let Some((base, suffix)) = code.split_once('.') {
        if STAKING_SUFFIXES.contains(&suffix) {
            code = base.to_owned();
        }
    }
    KRAKEN_CODES
        .iter()
        .find(|(raw, _)| *raw == code)
        .map(|(_, base)| (*base).to_owned())
        .unwrap_or(code)
}

pub fn is_fiat(symbol: &str) -> bool {
    FIAT.contains(&symbol)
}

fn header_index(headers: &[String], names: &[&str]) -> Option<usize> {
    names
        .iter()
        .find_map(|name| headers.iter().position(|header| header == name))
}

fn new_preview() -> CryptoFilePreview {
    CryptoFilePreview {
        preview_id: uuid::Uuid::new_v4().to_string(),
        format: None,
        row_count: 0,
        as_of_ms: None,
        rows: vec![],
        skipped: vec![],
        errors: vec![],
        mapping_headers: None,
        trades_only: false,
    }
}

fn ledger_time(value: &str) -> Option<i64> {
    let value = value.trim();
    [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.fZ",
        "%Y-%m-%dT%H:%M:%SZ",
    ]
    .iter()
    .find_map(|format| chrono::NaiveDateTime::parse_from_str(value, format).ok())
    .map(|time| time.and_utc().timestamp_millis())
}

/// Parses an exchange file into a preview plus the pending state for mapping
/// and commit. A malformed file still returns a preview that lists its errors.
pub(crate) fn parse(bytes: &[u8]) -> StoreResult<(CryptoFilePreview, CryptoPending)> {
    if bytes.len() > MAX_CSV_BYTES {
        return Err(invalid("CSV exceeds 5 MB"));
    }
    let input = std::str::from_utf8(bytes)
        .map_err(|_| invalid("CSV must be UTF-8"))?
        .trim_start_matches('\u{feff}');
    let mut preview = new_preview();
    let records = match csv_records(input) {
        Ok(records) => records,
        Err(error) => {
            preview.errors.push(CsvValidationError {
                row: 0,
                field: "csv".into(),
                message: error.to_string(),
            });
            return Ok((
                preview,
                CryptoPending {
                    records: vec![],
                    positions: vec![],
                },
            ));
        }
    };
    let Some(raw_headers) = records.first() else {
        return Err(invalid("CSV requires a header"));
    };
    let headers = raw_headers
        .iter()
        .map(|header| header.trim().to_lowercase())
        .collect::<Vec<_>>();
    preview.row_count = records.len() - 1;
    let positions = if ["refid", "time", "asset", "balance"]
        .iter()
        .all(|name| headers.iter().any(|header| header == name))
    {
        preview.format = Some(CryptoFileFormat::KrakenLedger);
        kraken(&records, &headers, &mut preview)
    } else {
        let asset = header_index(&headers, ASSET_HEADERS);
        let balance = header_index(&headers, BALANCE_HEADERS);
        let explicit = header_index(&headers, EXPLICIT_BALANCE).is_some();
        if !explicit && trades(&records, &headers) {
            preview.trades_only = true;
            vec![]
        } else if let (Some(asset), Some(balance)) = (asset, balance) {
            preview.format = Some(CryptoFileFormat::Balances);
            balances(&records, asset, balance, &mut preview)
        } else {
            preview.mapping_headers = Some(
                raw_headers
                    .iter()
                    .map(|header| header.trim().to_owned())
                    .collect(),
            );
            vec![]
        }
    };
    Ok((preview, CryptoPending { records, positions }))
}

/// Applies the user's column choice to a file whose headers were not recognized.
pub(crate) fn map_columns(
    preview_id: &str,
    pending: &mut CryptoPending,
    asset: &str,
    balance: &str,
) -> StoreResult<CryptoFilePreview> {
    let headers = pending
        .records
        .first()
        .ok_or_else(|| invalid("CSV requires a header"))?;
    let find = |name: &str| {
        headers
            .iter()
            .position(|header| header.trim() == name.trim())
    };
    let (Some(asset), Some(balance)) = (find(asset), find(balance)) else {
        return Err(invalid("choose two columns from this file"));
    };
    if asset == balance {
        return Err(invalid("the asset and balance columns must differ"));
    }
    let mut preview = new_preview();
    preview.preview_id = preview_id.to_owned();
    preview.format = Some(CryptoFileFormat::Mapped);
    preview.row_count = pending.records.len() - 1;
    pending.positions = balances(&pending.records, asset, balance, &mut preview);
    Ok(preview)
}

/// Trade markers in the headers, or a type column whose values are buy/sell.
fn trades(records: &[Vec<String>], headers: &[String]) -> bool {
    if headers
        .iter()
        .any(|header| TRADE_HEADERS.contains(&header.as_str()))
    {
        return true;
    }
    let Some(kind) = headers.iter().position(|header| header == "type") else {
        return false;
    };
    records.iter().skip(1).take(50).any(|row| {
        row.get(kind).is_some_and(|value| {
            matches!(
                value.trim().to_lowercase().as_str(),
                "buy" | "sell" | "trade"
            )
        })
    })
}

#[derive(Default)]
struct Merged {
    quantity: Option<String>,
    from: Vec<String>,
}

fn merge(totals: &mut BTreeMap<String, Merged>, code: &str, quantity: &str) -> StoreResult<()> {
    let symbol = normalize_code(code);
    let entry = totals.entry(symbol).or_default();
    entry.quantity = Some(match entry.quantity.take() {
        Some(total) => decimal_sum(&total, quantity)?,
        None => quantity.to_owned(),
    });
    let raw = code.trim().to_uppercase();
    if !entry.from.contains(&raw) {
        entry.from.push(raw);
    }
    Ok(())
}

fn finish(
    totals: BTreeMap<String, Merged>,
    preview: &mut CryptoFilePreview,
) -> Vec<HoldingsPosition> {
    let mut positions = vec![];
    for (symbol, merged) in totals {
        let quantity = merged.quantity.unwrap_or_else(|| "0".into());
        if is_fiat(&symbol) {
            preview.skipped.push(CryptoSkipped {
                symbol,
                reason: CryptoSkipReason::Cash,
            });
        } else if quantity == "0" {
            preview.skipped.push(CryptoSkipped {
                symbol,
                reason: CryptoSkipReason::Zero,
            });
        } else if identity(&symbol).is_err() {
            preview.errors.push(CsvValidationError {
                row: 0,
                field: "asset".into(),
                message: format!("{} is not a supported asset code", merged.from.join(", ")),
            });
        } else {
            preview.rows.push(CryptoPreviewRow {
                symbol: symbol.clone(),
                quantity: quantity.clone(),
                from: merged.from,
            });
            positions.push(HoldingsPosition {
                symbol,
                market: CRYPTO_MARKET.into(),
                display_name: None,
                quantity,
                cost_basis: None,
                market_value: None,
                currency: String::new(),
            });
        }
    }
    positions
}

fn quantity(value: &str) -> Result<String, &'static str> {
    let value = decimal(value).map_err(|_| "balance is not a plain decimal")?;
    if value.starts_with('-') {
        return Err("balance is negative");
    }
    Ok(value)
}

fn balances(
    records: &[Vec<String>],
    asset: usize,
    balance: usize,
    preview: &mut CryptoFilePreview,
) -> Vec<HoldingsPosition> {
    let width = records.first().map(Vec::len).unwrap_or_default();
    let mut totals = BTreeMap::new();
    for (index, row) in records.iter().enumerate().skip(1) {
        let mut error = |field: &str, message: &str| {
            preview.errors.push(CsvValidationError {
                row: index + 1,
                field: field.into(),
                message: message.into(),
            })
        };
        if row.len() != width {
            error("csv", "row column count differs from header");
            continue;
        }
        let code = row[asset].trim();
        if code.is_empty() {
            error("asset", "asset is empty");
            continue;
        }
        match quantity(&row[balance]) {
            Ok(value) => {
                if let Err(failure) = merge(&mut totals, code, &value) {
                    error("balance", &failure.to_string());
                }
            }
            Err(message) => error("balance", message),
        }
    }
    finish(totals, preview)
}

/// The latest balance per ledger asset (and Kraken wallet), then merged.
fn kraken(
    records: &[Vec<String>],
    headers: &[String],
    preview: &mut CryptoFilePreview,
) -> Vec<HoldingsPosition> {
    let column = |name: &str| headers.iter().position(|header| header == name);
    let (time, asset, balance) = (
        column("time").unwrap_or(0),
        column("asset").unwrap_or(0),
        column("balance").unwrap_or(0),
    );
    let wallet = column("wallet");
    let mut latest: BTreeMap<(String, String), (i64, usize, String)> = BTreeMap::new();
    for (index, row) in records.iter().enumerate().skip(1) {
        let mut error = |field: &str, message: &str| {
            preview.errors.push(CsvValidationError {
                row: index + 1,
                field: field.into(),
                message: message.into(),
            })
        };
        if row.len() != headers.len() {
            error("csv", "row column count differs from header");
            continue;
        }
        if row[balance].trim().is_empty() {
            continue;
        }
        let Some(at) = ledger_time(&row[time]) else {
            error("time", "time is not a ledger date and time");
            continue;
        };
        let value = match quantity(&row[balance]) {
            Ok(value) => value,
            Err(message) => {
                error("balance", message);
                continue;
            }
        };
        let key = (
            row[asset].trim().to_uppercase(),
            wallet.map(|i| row[i].trim().to_owned()).unwrap_or_default(),
        );
        preview.as_of_ms = Some(preview.as_of_ms.map_or(at, |current| current.max(at)));
        let newer = latest
            .get(&key)
            .is_none_or(|(seen, order, _)| (at, index) >= (*seen, *order));
        if newer {
            latest.insert(key, (at, index, value));
        }
    }
    let mut totals = BTreeMap::new();
    for ((code, _), (_, _, value)) in latest {
        if let Err(failure) = merge(&mut totals, &code, &value) {
            preview.errors.push(CsvValidationError {
                row: 0,
                field: "balance".into(),
                message: failure.to_string(),
            });
        }
    }
    finish(totals, preview)
}
