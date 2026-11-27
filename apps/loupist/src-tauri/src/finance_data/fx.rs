/// Currency codes shared by amount recognition, asset classes and reference rates.
pub(crate) const CURRENCY_CODES: &[&str] = &[
    "USD", "KRW", "EUR", "JPY", "GBP", "CNY", "CNH", "CHF", "AUD", "CAD", "HKD", "SGD", "NZD",
    "SEK", "NOK", "DKK", "INR", "TWD", "MXN", "BRL", "ZAR", "CZK", "HUF", "PLN", "RON", "ISK",
    "TRY", "IDR", "ILS", "MYR", "PHP", "THB",
];

pub(crate) fn has_currency_code_shape(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_uppercase())
}

pub(crate) fn is_currency_code(code: &str) -> bool {
    CURRENCY_CODES
        .iter()
        .any(|known| code.eq_ignore_ascii_case(known))
}

// The first 21 codes retain their pre-wave text matching behavior.
const LEGACY_CURRENCY_CODE_COUNT: usize = 21;

/// A currency marker in free text, before case normalization.
pub(crate) fn is_currency_marker(code: &str) -> bool {
    CURRENCY_CODES.iter().enumerate().any(|(index, known)| {
        code == *known || (index < LEGACY_CURRENCY_CODE_COUNT && code.eq_ignore_ascii_case(known))
    })
}

use crate::{
    broker::BrokerError,
    events::{FxChanged, FxDisplayState},
};
use chrono::{DateTime, Datelike, Days, Duration, NaiveDate, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    future::Future,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::sync::watch;

const SCHEMA: &str = "loupist-fx-reference/1";
pub(crate) const SOURCE_URL: &str = "https://www.ecb.europa.eu/stats/policy_and_exchange_rates/euro_reference_exchange_rates/html/index.en.html";
const SOURCE_NAME: &str = "European Central Bank euro foreign exchange reference rates";
pub(crate) const RATE_ROUTE: &str = "currency_reference_rates";
pub(crate) const RATE_PROVIDER: &str = "ecb";
// Scientific notation prints one leading digit plus these fractional digits.
const RATE_FRACTION_DIGITS: usize = 5;

/// Half a unit of the published rate's last significant digit.
pub(crate) fn published_rate_precision(rate: f64) -> Option<f64> {
    let text = format!("{rate:e}");
    let (_, exponent) = text.split_once('e')?;
    let exponent = exponent.parse::<i32>().ok()?;
    Some(0.5 * 10f64.powf(f64::from(exponent - RATE_FRACTION_DIGITS as i32)))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Table {
    schema: String,
    pub(crate) revision: String,
    #[serde(with = "day_wire")]
    pub(crate) date: NaiveDate,
    #[serde(with = "time_wire")]
    fetched_at: DateTime<Utc>,
    per_euro: BTreeMap<String, f64>,
}

impl Table {
    /// Accept the provider object or the host's MCP result wrapper.
    pub(crate) fn parse(value: &Value, now: DateTime<Utc>) -> Result<Self, BrokerError> {
        let value = value
            .pointer("/result/structuredContent/results")
            .or_else(|| value.pointer("/structuredContent/results"))
            .unwrap_or(value);
        let object = value.as_object().ok_or_else(invalid_table)?;
        let date = object
            .get("date")
            .and_then(Value::as_str)
            .filter(|day| day.len() == 10)
            .and_then(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            .ok_or_else(invalid_table)?;
        let mut per_euro: BTreeMap<String, f64> = object
            .iter()
            .filter_map(|(key, value)| {
                let number = value.as_f64()?;
                (has_currency_code_shape(key) && number.is_finite() && number > 0.0)
                    .then(|| (key.clone(), number))
            })
            .collect();
        if per_euro.is_empty() {
            return Err(invalid_table());
        }
        per_euro.insert("EUR".into(), 1.0);
        Ok(Self {
            schema: SCHEMA.into(),
            revision: format!("ecb:{date}"),
            date,
            fetched_at: now,
            per_euro,
        })
    }

    pub(crate) fn pair(&self, from: &str, to: &str) -> Result<Value, BrokerError> {
        if from == to {
            return Err(BrokerError::invalid("to", "the same currency as from"));
        }
        for (field, code) in [("from", from), ("to", to)] {
            if !self.per_euro.contains_key(code) {
                let codes = self.per_euro.keys().cloned().collect::<Vec<_>>().join(", ");
                return Err(BrokerError::invalid(field, format!("{code} is absent from this European Central Bank reference-rate table; fx_rate accepts {codes}")));
            }
        }
        let rounded = rounded_rate(self.per_euro[from], self.per_euro[to])?;
        let mut legs = serde_json::Map::new();
        for code in [from, to].into_iter().filter(|code| *code != "EUR") {
            legs.insert(format!("{code}_per_EUR"), json!(self.per_euro[code]));
        }
        let mut result = json!({"from":from, "to":to, "to_per_from":rounded, "date":self.date.to_string(),
            "source":{"id":RATE_PROVIDER, "name":SOURCE_NAME},
            "legs":legs, "revision":self.revision});
        if from != "EUR" {
            result["computed"] = json!(if to == "EUR" {
                "inverse of the reference rate"
            } else {
                "cross rate through EUR"
            });
        }
        Ok(result)
    }

    fn first_expired_day(&self) -> Option<NaiveDate> {
        self.date.checked_add_days(Days::new(8))
    }

    fn too_old(&self, now: DateTime<Utc>) -> bool {
        self.first_expired_day()
            .is_some_and(|day| now.with_timezone(&chrono_tz::Europe::Berlin).date_naive() >= day)
    }

    fn usable_until(&self) -> Option<String> {
        let midnight = self.first_expired_day()?.and_hms_opt(0, 0, 0)?;
        chrono_tz::Europe::Berlin
            .from_local_datetime(&midnight)
            .earliest()
            .map(|until| until.with_timezone(&Utc).to_rfc3339())
    }

    fn display_rates(&self, currency: &str) -> Option<BTreeMap<String, f64>> {
        let to = *self.per_euro.get(currency)?;
        // A target needs a representable rate from the reference unit.
        rounded_rate(1.0, to).ok()?;
        Some(
            self.per_euro
                .iter()
                .filter_map(|(code, from)| {
                    rounded_rate(*from, to)
                        .ok()
                        .map(|rate| (code.clone(), rate))
                })
                .collect(),
        )
    }
}

fn rounded_rate(from: f64, to: f64) -> Result<f64, BrokerError> {
    let number = to / from;
    // Scientific notation rounds even subnormal cross rates without
    // overflowing an intermediate power of ten.
    let rounded: f64 = format!("{number:.RATE_FRACTION_DIGITS$e}")
        .parse()
        .map_err(|_| invalid_table())?;
    if !rounded.is_finite() || rounded <= 0.0 {
        return Err(invalid_table());
    }
    Ok(rounded)
}

// Chrono's serde feature is intentionally not required by the host.
mod day_wire {
    use super::*;
    pub(super) fn serialize<S: serde::Serializer>(
        value: &NaiveDate,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<NaiveDate, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
mod time_wire {
    use super::*;
    pub(super) fn serialize<S: serde::Serializer>(
        value: &DateTime<Utc>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_rfc3339())
    }
    pub(super) fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<DateTime<Utc>, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

fn invalid_table() -> BrokerError {
    BrokerError::Execution("The finance data provider returned an invalid response".into())
}

pub(crate) fn expected_date(now: DateTime<Utc>) -> NaiveDate {
    let berlin = now.with_timezone(&chrono_tz::Europe::Berlin);
    let mut day = berlin.date_naive();
    if berlin.time() < chrono::NaiveTime::from_hms_opt(16, 30, 0).unwrap() {
        day = day.pred_opt().unwrap();
    }
    while matches!(day.weekday(), Weekday::Sat | Weekday::Sun) {
        day = day.pred_opt().unwrap();
    }
    day
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Behind {
    #[serde(with = "day_wire")]
    expected_date: NaiveDate,
    because: &'static str,
}

#[derive(Clone, Debug)]
pub(crate) struct Resolved {
    pub(crate) table: Arc<Table>,
    pub(crate) behind: Option<Behind>,
}

impl Resolved {
    pub(crate) fn pair(&self, from: &str, to: &str) -> Result<Value, BrokerError> {
        let mut result = self.table.pair(from, to)?;
        if let Some(behind) = &self.behind {
            result["behind"] = json!(behind);
        }
        Ok(result)
    }
}

#[derive(Default)]
struct State {
    loaded: bool,
    table: Option<Arc<Table>>,
    attempted_at: Option<DateTime<Utc>>,
    // Keep the failure until a successful read, including during the retry wait.
    failure: Option<Arc<BrokerError>>,
    refresh: Option<watch::Sender<()>>,
}

struct Snapshot {
    table: Option<Arc<Table>>,
    failure: Option<Arc<BrokerError>>,
    refreshing: bool,
}

impl State {
    fn snapshot(&self) -> Snapshot {
        Snapshot {
            table: self.table.clone(),
            failure: self.failure.clone(),
            refreshing: self.refresh.is_some(),
        }
    }
}

impl Snapshot {
    fn tool(self, now: DateTime<Utc>, connected: bool) -> Result<Resolved, Arc<BrokerError>> {
        let table = self.table.ok_or_else(|| {
            if connected {
                self.failure
                    .clone()
                    .unwrap_or_else(|| Arc::new(invalid_table()))
            } else {
                Arc::new(connection_off())
            }
        })?;
        Ok(Resolved {
            table,
            behind: if connected {
                self.failure.as_deref().map(FxDisplayBehind::from_error)
            } else {
                Some(FxDisplayBehind::ConnectionOff)
            }
            .map(|because| Behind {
                expected_date: expected_date(now),
                because: because.code(),
            }),
        })
    }

    fn display(self, now: DateTime<Utc>, currency: String, connected: bool) -> FxDisplay {
        let behind = if connected {
            self.failure.as_deref().map(FxDisplayBehind::from_error)
        } else {
            Some(FxDisplayBehind::ConnectionOff)
        };
        let unavailable = |reason, last_date| FxDisplay::Unavailable {
            currency: currency.clone(),
            reason,
            last_date,
        };
        if let Some(table) = self.table {
            let date = table.date.to_string();
            if table.too_old(now) {
                return unavailable(FxUnavailableReason::TooOld, Some(date));
            }
            let Some(rates) = table.display_rates(&currency) else {
                return unavailable(FxUnavailableReason::NoRateForCurrency, Some(date));
            };
            return FxDisplay::Ready {
                currency,
                revision: table.revision.clone(),
                date,
                source: SOURCE_NAME.into(),
                fetched_at: table.fetched_at.to_rfc3339(),
                usable_until: table.usable_until(),
                behind,
                rates,
            };
        }
        if connected && self.refreshing {
            FxDisplay::Waiting { currency }
        } else {
            unavailable(
                behind.unwrap_or(FxDisplayBehind::ProviderFailed).into(),
                None,
            )
        }
    }

    // An app-wide hint for an enabled reader. Each reread applies its own
    // configuration and grant and may project a different display state.
    fn hint(&self, now: DateTime<Utc>) -> (FxChanged, Option<FxDisplayBehind>) {
        let (state, behind) = match &self.table {
            Some(table) if table.too_old(now) => (FxDisplayState::Unavailable, None),
            Some(_) => (
                FxDisplayState::Ready,
                self.failure.as_deref().map(FxDisplayBehind::from_error),
            ),
            None if self.refreshing => (FxDisplayState::Waiting, None),
            None => (FxDisplayState::Unavailable, None),
        };
        (
            FxChanged {
                state,
                revision: self.table.as_ref().map(|table| table.revision.clone()),
            },
            behind,
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FxDisplayBehind {
    Offline,
    ProviderFailed,
    ConnectionOff,
}

impl FxDisplayBehind {
    fn from_error(error: &BrokerError) -> Self {
        if is_offline(error) {
            Self::Offline
        } else {
            Self::ProviderFailed
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::ProviderFailed => "provider_failed",
            Self::ConnectionOff => "connection_off",
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum FxUnavailableReason {
    Offline,
    ProviderFailed,
    ConnectionOff,
    NoRateForCurrency,
    TooOld,
}

impl From<FxDisplayBehind> for FxUnavailableReason {
    fn from(value: FxDisplayBehind) -> Self {
        match value {
            FxDisplayBehind::Offline => Self::Offline,
            FxDisplayBehind::ProviderFailed => Self::ProviderFailed,
            FxDisplayBehind::ConnectionOff => Self::ConnectionOff,
        }
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum FxDisplay {
    Unset,
    Ready {
        currency: String,
        revision: String,
        date: String,
        source: String,
        fetched_at: String,
        #[serde(rename = "usable_until", skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        usable_until: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        behind: Option<FxDisplayBehind>,
        #[cfg_attr(test, ts(type = "Record<string, number>"))]
        rates: BTreeMap<String, f64>,
    },
    Waiting {
        currency: String,
    },
    Unavailable {
        currency: String,
        reason: FxUnavailableReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(test, ts(optional))]
        last_date: Option<String>,
    },
}

enum Refresh {
    None,
    Join(watch::Receiver<()>),
    Own(RefreshOwner),
}

/// Dropping the owner closes the watch channel and releases joiners, without
/// recording a failed attempt or reserving retry spacing.
struct RefreshOwner {
    service: Arc<FxService>,
    now: DateTime<Utc>,
    completed: bool,
}

impl RefreshOwner {
    async fn run<F, Fut>(mut self, fetch: F) -> Snapshot
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Value, BrokerError>>,
    {
        let outcome = fetch()
            .await
            .and_then(|value| Table::parse(&value, self.now));
        self.completed = true;
        self.service.settle(self.now, Some(outcome))
    }
}

impl Drop for RefreshOwner {
    fn drop(&mut self) {
        if !self.completed {
            self.service.settle(self.now, None);
        }
    }
}

/// Provider I/O belongs to the one refresh owner. The snapshot lock never
/// spans an await, and display callers borrow the cached table immediately.
#[derive(Default)]
pub(crate) struct FxService {
    state: Mutex<State>,
    directory: Option<PathBuf>,
    #[cfg(test)]
    changes: Mutex<Vec<FxChanged>>,
    #[cfg(test)]
    pub(crate) event_test_app: Mutex<Option<tauri::AppHandle<tauri::test::MockRuntime>>>,
}

impl FxService {
    pub(crate) fn with_cache(directory: PathBuf) -> Self {
        Self {
            directory: Some(directory),
            ..Self::default()
        }
    }

    fn begin(self: &Arc<Self>, now: DateTime<Utc>, connected: bool) -> (Snapshot, Refresh) {
        let mut state = self.state.lock().unwrap();
        if !state.loaded {
            state.table = self.load().map(Arc::new);
            state.attempted_at = state.table.as_ref().map(|table| table.fetched_at);
            state.loaded = true;
        }
        let refresh = if !connected {
            Refresh::None
        } else if let Some(refresh) = &state.refresh {
            Refresh::Join(refresh.subscribe())
        } else if state
            .table
            .as_ref()
            .is_some_and(|table| table.date >= expected_date(now))
            || state
                .attempted_at
                .is_some_and(|at| now - at < Duration::hours(3))
        {
            Refresh::None
        } else {
            state.refresh = Some(watch::channel(()).0);
            Refresh::Own(RefreshOwner {
                service: self.clone(),
                now,
                completed: false,
            })
        };
        (state.snapshot(), refresh)
    }

    fn settle(&self, now: DateTime<Utc>, outcome: Option<Result<Table, BrokerError>>) -> Snapshot {
        // The same owner keeps its reservation while writing the private cache.
        // Readers can keep using the preceding in-memory publication meanwhile.
        let latest = self.state.lock().unwrap().table.clone();
        if let Some(Ok(table)) = &outcome {
            if latest
                .as_ref()
                .is_none_or(|current| table.date >= current.date)
            {
                self.store(table);
            }
        }
        let (wake, change, snapshot) = {
            let mut state = self.state.lock().unwrap();
            let before = state.snapshot().hint(now);
            let mut changed_rates = false;
            if let Some(outcome) = outcome {
                state.attempted_at = Some(now);
                match outcome {
                    Ok(table) => {
                        if latest
                            .as_ref()
                            .is_none_or(|current| table.date >= current.date)
                        {
                            changed_rates = latest
                                .as_ref()
                                .is_some_and(|current| table.per_euro != current.per_euro);
                            state.table = Some(Arc::new(table));
                        }
                        state.failure = None;
                    }
                    Err(error) => state.failure = Some(Arc::new(error)),
                }
            }
            let wake = state.refresh.take();
            let snapshot = state.snapshot();
            let after = snapshot.hint(now);
            (
                wake,
                (before != after || changed_rates).then_some(after.0),
                snapshot,
            )
        };
        if let Some(change) = change {
            self.emit_change(change);
        }
        drop(wake);
        snapshot
    }

    pub(crate) fn invalidate_display(&self) {
        self.emit_change(FxChanged {
            state: FxDisplayState::Invalidated,
            revision: None,
        });
    }

    fn emit_change(&self, change: FxChanged) {
        crate::briefings::desktop::emit(crate::events::FX_CHANGED, change.clone());
        #[cfg(test)]
        {
            let app = self.event_test_app.lock().unwrap().clone();
            if let Some(app) = app {
                tauri::Emitter::emit(&app, crate::events::FX_CHANGED, change.clone()).unwrap();
            }
            self.changes.lock().unwrap().push(change);
        }
    }

    #[cfg(test)]
    pub(crate) fn take_display_changes(&self) -> Vec<FxChanged> {
        std::mem::take(&mut *self.changes.lock().unwrap())
    }

    pub(crate) async fn resolve<F, Fut>(
        self: &Arc<Self>,
        now: DateTime<Utc>,
        connected: bool,
        fetch: F,
    ) -> Result<Resolved, Arc<BrokerError>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Value, BrokerError>>,
    {
        #[cfg(feature = "eval-harness")]
        if crate::eval_harness::replay::enabled() {
            return self.replay(now, connected).map_err(Arc::new);
        }
        loop {
            let (snapshot, refresh) = self.begin(now, connected);
            match refresh {
                Refresh::None => return snapshot.tool(now, connected),
                Refresh::Join(mut completion) => {
                    let _ = completion.changed().await;
                }
                Refresh::Own(owner) => {
                    return owner.run(fetch).await.tool(now, connected);
                }
            }
        }
    }

    pub(crate) fn display<F, Fut>(
        self: &Arc<Self>,
        now: DateTime<Utc>,
        currency: String,
        connected: bool,
        fetch: F,
    ) -> FxDisplay
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<Value, BrokerError>> + Send + 'static,
    {
        #[cfg(feature = "eval-harness")]
        if crate::eval_harness::replay::enabled() {
            let snapshot = match self.replay(now, connected) {
                Ok(result) => Snapshot {
                    table: Some(result.table),
                    failure: None,
                    refreshing: false,
                },
                Err(error) => Snapshot {
                    table: None,
                    failure: Some(Arc::new(error)),
                    refreshing: false,
                },
            };
            return snapshot.display(now, currency, connected);
        }
        let (snapshot, refresh) = self.begin(now, connected);
        if let Refresh::Own(owner) = refresh {
            tokio::spawn(owner.run(fetch));
        }
        snapshot.display(now, currency, connected)
    }

    fn load(&self) -> Option<Table> {
        let path = self.directory.as_ref()?.join("latest.json");
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() > 64 * 1024 {
            return None;
        }
        let table: Table = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        (table.schema == SCHEMA
            && table.revision == format!("ecb:{}", table.date)
            && table.per_euro.get("EUR") == Some(&1.0)
            && table.per_euro.iter().all(|(code, number)| {
                has_currency_code_shape(code) && number.is_finite() && *number > 0.0
            }))
        .then_some(table)
    }

    fn store(&self, table: &Table) {
        let Some(directory) = &self.directory else {
            return;
        };
        let _ = (|| -> std::io::Result<()> {
            if let Some(parent) = directory.parent() {
                crate::fs::artifact_trust::ensure_private_directory(parent)
                    .map_err(std::io::Error::other)?;
            }
            crate::fs::artifact_trust::ensure_private_directory(directory)
                .map_err(std::io::Error::other)?;
            let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
            temporary.write_all(&serde_json::to_vec(table)?)?;
            temporary
                .persist(directory.join("latest.json"))
                .map_err(|error| error.error)?;
            Ok(())
        })();
    }
}

pub(crate) fn connection_off() -> BrokerError {
    BrokerError::Execution("fx_rate reads through the Market data platform connection, which is off for this Loupist, and has no stored rates".into()).with_next("If connection_setup is available, calling it with {id: \"openbb.platform\"} shows the user the connection's setup card; otherwise the user can check the connection in Connections.")
}

fn is_offline(error: &BrokerError) -> bool {
    match error {
        BrokerError::Offline(_) => true,
        BrokerError::WithNext { source, .. } | BrokerError::WithReason { source, .. } => {
            is_offline(source)
        }
        _ => false,
    }
}

/// Reproduce a joined failure without flattening its typed refusal or next step.
pub(crate) fn shared_error(error: Arc<BrokerError>) -> BrokerError {
    match Arc::try_unwrap(error) {
        Ok(error) => error,
        Err(error) => clone_error(&error),
    }
}
fn clone_error(error: &BrokerError) -> BrokerError {
    use BrokerError::*;
    match error {
        OutboundPaused => OutboundPaused,
        UntrustedTextPaused => UntrustedTextPaused,
        ProtectedPath => ProtectedPath,
        OutsideWorkspace => OutsideWorkspace,
        CustomApprovalRequired {
            connection_id,
            tool_name,
        } => CustomApprovalRequired {
            connection_id: connection_id.clone(),
            tool_name: tool_name.clone(),
        },
        CustomApprovalRefused {
            connection_id,
            tool_name,
            cause,
        } => CustomApprovalRefused {
            connection_id: connection_id.clone(),
            tool_name: tool_name.clone(),
            cause: *cause,
        },
        ProviderApprovalRequired {
            provider,
            services,
            next,
        } => ProviderApprovalRequired {
            provider: provider.clone(),
            services: services.clone(),
            next: next.clone(),
        },
        Io(error) => Io(std::io::Error::new(error.kind(), error.to_string())),
        MethodDenied => MethodDenied,
        MemoryDisabled => MemoryDisabled,
        ProposalDisabled => ProposalDisabled,
        ConnectionDontSave => ConnectionDontSave,
        Malformed => Malformed,
        InvalidArguments { field, expected } => InvalidArguments {
            field: field.clone(),
            expected: expected.clone(),
        },
        WithNext { source, next } => WithNext {
            source: Box::new(clone_error(source)),
            next: next.clone(),
        },
        WithReason { source, reason } => clone_error(source).with_reason(reason.clone()),
        Authentication => Authentication,
        FrameTooLarge => FrameTooLarge,
        Execution(message) => Execution(message.clone()),
        PythonWorkerBusy => PythonWorkerBusy,
        EmptyResult => EmptyResult,
        ResultTooLarge { bytes, limit_bytes } => ResultTooLarge {
            bytes: *bytes,
            limit_bytes: *limit_bytes,
        },
        ProviderFailure {
            kind,
            message,
            retry_after,
            next,
        } => ProviderFailure {
            kind: *kind,
            message: message.clone(),
            retry_after: *retry_after,
            next: next.clone(),
        },
        SecContactRequired(message) => SecContactRequired(message),
        KeychainBusy => KeychainBusy,
        KeychainUnavailable => KeychainUnavailable,
        KeychainPermissionRequired => KeychainPermissionRequired,
        Offline(provider) => Offline(provider.clone()),
        BudgetExceeded { tool, limit } => BudgetExceeded {
            tool: tool.clone(),
            limit: *limit,
        },
        ArtifactLimit { limit } => ArtifactLimit { limit: *limit },
        FigureDiffers { field, figures } => FigureDiffers {
            field: field.clone(),
            figures: figures.clone(),
        },
    }
}

#[cfg(feature = "eval-harness")]
impl FxService {
    fn replay(&self, now: DateTime<Utc>, connected: bool) -> Result<Resolved, BrokerError> {
        if !connected {
            return Err(connection_off());
        }
        let request = json!({"route":RATE_ROUTE,"provider":RATE_PROVIDER});
        let response = crate::eval_harness::replay::lookup(RATE_PROVIDER, &request)
            .ok_or_else(|| {
                BrokerError::Execution("eval_replay_unmatched: ECB reference rates".into())
            })?
            .map_err(BrokerError::Execution)?;
        if response.status != 200 {
            return Err(BrokerError::Execution(format!(
                "The finance provider returned HTTP {}",
                response.status
            )));
        }
        let value: Value =
            serde_json::from_slice(&response.bytes().map_err(BrokerError::Execution)?)
                .map_err(|_| invalid_table())?;
        Ok(Resolved {
            table: Arc::new(Table::parse(&value, now)?),
            behind: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn now() -> DateTime<Utc> {
        "2026-11-17T16:00:00Z".parse().unwrap()
    }
    fn object(day: &str) -> Value {
        json!({"date":day, "USD":1.1014, "KRW":1470.23})
    }
    fn stored() -> Arc<FxService> {
        Arc::new(FxService {
            state: Mutex::new(State {
                loaded: true,
                table: Some(Arc::new(
                    Table::parse(&object("2026-11-16"), now()).unwrap(),
                )),
                ..State::default()
            }),
            ..FxService::default()
        })
    }
    #[test]
    fn parses_wrapper_drops_invalid_values_and_forces_euro() {
        let table = Table::parse(&json!({"result":{"structuredContent":{"results":{
            "date":"2026-11-16", "USD":1.1,"EUR":9,"BGN":null,"AAA":0,"BBB":-1,"usd":2,"ABCD":2,"AB":2,"CCC":"NaN","DDD":"Infinity"
        }}}}), now()).unwrap();
        assert_eq!(
            table.per_euro,
            BTreeMap::from([("EUR".into(), 1.0), ("USD".into(), 1.1)])
        );
        assert_eq!(
            Table::parse(&object("2026-11-16"), now()).unwrap().per_euro["EUR"],
            1.0
        );
        for value in [
            json!({"USD":1}),
            json!({"date":"2026-02-30","USD":1}),
            json!({"date":"2026-11-16"}),
            json!({"date":"2026-11-16","BGN":null}),
            json!({"date":"2026-11-16","USD":f64::INFINITY}),
        ] {
            assert!(Table::parse(&value, now()).is_err(), "{value}");
        }
    }
    #[test]
    fn rounded_cross_and_published_and_inverse_pairs() {
        let table = Table::parse(&object("2026-11-16"), now()).unwrap();
        let cross = table.pair("USD", "KRW").unwrap();
        assert_eq!(cross["to_per_from"], 1334.87);
        assert_eq!(cross["computed"], "cross rate through EUR");
        assert_eq!(cross["revision"], "ecb:2026-11-16");
        let direct = table.pair("EUR", "KRW").unwrap();
        assert_eq!(direct["to_per_from"], 1470.23);
        assert!(direct.get("computed").is_none());
        assert_eq!(direct["legs"], json!({"KRW_per_EUR":1470.23}));
        let inverse = table.pair("USD", "EUR").unwrap();
        assert_eq!(inverse["to_per_from"], 0.907935);
        assert_eq!(inverse["computed"], "inverse of the reference rate");
        assert_eq!(inverse["legs"], json!({"USD_per_EUR":1.1014}));
        assert!(
            matches!(table.pair("USD","USD"),Err(BrokerError::InvalidArguments {field,..}) if field=="to")
        );
        assert!(
            matches!(table.pair("ZZZ","KRW"),Err(BrokerError::InvalidArguments {field,..}) if field=="from")
        );
    }
    #[test]
    fn weekday_cutoff_and_weekend_in_frankfurt() {
        for (time, day) in [
            ("2026-11-17T15:29:59Z", "2026-11-16"),
            ("2026-11-17T15:30:00Z", "2026-11-17"),
            ("2026-11-21T20:00:00Z", "2026-11-20"),
            ("2026-11-23T10:00:00Z", "2026-11-20"),
            ("2026-07-14T14:30:00Z", "2026-07-14"),
        ] {
            assert_eq!(expected_date(time.parse().unwrap()).to_string(), day);
        }
    }
    async fn cancelled_refresh_retries(has_table: bool) {
        let service = if has_table {
            stored()
        } else {
            Arc::new(FxService::default())
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let (started, ready) = tokio::sync::oneshot::channel();
        let owner = tokio::spawn({
            let service = service.clone();
            let calls = calls.clone();
            async move {
                service
                    .resolve(now(), true, || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        started.send(()).unwrap();
                        std::future::pending::<Result<Value, BrokerError>>().await
                    })
                    .await
            }
        });
        ready.await.unwrap();
        let mut retry = tokio::spawn({
            let service = service.clone();
            let calls = calls.clone();
            async move {
                service
                    .resolve(now() + Duration::minutes(1), true, || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(object("2026-11-17"))
                    })
                    .await
            }
        });
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut retry)
                .await
                .is_err()
        );
        owner.abort();
        assert!(owner.await.unwrap_err().is_cancelled());
        let resolved = tokio::time::timeout(std::time::Duration::from_secs(2), retry)
            .await
            .unwrap()
            .unwrap()
            .expect("a cancelled fetch must allow a new read, not an invalid-table error");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            2,
            "a cancelled fetch must not reserve the retry wait"
        );
        assert_eq!(resolved.table.revision, "ecb:2026-11-17");
        assert!(resolved.behind.is_none());
    }

    #[tokio::test]
    async fn cancelled_refresh_with_stored_table_retries() {
        cancelled_refresh_retries(true).await;
    }

    #[tokio::test]
    async fn cancelled_refresh_without_stored_table_retries() {
        cancelled_refresh_retries(false).await;
    }

    #[tokio::test]
    async fn concurrent_refreshes_join_and_old_success_waits_three_hours() {
        let service = Arc::new(FxService::default());
        let calls = AtomicUsize::new(0);
        let fetch = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(object("2026-11-16"))
        };
        let (a, b) = tokio::join!(
            service.resolve(now(), true, fetch),
            service.resolve(now(), true, fetch)
        );
        assert!(a.unwrap().behind.is_none());
        assert!(b.unwrap().behind.is_none());
        service
            .resolve(now() + Duration::minutes(179), true, fetch)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        service
            .resolve(now() + Duration::hours(3), true, fetch)
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[tokio::test]
    async fn failures_keep_their_types_and_cached_reasons_until_success() {
        for kind in ["offline", "provider_failed", "connection_off"] {
            for has_table in [true, false] {
                let service = if has_table {
                    stored()
                } else {
                    Arc::new(FxService::default())
                };
                let read = || async {
                    Err(if kind == "offline" {
                        BrokerError::Offline("ecb".into()).with_next("retry later")
                    } else {
                        BrokerError::Execution("provider failed".into())
                    })
                };
                let result = service.resolve(now(), kind != "connection_off", read).await;
                if has_table {
                    let resolved = result.unwrap();
                    assert_eq!(resolved.behind.unwrap().because, kind);
                    let next = service
                        .resolve(
                            now() + Duration::minutes(1),
                            kind != "connection_off",
                            || async { panic!("retry too soon") },
                        )
                        .await
                        .unwrap();
                    assert_eq!(next.behind.unwrap().because, kind);
                    if kind != "connection_off" {
                        let recovered = service
                            .resolve(now() + Duration::hours(3), true, || async {
                                Ok(object("2026-11-16"))
                            })
                            .await
                            .unwrap();
                        assert!(recovered.behind.is_none());
                    }
                } else {
                    let again = service
                        .resolve(
                            now() + Duration::minutes(1),
                            kind != "connection_off",
                            || async { panic!("failed cold read retried too soon") },
                        )
                        .await
                        .unwrap_err();
                    assert_eq!(again.to_string(), result.as_ref().unwrap_err().to_string());
                    let error = shared_error(result.unwrap_err());
                    if kind == "offline" {
                        assert!(
                            matches!(error,BrokerError::WithNext {source,..} if matches!(*source,BrokerError::Offline(_)))
                        );
                    } else if kind == "connection_off" {
                        assert!(
                            matches!(error, BrokerError::WithNext { source, .. } if matches!(*source, BrokerError::Execution(_)))
                        );
                    } else {
                        assert!(matches!(error, BrokerError::Execution(_)));
                    }
                }
            }
        }
    }
    #[tokio::test]
    async fn late_publication_cannot_replace_a_newer_cached_day() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("fx-reference");
        let service = Arc::new(FxService::with_cache(path.clone()));
        let initial = service
            .resolve(now(), true, || async { Ok(object("2026-11-16")) })
            .await
            .unwrap();
        assert_eq!(initial.table.date.to_string(), "2026-11-16");
        let persisted = std::fs::read(path.join("latest.json")).unwrap();
        let late = service
            .resolve(now() + Duration::hours(3), true, || async {
                Ok(object("2026-11-13"))
            })
            .await
            .unwrap();
        assert_eq!(late.table.revision, initial.table.revision);
        assert!(late.behind.is_none());
        assert_eq!(persisted, std::fs::read(path.join("latest.json")).unwrap());
        assert_eq!(service.take_display_changes().len(), 1);
    }

    #[tokio::test]
    async fn private_cache_round_trip_and_unreadable_cache_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("cache/fx-reference");
        let service = Arc::new(FxService::with_cache(path.clone()));
        service
            .resolve(now(), true, || async { Ok(object("2026-11-17")) })
            .await
            .unwrap();
        let restored = Arc::new(FxService::with_cache(path.clone()));
        let table = restored
            .resolve(now(), true, || async { panic!("fresh cache") })
            .await
            .unwrap();
        assert_eq!(table.table.revision, "ecb:2026-11-17");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path.join("latest.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::write(path.join("latest.json"), b"unreadable json").unwrap();
        let ignored = Arc::new(FxService::with_cache(path));
        ignored
            .resolve(now(), true, || async { Ok(object("2026-11-17")) })
            .await
            .unwrap();
    }
}

#[cfg(all(test, feature = "eval-harness"))]
mod replay_tests {
    use super::*;
    use crate::eval_harness::replay::Replay;
    fn install(status: u16, requests: bool) -> crate::eval_harness::replay::Guard {
        let entries = if requests {
            json!([{"provider":"ecb","request":{"route":"currency_reference_rates","provider":"ecb"},"response":{"status":status,"body":{"date":"2026-11-16","USD":1.1,"KRW":1500}}}])
        } else {
            json!([])
        };
        serde_json::from_value::<Replay>(
            json!({"schema":"loupist-connection-replay/1","requests":entries}),
        )
        .unwrap()
        .install("fixture")
        .unwrap()
    }
    #[tokio::test]
    async fn replay_answers_without_fetch_or_cache_io() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("fx-reference");
        let service = Arc::new(FxService::with_cache(directory.clone()));
        let now: DateTime<Utc> = "2026-11-17T16:00:00Z".parse().unwrap();
        service
            .store(&Table::parse(&json!({"date":"2099-01-01","USD":2,"KRW":1000}), now).unwrap());
        let path = directory.join("latest.json");
        let before = std::fs::read(&path).unwrap();
        let guard = install(200, true);
        let result = service
            .resolve(now, true, || async { panic!("replay must not fetch") })
            .await
            .unwrap();
        assert_eq!(result.table.revision, "ecb:2026-11-16");
        let display = service.display(now, "KRW".into(), true, || async {
            panic!("replay display must not fetch")
        });
        assert!(
            matches!(display, FxDisplay::Ready { revision, .. } if revision == "ecb:2026-11-16")
        );
        let off = service.display(now, "KRW".into(), false, || async {
            panic!("off display must not fetch")
        });
        assert!(matches!(
            off,
            FxDisplay::Unavailable {
                reason: FxUnavailableReason::ConnectionOff,
                ..
            }
        ));
        assert!(!service.state.lock().unwrap().loaded);
        assert!(service.take_display_changes().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        drop(guard);
    }
    #[tokio::test]
    async fn replay_unmatched_and_provider_status_remain_failures() {
        let service = Arc::new(FxService::default());
        let guard = install(200, false);
        let unmatched = service
            .resolve(Utc::now(), true, || async {
                panic!("replay must not fetch")
            })
            .await
            .unwrap_err();
        assert!(unmatched.to_string().contains("eval_replay_unmatched"));
        drop(guard);
        let _guard = install(503, true);
        let failed = service
            .resolve(Utc::now(), true, || async {
                panic!("replay must not fetch")
            })
            .await
            .unwrap_err();
        assert!(failed.to_string().contains("HTTP 503"));
    }
}
