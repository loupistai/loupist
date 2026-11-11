//! Local watchers (finance-watchers.md): per-Loupist switches, the pause, each
//! route's last check and the events a check found. Checks call data sources
//! only; detection reads structured rows and dated observations, never model
//! text, and no holdings quantity or value ever enters these tables.
pub mod class;
pub mod detect;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;

use rusqlite::{params, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{
    assets::{AssetClass, AssetEvidence},
    model::mint_id,
    KnowledgeError, KnowledgeResult,
};

/// Facts kept per event (one sentence's worth).
pub const MAX_FACTS_BYTES: usize = 2048;
/// The Watching reader lists 50 alerts, then "Show earlier".
pub const MAX_EVENTS_LIST: usize = 200;

pub(crate) const DDL: &str = r#"
CREATE TABLE knowledge_watch_assets (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    scanned_to_ms INTEGER NOT NULL CHECK (scanned_to_ms >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms >= 0),
    PRIMARY KEY (loupist_id, asset_id)
) STRICT;
CREATE TABLE knowledge_watch_settings (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    enabled INTEGER NOT NULL CHECK (enabled IN (0,1)),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    PRIMARY KEY (loupist_id, asset_id, kind)
) STRICT;
CREATE TABLE knowledge_watch_state (
    loupist_id TEXT PRIMARY KEY NOT NULL,
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0,1)),
    last_checked_ms INTEGER,
    last_check_day TEXT,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;
CREATE TABLE knowledge_watch_routes (
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    route TEXT NOT NULL,
    checked_at_ms INTEGER NOT NULL CHECK (checked_at_ms >= 0),
    status TEXT NOT NULL,
    connection TEXT NOT NULL,
    baseline_done INTEGER NOT NULL CHECK (baseline_done IN (0,1)),
    PRIMARY KEY (loupist_id, asset_id, route)
) STRICT;
CREATE TABLE knowledge_watch_events (
    event_id TEXT PRIMARY KEY NOT NULL,
    loupist_id TEXT NOT NULL,
    asset_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    natural_key TEXT NOT NULL CHECK (length(natural_key) BETWEEN 1 AND 400),
    fact_date TEXT NOT NULL CHECK (length(fact_date) = 10),
    facts_json TEXT NOT NULL CHECK (length(facts_json) <= 2048),
    evidence_json TEXT NOT NULL,
    detected_at_ms INTEGER NOT NULL CHECK (detected_at_ms >= 0),
    seen_at_ms INTEGER,
    delivered_in TEXT,
    delivered_at_ms INTEGER,
    UNIQUE (loupist_id, asset_id, kind, natural_key)
) STRICT;
CREATE INDEX knowledge_watch_events_by_time ON knowledge_watch_events(loupist_id, detected_at_ms);
"#;

/// What a Loupist can watch per asset: six kinds for equities (v1) and the
/// class kinds for crypto and commodities (finance-crypto-and-templates.md §3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum WatchKind {
    Filings,
    Results,
    Insiders,
    Holders,
    Flows,
    Dividends,
    /// A daily close-to-close move of 10% or more, or 20% over 7 days.
    CryptoMove,
    /// The close falls past 20%, 30% or 50% below its 90-day high.
    Drawdown,
    /// A followed stablecoin closes outside 0.98–1.02.
    Depeg,
    /// Value locked on a followed chain or protocol falls 20% or more in 7 days.
    Tvl,
    /// The front contract moves 5% or more in a week.
    FuturesMove,
    /// CFTC managed-money net position changes sign.
    Positioning,
    /// A new EIA weekly petroleum report (oil only).
    OilInventory,
}

impl WatchKind {
    pub const ALL: [Self; 13] = [
        Self::Filings,
        Self::Results,
        Self::Insiders,
        Self::Holders,
        Self::Flows,
        Self::Dividends,
        Self::CryptoMove,
        Self::Drawdown,
        Self::Depeg,
        Self::Tvl,
        Self::FuturesMove,
        Self::Positioning,
        Self::OilInventory,
    ];
    pub const EQUITY: [Self; 6] = [
        Self::Filings,
        Self::Results,
        Self::Insiders,
        Self::Holders,
        Self::Flows,
        Self::Dividends,
    ];
    pub const CRYPTO: [Self; 4] = [Self::CryptoMove, Self::Drawdown, Self::Depeg, Self::Tvl];
    pub const COMMODITY: [Self; 3] = [Self::FuturesMove, Self::Positioning, Self::OilInventory];

    /// The stored and serialized name (the renderer's `assets:watch.kind.*` key).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Filings => "filings",
            Self::Results => "results",
            Self::Insiders => "insiders",
            Self::Holders => "holders",
            Self::Flows => "flows",
            Self::Dividends => "dividends",
            Self::CryptoMove => "cryptoMove",
            Self::Drawdown => "drawdown",
            Self::Depeg => "depeg",
            Self::Tvl => "tvl",
            Self::FuturesMove => "futuresMove",
            Self::Positioning => "positioning",
            Self::OilInventory => "oilInventory",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// The kinds of the same asset class as this one.
    pub fn class_kinds(self) -> &'static [Self] {
        if Self::CRYPTO.contains(&self) {
            &Self::CRYPTO
        } else if Self::COMMODITY.contains(&self) {
            &Self::COMMODITY
        } else {
            &Self::EQUITY
        }
    }

    /// Coordinator 2026-11-01: new filings, results within 7 days and dividend
    /// dates start on; insider buying, 5% holders and Korean flows start off.
    /// Class kinds start on only in a Loupist made from that class's template.
    pub fn default_on(self) -> bool {
        matches!(self, Self::Filings | Self::Results | Self::Dividends)
    }
}

/// The switches a newly watched asset starts with: the equity defaults, or a
/// class's kinds on when the Loupist was made from that class's template
/// (`template_class`) and off otherwise. Classes without watchers get none.
pub fn class_defaults(
    class: AssetClass,
    template_class: Option<AssetClass>,
) -> Vec<(WatchKind, bool)> {
    let kinds: &[WatchKind] = match class {
        AssetClass::Equity => {
            return WatchKind::EQUITY
                .into_iter()
                .map(|kind| (kind, kind.default_on()))
                .collect()
        }
        AssetClass::Crypto => &WatchKind::CRYPTO,
        AssetClass::Commodity => &WatchKind::COMMODITY,
        _ => &[],
    };
    let on = template_class == Some(class);
    kinds.iter().map(|kind| (*kind, on)).collect()
}

/// One data route a check reads; each serves one or more kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Route {
    Filings,
    Insiders,
    Calendar,
    Dividends,
    Flows,
    /// Daily closes of a coin or the front futures contract.
    Price,
    /// DefiLlama value locked for the chain or protocol behind a coin.
    Tvl,
    /// CFTC Commitments of Traders for a futures market.
    Positioning,
    /// EIA's weekly petroleum status report (one read for every oil asset).
    Inventory,
}

impl Route {
    pub const ALL: [Self; 9] = [
        Self::Filings,
        Self::Insiders,
        Self::Calendar,
        Self::Dividends,
        Self::Flows,
        Self::Price,
        Self::Tvl,
        Self::Positioning,
        Self::Inventory,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Filings => "filings",
            Self::Insiders => "insiders",
            Self::Calendar => "calendar",
            Self::Dividends => "dividends",
            Self::Flows => "flows",
            Self::Price => "price",
            Self::Tvl => "tvl",
            Self::Positioning => "positioning",
            Self::Inventory => "inventory",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|route| route.as_str() == value)
    }
}

/// A switch row of one asset: its kinds (only those with a stored decision)
/// and the revision that guards changes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AssetSwitches {
    pub revision: i64,
    pub kinds: BTreeMap<WatchKind, bool>,
}

impl AssetSwitches {
    pub fn enabled(&self, kind: WatchKind) -> bool {
        self.kinds.get(&kind).copied().unwrap_or(false)
    }
}

fn ensure_asset_row(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    now_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "INSERT OR IGNORE INTO knowledge_watch_assets(loupist_id,asset_id,revision,scanned_to_ms,created_at_ms) VALUES(?1,?2,1,?3,?3)",
        params![loupist_id, asset_id, now_ms.max(0)],
    )?;
    Ok(())
}

/// Every asset's switches in this Loupist.
pub fn switches(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<BTreeMap<String, AssetSwitches>> {
    let mut out: BTreeMap<String, AssetSwitches> = BTreeMap::new();
    let mut statement =
        tx.prepare("SELECT asset_id, revision FROM knowledge_watch_assets WHERE loupist_id=?1")?;
    for row in statement.query_map([loupist_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })? {
        let (asset, revision) = row?;
        out.entry(asset).or_default().revision = revision;
    }
    let mut statement = tx.prepare(
        "SELECT asset_id, kind, enabled FROM knowledge_watch_settings WHERE loupist_id=?1",
    )?;
    for row in statement.query_map([loupist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, bool>(2)?,
        ))
    })? {
        let (asset, kind, enabled) = row?;
        if let Some(kind) = WatchKind::parse(&kind) {
            out.entry(asset).or_default().kinds.insert(kind, enabled);
        }
    }
    for switches in out.values_mut() {
        switches.revision = switches.revision.max(1);
    }
    Ok(out)
}

/// The default kinds (`class_defaults`) for an asset with no decision yet in
/// this Loupist. Returns whether defaults were written.
pub fn apply_defaults(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    defaults: &[(WatchKind, bool)],
    now_ms: i64,
) -> KnowledgeResult<bool> {
    if defaults.is_empty() {
        return Ok(false);
    }
    ensure_asset_row(tx, loupist_id, asset_id, now_ms)?;
    let decided: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_watch_settings WHERE loupist_id=?1 AND asset_id=?2)",
        params![loupist_id, asset_id],
        |row| row.get(0),
    )?;
    if decided {
        return Ok(false);
    }
    for (kind, on) in defaults {
        tx.execute(
            "INSERT INTO knowledge_watch_settings(loupist_id,asset_id,kind,enabled,updated_at_ms) VALUES(?1,?2,?3,?4,?5)",
            params![loupist_id, asset_id, kind.as_str(), on, now_ms],
        )?;
    }
    tx.execute(
        "UPDATE knowledge_watch_assets SET revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id],
    )?;
    Ok(true)
}

/// One switch, guarded by the asset's revision. Returns the new revision.
pub fn set_kind(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    kind: WatchKind,
    enabled: bool,
    expected_revision: i64,
    now_ms: i64,
) -> KnowledgeResult<i64> {
    ensure_asset_row(tx, loupist_id, asset_id, now_ms)?;
    let revision: i64 = tx.query_row(
        "SELECT revision FROM knowledge_watch_assets WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id],
        |row| row.get(0),
    )?;
    if revision != expected_revision {
        return Err(KnowledgeError::RevisionConflict {
            expected: Some(expected_revision.to_string()),
            actual: Some(revision.to_string()),
        });
    }
    // A first explicit choice keeps the other kinds off rather than defaulting them.
    let decided: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM knowledge_watch_settings WHERE loupist_id=?1 AND asset_id=?2)",
        params![loupist_id, asset_id],
        |row| row.get(0),
    )?;
    if !decided {
        for other in kind.class_kinds() {
            tx.execute(
                "INSERT INTO knowledge_watch_settings(loupist_id,asset_id,kind,enabled,updated_at_ms) VALUES(?1,?2,?3,0,?4)",
                params![loupist_id, asset_id, other.as_str(), now_ms],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO knowledge_watch_settings(loupist_id,asset_id,kind,enabled,updated_at_ms) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(loupist_id,asset_id,kind) DO UPDATE SET enabled=excluded.enabled, updated_at_ms=excluded.updated_at_ms",
        params![loupist_id, asset_id, kind.as_str(), enabled, now_ms],
    )?;
    tx.execute(
        "UPDATE knowledge_watch_assets SET revision=revision+1 WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id],
    )?;
    Ok(revision + 1)
}

/// The Loupist's pause and last check.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WatchState {
    pub paused: bool,
    pub last_checked_ms: Option<i64>,
    pub last_check_day: Option<String>,
}

pub fn state(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<WatchState> {
    Ok(tx
        .query_row(
            "SELECT paused,last_checked_ms,last_check_day FROM knowledge_watch_state WHERE loupist_id=?1",
            [loupist_id],
            |row| {
                Ok(WatchState {
                    paused: row.get(0)?,
                    last_checked_ms: row.get(1)?,
                    last_check_day: row.get(2)?,
                })
            },
        )
        .optional()?
        .unwrap_or_default())
}

pub fn set_paused(
    tx: &Transaction<'_>,
    loupist_id: &str,
    paused: bool,
    now_ms: i64,
) -> KnowledgeResult<()> {
    tx.execute(
        "INSERT INTO knowledge_watch_state(loupist_id,paused,updated_at_ms) VALUES(?1,?2,?3)
         ON CONFLICT(loupist_id) DO UPDATE SET paused=excluded.paused, updated_at_ms=excluded.updated_at_ms",
        params![loupist_id, paused, now_ms],
    )?;
    Ok(())
}

/// Records a finished check; returns the previous check day (local), so the
/// caller can tell the day's first check.
pub fn record_check(
    tx: &Transaction<'_>,
    loupist_id: &str,
    day: &str,
    now_ms: i64,
) -> KnowledgeResult<Option<String>> {
    let previous = state(tx, loupist_id)?.last_check_day;
    tx.execute(
        "INSERT INTO knowledge_watch_state(loupist_id,paused,last_checked_ms,last_check_day,updated_at_ms) VALUES(?1,0,?2,?3,?2)
         ON CONFLICT(loupist_id) DO UPDATE SET last_checked_ms=excluded.last_checked_ms, last_check_day=excluded.last_check_day, updated_at_ms=excluded.updated_at_ms",
        params![loupist_id, now_ms, day],
    )?;
    Ok(previous)
}

/// One route's last check for an asset.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteState {
    pub route: Route,
    pub checked_at_ms: i64,
    /// `ok`, `failed` or `needs_connection`.
    pub status: String,
    pub connection: String,
    /// The route has stored its starting point; events may fire.
    pub baseline_done: bool,
}

pub fn routes(
    tx: &Transaction<'_>,
    loupist_id: &str,
) -> KnowledgeResult<BTreeMap<String, Vec<RouteState>>> {
    let mut statement = tx.prepare(
        "SELECT asset_id,route,checked_at_ms,status,connection,baseline_done FROM knowledge_watch_routes WHERE loupist_id=?1",
    )?;
    let mut out: BTreeMap<String, Vec<RouteState>> = BTreeMap::new();
    for row in statement.query_map([loupist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, bool>(5)?,
        ))
    })? {
        let (asset, route, checked_at_ms, status, connection, baseline_done) = row?;
        let Some(route) = Route::parse(&route) else {
            continue;
        };
        out.entry(asset).or_default().push(RouteState {
            route,
            checked_at_ms,
            status,
            connection,
            baseline_done,
        });
    }
    Ok(out)
}

#[cfg(test)]
pub fn route_state(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    route: Route,
) -> KnowledgeResult<Option<RouteState>> {
    Ok(routes(tx, loupist_id)?
        .remove(asset_id)
        .and_then(|states| states.into_iter().find(|state| state.route == route)))
}

/// A route's check: an `ok` check completes its baseline; a failure keeps the
/// last good state and only says which connection did not answer.
pub fn record_route(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    route: Route,
    status: &str,
    connection: &str,
    now_ms: i64,
) -> KnowledgeResult<()> {
    if !matches!(status, "ok" | "failed" | "needs_connection") {
        return Err(KnowledgeError::Invalid("watch route status"));
    }
    tx.execute(
        "INSERT INTO knowledge_watch_routes(loupist_id,asset_id,route,checked_at_ms,status,connection,baseline_done) VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(loupist_id,asset_id,route) DO UPDATE SET checked_at_ms=excluded.checked_at_ms, status=excluded.status, connection=excluded.connection,
            baseline_done=MAX(knowledge_watch_routes.baseline_done, excluded.baseline_done)",
        params![
            loupist_id,
            asset_id,
            route.as_str(),
            now_ms,
            status,
            super::assets::bounded_text_pub(connection, 120),
            status == "ok"
        ],
    )?;
    Ok(())
}

/// Observations of this asset recorded after this point have been scanned.
pub fn scanned_to(tx: &Transaction<'_>, loupist_id: &str, asset_id: &str) -> KnowledgeResult<i64> {
    Ok(tx
        .query_row(
            "SELECT scanned_to_ms FROM knowledge_watch_assets WHERE loupist_id=?1 AND asset_id=?2",
            params![loupist_id, asset_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

pub fn set_scanned_to(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    at_ms: i64,
) -> KnowledgeResult<()> {
    ensure_asset_row(tx, loupist_id, asset_id, at_ms)?;
    tx.execute(
        "UPDATE knowledge_watch_assets SET scanned_to_ms=MAX(scanned_to_ms, ?3) WHERE loupist_id=?1 AND asset_id=?2",
        params![loupist_id, asset_id, at_ms],
    )?;
    Ok(())
}

/// An event a check found, before it is stored.
#[derive(Clone, Debug, PartialEq)]
pub struct NewWatchEvent {
    pub asset_id: String,
    pub kind: WatchKind,
    pub natural_key: String,
    pub fact_date: String,
    pub facts: Value,
    pub evidence: Vec<AssetEvidence>,
    /// Found by a route's first check: kept as its starting point, never shown.
    pub baseline: bool,
}

/// Stores an event once per (asset, kind, natural key). Returns its id when new.
pub fn insert_event(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event: &NewWatchEvent,
    now_ms: i64,
) -> KnowledgeResult<Option<String>> {
    if super::assets::observe::parse_day(&event.fact_date).is_none()
        || !event.evidence.iter().any(AssetEvidence::has_source)
    {
        return Ok(None);
    }
    let facts = serde_json::to_string(&event.facts)?;
    if facts.len() > MAX_FACTS_BYTES {
        return Ok(None);
    }
    let id = mint_id("wev");
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO knowledge_watch_events(event_id,loupist_id,asset_id,kind,natural_key,fact_date,facts_json,evidence_json,detected_at_ms,delivered_in,delivered_at_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params![
            id,
            loupist_id,
            event.asset_id,
            event.kind.as_str(),
            super::assets::bounded_text_pub(&event.natural_key, 400),
            event.fact_date,
            facts,
            serde_json::to_string(&event.evidence)?,
            now_ms,
            event.baseline.then_some("baseline"),
            event.baseline.then_some(now_ms),
        ],
    )?;
    Ok((inserted > 0 && !event.baseline).then_some(id))
}

/// A stored event with its asset's names (never holdings).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WatchEventView {
    pub event_id: String,
    pub asset_id: String,
    pub name: String,
    pub symbol: String,
    pub market: String,
    pub kind: WatchKind,
    pub fact_date: String,
    pub detected_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub seen_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub delivered_in: Option<String>,
    pub facts: Value,
    pub evidence: Vec<AssetEvidence>,
}

const EVENT_COLUMNS: &str = "e.event_id,e.asset_id,COALESCE(p.display_name,e.asset_id),COALESCE(p.symbol,''),COALESCE(p.market,''),e.kind,e.fact_date,e.detected_at_ms,e.seen_at_ms,e.delivered_in,e.facts_json,e.evidence_json";

fn read_event(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<(WatchEventView, String, String, String)> {
    let kind: String = row.get(5)?;
    Ok((
        WatchEventView {
            event_id: row.get(0)?,
            asset_id: row.get(1)?,
            name: row.get(2)?,
            symbol: row.get(3)?,
            market: row.get(4)?,
            kind: WatchKind::parse(&kind).unwrap_or(WatchKind::Filings),
            fact_date: row.get(6)?,
            detected_at_ms: row.get(7)?,
            seen_at_ms: row.get(8)?,
            delivered_in: row.get(9)?,
            facts: Value::Null,
            evidence: Vec::new(),
        },
        kind,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn query_events(
    tx: &Transaction<'_>,
    filter: &str,
    values: &[&dyn rusqlite::ToSql],
    limit: usize,
) -> KnowledgeResult<Vec<WatchEventView>> {
    let sql = format!(
        "SELECT {EVENT_COLUMNS} FROM knowledge_watch_events e LEFT JOIN knowledge_asset_pages p ON p.loupist_id=e.loupist_id AND p.asset_id=e.asset_id WHERE {filter} ORDER BY e.detected_at_ms DESC, e.fact_date DESC, e.event_id LIMIT {}",
        limit.min(MAX_EVENTS_LIST)
    );
    let mut statement = tx.prepare(&sql)?;
    let rows = statement
        .query_map(values, read_event)?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .filter(|(_, kind, ..)| WatchKind::parse(kind).is_some())
        .map(|(mut view, _, facts, evidence)| {
            view.facts = serde_json::from_str(&facts)?;
            view.evidence = serde_json::from_str(&evidence)?;
            Ok(view)
        })
        .collect()
}

/// Shown events (never baselines), newest first.
pub fn list_events(
    tx: &Transaction<'_>,
    loupist_id: &str,
    since_ms: Option<i64>,
    limit: usize,
) -> KnowledgeResult<Vec<WatchEventView>> {
    query_events(
        tx,
        "e.loupist_id=?1 AND e.detected_at_ms>=?2 AND COALESCE(e.delivered_in,'')<>'baseline'",
        &[&loupist_id, &since_ms.unwrap_or(0)],
        limit,
    )
}

/// The asset's last shown events, for `asset_page_read`.
pub fn asset_events(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    limit: usize,
) -> KnowledgeResult<Vec<WatchEventView>> {
    query_events(
        tx,
        "e.loupist_id=?1 AND e.asset_id=?2 AND COALESCE(e.delivered_in,'')<>'baseline'",
        &[&loupist_id, &asset_id],
        limit,
    )
}

/// Found and not delivered yet, oldest first.
pub fn pending(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<Vec<WatchEventView>> {
    let mut events = query_events(
        tx,
        "e.loupist_id=?1 AND e.delivered_in IS NULL",
        &[&loupist_id],
        MAX_EVENTS_LIST,
    )?;
    events.reverse();
    Ok(events)
}

pub fn mark_delivered(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_ids: &[String],
    place: &str,
    now_ms: i64,
) -> KnowledgeResult<()> {
    if !matches!(place, "message" | "morning") {
        return Err(KnowledgeError::Invalid("watch delivery place"));
    }
    for id in event_ids {
        tx.execute(
            "UPDATE knowledge_watch_events SET delivered_in=?3, delivered_at_ms=?4 WHERE loupist_id=?1 AND event_id=?2 AND delivered_in IS NULL",
            params![loupist_id, id, place, now_ms],
        )?;
    }
    Ok(())
}

/// The alerts were on screen. Unknown ids are ignored.
pub fn mark_seen(
    tx: &Transaction<'_>,
    loupist_id: &str,
    event_ids: &[String],
    now_ms: i64,
) -> KnowledgeResult<usize> {
    let mut changed = 0;
    for id in event_ids {
        changed += tx.execute(
            "UPDATE knowledge_watch_events SET seen_at_ms=?3 WHERE loupist_id=?1 AND event_id=?2 AND seen_at_ms IS NULL",
            params![loupist_id, id, now_ms],
        )?;
    }
    Ok(changed)
}

/// Every delivered alert counts as seen (Schedule › Watching was opened).
pub fn mark_all_seen(
    tx: &Transaction<'_>,
    loupist_id: &str,
    now_ms: i64,
) -> KnowledgeResult<usize> {
    Ok(tx.execute(
        "UPDATE knowledge_watch_events SET seen_at_ms=?2 WHERE loupist_id=?1 AND seen_at_ms IS NULL AND delivered_in IN ('message','morning')",
        params![loupist_id, now_ms],
    )?)
}

/// Delivered alerts not on screen yet, and the newest one's time.
pub fn unseen(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<(u64, i64)> {
    Ok(tx.query_row(
        "SELECT count(*), COALESCE(MAX(detected_at_ms),0) FROM knowledge_watch_events WHERE loupist_id=?1 AND seen_at_ms IS NULL AND delivered_in IN ('message','morning')",
        [loupist_id],
        |row| Ok((row.get::<_, i64>(0)?.max(0) as u64, row.get(1)?)),
    )?)
}

pub fn erase(tx: &Transaction<'_>, loupist_id: &str) -> KnowledgeResult<()> {
    for table in [
        "knowledge_watch_events",
        "knowledge_watch_routes",
        "knowledge_watch_state",
        "knowledge_watch_settings",
        "knowledge_watch_assets",
    ] {
        tx.execute(
            &format!("DELETE FROM {table} WHERE loupist_id = ?1"),
            [loupist_id],
        )?;
    }
    Ok(())
}
