//! App-level factual context. No suitability profile or agent prompt injection.
use super::{SqliteStore, StoreError, StoreResult};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
    time::Instant,
};

pub const MAX_CSV_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_HOLDINGS_ROWS: usize = 2_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WatchlistEntry {
    pub symbol: String,
    pub market: String,
    pub display_name: Option<String>,
    pub added_at_ms: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HoldingsPosition {
    pub symbol: String,
    pub market: String,
    pub display_name: Option<String>,
    pub quantity: String,
    /// Average acquisition price per unit, in the position currency.
    pub cost_basis: Option<String>,
    pub market_value: Option<String>,
    /// Empty means the source did not declare a currency; never infer FX.
    pub currency: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum HoldingsSource {
    Csv,
    Kis,
    CryptoFile,
    /// One snapshot per wallet address: its latest successful read.
    Wallet,
}

impl HoldingsSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Kis => "kis",
            Self::CryptoFile => "crypto_file",
            Self::Wallet => "wallet",
        }
    }
    /// About you groups sources by kind: stocks and funds, or crypto.
    pub fn is_crypto(self) -> bool {
        matches!(self, Self::CryptoFile | Self::Wallet)
    }
}

fn read_stored_enum<T: serde::de::DeserializeOwned>(
    row: &rusqlite::Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    let value: String = row.get(index)?;
    serde_json::from_value(serde_json::Value::String(value)).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HoldingsSnapshot {
    pub snapshot_id: String,
    pub source: HoldingsSource,
    pub imported_at_ms: i64,
    /// Empty for mixed or unspecified currencies. No implicit conversion.
    pub currency: String,
    pub note: Option<String>,
    pub positions: Vec<HoldingsPosition>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct CsvValidationError {
    pub row: usize,
    pub field: String,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HoldingsCsvPreview {
    pub preview_id: String,
    /// Canonical field -> original header. Unrecognized columns are discarded.
    pub mapping: BTreeMap<String, String>,
    pub positions: Vec<HoldingsPosition>,
    pub errors: Vec<CsvValidationError>,
    pub row_count: usize,
}
pub(super) struct PendingPreview {
    pub created: Instant,
    pub body: PendingBody,
}
pub(super) enum PendingBody {
    Stocks(HoldingsCsvPreview),
    Crypto(Box<(crypto_file::CryptoFilePreview, crypto_file::CryptoPending)>),
}

fn invalid(message: &str) -> StoreError {
    StoreError::Invalid(message.into())
}
fn text(value: &str, maximum: usize) -> StoreResult<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(invalid(
            "text is empty, too long, or contains control characters",
        ));
    }
    Ok(value.to_owned())
}
pub(crate) fn identity(value: &str) -> StoreResult<String> {
    let value = text(value, 64)?.to_uppercase();
    if !value.is_ascii() || value.chars().any(char::is_whitespace) {
        return Err(invalid(
            "symbol and market must be ASCII identifiers without spaces",
        ));
    }
    Ok(value)
}
pub(crate) fn name(value: Option<&str>) -> StoreResult<Option<String>> {
    value
        .filter(|v| !v.trim().is_empty())
        .map(|v| text(v, 256))
        .transpose()
}
/// Exact string arithmetic only: removes redundant zeroes without rounding.
pub(crate) fn decimal(value: &str) -> StoreResult<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 {
        return Err(invalid("expected a decimal string of at most 128 bytes"));
    }
    let negative = value.starts_with('-');
    let value = value.strip_prefix(['-', '+']).unwrap_or(value);
    let mut parts = value.split('.');
    let whole = parts.next().unwrap_or_default();
    let fraction = parts.next().unwrap_or_default();
    if parts.next().is_some()
        || (whole.is_empty() && fraction.is_empty())
        || !whole
            .bytes()
            .chain(fraction.bytes())
            .all(|b| b.is_ascii_digit())
    {
        return Err(invalid(
            "expected a plain decimal string without grouping or exponents",
        ));
    }
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    let sign = if negative && (whole != "0" || !fraction.is_empty()) {
        "-"
    } else {
        ""
    };
    Ok(if fraction.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    })
}
/// Exact sum of two non-negative canonical decimals.
pub(crate) fn decimal_sum(left: &str, right: &str) -> StoreResult<String> {
    let (left, right) = (decimal(left)?, decimal(right)?);
    if left.starts_with('-') || right.starts_with('-') {
        return Err(invalid("balances cannot be negative"));
    }
    let split = |value: &str| {
        let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
        (whole.to_owned(), fraction.to_owned())
    };
    let ((left_whole, left_fraction), (right_whole, right_fraction)) =
        (split(&left), split(&right));
    let scale = left_fraction.len().max(right_fraction.len());
    let width = left_whole.len().max(right_whole.len());
    let digits =
        |whole: &str, fraction: &str| format!("{whole:0>width$}{fraction:0<scale$}").into_bytes();
    let (a, b) = (
        digits(&left_whole, &left_fraction),
        digits(&right_whole, &right_fraction),
    );
    let mut sum = Vec::with_capacity(a.len() + 1);
    let mut carry = 0;
    for (x, y) in a.iter().rev().zip(b.iter().rev()) {
        let digit = (x - b'0') + (y - b'0') + carry;
        sum.push(b'0' + digit % 10);
        carry = digit / 10;
    }
    if carry > 0 {
        sum.push(b'0' + carry);
    }
    sum.reverse();
    let text = String::from_utf8(sum).map_err(|_| invalid("decimal sum failed"))?;
    let point = text.len() - scale;
    decimal(&if scale == 0 {
        text
    } else {
        format!("{}.{}", &text[..point], &text[point..])
    })
}
fn currency(value: &str) -> StoreResult<String> {
    let value = value.trim().to_uppercase();
    if !value.is_empty() && (value.len() != 3 || !value.bytes().all(|b| b.is_ascii_uppercase())) {
        return Err(invalid("currency must be a three-letter code or empty"));
    }
    Ok(value)
}
fn normalize_position(mut p: HoldingsPosition) -> StoreResult<HoldingsPosition> {
    p.symbol = identity(&p.symbol)?;
    p.market = identity(&p.market)?;
    p.display_name = name(p.display_name.as_deref())?;
    p.quantity = decimal(&p.quantity)?;
    p.cost_basis = p.cost_basis.as_deref().map(decimal).transpose()?;
    p.market_value = p.market_value.as_deref().map(decimal).transpose()?;
    p.currency = currency(&p.currency)?;
    Ok(p)
}

impl SqliteStore {
    pub fn watchlist_list(&self) -> StoreResult<Vec<WatchlistEntry>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare("SELECT symbol, market, display_name, added_at_ms FROM user_watchlist ORDER BY added_at_ms, symbol, market")?;
        let entries = statement
            .query_map([], |r| {
                Ok(WatchlistEntry {
                    symbol: r.get(0)?,
                    market: r.get(1)?,
                    display_name: r.get(2)?,
                    added_at_ms: r.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(entries)
    }
    pub fn watchlist_add(
        &self,
        symbol: &str,
        market: &str,
        display_name: Option<&str>,
    ) -> StoreResult<WatchlistEntry> {
        let symbol = identity(symbol)?;
        let market = identity(market)?;
        let display_name = name(display_name)?;
        let connection = self.lock()?;
        connection.execute("INSERT INTO user_watchlist(symbol, market, display_name, added_at_ms) VALUES (?1,?2,?3,?4) ON CONFLICT(symbol,market) DO UPDATE SET display_name=COALESCE(excluded.display_name,user_watchlist.display_name)", params![symbol,market,display_name,chrono::Utc::now().timestamp_millis()])?;
        Ok(connection.query_row("SELECT symbol,market,display_name,added_at_ms FROM user_watchlist WHERE symbol=?1 AND market=?2", params![symbol,market], |r| Ok(WatchlistEntry { symbol:r.get(0)?,market:r.get(1)?,display_name:r.get(2)?,added_at_ms:r.get(3)? }))?)
    }
    pub fn watchlist_remove(&self, symbol: &str, market: &str) -> StoreResult<()> {
        self.lock()?.execute(
            "DELETE FROM user_watchlist WHERE symbol=?1 AND market=?2",
            params![identity(symbol)?, identity(market)?],
        )?;
        Ok(())
    }
    pub fn holdings_import_csv(&self, path: &Path) -> StoreResult<HoldingsCsvPreview> {
        self.preview_csv_bytes(&read_bounded(path)?)
    }
    pub fn preview_csv_bytes(&self, bytes: &[u8]) -> StoreResult<HoldingsCsvPreview> {
        let preview = parse_csv(bytes)?;
        self.remember_preview(
            preview.preview_id.clone(),
            PendingBody::Stocks(preview.clone()),
        )?;
        Ok(preview)
    }
    fn remember_preview(&self, preview_id: String, body: PendingBody) -> StoreResult<()> {
        let mut previews = self
            .user_context_previews
            .lock()
            .map_err(|_| StoreError::LockPoisoned)?;
        previews.retain(|_, p| p.created.elapsed().as_secs() < 1800);
        if previews.len() >= 8 {
            return Err(invalid(
                "too many pending previews; retry after previews expire",
            ));
        }
        previews.insert(
            preview_id,
            PendingPreview {
                created: Instant::now(),
                body,
            },
        );
        Ok(())
    }
    /// An exchange balance file. Rust detects the format, normalizes codes and
    /// freezes the bytes it read; the preview expires after 30 minutes.
    pub fn holdings_import_crypto_file(
        &self,
        path: &Path,
    ) -> StoreResult<crypto_file::CryptoFilePreview> {
        self.preview_crypto_bytes(&read_bounded(path)?)
    }
    pub fn preview_crypto_bytes(
        &self,
        bytes: &[u8],
    ) -> StoreResult<crypto_file::CryptoFilePreview> {
        let (preview, pending) = crypto_file::parse(bytes)?;
        self.remember_preview(
            preview.preview_id.clone(),
            PendingBody::Crypto(Box::new((preview.clone(), pending))),
        )?;
        Ok(preview)
    }
    /// The mapping step: the user names the asset and balance columns.
    pub fn holdings_map_columns(
        &self,
        preview_id: &str,
        asset: &str,
        balance: &str,
    ) -> StoreResult<crypto_file::CryptoFilePreview> {
        let mut previews = self
            .user_context_previews
            .lock()
            .map_err(|_| StoreError::LockPoisoned)?;
        let pending = previews
            .get_mut(preview_id)
            .filter(|pending| pending.created.elapsed().as_secs() < 1800)
            .ok_or_else(|| invalid("CSV preview is missing or expired"))?;
        let PendingBody::Crypto(crypto) = &mut pending.body else {
            return Err(invalid(
                "only an exchange file preview takes a column choice",
            ));
        };
        let (preview, state) = crypto.as_mut();
        if preview.format.is_some() && preview.format != Some(crypto_file::CryptoFileFormat::Mapped)
        {
            return Err(invalid("this file's columns were already recognized"));
        }
        if preview.mapping_headers.is_none() && preview.format.is_none() {
            return Err(invalid("this file cannot be mapped"));
        }
        let mapped = crypto_file::map_columns(preview_id, state, asset, balance)?;
        *preview = mapped.clone();
        Ok(mapped)
    }
    pub fn holdings_commit_csv(&self, preview_id: &str) -> StoreResult<HoldingsSnapshot> {
        let mut previews = self
            .user_context_previews
            .lock()
            .map_err(|_| StoreError::LockPoisoned)?;
        let pending = previews
            .get(preview_id)
            .ok_or_else(|| invalid("CSV preview is missing or already committed"))?;
        if pending.created.elapsed().as_secs() >= 1800 {
            previews.remove(preview_id);
            return Err(invalid("CSV preview expired"));
        }
        let (source, positions) = match &pending.body {
            PendingBody::Stocks(preview) => {
                if !preview.errors.is_empty() {
                    return Err(invalid("CSV preview contains validation errors"));
                }
                (HoldingsSource::Csv, preview.positions.clone())
            }
            PendingBody::Crypto(crypto) => {
                let (preview, state) = crypto.as_ref();
                if preview.trades_only {
                    return Err(invalid("this file lists trades, not balances"));
                }
                if preview.mapping_headers.is_some() {
                    return Err(invalid("choose the asset and balance columns first"));
                }
                if !preview.errors.is_empty() {
                    return Err(invalid("CSV preview contains validation errors"));
                }
                if state.positions.is_empty() {
                    return Err(invalid("this file has no crypto balances to import"));
                }
                (HoldingsSource::CryptoFile, state.positions.clone())
            }
        };
        let result = self.save_holdings_snapshot(source, positions)?;
        previews.remove(preview_id);
        Ok(result)
    }
    pub(crate) fn save_holdings_snapshot(
        &self,
        source: HoldingsSource,
        positions: Vec<HoldingsPosition>,
    ) -> StoreResult<HoldingsSnapshot> {
        if source == HoldingsSource::Wallet {
            return Err(invalid("wallet balances are saved by their read"));
        }
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let snapshot = insert_snapshot(
            &tx,
            source,
            None,
            positions,
            chrono::Utc::now().timestamp_millis(),
        )?;
        tx.commit()?;
        Ok(snapshot)
    }
    /// Counts and dates per source and per kind over the latest snapshot of each
    /// source (each wallet is its own source). Reads no quantity, value or cost
    /// column, so nothing about amounts can leave the store here.
    pub fn holdings_summary(&self) -> StoreResult<HoldingsOverview> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT s.source, s.imported_at_ms, p.symbol, p.market FROM ({LATEST_SNAPSHOTS}) s LEFT JOIN user_holdings_positions p ON p.snapshot_id = s.snapshot_id"
        ))?;
        let mut per_source: BTreeMap<HoldingsSource, (i64, BTreeSet<(String, String)>)> =
            BTreeMap::new();
        let mut per_kind: [(i64, BTreeSet<(String, String)>); 2] = Default::default();
        let rows = statement.query_map([], |row| {
            Ok((
                read_stored_enum::<HoldingsSource>(row, 0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        })?;
        for (source, at, symbol, market) in super::collect_readable_rows(rows, "holdings summary")?
        {
            let kind = &mut per_kind[usize::from(source.is_crypto())];
            kind.0 = kind.0.max(at);
            let entry = per_source.entry(source).or_insert((at, BTreeSet::new()));
            entry.0 = entry.0.max(at);
            if let (Some(symbol), Some(market)) = (symbol, market) {
                kind.1.insert((symbol.clone(), market.clone()));
                entry.1.insert((symbol, market));
            }
        }
        let [stocks, crypto] = per_kind;
        let kind = |(at, items): (i64, BTreeSet<(String, String)>)| {
            (at > 0).then_some(KindSummary {
                imported_at_ms: at,
                count: items.len(),
            })
        };
        Ok(HoldingsOverview {
            sources: per_source
                .into_iter()
                .filter(|(source, _)| *source != HoldingsSource::Wallet)
                .map(|(source, (imported_at_ms, items))| SourceSummary {
                    source,
                    imported_at_ms,
                    count: items.len(),
                })
                .collect(),
            stocks: kind(stocks),
            crypto: kind(crypto),
        })
    }
    /// Host privacy checks cover every latest-per-source snapshot, including
    /// each wallet. No snapshot metadata or amounts are exposed to the model.
    pub(crate) fn holdings_latest_positions(&self) -> StoreResult<Vec<HoldingsPosition>> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT p.symbol,p.market,p.display_name,p.quantity,p.cost_basis,p.market_value,p.currency FROM ({LATEST_SNAPSHOTS}) s JOIN user_holdings_positions p ON p.snapshot_id=s.snapshot_id ORDER BY s.source,s.wallet_id,p.symbol,p.market"
        ))?;
        let positions = statement
            .query_map([], |r| {
                Ok(HoldingsPosition {
                    symbol: r.get(0)?,
                    market: r.get(1)?,
                    display_name: r.get(2)?,
                    quantity: r.get(3)?,
                    cost_basis: r.get(4)?,
                    market_value: r.get(5)?,
                    currency: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(positions)
    }

    /// Test fixtures read the latest committed snapshot and its metadata.
    /// Model-facing holdings still come only from `map_context` identities.
    #[cfg(test)]
    pub(crate) fn holdings_latest(&self) -> StoreResult<Option<HoldingsSnapshot>> {
        use rusqlite::OptionalExtension;
        let connection = self.lock()?;
        let snapshot = connection.query_row("SELECT snapshot_id,source,imported_at_ms,currency,note FROM user_holdings_snapshots ORDER BY imported_at_ms DESC,rowid DESC LIMIT 1", [], |r| {
            Ok(HoldingsSnapshot { snapshot_id:r.get(0)?,source:read_stored_enum(r, 1)?,imported_at_ms:r.get(2)?,currency:r.get(3)?,note:r.get(4)?,positions:vec![] })
        }).optional()?;
        let Some(mut snapshot) = snapshot else {
            return Ok(None);
        };
        let mut statement = connection.prepare("SELECT symbol,market,display_name,quantity,cost_basis,market_value,currency FROM user_holdings_positions WHERE snapshot_id=?1 ORDER BY symbol,market")?;
        snapshot.positions = statement
            .query_map([&snapshot.snapshot_id], |r| {
                Ok(HoldingsPosition {
                    symbol: r.get(0)?,
                    market: r.get(1)?,
                    display_name: r.get(2)?,
                    quantity: r.get(3)?,
                    cost_basis: r.get(4)?,
                    market_value: r.get(5)?,
                    currency: r.get(6)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(Some(snapshot))
    }
}

/// The latest snapshot of each source; wallets are separate sources.
pub(crate) const LATEST_SNAPSHOTS: &str = "SELECT s.snapshot_id, s.source, s.wallet_id, s.imported_at_ms FROM user_holdings_snapshots s WHERE NOT EXISTS (SELECT 1 FROM user_holdings_snapshots t WHERE t.source = s.source AND t.wallet_id IS s.wallet_id AND (t.imported_at_ms > s.imported_at_ms OR (t.imported_at_ms = s.imported_at_ms AND t.rowid > s.rowid)))";

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SourceSummary {
    pub source: HoldingsSource,
    pub imported_at_ms: i64,
    pub count: usize,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct KindSummary {
    pub imported_at_ms: i64,
    pub count: usize,
}
/// Per-source and per-kind counts and dates; never positions or amounts.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct HoldingsOverview {
    pub sources: Vec<SourceSummary>,
    pub stocks: Option<KindSummary>,
    pub crypto: Option<KindSummary>,
}

pub(super) fn insert_snapshot(
    tx: &rusqlite::Transaction<'_>,
    source: HoldingsSource,
    wallet_id: Option<&str>,
    positions: Vec<HoldingsPosition>,
    imported_at_ms: i64,
) -> StoreResult<HoldingsSnapshot> {
    if positions.len() > MAX_HOLDINGS_ROWS {
        return Err(invalid("holdings exceed 2,000 rows"));
    }
    let positions = positions
        .into_iter()
        .map(normalize_position)
        .collect::<StoreResult<Vec<_>>>()?;
    let mut seen = BTreeSet::new();
    if positions
        .iter()
        .any(|p| !seen.insert((&p.symbol, &p.market)))
    {
        return Err(invalid("duplicate position symbol and market"));
    }
    let currency = positions
        .first()
        .map(|p| p.currency.clone())
        .filter(|c| positions.iter().all(|p| &p.currency == c))
        .unwrap_or_default();
    if source != HoldingsSource::Wallet {
        let newer: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM user_holdings_snapshots
            WHERE source=?1 AND wallet_id IS NULL AND imported_at_ms>?2)",
            params![source.as_str(), imported_at_ms],
            |row| row.get(0),
        )?;
        if newer {
            return Err(StoreError::Conflict(
                "holdings snapshot is older than the current source snapshot",
            ));
        }
    }
    let snapshot = HoldingsSnapshot {
        snapshot_id: uuid::Uuid::new_v4().to_string(),
        source,
        imported_at_ms,
        currency,
        note: None,
        positions,
    };
    tx.execute("INSERT INTO user_holdings_snapshots(snapshot_id,source,wallet_id,imported_at_ms,currency,note) VALUES (?1,?2,?3,?4,?5,?6)", params![snapshot.snapshot_id,snapshot.source.as_str(),wallet_id,snapshot.imported_at_ms,snapshot.currency,snapshot.note])?;
    for p in &snapshot.positions {
        tx.execute("INSERT INTO user_holdings_positions(snapshot_id,symbol,market,display_name,quantity,cost_basis,market_value,currency) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)", params![snapshot.snapshot_id,p.symbol,p.market,p.display_name,p.quantity,p.cost_basis,p.market_value,p.currency])?;
    }
    if source != HoldingsSource::Wallet {
        // Only the latest successful snapshot per source is read. Match the
        // readers' timestamp/rowid ordering; a backwards clock was refused above.
        tx.execute("DELETE FROM user_holdings_snapshots WHERE source=?1 AND wallet_id IS NULL
            AND snapshot_id != (SELECT snapshot_id FROM user_holdings_snapshots
                WHERE source=?1 AND wallet_id IS NULL ORDER BY imported_at_ms DESC,rowid DESC LIMIT 1)",
            [source.as_str()])?;
    }
    Ok(snapshot)
}

fn read_bounded(path: &Path) -> StoreResult<Vec<u8>> {
    let file = std::fs::File::open(path).map_err(|_| invalid("could not open CSV file"))?;
    if !file
        .metadata()
        .map_err(|_| invalid("could not inspect CSV file"))?
        .is_file()
    {
        return Err(invalid("CSV path must be a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_CSV_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid("could not read CSV file"))?;
    Ok(bytes)
}

fn header_field(header: &str) -> Option<&'static str> {
    match header.trim().to_lowercase().as_str() {
        "symbol" | "ticker" | "종목코드" => Some("symbol"),
        "market" | "exchange" | "시장" | "거래소" => Some("market"),
        "name" | "종목명" => Some("displayName"),
        "quantity" | "qty" | "수량" | "보유수량" => Some("quantity"),
        "cost" | "avg cost" | "매입가" | "평균단가" => Some("costBasis"),
        "value" | "평가금액" => Some("marketValue"),
        "currency" | "통화" => Some("currency"),
        _ => None,
    }
}
// Bounded RFC 4180-style parser: escaped quotes, embedded newlines, CRLF and UTF-8 BOM.
// Do not split lines: a quoted display name may span physical lines.
fn csv_records(input: &str) -> StoreResult<Vec<Vec<String>>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = input.chars().peekable();
    let mut quoted = false;
    let mut closed = false;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    quoted = false;
                    closed = true;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !closed => quoted = true,
            ',' => {
                row.push(std::mem::take(&mut field));
                closed = false;
            }
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
                closed = false;
                if rows.len() > MAX_HOLDINGS_ROWS + 1 {
                    return Err(invalid("CSV exceeds 2,000 rows"));
                }
            }
            '"' => return Err(invalid("unexpected quote in CSV field")),
            _ if closed => return Err(invalid("unexpected character after closing CSV quote")),
            _ => field.push(c),
        }
    }
    if quoted {
        return Err(invalid("unterminated CSV quote"));
    }
    if !row.is_empty() || !field.is_empty() || closed {
        row.push(field);
        rows.push(row);
    }
    if rows.len() > MAX_HOLDINGS_ROWS + 1 {
        return Err(invalid("CSV exceeds 2,000 rows"));
    }
    Ok(rows)
}
fn parse_csv(bytes: &[u8]) -> StoreResult<HoldingsCsvPreview> {
    if bytes.len() > MAX_CSV_BYTES {
        return Err(invalid("CSV exceeds 5 MB"));
    }
    let input = std::str::from_utf8(bytes)
        .map_err(|_| invalid("CSV must be UTF-8"))?
        .trim_start_matches('\u{feff}');
    let mut preview = HoldingsCsvPreview {
        preview_id: uuid::Uuid::new_v4().to_string(),
        mapping: BTreeMap::new(),
        positions: vec![],
        errors: vec![],
        row_count: 0,
    };
    let records = match csv_records(input) {
        Ok(rows) => rows,
        Err(error) => {
            preview.errors.push(CsvValidationError {
                row: 0,
                field: "csv".into(),
                message: error.to_string(),
            });
            return Ok(preview);
        }
    };
    let Some(headers) = records.first() else {
        return Err(invalid("CSV requires a header"));
    };
    let mut indexes = BTreeMap::new();
    for (index, header) in headers.iter().enumerate() {
        if let Some(key) = header_field(header) {
            if indexes.insert(key, index).is_some() {
                preview.errors.push(CsvValidationError {
                    row: 1,
                    field: key.into(),
                    message: "multiple headers map to this field".into(),
                });
            }
            preview.mapping.insert(key.into(), header.clone());
        }
    }
    for key in ["symbol", "quantity"] {
        if !indexes.contains_key(key) {
            preview.errors.push(CsvValidationError {
                row: 1,
                field: key.into(),
                message: "required header is missing".into(),
            });
        }
    }
    preview.row_count = records.len() - 1;
    if preview.row_count == 0 {
        preview.errors.push(CsvValidationError {
            row: 2,
            field: "csv".into(),
            message: "CSV contains no positions".into(),
        });
    }
    let mut seen = BTreeSet::new();
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
        let get = |key| {
            indexes
                .get(key)
                .and_then(|i| row.get(*i))
                .map(|s| s.trim())
                .unwrap_or("")
        };
        let optional = |key| {
            let value = get(key);
            (!value.is_empty()).then(|| value.to_owned())
        };
        let candidate = HoldingsPosition {
            symbol: get("symbol").into(),
            market: if get("market").is_empty() {
                crate::knowledge::UNKNOWN_MARKET.into()
            } else {
                get("market").into()
            },
            display_name: optional("displayName"),
            quantity: get("quantity").into(),
            cost_basis: optional("costBasis"),
            market_value: optional("marketValue"),
            currency: get("currency").into(),
        };
        match normalize_position(candidate) {
            Ok(p) => {
                if !seen.insert((p.symbol.clone(), p.market.clone())) {
                    error("symbol", "duplicate symbol and market");
                } else {
                    preview.positions.push(p);
                }
            }
            Err(e) => error("position", &e.to_string()),
        }
    }
    Ok(preview)
}

pub mod crypto_file;
pub mod wallet_address;
pub mod wallets;

#[cfg(test)]
mod tests;

/// Identity-only projection for the Map; quantities and values never cross this boundary.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MapContextItem {
    pub symbol: String,
    pub market: String,
    pub display_name: Option<String>,
}

#[derive(Default)]
pub struct MapContext {
    pub watchlist: Vec<MapContextItem>,
    pub holdings: Vec<MapContextItem>,
    pub updated_at_ms: i64,
}

impl SqliteStore {
    /// Read both app-level collections from one snapshot, releasing the app lock
    /// before the caller opens a Loupist transaction (the databases have separate locks).
    pub fn map_context(&self) -> StoreResult<MapContext> {
        let mut connection = self.lock()?;
        let tx = connection.transaction()?;
        let mut context = MapContext::default();
        {
            let mut statement = tx.prepare("SELECT symbol,market,display_name,added_at_ms FROM user_watchlist ORDER BY symbol,market")?;
            for row in statement.query_map([], |r| {
                Ok((
                    MapContextItem {
                        symbol: r.get(0)?,
                        market: r.get(1)?,
                        display_name: r.get(2)?,
                    },
                    r.get::<_, i64>(3)?,
                ))
            })? {
                let (item, at) = row?;
                context.updated_at_ms = context.updated_at_ms.max(at);
                context.watchlist.push(item);
            }
        }
        // The union of the latest snapshot per source: a crypto import never
        // replaces stock holdings, and one symbol held twice is one item.
        {
            let mut statement = tx.prepare(&format!(
                "SELECT s.imported_at_ms, p.symbol, p.market, p.display_name FROM ({LATEST_SNAPSHOTS}) s JOIN user_holdings_positions p ON p.snapshot_id = s.snapshot_id ORDER BY p.symbol, p.market, p.display_name IS NULL"
            ))?;
            let mut seen = BTreeSet::new();
            for row in statement.query_map([], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    MapContextItem {
                        symbol: r.get(1)?,
                        market: r.get(2)?,
                        display_name: r.get(3)?,
                    },
                ))
            })? {
                let (at, item) = row?;
                context.updated_at_ms = context.updated_at_ms.max(at);
                if seen.insert((item.symbol.clone(), item.market.clone())) {
                    context.holdings.push(item);
                }
            }
        }
        tx.commit()?;
        Ok(context)
    }
}
