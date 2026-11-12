//! Local watchers (docs/knowledge/watchers.md#local-watchers). While the app runs, Rust checks what
//! each Loupist follows against data sources only — no model call — records
//! dated observations on the asset pages, turns new ones into alerts for the
//! kinds that are on, and posts them in the home Thread or the morning message.
//! One read per (asset, route) per cadence serves every Loupist; the hour's
//! budget defers the rest. No OS notification is sent for a watch event.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Duration, NaiveDate, Timelike, Utc};
use rusqlite::Transaction;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use super::{
    assets::{self as asset_commands, with_assets},
    map_store, now_ms,
    tool_executor::{assets::HostReadError, AppToolExecutor},
};
use crate::{
    app::{AppState, CommandError},
    broker::ToolPolicy,
    knowledge::{
        assets::{
            self, class::is_korean_market, observe, plan, AssetClass, AssetEvidence,
            NewObservation, ObservationKind, ObservationOrigin, PageRecord,
        },
        earnings,
        watch::{
            self, class as watch_class, detect, NewWatchEvent, Route, RouteState, WatchEventView,
            WatchKind,
        },
        KnowledgeResult,
    },
    runs::maintenance::MaintenanceActivityKind,
    scheduled_tasks::types::{TaskKind, TaskStatus},
};

const HOUR_MS: i64 = 60 * 60 * 1000;
/// App-wide reads per rolling hour; a check past it defers the rest.
pub(crate) const MAX_READS_PER_HOUR: usize = 60;
/// The first scheduled check waits this long after launch.
pub(crate) const LAUNCH_DELAY_MS: i64 = 2 * 60 * 1000;
/// Insider purchases a read asks for.
const INSIDER_ROWS: u64 = 100;
/// Calendar reads look this far ahead.
const CALENDAR_DAYS: i64 = 21;
/// Alerts in one Thread message; the rest are "+n more".
pub(crate) const MESSAGE_EVENTS: usize = 5;
/// Market data the host activates for watchers and the earnings task.
pub(in crate::commands) const HOST_TOOLS: &[&str] = &[
    "equity_fundamental_filings",
    "equity_ownership_insider_trading",
    "equity_calendar_earnings",
    "equity_fundamental_dividends",
    "equity_fundamental_income",
    "equity_fundamental_cash",
    "equity_fundamental_balance",
    "equity_price_historical",
    "equity_estimates_forward_sales",
    "equity_estimates_forward_eps",
    "crypto_price_historical",
    "derivatives_futures_historical",
    "cftc_cot",
    "commodity_petroleum_status_report",
];
/// Providers whose insider rows say who bought on the open market.
const INSIDER_PROVIDERS: &[&str] = &["sec", "fmp", "intrinio"];
/// Calendar providers that publish an API (not a scraped page).
const CALENDAR_PROVIDERS: &[&str] = &["nasdaq", "fmp", "tmx"];
/// Commitments of Traders come from the CFTC itself (keyless in OpenBB).
const COT_PROVIDERS: &[&str] = &["cftc"];
/// The weekly petroleum report comes from EIA (its connection needs a free key).
const EIA_PROVIDERS: &[&str] = &["eia"];
const OPENBB_CONNECTION: &str = "openbb.platform";
const OPENDART_CONNECTION: &str = crate::finance_data::OPENDART_SOURCE_ID;
const FMP_CONNECTION: &str = "fmp.market-data";
const EIA_CONNECTION: &str = "eia.energy";
const DEFILLAMA_CONNECTION: &str = crate::finance_data::DEFILLAMA_SOURCE_ID;
/// Price and value-locked routes are read again after this long.
const PRICE_EVERY_MS: i64 = 6 * HOUR_MS;
/// Daily closes a price read asks for: the 90-day high needs 90 days.
const PRICE_DAYS: i64 = 120;
/// Value locked a read asks for: 7-day changes on the latest complete days.
const TVL_DAYS: i64 = 30;
/// Weekly reports a positioning or inventory read asks for.
const REPORT_DAYS: i64 = 60;

/// The classes whose assets have watchers.
pub(crate) fn watchable(page: &PageRecord) -> bool {
    matches!(
        page.class,
        AssetClass::Equity | AssetClass::Crypto | AssetClass::Commodity
    )
}

/// In-memory state of the watcher loop (the app process owns it).
#[derive(Default)]
pub(crate) struct Runtime {
    started_ms: Mutex<Option<i64>>,
    reads: Mutex<Vec<i64>>,
    checking: Mutex<BTreeSet<String>>,
    /// Loupists whose first check after launch already ran (morning rule).
    first_checked: Mutex<BTreeSet<String>>,
    /// Captures running now, by event id ("Capturing what's expected…").
    preparing: Mutex<BTreeSet<String>>,
    pass: tokio::sync::Mutex<()>,
}

impl Runtime {
    pub(crate) fn mark_started(&self, now_ms: i64) {
        let mut started = self.started_ms.lock().expect("watch runtime lock");
        started.get_or_insert(now_ms);
    }

    fn started(&self) -> Option<i64> {
        *self.started_ms.lock().expect("watch runtime lock")
    }

    fn reads_left(&self, now_ms: i64) -> usize {
        let mut reads = self.reads.lock().expect("watch runtime lock");
        reads.retain(|at| now_ms - at < HOUR_MS);
        MAX_READS_PER_HOUR.saturating_sub(reads.len())
    }

    fn count_read(&self, now_ms: i64) {
        self.reads.lock().expect("watch runtime lock").push(now_ms);
    }

    pub(crate) fn is_checking(&self, loupist_id: &str) -> bool {
        self.checking
            .lock()
            .expect("watch runtime lock")
            .contains(loupist_id)
    }

    pub(crate) fn preparing(&self, event_id: &str) -> bool {
        self.preparing
            .lock()
            .expect("watch runtime lock")
            .contains(event_id)
    }

    /// Marks an event's capture as running until the guard drops.
    pub(crate) fn prepare(self: &Arc<Self>, event_id: &str) -> PreparingGuard {
        self.preparing
            .lock()
            .expect("watch runtime lock")
            .insert(event_id.to_owned());
        PreparingGuard {
            runtime: self.clone(),
            event_id: event_id.to_owned(),
        }
    }
}

pub(crate) struct PreparingGuard {
    runtime: Arc<Runtime>,
    event_id: String,
}

impl Drop for PreparingGuard {
    fn drop(&mut self) {
        if let Ok(mut preparing) = self.runtime.preparing.lock() {
            preparing.remove(&self.event_id);
        }
    }
}

struct CheckingGuard<'a> {
    runtime: &'a Runtime,
    loupists: Vec<String>,
}

impl Drop for CheckingGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut checking) = self.runtime.checking.lock() {
            for id in &self.loupists {
                checking.remove(id);
            }
        }
    }
}

// ───────────────────────────── Host reads ─────────────────────────────

/// What one host read returned.
pub(crate) enum ReadOutcome {
    Ok {
        payload: Value,
        provider: Option<String>,
    },
    /// The connection this read needs is off (a capability id).
    NeedsConnection(String),
    /// The source did not answer (a capability id).
    Failed(String),
}

/// One Loupist's executor for host reads: its own connection grants, the
/// unattended allowlist, no model.
pub(in crate::commands) struct HostSession {
    pub(in crate::commands) capabilities: BTreeSet<String>,
    executor: AppToolExecutor,
    policy: ToolPolicy,
    connected: Option<bool>,
    openbb: BTreeSet<String>,
}

pub(in crate::commands) fn openbb_capabilities() -> BTreeSet<String> {
    crate::marketplace::bundled_catalog()
        .map(|catalog| {
            catalog
                .entries
                .into_iter()
                .filter(|entry| entry.runtime.server_id.as_deref() == Some(plan::OPENBB))
                .map(|entry| entry.id)
                .collect()
        })
        .unwrap_or_default()
}

pub(in crate::commands) fn connection_names() -> BTreeMap<String, String> {
    crate::marketplace::bundled_catalog()
        .map(|catalog| {
            catalog
                .entries
                .into_iter()
                .map(|entry| (entry.id, entry.name))
                .collect()
        })
        .unwrap_or_default()
}

impl HostSession {
    pub(in crate::commands) fn open(
        state: &AppState,
        loupist_id: &str,
        as_of: Option<NaiveDate>,
    ) -> Result<Self, CommandError> {
        let capabilities: BTreeSet<String> =
            super::capture_chat_connector_authority(state, loupist_id)?
                .capability_ids
                .into_iter()
                .collect();
        let home = crate::chat::home::ensure_home_thread(state, loupist_id)?;
        Ok(Self {
            executor: AppToolExecutor::for_host(state, loupist_id, capabilities.clone()),
            capabilities,
            policy: ToolPolicy {
                loupist_id: loupist_id.to_owned(),
                session_id: home.id,
                use_memory: false,
                propose_memory_updates: false,
                memory_proposal_budget: 0,
                as_of: as_of.map(|day| day.to_string()),
            },
            connected: None,
            openbb: openbb_capabilities(),
        })
    }

    /// Whether the Loupist's grants cover a planned read.
    pub(in crate::commands) fn granted(&self, read: &plan::PlannedRead) -> bool {
        match read.server {
            Some(_) => self.capabilities.iter().any(|id| self.openbb.contains(id)),
            None => self.capabilities.contains(read.connection),
        }
    }

    pub(in crate::commands) fn has(&self, capability: &str) -> bool {
        self.capabilities.contains(capability)
    }

    /// One read; `allowed` pins the providers that may serve it.
    pub(in crate::commands) async fn read(
        &mut self,
        read: &plan::PlannedRead,
        allowed: Option<&[&str]>,
    ) -> ReadOutcome {
        let connection = || {
            if read.server.is_some() {
                OPENBB_CONNECTION.to_owned()
            } else {
                read.connection.to_owned()
            }
        };
        if !self.granted(read) {
            return ReadOutcome::NeedsConnection(connection());
        }
        if let Some(server) = read.server {
            if self.connected.is_none() {
                self.connected = Some(
                    self.executor
                        .host_connect(&self.policy, server, HOST_TOOLS)
                        .await
                        .is_ok(),
                );
            }
            if self.connected != Some(true) {
                return ReadOutcome::Failed(connection());
            }
        }
        let outcome = match allowed {
            Some(allowed) => {
                self.executor
                    .host_read_among(&self.policy, read, None, allowed)
                    .await
            }
            None => self
                .executor
                .host_read(&self.policy, read, None)
                .await
                .map_err(HostReadError::Failed),
        };
        match outcome {
            Ok((payload, provider)) => ReadOutcome::Ok { payload, provider },
            Err(HostReadError::NoProvider) => ReadOutcome::NeedsConnection(
                if allowed.is_some_and(|allowed| allowed.contains(&"fmp")) {
                    FMP_CONNECTION.to_owned()
                } else if allowed.is_some_and(|allowed| allowed.contains(&"eia")) {
                    EIA_CONNECTION.to_owned()
                } else {
                    connection()
                },
            ),
            Err(HostReadError::Failed(error)) => {
                // An as-of cutoff a source cannot show is not a failure of the source.
                if format!("{error:?}").contains("as_of_unsupported") {
                    ReadOutcome::NeedsConnection(connection())
                } else {
                    ReadOutcome::Failed(connection())
                }
            }
        }
    }

    pub(in crate::commands) async fn close(self) {
        self.executor.host_shutdown().await;
    }
}

/// Where checks and captures read from: each Loupist's own executor in the
/// app, scripted rows in tests. Reads never involve a model.
pub(in crate::commands) trait HostReader: Send {
    /// Opens the Loupist's session when it has none; false when it cannot.
    fn ensure(&mut self, state: &AppState, loupist_id: &str) -> bool;
    /// Whether the Loupist's grants cover a planned read.
    fn granted(&self, loupist_id: &str, read: &plan::PlannedRead) -> bool;
    /// Whether the Loupist holds one connection.
    fn has(&self, loupist_id: &str, capability: &str) -> bool;
    fn read(
        &mut self,
        state: &AppState,
        loupist_id: &str,
        read: &plan::PlannedRead,
        allowed: Option<&'static [&'static str]>,
    ) -> impl std::future::Future<Output = ReadOutcome> + Send;
}

/// The app's reader: one host session per Loupist, all under one cutoff.
pub(in crate::commands) struct HostSessions {
    sessions: HashMap<String, HostSession>,
    as_of: Option<NaiveDate>,
}

impl HostSessions {
    pub(crate) fn new(as_of: Option<NaiveDate>) -> Self {
        Self {
            sessions: HashMap::new(),
            as_of,
        }
    }

    pub(crate) async fn close(self) {
        for (_, session) in self.sessions {
            session.close().await;
        }
    }
}

impl HostReader for HostSessions {
    fn ensure(&mut self, state: &AppState, loupist_id: &str) -> bool {
        if !self.sessions.contains_key(loupist_id) {
            match HostSession::open(state, loupist_id, self.as_of) {
                Ok(session) => {
                    self.sessions.insert(loupist_id.to_owned(), session);
                }
                Err(_) => return false,
            }
        }
        true
    }

    fn granted(&self, loupist_id: &str, read: &plan::PlannedRead) -> bool {
        self.sessions
            .get(loupist_id)
            .is_some_and(|session| session.granted(read))
    }

    fn has(&self, loupist_id: &str, capability: &str) -> bool {
        self.sessions
            .get(loupist_id)
            .is_some_and(|session| session.has(capability))
    }

    fn read(
        &mut self,
        state: &AppState,
        loupist_id: &str,
        read: &plan::PlannedRead,
        allowed: Option<&'static [&'static str]>,
    ) -> impl std::future::Future<Output = ReadOutcome> + Send {
        let ready = self.ensure(state, loupist_id);
        let session = self.sessions.get_mut(loupist_id).filter(|_| ready);
        let fallback = connection_of(read);
        async move {
            match session {
                Some(session) => session.read(read, allowed).await,
                None => ReadOutcome::Failed(fallback),
            }
        }
    }
}

// ───────────────────────────── Routes and cadence ─────────────────────────────

/// The read behind one route for one asset (calendar: all US assets at once).
pub(in crate::commands) fn route_read(
    route: Route,
    page: &PageRecord,
    today: NaiveDate,
) -> Option<plan::PlannedRead> {
    if page.class != AssetClass::Equity {
        return class_route_read(route, page, today);
    }
    let korean = is_korean_market(&page.market);
    let planned = |kind: &str| {
        plan::reads(AssetClass::Equity, &page.symbol, &page.market, today, false)
            .into_iter()
            .find(|read| read.kind == kind)
    };
    match (route, korean) {
        (Route::Filings, _) => planned("filings"),
        (Route::Dividends, false) => planned("dividends"),
        (Route::Insiders, false) => Some(plan::PlannedRead {
            kind: "insiders",
            tool: "equity_ownership_insider_trading",
            server: Some(plan::OPENBB),
            connection: OPENBB_CONNECTION,
            arguments: json!({"symbol": page.symbol.trim(), "limit": INSIDER_ROWS}),
        }),
        (Route::Calendar, false) => Some(plan::PlannedRead {
            kind: "results",
            tool: "equity_calendar_earnings",
            server: Some(plan::OPENBB),
            connection: OPENBB_CONNECTION,
            arguments: json!({"start_date": today.to_string(), "end_date": (today + Duration::days(CALENDAR_DAYS)).to_string()}),
        }),
        (Route::Flows, true) => Some(plan::PlannedRead {
            kind: "flows",
            tool: "kis_read",
            server: None,
            connection: crate::finance_data::KIS_SOURCE_ID,
            arguments: json!({"operation_id": "domestic_stock.investor_trade_by_stock_daily", "params": {
                "fid_cond_mrkt_div_code": "J",
                "fid_input_iscd": page.symbol.trim(),
                "fid_input_date_1": today.format("%Y%m%d").to_string(),
                "fid_org_adj_prc": "",
                "fid_etc_cls_code": "",
            }}),
        }),
        _ => None,
    }
}

/// The read behind a class route (crypto and commodities).
fn class_route_read(
    route: Route,
    page: &PageRecord,
    today: NaiveDate,
) -> Option<plan::PlannedRead> {
    let since = |days: i64| (today - Duration::days(days)).to_string();
    let openbb = |kind: &'static str, tool: &'static str, arguments: Value| plan::PlannedRead {
        kind,
        tool,
        server: Some(plan::OPENBB),
        connection: OPENBB_CONNECTION,
        arguments,
    };
    match (route, page.class) {
        (Route::Price, AssetClass::Crypto) => Some(openbb(
            "price",
            "crypto_price_historical",
            json!({"symbol": watch_class::crypto_pair(&page.symbol), "start_date": since(PRICE_DAYS)}),
        )),
        (Route::Price, AssetClass::Commodity) => Some(openbb(
            "price",
            "derivatives_futures_historical",
            json!({"symbol": watch_class::futures_root(&page.symbol), "start_date": since(PRICE_DAYS)}),
        )),
        (Route::Tvl, AssetClass::Crypto) => {
            let (route, id) = watch_class::llama_target(&page.symbol)?;
            Some(plan::PlannedRead {
                kind: "tvl",
                tool: "defillama_read",
                server: None,
                connection: DEFILLAMA_CONNECTION,
                // No end: DefiLlama dates by UTC, which can be a day behind the local date.
                arguments: json!({"route": route, "id": id, "start": since(TVL_DAYS)}),
            })
        }
        (Route::Positioning, AssetClass::Commodity) => Some(openbb(
            "positioning",
            "cftc_cot",
            json!({"code": watch_class::cftc_code(&page.symbol)?, "report_type": "disaggregated", "measure": "positions", "futures_only": false, "start_date": since(REPORT_DAYS)}),
        )),
        (Route::Inventory, AssetClass::Commodity) if watch_class::is_oil(&page.symbol) => {
            Some(openbb(
                "inventories",
                "commodity_petroleum_status_report",
                json!({"category": "crude_petroleum_stocks", "table": "stocks", "start_date": since(REPORT_DAYS)}),
            ))
        }
        _ => None,
    }
}

fn allowed_providers(route: Route) -> Option<&'static [&'static str]> {
    match route {
        Route::Insiders => Some(INSIDER_PROVIDERS),
        Route::Calendar => Some(CALENDAR_PROVIDERS),
        Route::Positioning => Some(COT_PROVIDERS),
        Route::Inventory => Some(EIA_PROVIDERS),
        _ => None,
    }
}

/// The route that serves a kind for this market.
pub(crate) fn kind_route(kind: WatchKind, korean: bool) -> Route {
    match (kind, korean) {
        (
            WatchKind::CryptoMove | WatchKind::Drawdown | WatchKind::Depeg | WatchKind::FuturesMove,
            _,
        ) => Route::Price,
        (WatchKind::Tvl, _) => Route::Tvl,
        (WatchKind::Positioning, _) => Route::Positioning,
        (WatchKind::OilInventory, _) => Route::Inventory,
        (WatchKind::Flows, _) => Route::Flows,
        (WatchKind::Filings | WatchKind::Holders, _) => Route::Filings,
        (_, true) => Route::Filings,
        (WatchKind::Insiders, false) => Route::Insiders,
        (WatchKind::Results, false) => Route::Calendar,
        (WatchKind::Dividends, false) => Route::Dividends,
    }
}

/// The connection a kind needs before any check (Korea stays under Connections).
fn kind_connection(kind: WatchKind, korean: bool) -> &'static str {
    match (kind, korean) {
        (WatchKind::Flows, _) => crate::finance_data::KIS_SOURCE_ID,
        (WatchKind::Tvl, _) => DEFILLAMA_CONNECTION,
        (WatchKind::OilInventory, _) => EIA_CONNECTION,
        (
            WatchKind::CryptoMove
            | WatchKind::Drawdown
            | WatchKind::Depeg
            | WatchKind::FuturesMove
            | WatchKind::Positioning,
            _,
        ) => OPENBB_CONNECTION,
        (_, true) => OPENDART_CONNECTION,
        (_, false) => OPENBB_CONNECTION,
    }
}

/// Whether a route is due: filings and insiders hourly; prices and value
/// locked every six hours; results, dividends, positioning and the petroleum
/// report once a day after 06:00 local; Korean flows on KRX trading days
/// (published holidays skipped) after 18:30 in Seoul.
pub(crate) fn route_due(route: Route, last: Option<i64>, now: DateTime<Utc>) -> bool {
    let now_ms = now.timestamp_millis();
    match route {
        Route::Filings | Route::Insiders => last.is_none_or(|at| now_ms - at >= HOUR_MS),
        Route::Price | Route::Tvl => last.is_none_or(|at| now_ms - at >= PRICE_EVERY_MS),
        Route::Calendar | Route::Dividends | Route::Positioning | Route::Inventory => {
            let local = now.with_timezone(&chrono::Local);
            if local.hour() < 6 {
                return last.is_none();
            }
            last.is_none_or(|at| {
                chrono::DateTime::from_timestamp_millis(at)
                    .map(|time| time.with_timezone(&chrono::Local).date_naive())
                    .is_none_or(|day| day < local.date_naive())
            })
        }
        Route::Flows => {
            let seoul = now.with_timezone(&chrono_tz::Asia::Seoul);
            let trading = crate::market_calendar::is_trading_day("KRX", seoul.date_naive());
            let after_close = (seoul.hour(), seoul.minute()) >= (18, 30);
            if !trading || !after_close {
                return false;
            }
            last.is_none_or(|at| {
                chrono::DateTime::from_timestamp_millis(at)
                    .map(|time| time.with_timezone(&chrono_tz::Asia::Seoul))
                    .is_none_or(|time| {
                        time.date_naive() < seoul.date_naive()
                            || (time.hour(), time.minute()) < (18, 30)
                    })
            })
        }
    }
}

/// The kinds that apply to an asset: for equities, flows only for Korean
/// stocks and dividends only for assets that paid in the last two years; for
/// crypto and commodities, the class kinds their symbol has a source for.
fn applicable(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    today: NaiveDate,
) -> KnowledgeResult<Vec<WatchKind>> {
    match page.class {
        AssetClass::Equity => {}
        AssetClass::Crypto => return Ok(watch_class::crypto_kinds(&page.symbol)),
        AssetClass::Commodity => return Ok(watch_class::commodity_kinds(&page.symbol)),
        _ => return Ok(Vec::new()),
    }
    let korean = is_korean_market(&page.market);
    let since = today - Duration::days(730);
    let paid = assets::observations(tx, loupist_id, &page.asset_id)?
        .iter()
        .any(|item| match item.kind {
            ObservationKind::Dividend => item.facts["exDate"]
                .as_str()
                .and_then(observe::parse_day)
                .is_some_and(|day| day >= since),
            ObservationKind::Filing if korean => {
                detect::classify_filing(&item.facts, true) == detect::FilingRoute::Dividend
                    && observe::parse_day(&item.fact_date).is_some_and(|day| day >= since)
            }
            _ => false,
        });
    Ok(WatchKind::EQUITY
        .into_iter()
        .filter(|kind| match kind {
            WatchKind::Flows => korean,
            WatchKind::Dividends => paid,
            _ => true,
        })
        .collect())
}

/// Freeze the earlier briefing opt-in on first access. New installations opt in per watcher.
pub(crate) fn consent_mode(state: &AppState, loupist_id: &str) -> Result<String, CommandError> {
    let legacy = state
        .store
        .scheduled_tasks_list(loupist_id)
        .map_err(map_store)?
        .iter()
        .any(|task| task.kind == TaskKind::Briefing && task.status == TaskStatus::Active);
    let db = state.store.lock().map_err(map_store)?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS watcher_consent (
        loupist_id TEXT PRIMARY KEY REFERENCES loupist_profiles(id) ON DELETE CASCADE,
        mode TEXT NOT NULL CHECK(mode IN ('legacy','selected','off'))
    ) STRICT;",
    )
    .map_err(super::map_internal)?;
    db.execute(
        "INSERT OR IGNORE INTO watcher_consent VALUES (?1,?2)",
        [loupist_id, if legacy { "legacy" } else { "off" }],
    )
    .map_err(super::map_internal)?;
    db.query_row(
        "SELECT mode FROM watcher_consent WHERE loupist_id=?1",
        [loupist_id],
        |row| row.get(0),
    )
    .map_err(super::map_internal)
}
pub(crate) fn consent(state: &AppState, loupist_id: &str) -> bool {
    consent_mode(state, loupist_id).is_ok_and(|mode| mode != "off")
}
/// Whether this Loupist's earnings task is on (it needs dates and filings).
fn earnings_on(state: &AppState, loupist_id: &str) -> bool {
    state
        .store
        .scheduled_tasks_list(loupist_id)
        .map(|tasks| {
            tasks
                .iter()
                .any(|task| task.kind == TaskKind::Earnings && task.status == TaskStatus::Active)
        })
        .unwrap_or(false)
}

/// Followed equities of a Loupist as pages (created when missing). Names only.
pub(crate) fn followed_pages(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<PageRecord>, CommandError> {
    let context = asset_commands::view_context(state)?;
    let identities = asset_commands::followed(&context);
    with_assets(state, loupist_id, true, |tx| {
        identities
            .iter()
            .map(|identity| assets::ensure_page(tx, loupist_id, identity, None, now_ms()))
            .collect::<KnowledgeResult<Vec<_>>>()
    })
}

// ───────────────────────────── One check pass ─────────────────────────────

/// Who asked for a read, and what they knew before it.
struct Want {
    loupist_id: String,
    page: PageRecord,
    prior: Option<RouteState>,
}

/// A shared read: the route, and for the calendar every US page that wants it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ReadKey {
    route: Route,
    /// `SYMBOL:MARKET`, or empty for the calendar.
    asset: String,
}

/// Which Loupists a pass checks.
pub(crate) enum Scope {
    /// The background cadence over every Loupist with consent and not paused.
    Scheduled,
    /// Check now: every enabled kind of one Loupist, ignoring cadence.
    Loupist(String),
}

/// Runs one pass with the app's own reads and returns new alerts per Loupist.
pub(crate) async fn run_pass(
    state: &AppState,
    runtime: &Runtime,
    scope: Scope,
    now: DateTime<Utc>,
) -> Result<BTreeMap<String, usize>, CommandError> {
    let mut reader = HostSessions::new(None);
    let result = Box::pin(run_pass_with(state, runtime, scope, now, &mut reader)).await;
    reader.close().await;
    result
}

/// One pass through a given reader.
pub(in crate::commands) async fn run_pass_with<R: HostReader>(
    state: &AppState,
    runtime: &Runtime,
    scope: Scope,
    now: DateTime<Utc>,
    reader: &mut R,
) -> Result<BTreeMap<String, usize>, CommandError> {
    let _pass = runtime.pass.lock().await;
    let now_ms = now.timestamp_millis();
    let today = now.with_timezone(&chrono::Local).date_naive();
    let loupists: Vec<String> = match &scope {
        Scope::Scheduled => state
            .store
            .list_loupists()
            .map_err(map_store)?
            .into_iter()
            .map(|loupist| loupist.id)
            .collect(),
        Scope::Loupist(id) => vec![id.clone()],
    };
    let forced = matches!(scope, Scope::Loupist(_));
    let mut wants: BTreeMap<ReadKey, Vec<Want>> = BTreeMap::new();
    let mut checked: Vec<String> = Vec::new();
    for loupist_id in &loupists {
        if state.ensure_loupist_available(loupist_id).is_err() || !consent(state, loupist_id) {
            continue;
        }
        let pages = followed_pages(state, loupist_id)?;
        let earnings = earnings_on(state, loupist_id);
        let (switches, routes, paused) = with_assets(state, loupist_id, false, |tx| {
            Ok((
                watch::switches(tx, loupist_id)?,
                watch::routes(tx, loupist_id)?,
                watch::state(tx, loupist_id)?.paused,
            ))
        })?;
        if paused {
            continue;
        }
        checked.push(loupist_id.clone());
        for page in pages.iter().filter(|page| watchable(page)) {
            let korean = is_korean_market(&page.market);
            let mut needed: BTreeSet<Route> = BTreeSet::new();
            if let Some(switch) = switches.get(&page.asset_id) {
                for (kind, enabled) in &switch.kinds {
                    if *enabled {
                        needed.insert(kind_route(*kind, korean));
                    }
                }
            }
            if earnings && page.class == AssetClass::Equity {
                needed.insert(Route::Filings);
                if !korean {
                    needed.insert(Route::Calendar);
                }
            }
            let states = routes.get(&page.asset_id);
            for route in needed {
                let prior = states
                    .and_then(|states| states.iter().find(|state| state.route == route))
                    .cloned();
                if !forced
                    && !route_due(route, prior.as_ref().map(|state| state.checked_at_ms), now)
                {
                    continue;
                }
                if route_read(route, page, today).is_none() {
                    continue;
                }
                let key = ReadKey {
                    route,
                    // The calendar and the petroleum report are one read for every asset.
                    asset: if matches!(route, Route::Calendar | Route::Inventory) {
                        String::new()
                    } else {
                        format!(
                            "{}:{}",
                            page.symbol.trim().to_uppercase(),
                            page.market.trim().to_uppercase()
                        )
                    },
                };
                wants.entry(key).or_default().push(Want {
                    loupist_id: loupist_id.clone(),
                    page: page.clone(),
                    prior,
                });
            }
        }
    }
    {
        let mut checking = runtime.checking.lock().expect("watch runtime lock");
        checking.extend(checked.iter().cloned());
    }
    let _checking = CheckingGuard {
        runtime,
        loupists: checked.clone(),
    };
    // Oldest-checked first, so a deferred route is first next time.
    let mut keys: Vec<(i64, ReadKey)> = wants
        .iter()
        .map(|(key, list)| {
            let oldest = list
                .iter()
                .map(|want| want.prior.as_ref().map_or(0, |state| state.checked_at_ms))
                .min()
                .unwrap_or(0);
            (oldest, key.clone())
        })
        .collect();
    keys.sort();
    let mut prior_baseline: BTreeMap<(String, String, Route), bool> = BTreeMap::new();
    // Class signals found by this pass's reads, per Loupist, before the switches filter them.
    let mut class_found: BTreeMap<String, Vec<NewWatchEvent>> = BTreeMap::new();
    for (_, key) in keys {
        if runtime.reads_left(now_ms) == 0 {
            break;
        }
        let list = wants.remove(&key).unwrap_or_default();
        for want in &list {
            prior_baseline.insert(
                (
                    want.loupist_id.clone(),
                    want.page.asset_id.clone(),
                    key.route,
                ),
                want.prior.as_ref().is_some_and(|state| state.baseline_done),
            );
        }
        let Some(first) = list.first() else { continue };
        let Some(read) = route_read(key.route, &first.page, today) else {
            continue;
        };
        // The first Loupist that holds the grant makes the read for everyone.
        let mut reading = None;
        for want in &list {
            if reader.ensure(state, &want.loupist_id) && reader.granted(&want.loupist_id, &read) {
                reading = Some(want.loupist_id.clone());
                break;
            }
        }
        let outcome = match &reading {
            Some(id) => {
                runtime.count_read(now_ms);
                reader
                    .read(state, id, &read, allowed_providers(key.route))
                    .await
            }
            None => ReadOutcome::NeedsConnection(if read.server.is_some() {
                OPENBB_CONNECTION.to_owned()
            } else {
                read.connection.to_owned()
            }),
        };
        let mut copies: HashMap<String, Value> = HashMap::new();
        for want in &list {
            let granted = reader.granted(&want.loupist_id, &read);
            let result = match (&outcome, granted) {
                (ReadOutcome::Ok { payload, provider }, true) => {
                    let payload = if Some(&want.loupist_id) == reading.as_ref() {
                        payload.clone()
                    } else if let Some(copy) = copies.get(&want.loupist_id) {
                        copy.clone()
                    } else {
                        let copy = share(state, &want.loupist_id, &read, payload)?;
                        copies.insert(want.loupist_id.clone(), copy.clone());
                        copy
                    };
                    let baseline_done =
                        want.prior.as_ref().is_some_and(|state| state.baseline_done);
                    with_assets(state, &want.loupist_id, true, |tx| {
                        let found = apply_read(
                            tx,
                            &want.loupist_id,
                            &want.page,
                            key.route,
                            &read,
                            &payload,
                            provider.as_deref(),
                            today,
                            now,
                            baseline_done,
                        )?;
                        watch::record_route(
                            tx,
                            &want.loupist_id,
                            &want.page.asset_id,
                            key.route,
                            "ok",
                            &connection_of(&read),
                            now_ms,
                        )?;
                        Ok(found)
                    })
                    .map(|found| {
                        class_found
                            .entry(want.loupist_id.clone())
                            .or_default()
                            .extend(found)
                    })
                }
                (ReadOutcome::Ok { .. }, false) => record(
                    state,
                    want,
                    key.route,
                    "needs_connection",
                    &connection_of(&read),
                    now_ms,
                ),
                (ReadOutcome::NeedsConnection(connection), _) => record(
                    state,
                    want,
                    key.route,
                    "needs_connection",
                    connection,
                    now_ms,
                ),
                (ReadOutcome::Failed(connection), _) => {
                    record(state, want, key.route, "failed", connection, now_ms)
                }
            };
            if let Err(error) = result {
                eprintln!("Watch check could not record a read: {}", error.code);
            }
        }
    }
    // Dates, releases and IR notices, then alerts and delivery, per Loupist.
    let mut alerts = BTreeMap::new();
    for loupist_id in &checked {
        let Ok(_activity) = state
            .maintenance
            .admit_kind(MaintenanceActivityKind::MemoryMutation)
        else {
            break;
        };
        let pages = followed_pages(state, loupist_id)?;
        for page in pages.iter().filter(|page| page.class == AssetClass::Equity) {
            sync_earnings(state, runtime, loupist_id, &mut *reader, page, today, now).await?;
        }
        let signals = class_found.remove(loupist_id).unwrap_or_default();
        let found = with_assets(state, loupist_id, true, |tx| {
            let switches = watch::switches(tx, loupist_id)?;
            let mut found = 0;
            for page in pages.iter().filter(|page| page.class == AssetClass::Equity) {
                let switch = switches.get(&page.asset_id).cloned().unwrap_or_default();
                let baseline = |route: Route| {
                    !prior_baseline
                        .get(&(loupist_id.clone(), page.asset_id.clone(), route))
                        .copied()
                        .unwrap_or(true)
                };
                for event in detect_events(tx, loupist_id, page, today, now_ms, &baseline)? {
                    if !switch.enabled(event.kind) {
                        continue;
                    }
                    if watch::insert_event(tx, loupist_id, &event, now_ms)?.is_some() {
                        found += 1;
                    }
                }
                watch::set_scanned_to(tx, loupist_id, &page.asset_id, now_ms)?;
            }
            for event in &signals {
                let on = switches
                    .get(&event.asset_id)
                    .is_some_and(|switch| switch.enabled(event.kind));
                if on && watch::insert_event(tx, loupist_id, event, now_ms)?.is_some() {
                    found += 1;
                }
            }
            let previous_day = watch::record_check(tx, loupist_id, &today.to_string(), now_ms)?;
            // The first check after launch, or the day's first, feeds the
            // morning message instead of a separate Thread message.
            let first_after_launch = runtime
                .first_checked
                .lock()
                .expect("watch runtime lock")
                .insert(loupist_id.clone());
            let morning = first_after_launch || previous_day.as_deref() != Some(&today.to_string());
            if morning {
                let pending: Vec<String> = watch::pending(tx, loupist_id)?
                    .into_iter()
                    .map(|event| event.event_id)
                    .collect();
                watch::mark_delivered(tx, loupist_id, &pending, "morning", now_ms)?;
            }
            Ok(found)
        })?;
        alerts.insert(loupist_id.clone(), found);
        let _ = crate::chat::home::deliver_finished_runs(state, loupist_id);
        changed(loupist_id);
    }
    Ok(alerts)
}

fn connection_of(read: &plan::PlannedRead) -> String {
    if read.server.is_some() {
        OPENBB_CONNECTION.to_owned()
    } else {
        read.connection.to_owned()
    }
}

fn record(
    state: &AppState,
    want: &Want,
    route: Route,
    status: &str,
    connection: &str,
    now_ms: i64,
) -> Result<(), CommandError> {
    with_assets(state, &want.loupist_id, true, |tx| {
        watch::record_route(
            tx,
            &want.loupist_id,
            &want.page.asset_id,
            route,
            status,
            connection,
            now_ms,
        )
    })
}

/// A shared read lands in another Loupist's own dataset store, so its rows
/// cite that Loupist's copy.
fn share(
    state: &AppState,
    loupist_id: &str,
    read: &plan::PlannedRead,
    payload: &Value,
) -> Result<Value, CommandError> {
    let mut copy = payload.clone();
    if let Some(object) = copy.as_object_mut() {
        object.remove("dataset_ref");
        object.remove("result_ref");
    }
    let key = crate::util::sha256(
        json!({"watch_share": read.tool, "arguments": read.arguments})
            .to_string()
            .as_bytes(),
    );
    let reference = state.store.dataset_put(
        loupist_id,
        &format!("watch-share:{key}"),
        &copy,
        json!({"producer": "watch", "tool": read.tool, "request": read.arguments, "fetched_at": payload.get("fetched_at"), "coverage": {"retained": "complete provider result"},
            "trust": shared_read_trust(state, read)}),
    )?;
    copy["dataset_ref"] = json!(reference);
    Ok(copy)
}

/// The trust class of a shared read, as the reading Loupist recorded its own
/// copy. A tool the classifier cannot name stays third-party text.
fn shared_read_trust(state: &AppState, read: &plan::PlannedRead) -> &'static str {
    use crate::custom_connections::taint::{retained_trust, TRUST_THIRD_PARTY_TEXT};
    match read.server {
        Some(server) => retained_trust(
            crate::broker::ToolMethod::McpCall,
            &json!({"server_id": server, "tool_name": read.tool}),
            &state.artifacts.mcp_runtimes,
        ),
        None => serde_json::from_value(json!(read.tool)).map_or(TRUST_THIRD_PARTY_TEXT, |method| {
            retained_trust(method, &read.arguments, &state.artifacts.mcp_runtimes)
        }),
    }
}

/// A class signal as a watch event citing the row it came from; a route's
/// first check keeps it as its starting point.
fn signal_event(
    page: &PageRecord,
    read: &observe::ToolRead<'_>,
    signal: watch_class::Signal,
    baseline_done: bool,
) -> NewWatchEvent {
    NewWatchEvent {
        asset_id: page.asset_id.clone(),
        kind: signal.kind,
        natural_key: signal.key,
        fact_date: signal.date.to_string(),
        facts: signal.facts,
        evidence: vec![observe::evidence_for(
            read,
            signal.pointer,
            signal.url,
            signal.date,
        )],
        baseline: !baseline_done,
    }
}

/// Records what one read says about one asset in one Loupist, and returns
/// what a class route's rows signal (the switches decide which become alerts).
#[allow(clippy::too_many_arguments)]
fn apply_read(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    route: Route,
    read: &plan::PlannedRead,
    payload: &Value,
    provider: Option<&str>,
    today: NaiveDate,
    now: DateTime<Utc>,
    baseline_done: bool,
) -> KnowledgeResult<Vec<NewWatchEvent>> {
    let dataset = payload
        .get("dataset_ref")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    let retrieved = payload
        .get("fetched_at")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let tool_read = observe::ToolRead {
        tool: read.tool,
        provider,
        payload,
        dataset,
        retrieved_at: retrieved.as_deref(),
        citation_url: None,
        today,
        session_day: assets::class::session_day(&page.market, now),
    };
    let class_kinds = match page.class {
        AssetClass::Crypto => watch_class::crypto_kinds(&page.symbol),
        AssetClass::Commodity => watch_class::commodity_kinds(&page.symbol),
        _ => Vec::new(),
    };
    let signals: Vec<watch_class::Signal> = match route {
        Route::Price => watch_class::price_signals(&tool_read, &class_kinds),
        Route::Tvl => watch_class::tvl_signal(&tool_read).into_iter().collect(),
        Route::Positioning => watch_class::positioning_signal(&tool_read)
            .into_iter()
            .collect(),
        Route::Inventory => watch_class::inventory_signal(&tool_read)
            .into_iter()
            .collect(),
        _ => Vec::new(),
    };
    let events = signals
        .into_iter()
        .filter(|signal| class_kinds.contains(&signal.kind))
        .map(|signal| signal_event(page, &tool_read, signal, baseline_done))
        .collect();
    let found: Vec<NewObservation> = match route {
        // A price read also keeps the page's closes (its price row and the
        // crypto fall from the high).
        Route::Filings | Route::Calendar | Route::Dividends | Route::Price => {
            observe::extract(&tool_read, &page.symbol)
        }
        Route::Tvl | Route::Positioning | Route::Inventory => Vec::new(),
        Route::Insiders => insider_observation(tx, loupist_id, page, &tool_read)?
            .into_iter()
            .collect(),
        Route::Flows => flows_observation(tx, loupist_id, page, &tool_read, baseline_done)?
            .into_iter()
            .collect(),
    };
    let override_baseline = match route {
        // A cluster or a turn seen by the route's first check is its starting point.
        Route::Insiders | Route::Flows => Some(!baseline_done),
        _ => None,
    };
    assets::record_observations_with(
        tx,
        loupist_id,
        &page.asset_id,
        found,
        ObservationOrigin::Refresh,
        None,
        now.timestamp_millis(),
        override_baseline,
    )?;
    Ok(events)
}

fn latest_of(
    tx: &Transaction<'_>,
    loupist_id: &str,
    asset_id: &str,
    kind: ObservationKind,
) -> KnowledgeResult<Option<assets::StoredObservation>> {
    Ok(assets::observations(tx, loupist_id, asset_id)?
        .into_iter()
        .filter(|item| item.kind == kind)
        .max_by(|a, b| a.fact_date.cmp(&b.fact_date)))
}

/// The newest cluster of insider purchases, unless it continues the last one.
fn insider_observation(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    read: &observe::ToolRead<'_>,
) -> KnowledgeResult<Option<NewObservation>> {
    let Some(cluster) = detect::insider_cluster(read.payload, read.today) else {
        return Ok(None);
    };
    if let Some(last) = latest_of(tx, loupist_id, &page.asset_id, ObservationKind::Insiders)? {
        let last_to = last.facts["to"].as_str().and_then(observe::parse_day);
        if last_to.is_some_and(|to| to >= cluster.from) {
            return Ok(None);
        }
    }
    Ok(Some(NewObservation {
        kind: ObservationKind::Insiders,
        fact_date: cluster.to.to_string(),
        dedupe_key: format!("insiders:{}:{}", cluster.from, cluster.to),
        facts: json!({"count": cluster.count, "from": cluster.from.to_string(), "to": cluster.to.to_string()}),
        evidence: observe::evidence_for(read, cluster.pointer, cluster.url, cluster.to),
    }))
}

/// A turn in foreign and institution buying; the first check stores the
/// current direction as its starting point.
fn flows_observation(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    read: &observe::ToolRead<'_>,
    baseline_done: bool,
) -> KnowledgeResult<Option<NewObservation>> {
    let Some(reading) = detect::flows(read.payload) else {
        return Ok(None);
    };
    let Some(direction) = reading.now else {
        return Ok(None);
    };
    let last = latest_of(tx, loupist_id, &page.asset_id, ObservationKind::Flows)?;
    let same = last
        .as_ref()
        .is_some_and(|last| last.facts["direction"] == direction.as_str());
    if baseline_done && (!reading.turned || same) {
        return Ok(None);
    }
    if !baseline_done && last.is_some() {
        return Ok(None);
    }
    Ok(Some(NewObservation {
        kind: ObservationKind::Flows,
        fact_date: reading.to.to_string(),
        dedupe_key: format!("flows:{}:{}", direction.as_str(), reading.to),
        facts: json!({"direction": direction.as_str(), "to": reading.to.to_string(), "net10": reading.net10, "prior10": reading.prior10, "traded10": reading.traded10}),
        evidence: observe::evidence_for(read, reading.pointer, None, reading.to),
    }))
}

/// The asset's events: new observations since the last scan routed to kinds,
/// and dates that came within 7 days. `baseline(route)` says the route's
/// first check is this one, so what it finds is kept and never shown.
pub(crate) fn detect_events(
    tx: &Transaction<'_>,
    loupist_id: &str,
    page: &PageRecord,
    today: NaiveDate,
    now_ms: i64,
    baseline: &dyn Fn(Route) -> bool,
) -> KnowledgeResult<Vec<NewWatchEvent>> {
    let korean = is_korean_market(&page.market);
    let scanned = watch::scanned_to(tx, loupist_id, &page.asset_id)?;
    let observed = assets::observations(tx, loupist_id, &page.asset_id)?;
    let mut events = Vec::new();
    let event = |kind: WatchKind,
                 key: String,
                 date: &str,
                 facts: Value,
                 evidence: &AssetEvidence,
                 base: bool| NewWatchEvent {
        asset_id: page.asset_id.clone(),
        kind,
        natural_key: key,
        fact_date: date.to_owned(),
        facts,
        evidence: vec![evidence.clone()],
        baseline: base,
    };
    for item in observed
        .iter()
        .filter(|item| item.recorded_at_ms > scanned && !item.baseline)
    {
        match item.kind {
            ObservationKind::Filing => {
                let facts = json!({"form": item.facts["form"], "title": item.facts["title"]});
                match detect::classify_filing(&item.facts, korean) {
                    detect::FilingRoute::Filings => events.push(event(WatchKind::Filings, item.dedupe_key.clone(), &item.fact_date, facts, &item.evidence, baseline(Route::Filings))),
                    detect::FilingRoute::Holders => events.push(event(WatchKind::Holders, item.dedupe_key.clone(), &item.fact_date, facts, &item.evidence, baseline(Route::Filings))),
                    detect::FilingRoute::Dividend => {
                        let mut facts = facts;
                        facts["declared"] = json!(true);
                        events.push(event(WatchKind::Dividends, item.dedupe_key.clone(), &item.fact_date, facts, &item.evidence, baseline(Route::Filings)));
                    }
                    detect::FilingRoute::InsiderReport => {
                        let dates: Vec<NaiveDate> = observed
                            .iter()
                            .filter(|other| other.kind == ObservationKind::Filing)
                            .filter(|other| {
                                detect::classify_filing(&other.facts, true)
                                    == detect::FilingRoute::InsiderReport
                            })
                            .filter_map(|other| observe::parse_day(&other.fact_date))
                            .collect();
                        if let Some((count, from, to)) = detect::report_cluster(&dates) {
                            let fired = watch::asset_events(tx, loupist_id, &page.asset_id, 20)?
                                .into_iter()
                                .any(|known| {
                                    known.kind == WatchKind::Insiders
                                        && known.facts["to"]
                                            .as_str()
                                            .and_then(observe::parse_day)
                                            .is_some_and(|known_to| known_to >= from)
                                });
                            if !fired {
                                events.push(event(
                                    WatchKind::Insiders,
                                    format!("insiders:{to}"),
                                    &to.to_string(),
                                    json!({"count": count, "from": from.to_string(), "to": to.to_string(), "buyNotChecked": true}),
                                    &item.evidence,
                                    baseline(Route::Filings),
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
            ObservationKind::Insiders => events.push(event(
                WatchKind::Insiders,
                item.dedupe_key.clone(),
                &item.fact_date,
                json!({"count": item.facts["count"], "from": item.facts["from"], "to": item.facts["to"]}),
                &item.evidence,
                false,
            )),
            ObservationKind::Flows => events.push(event(
                WatchKind::Flows,
                item.dedupe_key.clone(),
                &item.fact_date,
                json!({"direction": item.facts["direction"], "to": item.facts["to"]}),
                &item.evidence,
                false,
            )),
            ObservationKind::Dividend => events.push(event(
                WatchKind::Dividends,
                item.dedupe_key.clone(),
                &item.fact_date,
                json!({"amount": item.facts["amount"], "currency": item.facts["currency"], "exDate": item.facts["exDate"], "recordDate": item.facts["recordDate"], "declared": true}),
                &item.evidence,
                baseline(Route::Dividends),
            )),
            _ => {}
        }
    }
    // Results within 7 days: an announced or IR date, again if it moves. A
    // legal deadline is only a bound and never announced as a date.
    let results_route = if korean {
        Route::Filings
    } else {
        Route::Calendar
    };
    for event_record in earnings::events_for(tx, loupist_id, &page.asset_id)? {
        if event_record.date_source == earnings::DateSource::LegalDeadline
            || !matches!(
                event_record.status,
                earnings::Status::Upcoming | earnings::Status::BeforeCaptured
            )
        {
            continue;
        }
        let days = (event_record.report_date - today).num_days();
        if !(0..=7).contains(&days) {
            continue;
        }
        let date = event_record.report_date.to_string();
        let source = assets::observations(tx, loupist_id, &page.asset_id)?
            .into_iter()
            .filter(|item| {
                matches!(
                    item.kind,
                    ObservationKind::Earnings | ObservationKind::Filing
                )
            })
            .find(|item| {
                item.facts["date"] == date.as_str()
                    || (item.kind == ObservationKind::Filing
                        && detect::classify_filing(&item.facts, korean)
                            == detect::FilingRoute::IrNotice)
            });
        let Some(source) = source else { continue };
        events.push(event(
            WatchKind::Results,
            format!("results:{date}"),
            &source.fact_date,
            json!({"date": date, "period": event_record.period, "eventId": event_record.event_id}),
            &source.evidence,
            baseline(results_route),
        ));
    }
    // A dividend record date within 7 days.
    for item in observed
        .iter()
        .filter(|item| item.kind == ObservationKind::Dividend)
    {
        let Some(record) = item.facts["recordDate"]
            .as_str()
            .and_then(observe::parse_day)
        else {
            continue;
        };
        let days = (record - today).num_days();
        if !(0..=7).contains(&days) {
            continue;
        }
        events.push(event(
            WatchKind::Dividends,
            format!("record:{record}"),
            &item.fact_date,
            json!({"amount": item.facts["amount"], "currency": item.facts["currency"], "exDate": item.facts["exDate"], "recordDate": record.to_string()}),
            &item.evidence,
            baseline(Route::Dividends),
        ));
    }
    let _ = now_ms;
    Ok(events)
}

/// Keeps earnings events in step with what the checks recorded, reading new
/// Korean IR notices for their dates (one native read each, within budget).
pub(in crate::commands) async fn sync_earnings<R: HostReader>(
    state: &AppState,
    runtime: &Runtime,
    loupist_id: &str,
    reader: &mut R,
    page: &PageRecord,
    today: NaiveDate,
    now: DateTime<Utc>,
) -> Result<(), CommandError> {
    let loupist_id = loupist_id.to_owned();
    let outcome = with_assets(state, &loupist_id, true, |tx| {
        earnings::sync(tx, &loupist_id, page, today, now.timestamp_millis())
    })?;
    for notice in outcome.ir_notices.iter().take(2) {
        if runtime.reads_left(now.timestamp_millis()) == 0 {
            break;
        }
        runtime.count_read(now.timestamp_millis());
        let read = plan::PlannedRead {
            kind: "results",
            tool: "dart_filing_read",
            server: None,
            connection: OPENDART_CONNECTION,
            arguments: json!({"rcept_no": notice.filing_id}),
        };
        let date = match reader.read(state, &loupist_id, &read, None).await {
            ReadOutcome::Ok { payload, .. } => ["/data/document/text", "/document/text", "/text"]
                .iter()
                .find_map(|pointer| payload.pointer(pointer).and_then(Value::as_str))
                .and_then(|text| detect::ir_date(text, notice.filed)),
            _ => continue,
        };
        with_assets(state, &loupist_id, true, |tx| {
            earnings::apply_ir_date(tx, &loupist_id, page, notice, date, now.timestamp_millis())
        })?;
    }
    Ok(())
}

/// Tells the renderer something changed for this Loupist's watchers.
pub(crate) fn changed(loupist_id: &str) {
    crate::briefings::desktop::emit(
        crate::events::WATCH_CHANGED,
        crate::events::WatchChanged {
            loupist_id: loupist_id.to_owned(),
        },
    );
}

// ───────────────────────────── Defaults and consent ─────────────────────────────

/// The asset class of the template this Loupist was made from, if any.
fn template_class(state: &AppState, loupist_id: &str) -> Option<AssetClass> {
    state
        .store
        .get_loupist(loupist_id)
        .ok()
        .flatten()
        .and_then(|profile| profile.template)
        .and_then(crate::domain::LoupistTemplate::asset_class)
}

/// A Loupist gives proactive consent: followed assets with no decision get
/// the default kinds (`watch::class_defaults`), and an untouched earnings task
/// turns on.
#[cfg(any(test, feature = "eval-harness"))]
pub(crate) fn on_consent(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    let pages = followed_pages(state, loupist_id)?;
    let template = template_class(state, loupist_id);
    with_assets(state, loupist_id, true, |tx| {
        for page in pages.iter().filter(|page| watchable(page)) {
            let defaults = watch::class_defaults(page.class, template);
            watch::apply_defaults(tx, loupist_id, &page.asset_id, &defaults, now_ms())?;
        }
        Ok(())
    })?;
    super::earnings::ensure_task(state, loupist_id, true)?;
    changed(loupist_id);
    Ok(())
}

/// A newly followed asset gets the default kinds in the Loupist that was open
/// (with consent); other Loupists list it as not watched.
pub(crate) fn on_followed(
    state: &AppState,
    loupist_id: &str,
    symbol: &str,
    market: &str,
) -> Result<(), CommandError> {
    if state.ensure_loupist_available(loupist_id).is_err()
        || consent_mode(state, loupist_id)? != "legacy"
    {
        return Ok(());
    }
    let page = followed_pages(state, loupist_id)?.into_iter().find(|page| {
        page.symbol.eq_ignore_ascii_case(symbol.trim())
            && page.market.eq_ignore_ascii_case(market.trim())
    });
    if let Some(page) = page.filter(watchable) {
        let defaults = watch::class_defaults(page.class, template_class(state, loupist_id));
        with_assets(state, loupist_id, true, |tx| {
            watch::apply_defaults(tx, loupist_id, &page.asset_id, &defaults, now_ms()).map(|_| ())
        })?;
        if page.class == AssetClass::Equity {
            super::earnings::ensure_task(state, loupist_id, false)?;
        }
        changed(loupist_id);
    }
    Ok(())
}

// ───────────────────────────── Views and commands ─────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRef {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct KindFailure {
    pub connection: String,
    pub at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct KindStateView {
    pub kind: WatchKind,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub needs_connection: Option<ConnectionRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub failed: Option<KindFailure>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WatchAssetView {
    pub asset_id: String,
    pub symbol: String,
    pub market: String,
    pub name: String,
    pub kinds: Vec<KindStateView>,
    pub revision: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_alert: Option<WatchEventView>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WatchSettingsView {
    pub loupist_id: String,
    pub paused: bool,
    pub consent_off: bool,
    pub checking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_checked_ms: Option<i64>,
    pub watched: Vec<WatchAssetView>,
    pub not_watched: Vec<WatchAssetView>,
}

/// The Loupist's switches, connections and last alerts, for the asset page
/// and Schedule › Watching.
pub(crate) fn settings_view(
    state: &AppState,
    runtime: &Runtime,
    loupist_id: &str,
) -> Result<WatchSettingsView, CommandError> {
    let pages = followed_pages(state, loupist_id)?;
    let consent = consent(state, loupist_id);
    let capabilities: BTreeSet<String> =
        super::capture_chat_connector_authority(state, loupist_id)?
            .capability_ids
            .into_iter()
            .collect();
    let openbb = openbb_capabilities();
    let names = connection_names();
    let name = |id: &str| names.get(id).cloned().unwrap_or_else(|| id.to_owned());
    let today = chrono::Local::now().date_naive();
    let (watch_state, switches, routes, assets) = with_assets(state, loupist_id, false, |tx| {
        let mut out = Vec::new();
        for page in pages.iter().filter(|page| watchable(page)) {
            let kinds = applicable(tx, loupist_id, page, today)?;
            let last = watch::asset_events(tx, loupist_id, &page.asset_id, 1)?
                .into_iter()
                .next();
            out.push((page.clone(), kinds, last));
        }
        Ok((
            watch::state(tx, loupist_id)?,
            watch::switches(tx, loupist_id)?,
            watch::routes(tx, loupist_id)?,
            out,
        ))
    })?;
    let mut watched = Vec::new();
    let mut not_watched = Vec::new();
    for (page, kinds, last_alert) in assets {
        let korean = is_korean_market(&page.market);
        let switch = switches.get(&page.asset_id).cloned().unwrap_or_default();
        let states = routes.get(&page.asset_id).cloned().unwrap_or_default();
        let kind_views: Vec<KindStateView> = kinds
            .into_iter()
            .map(|kind| {
                let needed = kind_connection(kind, korean);
                let granted = if needed == OPENBB_CONNECTION {
                    capabilities.iter().any(|id| openbb.contains(id))
                } else {
                    capabilities.contains(needed)
                };
                let route = states
                    .iter()
                    .find(|state| state.route == kind_route(kind, korean));
                // What the last check learned is more specific than the grant.
                let needs_connection = route
                    .filter(|state| state.status == "needs_connection")
                    .map(|state| ConnectionRef {
                        id: state.connection.clone(),
                        name: name(&state.connection),
                    })
                    .or_else(|| {
                        (!granted).then(|| ConnectionRef {
                            id: needed.to_owned(),
                            name: name(needed),
                        })
                    });
                KindStateView {
                    kind,
                    enabled: consent && switch.enabled(kind),
                    failed: route.filter(|state| state.status == "failed").map(|state| {
                        KindFailure {
                            connection: name(&state.connection),
                            at_ms: state.checked_at_ms,
                        }
                    }),
                    needs_connection,
                }
            })
            .collect();
        let view = WatchAssetView {
            asset_id: page.asset_id.clone(),
            symbol: page.symbol.clone(),
            market: page.market.clone(),
            name: page.name.clone(),
            revision: switch.revision.max(1),
            last_alert,
            kinds: kind_views,
        };
        if view.kinds.iter().any(|kind| kind.enabled) {
            watched.push(view);
        } else {
            not_watched.push(view);
        }
    }
    Ok(WatchSettingsView {
        loupist_id: loupist_id.to_owned(),
        paused: watch_state.paused,
        consent_off: !consent,
        checking: runtime.is_checking(loupist_id),
        last_checked_ms: watch_state.last_checked_ms,
        watched,
        not_watched,
    })
}

fn admit(state: &AppState, loupist_id: &str) -> Result<(), CommandError> {
    state.ensure_loupist_available(loupist_id)?;
    state
        .store
        .get_loupist(loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchSettingsListRequest {
    pub loupist_id: String,
}

#[tauri::command]
pub fn watch_settings_list(
    state: State<'_, AppState>,
    request: WatchSettingsListRequest,
) -> Result<WatchSettingsView, CommandError> {
    admit(&state, &request.loupist_id)?;
    settings_view(
        &state,
        &state.scheduled.watch_runtime(),
        &request.loupist_id,
    )
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchSettingSetRequest {
    pub loupist_id: String,
    pub asset_id: String,
    pub kind: WatchKind,
    pub enabled: bool,
    pub expected_revision: i64,
}

#[tauri::command]
pub fn watch_setting_set(
    state: State<'_, AppState>,
    request: WatchSettingSetRequest,
) -> Result<WatchSettingsView, CommandError> {
    update_setting(&state, request)
}

pub(crate) fn update_setting(
    state: &AppState,
    request: WatchSettingSetRequest,
) -> Result<WatchSettingsView, CommandError> {
    admit(state, &request.loupist_id)?;
    if !assets::valid_asset_id(&request.asset_id) {
        return Err(CommandError::invalid("assetId names no asset page"));
    }
    let mode = consent_mode(state, &request.loupist_id)?;
    let loupist = request.loupist_id.clone();
    let applies = with_assets(state, &loupist, true, |tx| {
        let Some(page) = assets::get_page(tx, &loupist, &request.asset_id)? else {
            return Ok(false);
        };
        let today = chrono::Local::now().date_naive();
        if !applicable(tx, &loupist, &page, today)?.contains(&request.kind) {
            return Ok(false);
        }
        watch::set_kind(
            tx,
            &loupist,
            &request.asset_id,
            request.kind,
            request.enabled,
            request.expected_revision,
            now_ms(),
        )?;
        Ok(true)
    })?;
    if !applies {
        return Err(CommandError::invalid(
            "this kind does not apply to this asset",
        ));
    }
    if request.enabled && mode == "off" {
        state
            .store
            .lock()
            .map_err(map_store)?
            .execute(
                "UPDATE watcher_consent SET mode='selected' WHERE loupist_id=?1 AND mode='off'",
                [&loupist],
            )
            .map_err(super::map_internal)?;
    }
    if request.enabled {
        crate::briefings::desktop::enable_for_watching(state)?;
    }
    changed(&loupist);
    settings_view(state, &state.scheduled.watch_runtime(), &loupist)
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchPauseSetRequest {
    pub loupist_id: String,
    pub paused: bool,
}

#[tauri::command]
pub fn watch_pause_set(
    state: State<'_, AppState>,
    request: WatchPauseSetRequest,
) -> Result<WatchSettingsView, CommandError> {
    admit(&state, &request.loupist_id)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, true, |tx| {
        watch::set_paused(tx, &loupist, request.paused, now_ms())
    })?;
    changed(&loupist);
    settings_view(&state, &state.scheduled.watch_runtime(), &loupist)
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchCheckNowRequest {
    pub loupist_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct WatchCheckResult {
    pub new_alerts: usize,
    pub settings: WatchSettingsView,
}

/// Check now: every enabled kind of this Loupist once (one check at a time).
#[tauri::command]
pub async fn watch_check_now(
    state: State<'_, AppState>,
    request: WatchCheckNowRequest,
) -> Result<WatchCheckResult, CommandError> {
    admit(&state, &request.loupist_id)?;
    let runtime = state.scheduled.watch_runtime();
    if runtime.is_checking(&request.loupist_id) {
        return Err(CommandError::conflict("a check is already running"));
    }
    let alerts = Box::pin(run_pass(
        &state,
        &runtime,
        Scope::Loupist(request.loupist_id.clone()),
        Utc::now(),
    ))
    .await?;
    Ok(WatchCheckResult {
        new_alerts: alerts.get(&request.loupist_id).copied().unwrap_or(0),
        settings: settings_view(&state, &runtime, &request.loupist_id)?,
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchEventsListRequest {
    pub loupist_id: String,
    pub since_ms: Option<i64>,
    pub limit: usize,
}

#[tauri::command]
pub fn watch_events_list(
    state: State<'_, AppState>,
    request: WatchEventsListRequest,
) -> Result<Vec<WatchEventView>, CommandError> {
    admit(&state, &request.loupist_id)?;
    let loupist = request.loupist_id.clone();
    with_assets(&state, &loupist, false, |tx| {
        watch::list_events(
            tx,
            &loupist,
            request.since_ms,
            request.limit.clamp(1, watch::MAX_EVENTS_LIST),
        )
    })
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WatchEventsSeenRequest {
    pub loupist_id: String,
    /// Empty: every delivered alert (Schedule › Watching was opened).
    pub event_ids: Vec<String>,
}

#[tauri::command]
pub fn watch_events_seen(
    state: State<'_, AppState>,
    request: WatchEventsSeenRequest,
) -> Result<(), CommandError> {
    admit(&state, &request.loupist_id)?;
    if request.event_ids.len() > watch::MAX_EVENTS_LIST {
        return Err(CommandError::invalid("too many alerts at once"));
    }
    let loupist = request.loupist_id.clone();
    let changed_count = with_assets(&state, &loupist, true, |tx| {
        if request.event_ids.is_empty() {
            watch::mark_all_seen(tx, &loupist, now_ms())
        } else {
            watch::mark_seen(tx, &loupist, &request.event_ids, now_ms())
        }
    })?;
    if changed_count > 0 {
        changed(&loupist);
    }
    Ok(())
}

/// The background loop's step: due routes for every Loupist with consent.
pub(crate) async fn tick(state: &AppState, runtime: &Runtime) {
    let now = Utc::now();
    runtime.mark_started(now.timestamp_millis());
    if runtime
        .started()
        .is_some_and(|started| now.timestamp_millis() - started < LAUNCH_DELAY_MS)
    {
        return;
    }
    if let Ok(loupists) = state.store.list_loupists() {
        for loupist in loupists {
            if state.ensure_loupist_available(&loupist.id).is_ok() {
                let _ = super::earnings::ensure_task(state, &loupist.id, false);
            }
        }
    }
    if let Err(error) = Box::pin(run_pass(state, runtime, Scope::Scheduled, now)).await {
        if error.code != "maintenance_active" {
            eprintln!("Watch check failed: {}", error.code);
        }
    }
}

#[cfg(test)]
pub(in crate::commands) mod scripted;
#[cfg(test)]
mod tests;
