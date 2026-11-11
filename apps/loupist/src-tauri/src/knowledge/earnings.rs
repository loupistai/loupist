//! Results before and after (finance-earnings-snapshot.md). Around each results
//! date of an equity the user follows, Rust keeps one event: what was expected
//! the day before, what was reported, and the filings that followed. Every row
//! cites dated evidence; differences, revisions and the filing chain are
//! computed here, never by the model. Holdings quantities never enter these
//! tables.
pub mod dates;
pub mod diff;
#[cfg(test)]
mod tests;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    assets::{
        self, class::is_korean_market, observe, AssetClass, AssetEvidence, ObservationKind,
        PageRecord,
    },
    model::mint_id,
    watch::detect::{classify_filing, FilingRoute},
    KnowledgeError, KnowledgeResult,
};
use diff::{CalcView, Comparable, DifferenceView};

pub(crate) const DDL: &str = r#"
CREATE TABLE knowledge_earnings_events (
    loupist_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    period TEXT NOT NULL CHECK (length(period) BETWEEN 1 AND 40),
    fiscal_period_end TEXT CHECK (fiscal_period_end IS NULL OR length(fiscal_period_end) = 10),
    report_date TEXT NOT NULL CHECK (length(report_date) = 10),
    date_source TEXT NOT NULL,
    status TEXT NOT NULL,
    release_json TEXT,
    final_json TEXT,
    before_at_ms INTEGER,
    before_as_of TEXT CHECK (before_as_of IS NULL OR length(before_as_of) = 10),
    after_at_ms INTEGER,
    final_at_ms INTEGER,
    flags_json TEXT NOT NULL DEFAULT '{}',
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (loupist_id, event_id),
    UNIQUE (loupist_id, asset_id, period)
) STRICT;
CREATE TABLE knowledge_earnings_rows (
    row_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    side TEXT NOT NULL,
    measure TEXT NOT NULL CHECK (length(measure) BETWEEN 1 AND 80),
    basis TEXT NOT NULL DEFAULT '' CHECK (length(basis) <= 40),
    period TEXT NOT NULL CHECK (length(period) BETWEEN 1 AND 40),
    low TEXT,
    high TEXT,
    point TEXT,
    unit TEXT NOT NULL,
    currency TEXT CHECK (currency IS NULL OR length(currency) = 3),
    kind TEXT NOT NULL,
    stage TEXT,
    provider TEXT,
    evidence_json TEXT NOT NULL,
    extra_json TEXT NOT NULL DEFAULT '{}' CHECK (length(extra_json) <= 8192),
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    captured_at_ms INTEGER NOT NULL CHECK (captured_at_ms >= 0),
    captured_as_of TEXT CHECK (captured_as_of IS NULL OR length(captured_as_of) = 10),
    origin TEXT NOT NULL,
    origin_message_id TEXT,
    CHECK (point IS NOT NULL OR (low IS NOT NULL AND high IS NOT NULL))
) STRICT;
CREATE INDEX knowledge_earnings_rows_by_event ON knowledge_earnings_rows(loupist_id, event_id, side);
CREATE TABLE knowledge_earnings_tone (
    loupist_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    text TEXT NOT NULL CHECK (length(text) BETWEEN 1 AND 800),
    spans_json TEXT NOT NULL,
    evidence_json TEXT NOT NULL,
    captured_at_ms INTEGER NOT NULL CHECK (captured_at_ms >= 0),
    PRIMARY KEY (loupist_id, event_id)
) STRICT;
CREATE TABLE knowledge_earnings_notices (
    loupist_id TEXT NOT NULL,
    notice_id TEXT NOT NULL,
    event_id TEXT NOT NULL,
    outcome TEXT NOT NULL,
    side TEXT,
    at_ms INTEGER NOT NULL CHECK (at_ms >= 0),
    posted_at_ms INTEGER,
    PRIMARY KEY (loupist_id, notice_id)
) STRICT;
"#;

/// The card shows an event this far ahead, and keeps a reported one this long.
pub const AHEAD_DAYS: i64 = 14;
pub const KEEP_DAYS: i64 = 30;
/// An after- or final-capture is retried hourly at most this many times.
pub const MAX_ATTEMPTS: u32 = 6;
/// The after-capture waits this long after the release is seen.
pub const AFTER_DELAY_MS: i64 = 30 * 60 * 1000;
const RETRY_MS: i64 = 60 * 60 * 1000;
pub const MAX_TONE_CHARS: usize = 200;

/// The fixed measure keys; anything else is `label:` plus the run's own words.
pub const MEASURE_KEYS: &[&str] = &[
    "revenue",
    "operatingIncome",
    "operatingMargin",
    "netIncome",
    "eps",
    "fcf",
];

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[cfg_attr(test, derive(ts_rs::TS))]
        pub enum $name {
            $(#[serde(rename = $text)] $variant),+
        }
        impl $name {
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }
            pub fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some(Self::$variant),)+ _ => None }
            }
        }
    };
}

string_enum!(Unit { Currency => "currency", Percent => "percent", PerShare => "per_share", Number => "number" });
string_enum!(Side { Expected => "expected", Reported => "reported", GuidanceNew => "guidance_new" });
string_enum!(ValueKind { Guidance => "guidance", Consensus => "consensus", BrokerEstimate => "broker_estimate", Implied => "implied", Reported => "reported" });
string_enum!(Stage { Preliminary => "preliminary", Final => "final" });
string_enum!(Status { Upcoming => "upcoming", BeforeCaptured => "before_captured", Reported => "reported", Final => "final" });
string_enum!(DateSource { Calendar => "calendar", IrNotice => "ir_notice", LegalDeadline => "legal_deadline" });
string_enum!(Origin { Host => "host", Chat => "chat", Briefing => "briefing" });

pub fn valid_measure(measure: &str) -> bool {
    MEASURE_KEYS.contains(&measure)
        || measure.strip_prefix("label:").is_some_and(|label| {
            let count = label.trim().chars().count();
            (1..=60).contains(&count) && !label.chars().any(char::is_control)
        })
}

/// `ern_` + 16 hex digits of sha256(asset:period).
pub fn event_id(asset_id: &str, period: &str) -> String {
    format!(
        "ern_{}",
        &crate::util::sha256(format!("{asset_id}:{period}").as_bytes())[..16]
    )
}

pub fn valid_event_id(value: &str) -> bool {
    value.len() == 20
        && value.starts_with("ern_")
        && value[4..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// A filing that closes a step of the chain.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FilingRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    pub title: String,
    pub date: String,
    pub filing_id: String,
    pub evidence: AssetEvidence,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub side: String,
    pub at_ms: i64,
}

/// What the captures found besides rows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flags {
    #[serde(default)]
    pub consensus_late: bool,
    #[serde(default)]
    pub consensus_needs_key: bool,
    #[serde(default)]
    pub failed: Option<Failure>,
    #[serde(default)]
    pub after_attempts: u32,
    #[serde(default)]
    pub final_attempts: u32,
    #[serde(default)]
    pub last_attempt_ms: Option<i64>,
    /// When a check first saw the release (the after-capture waits 30 minutes).
    #[serde(default)]
    pub release_seen_ms: Option<i64>,
    /// Korean annual chain: the audit report.
    #[serde(default)]
    pub audit: Option<FilingRef>,
    /// IR notices already read for a date.
    #[serde(default)]
    pub ir_read: Vec<String>,
    /// The Thread was told the results came in.
    #[serde(default)]
    pub notified: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventRecord {
    pub event_id: String,
    pub asset_id: String,
    pub period: String,
    pub fiscal_period_end: Option<NaiveDate>,
    pub report_date: NaiveDate,
    pub date_source: DateSource,
    pub status: Status,
    pub release: Option<FilingRef>,
    pub final_report: Option<FilingRef>,
    pub before_at_ms: Option<i64>,
    pub before_as_of: Option<NaiveDate>,
    pub after_at_ms: Option<i64>,
    pub final_at_ms: Option<i64>,
    pub flags: Flags,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl EventRecord {
    /// The period end, from the provider or the label.
    pub fn period_end(&self) -> Option<NaiveDate> {
        self.fiscal_period_end
            .or_else(|| dates::label_end(&self.period))
    }

    /// The date the results came out: the release's filing date, else the report date.
    pub fn reported_on(&self) -> NaiveDate {
        self.release
            .as_ref()
            .and_then(|release| observe::parse_day(&release.date))
            .unwrap_or(self.report_date)
    }
}

const EVENT_COLUMNS: &str = "event_id,asset_id,period,fiscal_period_end,report_date,date_source,status,release_json,final_json,before_at_ms,before_as_of,after_at_ms,final_at_ms,flags_json,created_at_ms,updated_at_ms";

fn day(text: &str) -> Option<NaiveDate> {
    observe::parse_day(text)
}

fn read_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawEvent> {
    Ok(RawEvent {
        event_id: row.get(0)?,
        asset_id: row.get(1)?,
        period: row.get(2)?,
        fiscal_period_end: row.get(3)?,
        report_date: row.get(4)?,
        date_source: row.get(5)?,
        status: row.get(6)?,
        release: row.get(7)?,
        final_report: row.get(8)?,
        before_at_ms: row.get(9)?,
        before_as_of: row.get(10)?,
        after_at_ms: row.get(11)?,
        final_at_ms: row.get(12)?,
        flags: row.get(13)?,
        created_at_ms: row.get(14)?,
        updated_at_ms: row.get(15)?,
    })
}

struct RawEvent {
    event_id: String,
    asset_id: String,
    period: String,
    fiscal_period_end: Option<String>,
    report_date: String,
    date_source: String,
    status: String,
    release: Option<String>,
    final_report: Option<String>,
    before_at_ms: Option<i64>,
    before_as_of: Option<String>,
    after_at_ms: Option<i64>,
    final_at_ms: Option<i64>,
    flags: String,
    created_at_ms: i64,
    updated_at_ms: i64,
}

impl RawEvent {
    fn decode(self) -> KnowledgeResult<EventRecord> {
        let parse_json = |text: Option<String>| -> KnowledgeResult<Option<FilingRef>> {
            text.map(|text| serde_json::from_str(&text))
                .transpose()
                .map_err(Into::into)
        };
        Ok(EventRecord {
            event_id: self.event_id,
            asset_id: self.asset_id,
            period: self.period,
            fiscal_period_end: self.fiscal_period_end.as_deref().and_then(day),
            report_date: day(&self.report_date)
                .ok_or(KnowledgeError::Invalid("earnings report date"))?,
            date_source: DateSource::parse(&self.date_source)
                .ok_or(KnowledgeError::Invalid("earnings date source"))?,
            status: Status::parse(&self.status)
                .ok_or(KnowledgeError::Invalid("earnings status"))?,
            release: parse_json(self.release)?,
            final_report: parse_json(self.final_report)?,
            before_at_ms: self.before_at_ms,
            before_as_of: self.before_as_of.as_deref().and_then(day),
            after_at_ms: self.after_at_ms,
            final_at_ms: self.final_at_ms,
            flags: serde_json::from_str(&self.flags).unwrap_or_default(),
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
        })
    }
}

pub fn get_event(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
) -> KnowledgeResult<Option<EventRecord>> {
    tx.query_row(
        &format!("SELECT {EVENT_COLUMNS} FROM knowledge_earnings_events WHERE loupist_id=?1 AND event_id=?2"),
        params![loupist_id, event_id],
        read_event,
    )
    .optional()?
    .map(RawEvent::decode)
    .transpose()
}

/// The asset's events, newest report date first.
pub fn events_for(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Vec<EventRecord>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {EVENT_COLUMNS} FROM knowledge_earnings_events WHERE loupist_id=?1 AND asset_id=?2 ORDER BY report_date DESC, event_id"
    ))?;
    let rows = statement
        .query_map(params![loupist_id, asset_id], read_event)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(RawEvent::decode).collect()
}

/// Every event of the Loupist whose report date is on or after `from`.
pub fn events_since(
    tx: &Transaction<'_>,
    loupist_id: &str,
    from: NaiveDate,
) -> KnowledgeResult<Vec<EventRecord>> {
    let mut statement = tx.prepare(&format!(
        "SELECT {EVENT_COLUMNS} FROM knowledge_earnings_events WHERE loupist_id=?1 AND report_date>=?2 ORDER BY report_date, event_id"
    ))?;
    let rows = statement
        .query_map(params![loupist_id, from.to_string()], read_event)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter().map(RawEvent::decode).collect()
}

fn source_rank(source: DateSource) -> u8 {
    match source {
        DateSource::LegalDeadline => 0,
        DateSource::Calendar => 1,
        DateSource::IrNotice => 2,
    }
}

/// Creates the period's event, or moves its date. A legal deadline never
/// replaces an announced date; a reported event keeps its date.
#[allow(clippy::too_many_arguments)]
pub fn upsert_event(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    period: &str,
    fiscal_period_end: Option<NaiveDate>,
    report_date: NaiveDate,
    source: DateSource,
    now_ms: i64,
) -> KnowledgeResult<(EventRecord, bool)> {
    let id = event_id(asset_id, period);
    if let Some(existing) = get_event(tx, loupist_id, &id)? {
        let movable = matches!(existing.status, Status::Upcoming | Status::BeforeCaptured);
        let outranked = source_rank(source) < source_rank(existing.date_source);
        if !movable
            || outranked
            || (existing.report_date == report_date && existing.date_source == source)
        {
            return Ok((existing, false));
        }
        tx.execute(
            "UPDATE knowledge_earnings_events SET report_date=?3, date_source=?4, fiscal_period_end=COALESCE(fiscal_period_end, ?5), updated_at_ms=?6 WHERE loupist_id=?1 AND event_id=?2",
            params![
                loupist_id,
                id,
                report_date.to_string(),
                source.as_str(),
                fiscal_period_end.map(|day| day.to_string()),
                now_ms
            ],
        )?;
        let updated = get_event(tx, loupist_id, &id)?.ok_or(KnowledgeError::NotFound)?;
        return Ok((updated, true));
    }
    tx.execute(
        "INSERT INTO knowledge_earnings_events(loupist_id,event_id,asset_id,period,fiscal_period_end,report_date,date_source,status,created_at_ms,updated_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,'upcoming',?8,?8)",
        params![
            loupist_id,
            id,
            asset_id,
            period,
            fiscal_period_end.map(|day| day.to_string()),
            report_date.to_string(),
            source.as_str(),
            now_ms
        ],
    )?;
    let created = get_event(tx, loupist_id, &id)?.ok_or(KnowledgeError::NotFound)?;
    Ok((created, true))
}

fn update_flags(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    flags: &Flags,
    now_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_earnings_events SET flags_json=?3, updated_at_ms=?4 WHERE loupist_id=?1 AND event_id=?2",
        params![loupist_id, event_id, serde_json::to_string(flags)?, now_ms],
    )?;
    Ok(())
}

pub fn edit_flags(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    now_ms: i64,
    edit: impl FnOnce(&mut Flags),
) -> KnowledgeResult<()> {
    let mut event = get_event(tx, loupist_id, event_id)?.ok_or(KnowledgeError::NotFound)?;
    edit(&mut event.flags);
    update_flags(tx, loupist_id, event_id, &event.flags, now_ms)
}

/// The release came out: results are reported (preliminary until the final report).
pub fn set_release(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event: &EventRecord,
    filing: &FilingRef,
    now_ms: i64,
) -> KnowledgeResult<bool> {
    if event.release.is_some() {
        return Ok(false);
    }
    let mut flags = event.flags.clone();
    flags.release_seen_ms.get_or_insert(now_ms);
    let status = match event.status {
        Status::Final => Status::Final,
        _ => Status::Reported,
    };
    // A deadline was only a bound: the results came out on the release's day.
    let report_date = if event.date_source == DateSource::LegalDeadline {
        day(&filing.date).unwrap_or(event.report_date)
    } else {
        event.report_date
    };
    tx.execute(
        "UPDATE knowledge_earnings_events SET release_json=?3, status=?4, flags_json=?5, report_date=?6, updated_at_ms=?7 WHERE loupist_id=?1 AND event_id=?2",
        params![
            loupist_id,
            event.event_id,
            serde_json::to_string(filing)?,
            status.as_str(),
            serde_json::to_string(&flags)?,
            report_date.to_string(),
            now_ms
        ],
    )?;
    Ok(true)
}

/// The quarterly or annual report arrived.
pub fn set_final(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event: &EventRecord,
    filing: &FilingRef,
    now_ms: i64,
) -> KnowledgeResult<bool> {
    if event.final_report.is_some() {
        return Ok(false);
    }
    tx.execute(
        "UPDATE knowledge_earnings_events SET final_json=?3, status='final', updated_at_ms=?4 WHERE loupist_id=?1 AND event_id=?2",
        params![loupist_id, event.event_id, serde_json::to_string(filing)?, now_ms],
    )?;
    Ok(true)
}

/// A before-capture finished (the expected rows are written in the same transaction).
pub fn mark_before(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    at_ms: i64,
    as_of: Option<NaiveDate>,
) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_earnings_events SET before_at_ms=?3, before_as_of=COALESCE(?4, before_as_of), status=CASE WHEN status='upcoming' THEN 'before_captured' ELSE status END, updated_at_ms=?3 WHERE loupist_id=?1 AND event_id=?2",
        params![loupist_id, event_id, at_ms, as_of.map(|day| day.to_string())],
    )?;
    Ok(())
}

pub fn mark_after(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    at_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_earnings_events SET after_at_ms=?3, updated_at_ms=?3 WHERE loupist_id=?1 AND event_id=?2",
        params![loupist_id, event_id, at_ms],
    )?;
    Ok(())
}

pub fn mark_final_captured(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    at_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "UPDATE knowledge_earnings_events SET final_at_ms=?3, after_at_ms=COALESCE(after_at_ms, ?3), updated_at_ms=?3 WHERE loupist_id=?1 AND event_id=?2",
        params![loupist_id, event_id, at_ms],
    )?;
    Ok(())
}

/// One value before it is stored. Values are base-unit decimal strings.
#[derive(Clone, Debug, PartialEq)]
pub struct NewRow {
    pub side: Side,
    pub measure: String,
    pub basis: String,
    pub period: String,
    pub low: Option<String>,
    pub high: Option<String>,
    pub point: Option<String>,
    pub unit: Unit,
    pub currency: Option<String>,
    pub kind: ValueKind,
    pub stage: Option<Stage>,
    pub provider: Option<String>,
    pub evidence: Vec<AssetEvidence>,
    /// Implied: {years, price, currency, closeDate, calc}; printed values: {printed: {point, scale}}.
    pub extra: Value,
    pub fact_date: NaiveDate,
    pub captured_as_of: Option<NaiveDate>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RowRecord {
    pub row_id: String,
    pub side: Side,
    pub measure: String,
    pub basis: String,
    pub period: String,
    pub low: Option<f64>,
    pub high: Option<f64>,
    pub point: Option<f64>,
    pub unit: Unit,
    pub currency: Option<String>,
    pub kind: ValueKind,
    pub stage: Option<Stage>,
    pub provider: Option<String>,
    pub evidence: Vec<AssetEvidence>,
    pub extra: Value,
    pub fact_date: String,
    pub captured_at_ms: i64,
    pub captured_as_of: Option<String>,
    pub origin: Origin,
}

impl RowRecord {
    pub fn comparable(&self) -> Comparable<'_> {
        Comparable {
            measure: &self.measure,
            basis: &self.basis,
            period: &self.period,
            unit: self.unit,
            currency: self.currency.as_deref(),
            low: self.low,
            high: self.high,
            point: self.point,
        }
    }

    fn amount(&self) -> AmountView {
        AmountView {
            low: self.low,
            high: self.high,
            point: self.point,
            unit: self.unit,
            currency: self.currency.clone(),
        }
    }
}

fn decimal(value: &Option<String>) -> KnowledgeResult<Option<f64>> {
    match value {
        None => Ok(None),
        Some(text) => text
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && value.abs() <= 1e18)
            .map(Some)
            .ok_or(KnowledgeError::Invalid("earnings value")),
    }
}

/// Stores rows of one capture or one save. Every row needs a source.
pub fn insert_rows(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    rows: &[NewRow],
    origin: Origin,
    message_id: Option<&str>,
    at_ms: i64,
) -> KnowledgeResult<usize> {
    let mut stored = 0;
    for row in rows {
        if !valid_measure(&row.measure)
            || row.evidence.is_empty()
            || row.evidence.len() > 3
            || !row.evidence.iter().any(AssetEvidence::has_source)
        {
            return Err(KnowledgeError::Invalid(
                "earnings row needs a measure and 1–3 sources",
            ));
        }
        let (low, high, point) = (
            decimal(&row.low)?,
            decimal(&row.high)?,
            decimal(&row.point)?,
        );
        if point.is_none() && (low.is_none() || high.is_none()) {
            return Err(KnowledgeError::Invalid(
                "earnings row needs a point or a range",
            ));
        }
        if let (Some(low), Some(high)) = (low, high) {
            if low > high {
                return Err(KnowledgeError::Invalid("earnings range low above high"));
            }
        }
        let extra = serde_json::to_string(&row.extra)?;
        if extra.len() > 8192 {
            return Err(KnowledgeError::Invalid("earnings row extra too large"));
        }
        tx.execute(
            "INSERT INTO knowledge_earnings_rows(row_id,loupist_id,event_id,side,measure,basis,period,low,high,point,unit,currency,kind,stage,provider,evidence_json,extra_json,fact_date,captured_at_ms,captured_as_of,origin,origin_message_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)",
            params![
                mint_id("erw"),
                loupist_id,
                event_id,
                row.side.as_str(),
                row.measure,
                row.basis.trim(),
                row.period.trim(),
                row.low,
                row.high,
                row.point,
                row.unit.as_str(),
                row.currency.as_ref().map(|code| code.to_ascii_uppercase()),
                row.kind.as_str(),
                row.stage.map(Stage::as_str),
                row.provider,
                serde_json::to_string(&row.evidence)?,
                extra,
                row.fact_date.to_string(),
                at_ms,
                row.captured_as_of.map(|day| day.to_string()),
                origin.as_str(),
                message_id,
            ],
        )?;
        stored += 1;
    }
    Ok(stored)
}

/// Prepare again before the report date: the host's earlier expectations go.
pub fn delete_host_expected(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
) -> KnowledgeResult<usize> {
    Ok(tx.execute(
        "DELETE FROM knowledge_earnings_rows WHERE loupist_id=?1 AND event_id=?2 AND side='expected' AND origin='host'",
        params![loupist_id, event_id],
    )?)
}

pub fn rows(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
) -> KnowledgeResult<Vec<RowRecord>> {
    let mut statement = tx.prepare(
        "SELECT row_id,side,measure,basis,period,low,high,point,unit,currency,kind,stage,provider,evidence_json,extra_json,fact_date,captured_at_ms,captured_as_of,origin FROM knowledge_earnings_rows WHERE loupist_id=?1 AND event_id=?2 ORDER BY captured_at_ms, row_id",
    )?;
    let raw = statement
        .query_map(params![loupist_id, event_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                [
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ],
                row.get::<_, String>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, Option<String>>(11)?,
                row.get::<_, Option<String>>(12)?,
                (
                    row.get::<_, String>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, String>(15)?,
                    row.get::<_, i64>(16)?,
                    row.get::<_, Option<String>>(17)?,
                    row.get::<_, String>(18)?,
                ),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    raw.into_iter()
        .map(
            |(
                row_id,
                side,
                measure,
                basis,
                period,
                [low, high, point],
                unit,
                currency,
                kind,
                stage,
                provider,
                (evidence, extra, fact_date, captured_at_ms, captured_as_of, origin),
            )| {
                Ok(RowRecord {
                    row_id,
                    side: Side::parse(&side).ok_or(KnowledgeError::Invalid("earnings side"))?,
                    measure,
                    basis,
                    period,
                    low: decimal(&low)?,
                    high: decimal(&high)?,
                    point: decimal(&point)?,
                    unit: Unit::parse(&unit).ok_or(KnowledgeError::Invalid("earnings unit"))?,
                    currency,
                    kind: ValueKind::parse(&kind)
                        .ok_or(KnowledgeError::Invalid("earnings kind"))?,
                    stage: stage.as_deref().and_then(Stage::parse),
                    provider,
                    evidence: serde_json::from_str(&evidence)?,
                    extra: serde_json::from_str(&extra)?,
                    fact_date,
                    captured_at_ms,
                    captured_as_of,
                    origin: Origin::parse(&origin)
                        .ok_or(KnowledgeError::Invalid("earnings origin"))?,
                })
            },
        )
        .collect()
}

/// Expected rows can be written only before the report date, or once by a
/// late capture under its host-set cutoff when none exist; then they are locked.
pub fn expected_writable(
    event: &EventRecord,
    existing: &[RowRecord],
    today: NaiveDate,
    late_cutoff: Option<NaiveDate>,
) -> Result<(), &'static str> {
    if event.release.is_none()
        && matches!(event.status, Status::Upcoming | Status::BeforeCaptured)
        && today < event.report_date
        && late_cutoff.is_none()
    {
        return Ok(());
    }
    let any_expected = existing.iter().any(|row| row.side == Side::Expected);
    if late_cutoff.is_some() && !any_expected {
        return Ok(());
    }
    Err("earnings_expected_locked")
}

pub fn set_tone(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    text: &str,
    spans: &[String],
    evidence: &[AssetEvidence],
    at_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "INSERT INTO knowledge_earnings_tone(loupist_id,event_id,text,spans_json,evidence_json,captured_at_ms) VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(loupist_id,event_id) DO UPDATE SET text=excluded.text, spans_json=excluded.spans_json, evidence_json=excluded.evidence_json, captured_at_ms=excluded.captured_at_ms",
        params![
            loupist_id,
            event_id,
            text,
            serde_json::to_string(spans)?,
            serde_json::to_string(evidence)?,
            at_ms
        ],
    )?;
    Ok(())
}

fn tone(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
) -> KnowledgeResult<Option<ToneView>> {
    let raw: Option<(String, String, String)> = tx
        .query_row(
            "SELECT text,spans_json,evidence_json FROM knowledge_earnings_tone WHERE loupist_id=?1 AND event_id=?2",
            params![loupist_id, event_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    raw.map(|(text, spans, evidence)| {
        Ok(ToneView {
            text,
            spans: serde_json::from_str(&spans)?,
            evidence: serde_json::from_str(&evidence)?,
        })
    })
    .transpose()
}

/// A Thread message the earnings task owes: reported results, or a failed before-capture.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub notice_id: String,
    pub event_id: String,
    pub outcome: String,
    pub side: Option<String>,
    pub at_ms: i64,
}

pub fn add_notice(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_id: &str,
    outcome: &str,
    side: Option<&str>,
    at_ms: i64,
) -> KnowledgeResult<()> {
    // Rust owns these lists (Knowledge schema 7 dropped the SQL whitelists).
    if !matches!(outcome, "reported" | "failed") || !matches!(side, None | Some("before" | "after"))
    {
        return Err(KnowledgeError::Invalid("results notice is invalid"));
    }
    tx.execute(
        "INSERT INTO knowledge_earnings_notices(loupist_id,notice_id,event_id,outcome,side,at_ms) VALUES(?1,?2,?3,?4,?5,?6)",
        params![loupist_id, mint_id("ent"), event_id, outcome, side, at_ms],
    )?;
    Ok(())
}

pub fn pending_notices(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<Notice>> {
    let mut statement = tx.prepare(
        "SELECT notice_id,event_id,outcome,side,at_ms FROM knowledge_earnings_notices WHERE loupist_id=?1 AND posted_at_ms IS NULL ORDER BY at_ms, notice_id",
    )?;
    let rows = statement
        .query_map([loupist_id], |row| {
            Ok(Notice {
                notice_id: row.get(0)?,
                event_id: row.get(1)?,
                outcome: row.get(2)?,
                side: row.get(3)?,
                at_ms: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn mark_notices_posted(
    tx: &Transaction<'_>,
    loupist_id: &str,
    notice_ids: &[String],
    at_ms: i64,
) -> KnowledgeResult<()> {
    for id in notice_ids {
        tx.execute(
            "UPDATE knowledge_earnings_notices SET posted_at_ms=?3 WHERE loupist_id=?1 AND notice_id=?2",
            params![loupist_id, id, at_ms],
        )?;
    }
    Ok(())
}

pub fn erase(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<()> {
    for table in [
        "knowledge_earnings_notices",
        "knowledge_earnings_tone",
        "knowledge_earnings_rows",
        "knowledge_earnings_events",
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE loupist_id = ?1"),
            [loupist_id],
        )?;
    }
    Ok(())
}

// ───────────────────────────── Views ─────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AmountView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub low: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub high: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub point: Option<f64>,
    pub unit: Unit,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub currency: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ValueView {
    pub row_id: String,
    pub kind: ValueKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub stage: Option<Stage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub provider: Option<String>,
    pub fact_date: String,
    pub evidence: Vec<AssetEvidence>,
    #[serde(flatten)]
    pub amount: AmountView,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct MeasureView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub label: Option<String>,
    pub period: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub basis: Option<String>,
    pub expected: Vec<ValueView>,
    pub reported: Vec<ValueView>,
    pub differences: Vec<DifferenceView>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub guidance: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ChainStepView {
    pub stage: String,
    pub filed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub form: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub due_by: Option<String>,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct RevisionView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub label: Option<String>,
    pub from: AmountView,
    pub to: AmountView,
    pub calc: CalcView,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ImpliedView {
    pub low_pct: f64,
    pub high_pct: f64,
    pub years: u32,
    pub price: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub currency: Option<String>,
    pub close_date: String,
    pub calc: CalcView,
    /// The whole reverse-DCF grid behind the range ("Open the grid").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub grid: Option<crate::domain::SensitivityGrid>,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ReactionView {
    pub from: f64,
    pub to: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub currency: Option<String>,
    pub from_date: String,
    pub to_date: String,
    pub calc: CalcView,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ToneView {
    pub text: String,
    pub spans: Vec<String>,
    pub evidence: Vec<AssetEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct BeforeView {
    pub captured_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub late_as_of: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct AfterView {
    pub captured_at_ms: i64,
    pub date: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    pub event_id: String,
    pub asset_id: String,
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub period: String,
    pub report_date: String,
    pub date_source: DateSource,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub capture_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub before: Option<BeforeView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub after: Option<AfterView>,
    pub measures: Vec<MeasureView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub implied: Option<ImpliedView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reaction: Option<ReactionView>,
    pub reaction_waiting: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tone: Option<ToneView>,
    pub chain: Vec<ChainStepView>,
    pub revisions: Vec<RevisionView>,
    pub unchanged: bool,
    pub consensus_late: bool,
    pub consensus_needs_key: bool,
    pub no_guidance: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub failed: Option<Failure>,
    pub preparing: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct EventSummary {
    pub event_id: String,
    pub period: String,
    pub report_date: String,
    pub date_source: DateSource,
    pub status: Status,
}

impl From<&EventRecord> for EventSummary {
    fn from(event: &EventRecord) -> Self {
        Self {
            event_id: event.event_id.clone(),
            period: event.period.clone(),
            report_date: event.report_date.to_string(),
            date_source: event.date_source,
            status: event.status,
        }
    }
}

/// One completed close of the asset (from its page's observations).
#[derive(Clone, Debug, PartialEq)]
pub struct Close {
    pub date: NaiveDate,
    pub close: f64,
    pub currency: Option<String>,
    pub evidence: AssetEvidence,
}

/// The asset's price closes, oldest first, from the one series it records most.
pub fn closes(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
) -> KnowledgeResult<Vec<Close>> {
    let observed = assets::observations(tx, loupist_id, asset_id)?;
    let mut by_series: std::collections::BTreeMap<String, Vec<Close>> = Default::default();
    for item in observed {
        if item.kind != ObservationKind::Close {
            continue;
        }
        let series = item.facts["series"]
            .as_str()
            .unwrap_or(observe::PRICE)
            .to_owned();
        if !observe::is_price_series(&series) {
            continue;
        }
        let (Some(date), Some(close)) = (day(&item.fact_date), item.facts["close"].as_f64()) else {
            continue;
        };
        by_series.entry(series).or_default().push(Close {
            date,
            close,
            currency: item.facts["currency"].as_str().map(str::to_owned),
            evidence: item.evidence,
        });
    }
    let mut best = by_series
        .into_values()
        .max_by_key(|closes| closes.len())
        .unwrap_or_default();
    best.sort_by_key(|close| close.date);
    Ok(best)
}

/// When the before-capture runs: 18:00 in the listing's zone the day before
/// the report date (14 days before a legal deadline, which is only a bound).
pub fn capture_at(event: &EventRecord, market: &str) -> DateTime<Utc> {
    let lead = if event.date_source == DateSource::LegalDeadline {
        AHEAD_DAYS
    } else {
        1
    };
    dates::evening(event.report_date - Duration::days(lead), market)
}

/// What the page shows now: an event within 14 days ahead, or reported in the
/// last 30. A passed date whose release has not been seen yet stays current
/// ("Not reported yet") for the same 30 days.
pub fn current_event(events: &[EventRecord], today: NaiveDate) -> Option<&EventRecord> {
    let shown = |event: &&EventRecord| {
        let reported =
            event.release.is_some() || matches!(event.status, Status::Reported | Status::Final);
        if reported {
            event.reported_on() >= today - Duration::days(KEEP_DAYS)
        } else {
            event.report_date >= today - Duration::days(KEEP_DAYS)
                && event.report_date <= today + Duration::days(AHEAD_DAYS)
        }
    };
    events.iter().filter(shown).min_by_key(|event| {
        // An upcoming event wins over one reported a while ago.
        let reported = event.release.is_some();
        (reported, (event.report_date - today).num_days().abs())
    })
}

fn measure_order(measure: &str) -> usize {
    MEASURE_KEYS
        .iter()
        .position(|key| *key == measure)
        .unwrap_or(MEASURE_KEYS.len())
}

fn kind_order(kind: ValueKind) -> u8 {
    match kind {
        ValueKind::Guidance => 0,
        ValueKind::Consensus => 1,
        ValueKind::BrokerEstimate => 2,
        ValueKind::Implied => 3,
        ValueKind::Reported => 4,
    }
}

fn value_view(row: &RowRecord) -> ValueView {
    ValueView {
        row_id: row.row_id.clone(),
        kind: row.kind,
        stage: row.stage,
        provider: row.provider.clone(),
        fact_date: row.fact_date.clone(),
        evidence: row.evidence.clone(),
        amount: row.amount(),
    }
}

fn key_and_label(measure: &str) -> (Option<String>, Option<String>) {
    match measure.strip_prefix("label:") {
        Some(label) => (None, Some(label.trim().to_owned())),
        None => (Some(measure.to_owned()), None),
    }
}

fn scale_of(row: &RowRecord) -> f64 {
    row.extra
        .pointer("/printed/scale")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
}

fn printed_point(row: &RowRecord) -> Option<&str> {
    row.extra.pointer("/printed/point").and_then(Value::as_str)
}

/// The measure rows of an event, with Rust's differences.
pub fn measures(
    event: &EventRecord,
    rows: &[RowRecord],
    earlier_guidance: &[RowRecord],
) -> (Vec<MeasureView>, Vec<RevisionView>) {
    // One row per measure and period: a value of another basis stays in the
    // row and its difference says why it is not comparable.
    let mut groups: Vec<(String, String, String)> = Vec::new();
    for row in rows.iter().filter(|row| row.kind != ValueKind::Implied) {
        if row.side == Side::GuidanceNew {
            continue;
        }
        let key = (row.measure.clone(), String::new(), row.period.clone());
        if !groups.contains(&key) {
            groups.push(key);
        }
    }
    groups.sort_by_key(|(measure, _, period)| {
        (period.clone(), measure_order(measure), measure.clone())
    });
    let mut views = Vec::new();
    let mut revisions = Vec::new();
    for (measure, _, period) in &groups {
        let same = |row: &&RowRecord| &row.measure == measure && &row.period == period;
        let mut expected: Vec<&RowRecord> = rows
            .iter()
            .filter(same)
            .filter(|row| row.side == Side::Expected && row.kind != ValueKind::Implied)
            .collect();
        expected.sort_by_key(|row| (kind_order(row.kind), row.captured_at_ms));
        let mut reported: Vec<&RowRecord> = rows
            .iter()
            .filter(same)
            .filter(|row| row.side == Side::Reported)
            .collect();
        // The final report first, then the newest preliminary figure.
        reported.sort_by_key(|row| {
            (
                row.stage != Some(Stage::Final),
                std::cmp::Reverse(row.captured_at_ms),
            )
        });
        // Rows of another basis or period still show, each compared against
        // the latest reported value, which says why they are not comparable.
        let latest = reported.first().copied();
        let differences = match latest {
            Some(actual) => expected
                .iter()
                .map(|value| diff::difference(&value.comparable(), &actual.comparable()))
                .collect(),
            None => Vec::new(),
        };
        let preliminary = reported
            .iter()
            .find(|row| row.stage == Some(Stage::Preliminary));
        let final_row = reported.iter().find(|row| row.stage == Some(Stage::Final));
        if let (Some(before), Some(after)) = (preliminary, final_row) {
            if let (Some(from), Some(to)) = (before.point, after.point) {
                let tolerance = diff::half_unit(printed_point(before), scale_of(before));
                if (to - from).abs() > tolerance {
                    let (key, label) = key_and_label(measure);
                    revisions.push(RevisionView {
                        key,
                        label,
                        from: before.amount(),
                        to: after.amount(),
                        calc: diff::percent_change(from, to, "Preliminary", "Final"),
                        evidence: after.evidence.clone(),
                    });
                }
            }
        }
        let (key, label) = key_and_label(measure);
        views.push(MeasureView {
            key,
            label,
            period: period.clone(),
            // The reported figure's basis names the row; an expected value of
            // another basis says so in its difference.
            basis: reported
                .first()
                .or(expected.first())
                .map(|row| row.basis.clone())
                .filter(|basis| !basis.is_empty()),
            expected: expected.into_iter().map(value_view).collect(),
            reported: reported.into_iter().map(value_view).collect(),
            differences,
            guidance: false,
        });
    }
    // New guidance rows: against earlier guidance for the same period, by midpoint.
    let mut guidance: Vec<&RowRecord> = rows
        .iter()
        .filter(|row| row.side == Side::GuidanceNew)
        .collect();
    guidance.sort_by_key(|row| (row.period.clone(), measure_order(&row.measure)));
    for row in guidance {
        let earlier = earlier_guidance
            .iter()
            .filter(|other| {
                other.row_id != row.row_id
                    && other.measure == row.measure
                    && other.period == row.period
                    && other.captured_at_ms < row.captured_at_ms
            })
            .max_by_key(|other| other.captured_at_ms);
        let (key, label) = key_and_label(&row.measure);
        views.push(MeasureView {
            key,
            label,
            period: row.period.clone(),
            basis: (!row.basis.is_empty()).then(|| row.basis.clone()),
            expected: Vec::new(),
            reported: vec![value_view(row)],
            differences: vec![diff::guidance(
                earlier.map(RowRecord::comparable).as_ref(),
                &row.comparable(),
            )],
            guidance: true,
        });
    }
    let _ = event;
    (views, revisions)
}

fn chain(event: &EventRecord, korean: bool) -> Vec<ChainStepView> {
    let filed =
        |filing: &Option<FilingRef>| -> (bool, Option<String>, Option<String>, Vec<AssetEvidence>) {
            match filing {
                Some(filing) => (
                    true,
                    Some(filing.date.clone()),
                    filing.form.clone().or_else(|| Some(filing.title.clone())),
                    vec![filing.evidence.clone()],
                ),
                None => (false, None, None, Vec::new()),
            }
        };
    let end = event.period_end();
    let year_end = end.is_some_and(dates::is_year_end);
    let due = |stage_is_final: bool| -> Option<String> {
        (korean && stage_is_final)
            .then(|| end.map(dates::korea_deadline))
            .flatten()
            .map(|date| date.to_string())
    };
    let mut steps = Vec::new();
    let (release_filed, release_date, release_form, release_evidence) = filed(&event.release);
    steps.push(ChainStepView {
        stage: if korean { "preliminary" } else { "release" }.into(),
        filed: release_filed,
        date: release_date,
        form: release_form,
        due_by: None,
        evidence: release_evidence,
    });
    if korean && year_end {
        let (audit_filed, audit_date, audit_form, audit_evidence) = filed(&event.flags.audit);
        steps.push(ChainStepView {
            stage: "audit".into(),
            filed: audit_filed,
            date: audit_date,
            form: audit_form,
            due_by: None,
            evidence: audit_evidence,
        });
    }
    let stage = if year_end {
        "annual"
    } else if korean && end.is_some_and(|end| chrono::Datelike::month(&end) == 6) {
        "half"
    } else {
        "quarterly"
    };
    let (final_filed, final_date, final_form, final_evidence) = filed(&event.final_report);
    steps.push(ChainStepView {
        stage: stage.into(),
        filed: final_filed,
        date: final_date,
        form: final_form,
        due_by: (!final_filed).then(|| due(true)).flatten(),
        evidence: final_evidence,
    });
    steps
}

fn reaction(event: &EventRecord, closes: &[Close]) -> (Option<ReactionView>, bool) {
    let reported =
        event.release.is_some() || matches!(event.status, Status::Reported | Status::Final);
    if !reported {
        return (None, false);
    }
    let on = event.reported_on();
    let before = closes.iter().rev().find(|close| close.date < on);
    let after = closes.iter().find(|close| close.date > on);
    match (before, after) {
        (Some(from), Some(to)) => (
            Some(ReactionView {
                from: from.close,
                to: to.close,
                currency: to.currency.clone().or_else(|| from.currency.clone()),
                from_date: from.date.to_string(),
                to_date: to.date.to_string(),
                calc: diff::percent_change(from.close, to.close, "Close before", "Close after"),
                evidence: vec![from.evidence.clone(), to.evidence.clone()],
            }),
            false,
        ),
        (Some(_), None) => (None, true),
        _ => (None, false),
    }
}

fn implied(rows: &[RowRecord]) -> Option<ImpliedView> {
    let row = rows
        .iter()
        .filter(|row| row.kind == ValueKind::Implied)
        .max_by_key(|row| row.captured_at_ms)?;
    let extra = &row.extra;
    Some(ImpliedView {
        low_pct: row.low?,
        high_pct: row.high?,
        years: extra["years"].as_u64().unwrap_or(5) as u32,
        price: extra["price"].as_f64()?,
        currency: extra["currency"].as_str().map(str::to_owned),
        close_date: extra["closeDate"].as_str()?.to_owned(),
        calc: serde_json::from_value(extra["calc"].clone()).ok()?,
        grid: serde_json::from_value::<crate::domain::SensitivityGrid>(extra["grid"].clone())
            .ok()
            .filter(crate::domain::SensitivityGrid::valid),
        evidence: row.evidence.clone(),
    })
}

/// Everything the card needs for one event.
pub struct SnapshotContext {
    pub preparing: bool,
    /// Whether FMP or Intrinio is connected for this Loupist.
    pub consensus_connected: bool,
}

pub fn snapshot(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event: &EventRecord,
    page: &PageRecord,
    context: &SnapshotContext,
) -> KnowledgeResult<SnapshotView> {
    let rows = rows(tx, loupist_id, &event.event_id)?;
    // Earlier guidance for the same periods, from this asset's other events.
    let mut earlier = Vec::new();
    for other in events_for(tx, loupist_id, &event.asset_id)? {
        for row in self::rows(tx, loupist_id, &other.event_id)? {
            if row.side == Side::GuidanceNew
                || (row.side == Side::Expected && row.kind == ValueKind::Guidance)
            {
                earlier.push(row);
            }
        }
    }
    let (measures, revisions) = measures(event, &rows, &earlier);
    let closes = closes(tx, loupist_id, &event.asset_id)?;
    let (reaction, reaction_waiting) = reaction(event, &closes);
    let korean = is_korean_market(&page.market);
    let reported =
        event.release.is_some() || matches!(event.status, Status::Reported | Status::Final);
    let final_rows = rows
        .iter()
        .any(|row| row.side == Side::Reported && row.stage == Some(Stage::Final));
    let captured = event.before_at_ms.is_some();
    let guidance = rows
        .iter()
        .any(|row| row.side == Side::Expected && row.kind == ValueKind::Guidance);
    Ok(SnapshotView {
        event_id: event.event_id.clone(),
        asset_id: event.asset_id.clone(),
        name: page.name.clone(),
        symbol: page.symbol.clone(),
        market: page.market.clone(),
        period: event.period.clone(),
        report_date: event.report_date.to_string(),
        date_source: event.date_source,
        status: event.status,
        capture_at_ms: (!captured && !reported)
            .then(|| capture_at(event, &page.market).timestamp_millis()),
        before: event.before_at_ms.map(|at| BeforeView {
            captured_at_ms: at,
            late_as_of: event.before_as_of.map(|day| day.to_string()),
        }),
        after: event.after_at_ms.map(|at| AfterView {
            captured_at_ms: at,
            date: event.reported_on().to_string(),
        }),
        measures,
        implied: implied(&rows),
        reaction,
        reaction_waiting,
        tone: tone(tx, loupist_id, &event.event_id)?,
        chain: chain(event, korean),
        unchanged: event.status == Status::Final && final_rows && revisions.is_empty(),
        revisions,
        consensus_late: event.flags.consensus_late,
        consensus_needs_key: !korean
            && (event.flags.consensus_needs_key || (!captured && !context.consensus_connected)),
        no_guidance: (captured || reported) && !guidance,
        failed: event.flags.failed.clone(),
        preparing: context.preparing,
    })
}

/// The compact form `asset_page_read` and briefings hand the model: values
/// with their dates and sources, Rust's differences, never quantities held.
pub fn model_view(snapshot: &SnapshotView) -> Value {
    let source = |evidence: &[AssetEvidence]| -> Vec<Value> {
        evidence
            .iter()
            .map(|item| json!({"title": item.title, "url": item.url, "date": item.date}))
            .collect()
    };
    let value = |item: &ValueView| {
        json!({
            "kind": item.kind, "stage": item.stage, "provider": item.provider,
            "low": item.amount.low, "high": item.amount.high, "point": item.amount.point,
            "unit": item.amount.unit, "currency": item.amount.currency,
            "factDate": item.fact_date, "sources": source(&item.evidence),
        })
    };
    json!({
        "eventId": snapshot.event_id,
        "period": snapshot.period,
        "reportDate": snapshot.report_date,
        "dateSource": snapshot.date_source,
        "status": snapshot.status,
        "expectationsCapturedAtMs": snapshot.before.as_ref().map(|before| before.captured_at_ms),
        "lateCaptureAsOf": snapshot.before.as_ref().and_then(|before| before.late_as_of.clone()),
        "measures": snapshot.measures.iter().map(|measure| json!({
            "measure": measure.key.clone().or(measure.label.clone()),
            "period": measure.period,
            "basis": measure.basis,
            "newGuidance": measure.guidance,
            "expected": measure.expected.iter().map(value).collect::<Vec<_>>(),
            "reported": measure.reported.iter().map(value).collect::<Vec<_>>(),
            "differencesComputedByRust": measure.differences,
        })).collect::<Vec<_>>(),
        "priceImplied": snapshot.implied.as_ref().map(|implied| json!({"lowPct": implied.low_pct, "highPct": implied.high_pct, "years": implied.years, "closeDate": implied.close_date, "price": implied.price})),
        "priceReaction": snapshot.reaction.as_ref().map(|reaction| json!({"from": reaction.from, "to": reaction.to, "fromDate": reaction.from_date, "toDate": reaction.to_date, "change": reaction.calc.display})),
        "callTone": snapshot.tone.as_ref().map(|tone| tone.text.clone()),
        "filedSoFar": snapshot.chain.iter().map(|step| json!({"stage": step.stage, "filed": step.filed, "date": step.date, "dueBy": step.due_by, "sources": source(&step.evidence)})).collect::<Vec<_>>(),
        "revisions": snapshot.revisions.iter().map(|revision| json!({"measure": revision.key.clone().or(revision.label.clone()), "from": revision.from.point, "to": revision.to.point, "change": revision.calc.display})).collect::<Vec<_>>(),
        "consensusNotCapturedInTime": snapshot.consensus_late,
        "consensusNeedsKey": snapshot.consensus_needs_key,
    })
}

// ───────────────────────────── Dates and filings ─────────────────────────────

fn filing_ref(item: &assets::StoredObservation) -> FilingRef {
    FilingRef {
        form: item.facts["form"].as_str().map(str::to_owned),
        title: item.facts["title"].as_str().unwrap_or_default().to_owned(),
        date: item.fact_date.clone(),
        filing_id: item.facts["filingId"]
            .as_str()
            .unwrap_or(&item.dedupe_key)
            .to_owned(),
        evidence: item.evidence.clone(),
    }
}

/// An IR notice the caller should read for its date (one native read each).
#[derive(Clone, Debug, PartialEq)]
pub struct IrNotice {
    pub filing_id: String,
    pub filed: NaiveDate,
    pub evidence: AssetEvidence,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SyncOutcome {
    pub changed: bool,
    /// Releases newly seen (the after-capture is due 30 minutes later).
    pub released: Vec<String>,
    pub ir_notices: Vec<IrNotice>,
}

fn us_final_form(form: &str) -> bool {
    let form = form.trim().to_uppercase();
    matches!(form.as_str(), "10-Q" | "10-K" | "20-F" | "40-F" | "6-K")
}

/// Keeps the asset's events in step with its observations: announced dates,
/// Korean deadlines and IR notices, releases and final reports.
pub fn sync(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    today: NaiveDate,
    now_ms: i64,
) -> KnowledgeResult<SyncOutcome> {
    let mut outcome = SyncOutcome::default();
    if page.class != AssetClass::Equity {
        return Ok(outcome);
    }
    let korean = is_korean_market(&page.market);
    let observed = assets::observations(tx, loupist_id, &page.asset_id)?;
    // Announced dates (calendar reads): the latest announcement per period.
    let mut announced: std::collections::BTreeMap<String, (NaiveDate, NaiveDate, (String, i64))> =
        Default::default();
    for item in observed
        .iter()
        .filter(|item| item.kind == ObservationKind::Earnings)
    {
        let Some(date) = item.facts["date"].as_str().and_then(day) else {
            continue;
        };
        if date < today - Duration::days(1) {
            continue;
        }
        let end = item.facts["periodEnd"]
            .as_str()
            .and_then(day)
            .filter(|end| *end < date)
            .unwrap_or_else(|| dates::quarter_end_before(date));
        let period = dates::period_label(dates::quarter_end_on_or_after(end));
        let seen = (item.fact_date.clone(), item.recorded_at_ms);
        if announced
            .get(&period)
            .is_none_or(|(_, _, latest)| seen >= *latest)
        {
            announced.insert(period, (end, date, seen));
        }
    }
    for (period, (end, date, _)) in announced {
        let (_, changed) = upsert_event(
            tx,
            loupist_id,
            &page.asset_id,
            &period,
            Some(end),
            date,
            DateSource::Calendar,
            now_ms,
        )?;
        outcome.changed |= changed;
    }
    if korean {
        let end = dates::korea_next_period(today);
        let (_, changed) = upsert_event(
            tx,
            loupist_id,
            &page.asset_id,
            &dates::period_label(end),
            Some(end),
            dates::korea_deadline(end),
            DateSource::LegalDeadline,
            now_ms,
        )?;
        outcome.changed |= changed;
    }
    let filings: Vec<&assets::StoredObservation> = observed
        .iter()
        .filter(|item| item.kind == ObservationKind::Filing)
        .collect();
    let events = events_for(tx, loupist_id, &page.asset_id)?;
    for filing in &filings {
        let Some(filed) = day(&filing.fact_date) else {
            continue;
        };
        if filed < today - Duration::days(200) {
            continue;
        }
        let title = filing.facts["title"].as_str().unwrap_or_default();
        let form = filing.facts["form"].as_str().unwrap_or_default();
        if korean {
            match classify_filing(&filing.facts, true) {
                FilingRoute::IrNotice => {
                    let id = filing_ref(filing).filing_id;
                    let read = events.iter().any(|event| event.flags.ir_read.contains(&id));
                    if !read && filed >= today - Duration::days(60) {
                        outcome.ir_notices.push(IrNotice {
                            filing_id: id,
                            filed,
                            evidence: filing.evidence.clone(),
                        });
                    }
                }
                FilingRoute::PreliminaryResults => {
                    let end = dates::quarter_end_before(filed);
                    let period = dates::period_label(end);
                    let event = match events.iter().find(|event| event.period == period) {
                        Some(event) => event.clone(),
                        None => {
                            upsert_event(
                                tx,
                                loupist_id,
                                &page.asset_id,
                                &period,
                                Some(end),
                                filed,
                                DateSource::LegalDeadline,
                                now_ms,
                            )?
                            .0
                        }
                    };
                    if set_release(tx, loupist_id, &event, &filing_ref(filing), now_ms)? {
                        outcome.changed = true;
                        outcome.released.push(event.event_id.clone());
                    }
                }
                _ => {
                    if let Some(report) = dates::korean_report(title) {
                        let Some((year, month)) = dates::korean_report_month(title) else {
                            continue;
                        };
                        let Some(event) = events.iter().find(|event| {
                            event.period_end().is_some_and(|end| {
                                chrono::Datelike::year(&end) == year
                                    && chrono::Datelike::month(&end) == month
                            })
                        }) else {
                            continue;
                        };
                        if report == dates::KoreanReport::Audit {
                            if event.flags.audit.is_none() {
                                let audit = filing_ref(filing);
                                edit_flags(tx, loupist_id, &event.event_id, now_ms, |flags| {
                                    flags.audit = Some(audit)
                                })?;
                                outcome.changed = true;
                            }
                        } else if set_final(tx, loupist_id, event, &filing_ref(filing), now_ms)? {
                            outcome.changed = true;
                        }
                    } else if title.contains("감사보고서") {
                        // An audit report title without a period: the latest year-end event.
                        if let Some(event) = events.iter().find(|event| {
                            event.period_end().is_some_and(dates::is_year_end)
                                && event.flags.audit.is_none()
                                && event.period_end().is_some_and(|end| filed > end)
                        }) {
                            let audit = filing_ref(filing);
                            edit_flags(tx, loupist_id, &event.event_id, now_ms, |flags| {
                                flags.audit = Some(audit)
                            })?;
                            outcome.changed = true;
                        }
                    }
                }
            }
            continue;
        }
        // US and global: an 8-K with item 2.02, or any 8-K the day before to
        // the day after the date, is the release; a 10-Q or 10-K is final.
        let upper = form.trim().to_uppercase();
        if upper == "8-K" || upper == "6-K" {
            let items = filing.facts["items"].as_str().unwrap_or_default();
            let earnings_item = items.contains("2.02");
            for event in events.iter().filter(|event| event.release.is_none()) {
                let offset = (filed - event.report_date).num_days();
                let near = (-1..=1).contains(&offset);
                let item_near = earnings_item && (-3..=10).contains(&offset);
                if (near && (upper == "8-K" || earnings_item)) || item_near {
                    if set_release(tx, loupist_id, event, &filing_ref(filing), now_ms)? {
                        outcome.changed = true;
                        outcome.released.push(event.event_id.clone());
                    }
                    break;
                }
            }
        }
        if us_final_form(&upper) && upper != "6-K" {
            for event in events.iter().filter(|event| event.final_report.is_none()) {
                let after_release = event
                    .release
                    .as_ref()
                    .and_then(|release| day(&release.date))
                    .map_or(filed >= event.report_date - Duration::days(1), |release| {
                        filed >= release
                    });
                let in_period = event
                    .period_end()
                    .is_some_and(|end| filed > end && filed <= end + Duration::days(120));
                if after_release && in_period && event.report_date <= today + Duration::days(1) {
                    if set_final(tx, loupist_id, event, &filing_ref(filing), now_ms)? {
                        outcome.changed = true;
                    }
                    break;
                }
            }
        }
    }
    Ok(outcome)
}

/// An IR notice's date was read: the period after it gets that date.
pub fn apply_ir_date(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    notice: &IrNotice,
    ir_date: Option<NaiveDate>,
    now_ms: i64,
) -> KnowledgeResult<Option<EventRecord>> {
    let mut touched = None;
    if let Some(date) = ir_date {
        let end = dates::quarter_end_before(date);
        let (event, _) = upsert_event(
            tx,
            loupist_id,
            &page.asset_id,
            &dates::period_label(end),
            Some(end),
            date,
            DateSource::IrNotice,
            now_ms,
        )?;
        touched = Some(event);
    }
    // Remember the notice on the next event either way, so it is read once.
    let events = events_for(tx, loupist_id, &page.asset_id)?;
    if let Some(event) = touched.as_ref().or(events.first()) {
        let id = notice.filing_id.clone();
        edit_flags(tx, loupist_id, &event.event_id, now_ms, |flags| {
            if !flags.ir_read.contains(&id) {
                flags.ir_read.push(id);
                if flags.ir_read.len() > 20 {
                    flags.ir_read.remove(0);
                }
            }
        })?;
    }
    Ok(touched)
}

/// One capture the earnings task owes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureSide {
    Before,
    After,
    Final,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Due {
    pub event: EventRecord,
    pub side: CaptureSide,
    /// A late before-capture reads only what was published by this date.
    pub late_as_of: Option<NaiveDate>,
}

/// What is due for one asset's events at `now`, and the next future time
/// something will be.
pub fn due(events: &[EventRecord], market: &str, now: DateTime<Utc>) -> (Vec<Due>, Option<i64>) {
    let now_ms = now.timestamp_millis();
    let today = dates::listing_today(market, now);
    let mut out = Vec::new();
    let mut next: Option<i64> = None;
    let mut later = |at: i64| next = Some(next.map_or(at, |current: i64| current.min(at)));
    for event in events {
        // Old events are history.
        if event.reported_on() < today - Duration::days(120) {
            continue;
        }
        let reported =
            event.release.is_some() || matches!(event.status, Status::Reported | Status::Final);
        if event.before_at_ms.is_none()
            && event
                .flags
                .failed
                .as_ref()
                .is_none_or(|failed| failed.side != "before" || now_ms - failed.at_ms >= RETRY_MS)
        {
            let late_day = if reported {
                Some(event.reported_on())
            } else if today >= event.report_date && event.date_source != DateSource::LegalDeadline {
                Some(event.report_date)
            } else {
                None
            };
            if let Some(reported_on) = late_day {
                // Only a recent event is worth a late capture.
                if reported_on >= today - Duration::days(KEEP_DAYS) {
                    out.push(Due {
                        event: event.clone(),
                        side: CaptureSide::Before,
                        late_as_of: Some(reported_on - Duration::days(1)),
                    });
                    continue;
                }
            } else {
                let at = capture_at(event, market).timestamp_millis();
                // A date first seen a day or less away is captured at once.
                let first_seen = DateTime::from_timestamp_millis(event.created_at_ms)
                    .map(|at| dates::listing_today(market, at))
                    .unwrap_or(today);
                let close = (event.report_date - first_seen).num_days() <= 1;
                if now_ms >= at || close {
                    out.push(Due {
                        event: event.clone(),
                        side: CaptureSide::Before,
                        late_as_of: None,
                    });
                    continue;
                }
                later(at);
            }
        }
        let spaced = |attempts: u32| {
            attempts < MAX_ATTEMPTS
                && event
                    .flags
                    .last_attempt_ms
                    .is_none_or(|last| now_ms - last >= RETRY_MS)
        };
        if event.status == Status::Final && event.final_at_ms.is_none() {
            if spaced(event.flags.final_attempts) {
                out.push(Due {
                    event: event.clone(),
                    side: CaptureSide::Final,
                    late_as_of: None,
                });
            } else if event.flags.final_attempts < MAX_ATTEMPTS {
                later(event.flags.last_attempt_ms.unwrap_or(now_ms) + RETRY_MS);
            }
            continue;
        }
        if event.release.is_some() && event.after_at_ms.is_none() && event.status != Status::Final {
            let ready = event.flags.release_seen_ms.unwrap_or(0) + AFTER_DELAY_MS;
            if now_ms < ready {
                later(ready);
            } else if spaced(event.flags.after_attempts) {
                out.push(Due {
                    event: event.clone(),
                    side: CaptureSide::After,
                    late_as_of: None,
                });
            } else if event.flags.after_attempts < MAX_ATTEMPTS {
                later(event.flags.last_attempt_ms.unwrap_or(now_ms) + RETRY_MS);
            }
        }
    }
    (out, next)
}
