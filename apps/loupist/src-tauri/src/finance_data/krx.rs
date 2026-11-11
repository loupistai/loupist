//! KRX Open API daily trading files (KOSPI and KOSDAQ), one file per market
//! and date. A quote reads the issue's row from that market-wide file; past
//! files are immutable and may be served from a bounded local cache.

use super::*;
use serde::Serialize;
use std::{collections::BTreeSet, future::Future, io::Write, time::SystemTime};

const KRX_CACHE_SCHEMA: &str = "loupist-krx-daily-cache/1";
/// Weekday dates one call may cover. Each uncached date costs one or two
/// paced provider reads.
pub(super) const MAX_KRX_RANGE_DATES: usize = 31;
const MAX_KRX_DAILY_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_KRX_CACHE_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_KRX_CACHE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_KRX_CACHE_FILES: usize = 1_024;
const KRX_CACHE_MAX_AGE: Duration = Duration::from_secs(90 * 24 * 60 * 60);
/// KRX publishes a day's file on the next business day. An empty file is
/// cached only once it can no longer be a not-yet-published day.
const KRX_EMPTY_DAY_SETTLED_DAYS: i64 = 7;
const KRX_RECORD_FIELDS: [&str; 15] = [
    "BAS_DD",
    "ISU_CD",
    "ISU_SRT_CD",
    "ISU_NM",
    "MKT_NM",
    "TDD_OPNPRC",
    "TDD_HGPRC",
    "TDD_LWPRC",
    "TDD_CLSPRC",
    "CMPPREVDD_PRC",
    "FLUC_RT",
    "ACC_TRDVOL",
    "ACC_TRDVAL",
    "MKTCAP",
    "LIST_SHRS",
];

pub(super) fn seoul_today() -> NaiveDate {
    Utc::now()
        .with_timezone(&chrono_tz::Asia::Seoul)
        .date_naive()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KrxMarket {
    Kospi,
    Kosdaq,
}

impl KrxMarket {
    fn endpoint(self) -> &'static str {
        match self {
            Self::Kospi => "stk_bydd_trd",
            Self::Kosdaq => "ksq_bydd_trd",
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Kospi => "kospi",
            Self::Kosdaq => "kosdaq",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Kospi => "KOSPI",
            Self::Kosdaq => "KOSDAQ",
        }
    }

    fn service(self) -> &'static str {
        match self {
            Self::Kospi => "KOSPI daily trading (stk_bydd_trd)",
            Self::Kosdaq => "KOSDAQ daily trading (ksq_bydd_trd)",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Kospi => Self::Kosdaq,
            Self::Kosdaq => Self::Kospi,
        }
    }

    pub(super) fn daily_url(self, date: NaiveDate) -> Result<Url, FinanceDataError> {
        let mut url = fixed_url(KRX_API_ROOT, &["svc", "apis", "sto", self.endpoint()])?;
        url.query_pairs_mut()
            .append_pair("basDd", &date.format("%Y%m%d").to_string());
        Ok(url)
    }
}

/// `None` is automatic: KOSPI first, then KOSDAQ; a resolved market is kept
/// for later dates and the other one is still checked if the row is absent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum KrxMarketChoice {
    Auto,
    Fixed(KrxMarket),
}

impl KrxMarketChoice {
    fn parse(value: Option<&str>) -> Result<Self, FinanceDataError> {
        match value {
            None | Some("auto") => Ok(Self::Auto),
            Some("kospi") => Ok(Self::Fixed(KrxMarket::Kospi)),
            Some("kosdaq") => Ok(Self::Fixed(KrxMarket::Kosdaq)),
            Some(_) => Err(FinanceDataError::InvalidQuery(
                "KRX market must be auto, kospi or kosdaq",
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fixed(market) => market.as_str(),
        }
    }

    fn order(self, resolved: Option<KrxMarket>) -> Vec<KrxMarket> {
        match (self, resolved) {
            (Self::Fixed(market), _) => vec![market],
            (Self::Auto, Some(market)) => vec![market, market.other()],
            (Self::Auto, None) => vec![KrxMarket::Kospi, KrxMarket::Kosdaq],
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct KrxMarketQuery {
    pub(super) symbol: String,
    #[serde(default)]
    pub(super) date: Option<String>,
    #[serde(default)]
    pub(super) start: Option<String>,
    #[serde(default)]
    pub(super) end: Option<String>,
    #[serde(default)]
    pub(super) market: Option<String>,
}

#[derive(Debug)]
pub(super) struct KrxRequest {
    pub(super) symbol: String,
    pub(super) choice: KrxMarketChoice,
    pub(super) first: NaiveDate,
    pub(super) last: NaiveDate,
    /// Weekdays only; KRX has no weekend sessions.
    pub(super) dates: Vec<NaiveDate>,
}

pub(super) fn validate_krx_query(
    query: KrxMarketQuery,
    today: NaiveDate,
) -> Result<KrxRequest, FinanceDataError> {
    let symbol = validate_digits(&query.symbol, 6, "KRX symbol must be six digits")?;
    let choice = KrxMarketChoice::parse(query.market.as_deref())?;
    let (first, last) = match (query.date, query.start, query.end) {
        (Some(date), None, None) => {
            let date = parse_date(&date, "KRX date is invalid")?;
            (date, date)
        }
        (None, Some(start), Some(end)) => (
            parse_date(&start, "KRX start date is invalid")?,
            parse_date(&end, "KRX end date is invalid")?,
        ),
        _ => {
            return Err(FinanceDataError::InvalidQuery(
                "pass either date or both start and end",
            ))
        }
    };
    if first > last || last > today || first.year() < 1990 {
        return Err(FinanceDataError::InvalidQuery(
            "KRX dates must be historical, from 1990, with start on or before end",
        ));
    }
    // Bound the calendar walk before counting weekdays.
    if (last - first).num_days() > (MAX_KRX_RANGE_DATES as i64) * 7 / 5 + 7 {
        return Err(FinanceDataError::InvalidQuery(
            "KRX range covers at most 31 weekdays; split the range",
        ));
    }
    let dates = first
        .iter_days()
        .take_while(|date| *date <= last)
        .filter(|date| date.weekday().number_from_monday() <= 5)
        .collect::<Vec<_>>();
    if dates.len() > MAX_KRX_RANGE_DATES {
        return Err(FinanceDataError::InvalidQuery(
            "KRX range covers at most 31 weekdays; split the range",
        ));
    }
    if dates.is_empty() {
        return Err(FinanceDataError::NoData(KRX_SOURCE_ID));
    }
    Ok(KrxRequest {
        symbol,
        choice,
        first,
        last,
        dates,
    })
}

/// One market's validated daily file, projected to the fields Loupist reads.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct KrxDailyFile {
    schema: String,
    market: String,
    date: String,
    source_url: String,
    retrieved_at: String,
    records: Vec<Map<String, Value>>,
}

impl KrxDailyFile {
    fn find(&self, symbol: &str) -> Option<&Map<String, Value>> {
        self.records.iter().find(|record| {
            record.get("ISU_SRT_CD").and_then(Value::as_str) == Some(symbol)
                || record.get("ISU_CD").and_then(Value::as_str) == Some(symbol)
        })
    }
}

pub(super) fn parse_krx_daily_file(
    market: KrxMarket,
    date: NaiveDate,
    raw: &RawProviderResponse,
) -> Result<KrxDailyFile, FinanceDataError> {
    let root = detect_krx_error(&raw.bytes)?;
    let records = root
        .get("OutBlock_1")
        .and_then(Value::as_array)
        .ok_or(FinanceDataError::InvalidResponse)?;
    let mut projected = Vec::with_capacity(records.len());
    for record in records {
        let object = record
            .as_object()
            .ok_or(FinanceDataError::InvalidResponse)?;
        let row = KRX_RECORD_FIELDS
            .iter()
            .filter_map(|field| {
                object
                    .get(*field)
                    .filter(|value| value.is_string() || value.is_number())
                    .map(|value| ((*field).to_owned(), value.clone()))
            })
            .collect::<Map<_, _>>();
        projected.push(row);
    }
    Ok(KrxDailyFile {
        schema: KRX_CACHE_SCHEMA.to_owned(),
        market: market.as_str().to_owned(),
        date: date.to_string(),
        source_url: raw.source_url.clone(),
        retrieved_at: raw.retrieved_at.clone(),
        records: projected,
    })
}

/// Past, published daily files are immutable. Today's file and recent empty
/// files (possibly not yet published) are never cached.
fn cacheable(file: &KrxDailyFile, date: NaiveDate, today: NaiveDate) -> bool {
    date < today
        && (!file.records.is_empty() || (today - date).num_days() >= KRX_EMPTY_DAY_SETTLED_DAYS)
}

/// Bounded on-disk cache of projected KRX daily files: at most 64 MiB, 1,024
/// files and 90 days per file, evicted oldest-written first. Entries that fail
/// validation are deleted and refetched.
pub(super) struct KrxDailyCache {
    directory: PathBuf,
}

impl KrxDailyCache {
    pub(super) fn open(directory: PathBuf) -> Option<Self> {
        let parent = directory.parent()?;
        crate::fs::artifact_trust::ensure_private_directory(parent).ok()?;
        crate::fs::artifact_trust::ensure_private_directory(&directory).ok()?;
        Some(Self { directory })
    }

    fn path(&self, market: KrxMarket, date: NaiveDate) -> PathBuf {
        self.directory.join(format!(
            "{}-{}.json",
            market.as_str(),
            date.format("%Y%m%d")
        ))
    }

    fn expired(modified: std::io::Result<SystemTime>, now: SystemTime) -> bool {
        // A write time after `now` is fresh; an unreadable one is not.
        modified.is_err()
            || modified.is_ok_and(|modified| {
                now.duration_since(modified)
                    .is_ok_and(|age| age > KRX_CACHE_MAX_AGE)
            })
    }

    pub(super) fn load(
        &self,
        market: KrxMarket,
        date: NaiveDate,
        now: SystemTime,
    ) -> Option<KrxDailyFile> {
        let path = self.path(market, date);
        let metadata = std::fs::symlink_metadata(&path).ok()?;
        let usable = metadata.file_type().is_file()
            && metadata.len() <= MAX_KRX_CACHE_FILE_BYTES
            && !Self::expired(metadata.modified(), now);
        let file = usable
            .then(|| std::fs::read(&path).ok())
            .flatten()
            .and_then(|bytes| serde_json::from_slice::<KrxDailyFile>(&bytes).ok())
            .filter(|file| {
                file.schema == KRX_CACHE_SCHEMA
                    && file.market == market.as_str()
                    && file.date == date.to_string()
            });
        if file.is_none() && metadata.file_type().is_file() {
            let _ = std::fs::remove_file(&path);
        }
        file
    }

    pub(super) fn store(
        &self,
        market: KrxMarket,
        date: NaiveDate,
        file: &KrxDailyFile,
        now: SystemTime,
    ) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(file).map_err(std::io::Error::other)?;
        if bytes.len() as u64 > MAX_KRX_CACHE_FILE_BYTES {
            return Ok(());
        }
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        temporary.write_all(&bytes)?;
        temporary
            .persist(self.path(market, date))
            .map_err(|error| error.error)?;
        self.evict(now)
    }

    fn evict(&self, now: SystemTime) -> std::io::Result<()> {
        self.evict_within(now, MAX_KRX_CACHE_BYTES, MAX_KRX_CACHE_FILES)
    }

    fn evict_within(
        &self,
        now: SystemTime,
        max_bytes: u64,
        max_files: usize,
    ) -> std::io::Result<()> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&self.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            if !name.to_str().is_some_and(cache_file_name) {
                continue;
            }
            // DirEntry metadata does not follow symbolic links.
            let metadata = entry.metadata()?;
            if !metadata.file_type().is_file() {
                continue;
            }
            entries.push((metadata.modified(), metadata.len(), entry.path()));
        }
        entries.sort_by_key(|(modified, _, _)| modified.as_ref().ok().copied());
        let mut bytes = entries.iter().map(|(_, length, _)| *length).sum::<u64>();
        let mut files = entries.len();
        for (modified, length, path) in entries {
            if Self::expired(modified, now) || bytes > max_bytes || files > max_files {
                std::fs::remove_file(&path)?;
                bytes = bytes.saturating_sub(length);
                files -= 1;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    fn file_count(&self) -> usize {
        std::fs::read_dir(&self.directory)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .ok()
                    .and_then(|entry| entry.file_name().to_str().map(cache_file_name))
                    .unwrap_or(false)
            })
            .count()
    }
}

fn cache_file_name(name: &str) -> bool {
    let Some((market, rest)) = name.split_once('-') else {
        return false;
    };
    matches!(market, "kospi" | "kosdaq")
        && rest
            .strip_suffix(".json")
            .is_some_and(|date| date.len() == 8 && date.bytes().all(|byte| byte.is_ascii_digit()))
}

struct KrxBar {
    market: KrxMarket,
    file: Arc<KrxDailyFile>,
    record: Map<String, Value>,
}

#[derive(Default)]
struct KrxFetchStats {
    cache_hits: usize,
    provider_reads: usize,
}

async fn daily_file<F, Fut>(
    cache: Option<&Arc<KrxDailyCache>>,
    market: KrxMarket,
    date: NaiveDate,
    today: NaiveDate,
    fetch: &F,
    pacer: &RequestPacer<'_>,
    stats: &mut KrxFetchStats,
) -> Result<Arc<KrxDailyFile>, FinanceDataError>
where
    F: Fn(KrxMarket, NaiveDate) -> Fut,
    Fut: Future<Output = Result<RawProviderResponse, FinanceDataError>>,
{
    #[cfg(feature = "eval-harness")]
    let cache = cache.filter(|_| !crate::eval_harness::replay::enabled());
    if let Some(cache) = cache.cloned() {
        let loaded =
            tokio::task::spawn_blocking(move || cache.load(market, date, SystemTime::now()))
                .await
                .ok()
                .flatten();
        if let Some(file) = loaded {
            stats.cache_hits += 1;
            return Ok(Arc::new(file));
        }
    }
    if stats.provider_reads > 0 {
        pacer().await;
    }
    stats.provider_reads += 1;
    let raw = fetch(market, date).await?;
    let file = Arc::new(parse_krx_daily_file(market, date, &raw)?);
    if let Some(cache) = cache.filter(|_| cacheable(&file, date, today)).cloned() {
        let stored = file.clone();
        // A cache write failure only costs a later provider read.
        let _ = tokio::task::spawn_blocking(move || {
            cache.store(market, date, &stored, SystemTime::now())
        })
        .await;
    }
    Ok(file)
}

pub(super) async fn collect_krx_quotes<F, Fut>(
    request: &KrxRequest,
    cache: Option<&Arc<KrxDailyCache>>,
    today: NaiveDate,
    fetch: F,
    pacer: &RequestPacer<'_>,
) -> Result<Value, FinanceDataError>
where
    F: Fn(KrxMarket, NaiveDate) -> Fut,
    Fut: Future<Output = Result<RawProviderResponse, FinanceDataError>>,
{
    let mut stats = KrxFetchStats::default();
    let mut resolved = None;
    let mut bars = Vec::new();
    let mut missing = Vec::new();
    let mut denied = Vec::new();
    for date in &request.dates {
        if !crate::market_calendar::is_trading_day("KRX", *date) {
            missing.push(*date);
            continue;
        }
        let mut found = None;
        for market in request.choice.order(resolved) {
            if denied.contains(&market) {
                continue;
            }
            let file =
                match daily_file(cache, market, *date, today, &fetch, pacer, &mut stats).await {
                    Ok(file) => file,
                    Err(FinanceDataError::CredentialRejected(KRX_SOURCE_ID)) => {
                        // KRX grants the two daily trading services independently.
                        // A denial in one market says nothing about the other.
                        denied.push(market);
                        continue;
                    }
                    Err(error) => return Err(error),
                };
            if file.records.is_empty() {
                // Both markets share the trading calendar and publication time.
                break;
            }
            if let Some(record) = file.find(&request.symbol).cloned() {
                found = Some(KrxBar {
                    market,
                    file: file.clone(),
                    record,
                });
                break;
            }
        }
        match found {
            Some(bar) => {
                if request.choice == KrxMarketChoice::Auto {
                    resolved = Some(bar.market);
                }
                bars.push((*date, bar));
            }
            None => missing.push(*date),
        }
    }
    if bars.is_empty() && !denied.is_empty() {
        return Err(approval_required(&denied));
    }
    let mut result = normalize_krx_quotes(request, &bars, &missing, &stats)?;
    if !missing.is_empty() && !denied.is_empty() {
        let warning = json!(format!(
            "Some dates could not be checked in {}. Verify this key's KRX service approvals.",
            denied
                .iter()
                .map(|market| market.service())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        result["quality"]["warnings"]
            .as_array_mut()
            .unwrap()
            .push(warning.clone());
        result["warnings"].as_array_mut().unwrap().push(warning);
    }
    Ok(result)
}

/// KRX approves each daily trading service per key, so a read can still use
/// the other market.
const APPROVAL_NEXT: &str = "The user requests the missing service approvals in the provider's portal, then verifies the key: if connection_setup is available, call it with {id: \"krx.market-data\"} so they can do it from a card; otherwise point them to Connections. Use an approved market or another source meanwhile.";

fn approval_required(markets: &[KrxMarket]) -> FinanceDataError {
    FinanceDataError::ServiceApprovalRequired {
        provider: KRX_SOURCE_ID,
        services: markets.iter().map(|market| market.service()).collect(),
        next: APPROVAL_NEXT,
    }
}

async fn verify_market_coverage<F, Fut>(
    fetch: F,
) -> Result<crate::finance_credentials::KrxCoverage, FinanceDataError>
where
    F: Fn(KrxMarket) -> Fut,
    Fut: Future<Output = Result<RawProviderResponse, FinanceDataError>>,
{
    use crate::finance_credentials::{KrxCoverage, KrxServiceAccess};
    let mut statuses = Vec::new();
    let mut denied = Vec::new();
    let mut unavailable = None;
    for market in [KrxMarket::Kospi, KrxMarket::Kosdaq] {
        let outcome = fetch(market)
            .await
            .and_then(|raw| detect_krx_error(&raw.bytes));
        statuses.push(match outcome {
            Ok(_) => KrxServiceAccess::Verified,
            Err(FinanceDataError::CredentialRejected(KRX_SOURCE_ID)) => {
                denied.push(market);
                KrxServiceAccess::NeedsApproval
            }
            Err(error) => {
                unavailable.get_or_insert(error);
                KrxServiceAccess::Unavailable
            }
        });
    }
    if !statuses.contains(&KrxServiceAccess::Verified) {
        return Err(unavailable.unwrap_or_else(|| approval_required(&denied)));
    }
    Ok(KrxCoverage {
        kospi: statuses[0],
        kosdaq: statuses[1],
    })
}

fn normalize_krx_quotes(
    request: &KrxRequest,
    found: &[(NaiveDate, KrxBar)],
    missing: &[NaiveDate],
    stats: &KrxFetchStats,
) -> Result<Value, FinanceDataError> {
    let Some((_, latest)) = found.last() else {
        return Err(FinanceDataError::NoData(KRX_SOURCE_ID));
    };
    let mut bars = Vec::with_capacity(found.len());
    let mut source_urls = BTreeSet::new();
    let mut ordered_urls = Vec::new();
    for (date, bar) in found {
        let object = &bar.record;
        if object.get("BAS_DD").and_then(Value::as_str)
            != Some(date.format("%Y%m%d").to_string().as_str())
        {
            return Err(FinanceDataError::InvalidResponse);
        }
        let field = |name: &str| {
            object
                .get(name)
                .and_then(canonical_decimal)
                .ok_or(FinanceDataError::InvalidResponse)
        };
        let optional = |name: &str| object.get(name).and_then(canonical_decimal);
        let open = field("TDD_OPNPRC")?;
        let high = field("TDD_HGPRC")?;
        let low = field("TDD_LWPRC")?;
        let close = field("TDD_CLSPRC")?;
        let volume = field("ACC_TRDVOL")?;
        validate_ohlcv_strings(&open, &high, &low, &close, &volume)?;
        bars.push(json!({
            "date": date.to_string(),
            "open": open,
            "high": high,
            "low": low,
            "close": close,
            "adjusted_close": null,
            "volume": volume,
            "change": optional("CMPPREVDD_PRC"),
            "change_percent": optional("FLUC_RT"),
            "value_traded": optional("ACC_TRDVAL"),
            "market_cap": optional("MKTCAP"),
            "listed_shares": optional("LIST_SHRS"),
            "listing_market": bar.market.label(),
            "dividend": null,
            "split_ratio": null
        }));
        if source_urls.insert(bar.file.source_url.clone()) {
            ordered_urls.push(bar.file.source_url.clone());
        }
    }
    // Cached files keep their original retrieval time; report the oldest.
    let retrieved_at = found
        .iter()
        .map(|(_, bar)| bar.file.retrieved_at.as_str())
        .min()
        .unwrap_or_default()
        .to_owned();
    let mut warnings = Vec::new();
    if !missing.is_empty() {
        warnings.push(format!(
            "KRX had no row for {} on {} weekday(s): {}. Exchange holidays, suspended trading and days not yet published have no row.",
            request.symbol,
            missing.len(),
            missing
                .iter()
                .map(NaiveDate::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let raw = RawProviderResponse {
        bytes: Vec::new(),
        source_url: ordered_urls.first().cloned().unwrap_or_default(),
        retrieved_at,
        continuation: None,
    };
    let returned = bars.len();
    let mut result = finance_result(
        "krx_daily_quote",
        "market.ohlcv",
        KRX_SOURCE_ID,
        json!({
            "provider": KRX_SOURCE_ID,
            "symbol": request.symbol,
            "market": request.choice.as_str(),
            "start": request.first.to_string(),
            "end": request.last.succ_opt().map(|next| next.to_string()),
            "end_is_exclusive": true,
            "interval": "1d",
            "adjustment": "raw"
        }),
        json!({
            "instrument": {
                "symbol": request.symbol,
                "name": latest.record.get("ISU_NM").and_then(Value::as_str),
                "exchange": "KRX",
                "market": latest.record.get("MKT_NM").and_then(Value::as_str),
                "listing_market": latest.market.label(),
                "currency": "KRW",
                "timezone": "Asia/Seoul"
            },
            "bars": bars,
            "missing_dates": missing.iter().map(NaiveDate::to_string).collect::<Vec<_>>()
        }),
        &raw,
        "Korea Exchange",
        "KRX Open API",
        true,
        "official",
        "bounded",
        if missing.is_empty() { "pass" } else { "warn" },
        json!([
            {"code": "provider_success", "status": "pass"},
            {"code": "response_complete", "status": "pass"},
            {"code": "schema_valid", "status": "pass"},
            {"code": "response_identity", "status": "pass"},
            {"code": "requested_range_coverage", "status": if missing.is_empty() { "pass" } else { "warn" }},
            {"code": "ohlc_invariants", "status": "pass"},
            {"code": "nonnegative_volume", "status": "pass"}
        ]),
        warnings,
        returned,
        returned,
        false,
    )?;
    if let Some(provenance) = result.get_mut("provenance").and_then(Value::as_object_mut) {
        provenance.insert("source_urls".into(), json!(ordered_urls));
        provenance.insert(
            "cache".into(),
            json!({"hits": stats.cache_hits, "provider_reads": stats.provider_reads}),
        );
    }
    Ok(result)
}

impl FinanceDataService {
    pub(super) async fn verify_krx_coverage(
        &self,
        api_key: &str,
    ) -> Result<crate::finance_credentials::KrxCoverage, FinanceDataError> {
        verify_market_coverage(|market: KrxMarket| async move {
            if market == KrxMarket::Kosdaq {
                fixed_request_spacing().await;
            }
            let public = market.daily_url(seoul_today())?;
            self.fetch_provider(
                self.client.get(public.clone()).header("AUTH_KEY", api_key),
                KRX_SOURCE_ID,
                public.to_string(),
                MAX_KRX_DAILY_RESPONSE_BYTES,
            )
            .await
        })
        .await
    }

    pub async fn krx_market_data(
        &self,
        params: Value,
        api_key: &str,
        pacer: &RequestPacer<'_>,
    ) -> Result<Value, FinanceDataError> {
        validate_api_key(api_key)?;
        let query: KrxMarketQuery = strict_query(params, "expected the exact KRX market schema")?;
        let today = seoul_today();
        let request = validate_krx_query(query, today)?;
        let fetch = |market: KrxMarket, date: NaiveDate| async move {
            let public_url = market.daily_url(date)?;
            self.fetch_provider(
                self.client
                    .get(public_url.clone())
                    .header("AUTH_KEY", api_key),
                KRX_SOURCE_ID,
                public_url.to_string(),
                MAX_KRX_DAILY_RESPONSE_BYTES,
            )
            .await
        };
        collect_krx_quotes(&request, self.krx_daily_cache.as_ref(), today, fetch, pacer).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    fn date(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
    }

    fn row(day: &str, symbol: &str, name: &str, market: &str, close: &str) -> Value {
        json!({
            "BAS_DD": day.replace('-', ""), "ISU_CD": symbol, "ISU_NM": name, "MKT_NM": market,
            "TDD_OPNPRC": "1,000", "TDD_HGPRC": "1,200", "TDD_LWPRC": "900", "TDD_CLSPRC": close,
            "CMPPREVDD_PRC": "-50", "FLUC_RT": "-4.76", "ACC_TRDVOL": "12,345",
            "ACC_TRDVAL": "12345000", "MKTCAP": "100000000000", "LIST_SHRS": "100000000",
            "SECT_TP_NM": "ignored"
        })
    }

    /// Fixture provider: KOSPI lists 111111; KOSDAQ lists 222222. 2026-03-02
    /// (Monday) is an exchange holiday with empty files for both markets.
    struct FixtureKrx {
        calls: StdMutex<Vec<(KrxMarket, NaiveDate)>>,
    }

    impl FixtureKrx {
        fn new() -> Self {
            Self {
                calls: StdMutex::new(Vec::new()),
            }
        }

        fn respond(
            &self,
            market: KrxMarket,
            day: NaiveDate,
        ) -> Result<RawProviderResponse, FinanceDataError> {
            self.calls.lock().unwrap().push((market, day));
            let text = day.to_string();
            let rows = if text == "2026-03-02" {
                vec![]
            } else {
                match market {
                    KrxMarket::Kospi => vec![row(&text, "111111", "코스피상사", "KOSPI", "1,100")],
                    KrxMarket::Kosdaq => vec![
                        row(&text, "333333", "다른종목", "KOSDAQ", "1,000"),
                        row(&text, "222222", "코스닥바이오", "KOSDAQ", "1,050"),
                    ],
                }
            };
            Ok(RawProviderResponse {
                bytes: serde_json::to_vec(&json!({ "OutBlock_1": rows })).unwrap(),
                source_url: market.daily_url(day).unwrap().to_string(),
                retrieved_at: "2026-10-30T00:00:00.000Z".into(),
                continuation: None,
            })
        }

        fn calls(&self) -> Vec<(KrxMarket, NaiveDate)> {
            self.calls.lock().unwrap().clone()
        }
    }

    fn request(json: Value) -> KrxRequest {
        validate_krx_query(serde_json::from_value(json).unwrap(), date("2026-10-30")).unwrap()
    }

    async fn run(
        request: &KrxRequest,
        cache: Option<&Arc<KrxDailyCache>>,
        provider: &FixtureKrx,
        paced: &AtomicUsize,
    ) -> Result<Value, FinanceDataError> {
        let pacer = || {
            paced.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {}) as futures_util::future::BoxFuture<'_, ()>
        };
        collect_krx_quotes(
            request,
            cache,
            date("2026-10-30"),
            |market, day| {
                let response = provider.respond(market, day);
                async move { response }
            },
            &pacer,
        )
        .await
    }

    #[tokio::test]
    async fn auto_market_finds_a_kosdaq_issue_after_kospi_and_keeps_the_resolution() {
        let provider = FixtureKrx::new();
        let paced = AtomicUsize::new(0);
        let request =
            request(json!({"symbol": "222222", "start": "2026-03-02", "end": "2026-03-04"}));
        let result = run(&request, None, &provider, &paced).await.unwrap();
        let bars = result["data"]["bars"].as_array().unwrap();
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0]["date"], "2026-03-03");
        assert_eq!(bars[0]["close"], "1050");
        assert_eq!(bars[0]["volume"], "12345");
        assert_eq!(bars[0]["change"], "-50");
        assert_eq!(bars[0]["listing_market"], "KOSDAQ");
        assert_eq!(result["data"]["instrument"]["listing_market"], "KOSDAQ");
        assert!(bars[0]["dividend"].is_null());
        assert!(bars[0]["split_ratio"].is_null());
        assert_eq!(result["data"]["missing_dates"], json!(["2026-03-02"]));
        assert_eq!(result["quality"]["status"], "warn");
        assert_eq!(result["query"]["market"], "auto");
        assert_eq!(result["query"]["end"], "2026-03-05");
        // The published holiday makes no request. 03-03: KOSPI miss, KOSDAQ hit.
        // 03-04: the resolved KOSDAQ first.
        assert_eq!(
            provider.calls(),
            vec![
                (KrxMarket::Kospi, date("2026-03-03")),
                (KrxMarket::Kosdaq, date("2026-03-03")),
                (KrxMarket::Kosdaq, date("2026-03-04")),
            ]
        );
        // Every provider read after the first waits for the shared spacing.
        assert_eq!(paced.load(Ordering::SeqCst), 2);
        assert_eq!(
            result["provenance"]["source_urls"],
            json!([
                "https://data-dbg.krx.co.kr/svc/apis/sto/ksq_bydd_trd?basDd=20260303",
                "https://data-dbg.krx.co.kr/svc/apis/sto/ksq_bydd_trd?basDd=20260304"
            ])
        );
    }

    #[tokio::test]
    async fn published_holidays_are_missing_without_provider_requests() {
        let provider = FixtureKrx::new();
        let paced = AtomicUsize::new(0);
        let query =
            request(json!({"symbol": "111111", "start": "2026-02-27", "end": "2026-03-03"}));
        let result = run(&query, None, &provider, &paced).await.unwrap();
        assert_eq!(result["data"]["missing_dates"], json!(["2026-03-02"]));
        assert_eq!(result["quality"]["status"], "warn");
        assert_eq!(
            provider.calls(),
            vec![
                (KrxMarket::Kospi, date("2026-02-27")),
                (KrxMarket::Kospi, date("2026-03-03"))
            ]
        );
        let holiday = request(json!({"symbol": "111111", "date": "2026-03-02"}));
        assert!(matches!(
            run(&holiday, None, &provider, &paced).await,
            Err(FinanceDataError::NoData(KRX_SOURCE_ID))
        ));
        assert_eq!(provider.calls().len(), 2);
    }

    #[tokio::test]
    async fn explicit_market_reads_one_endpoint_and_a_missing_issue_is_no_data() {
        let provider = FixtureKrx::new();
        let paced = AtomicUsize::new(0);
        let kospi = request(json!({"symbol": "222222", "date": "2026-03-03", "market": "kospi"}));
        assert!(matches!(
            run(&kospi, None, &provider, &paced).await,
            Err(FinanceDataError::NoData(KRX_SOURCE_ID))
        ));
        assert_eq!(
            provider.calls(),
            vec![(KrxMarket::Kospi, date("2026-03-03"))]
        );

        let single = request(json!({"symbol": "111111", "date": "2026-03-03"}));
        let result = run(&single, None, &provider, &paced).await.unwrap();
        assert_eq!(result["data"]["bars"][0]["close"], "1100");
        assert_eq!(result["data"]["instrument"]["market"], "KOSPI");
        assert_eq!(result["quality"]["status"], "pass");
        assert_eq!(result["page"]["returned"], 1);
    }

    #[tokio::test]
    async fn market_approval_denials_do_not_hide_the_other_market() {
        let pacer = || Box::pin(async {}) as futures_util::future::BoxFuture<'static, ()>;
        for (approved, symbol, succeeds) in [
            (KrxMarket::Kospi, "111111", true),
            (KrxMarket::Kosdaq, "222222", true),
            (KrxMarket::Kospi, "222222", false),
            (KrxMarket::Kosdaq, "111111", false),
        ] {
            let provider = FixtureKrx::new();
            let calls = std::sync::Mutex::new(Vec::new());
            let query =
                request(json!({"symbol": symbol, "start": "2026-03-03", "end": "2026-03-04"}));
            let result = collect_krx_quotes(
                &query,
                None,
                date("2026-10-30"),
                |market, day| {
                    calls.lock().unwrap().push(market);
                    let result = if market == approved {
                        provider.respond(market, day)
                    } else {
                        Err(FinanceDataError::CredentialRejected(KRX_SOURCE_ID))
                    };
                    async move { result }
                },
                &pacer,
            )
            .await;
            if succeeds {
                let result = result.unwrap();
                assert_eq!(
                    result["data"]["instrument"]["listing_market"],
                    approved.label()
                );
                assert_eq!(result["data"]["bars"].as_array().unwrap().len(), 2);
            } else {
                let error = result.unwrap_err();
                assert!(
                    matches!(&error, FinanceDataError::ServiceApprovalRequired { services, .. }
                    if services == &[approved.other().service()])
                );
                assert!(error.to_string().contains(approved.other().endpoint()));
            }
            // Do not repeat a rejected service for each date in a range.
            assert!(
                calls
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|market| **market != approved)
                    .count()
                    <= 1
            );
        }
        let fixed = request(json!({"symbol":"222222", "date":"2026-03-03", "market":"kosdaq"}));
        let error = collect_krx_quotes(
            &fixed,
            None,
            date("2026-10-30"),
            |_, _| async { Err(FinanceDataError::CredentialRejected(KRX_SOURCE_ID)) },
            &pacer,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("ksq_bydd_trd"));
    }

    #[tokio::test]
    async fn verification_checks_both_services_and_reports_independent_access() {
        use crate::finance_credentials::{KrxCoverage, KrxServiceAccess::*};
        for (approved, expected) in [
            (
                vec![KrxMarket::Kospi],
                KrxCoverage {
                    kospi: Verified,
                    kosdaq: NeedsApproval,
                },
            ),
            (
                vec![KrxMarket::Kosdaq],
                KrxCoverage {
                    kospi: NeedsApproval,
                    kosdaq: Verified,
                },
            ),
            (
                vec![KrxMarket::Kospi, KrxMarket::Kosdaq],
                KrxCoverage {
                    kospi: Verified,
                    kosdaq: Verified,
                },
            ),
        ] {
            let calls = std::sync::Mutex::new(Vec::new());
            let result = verify_market_coverage(|market| {
                calls.lock().unwrap().push(market);
                let body = if approved.contains(&market) {
                    json!({"OutBlock_1": []})
                } else {
                    json!({"result":{"code":"403", "message":"private canary"}})
                };
                async move {
                    Ok(RawProviderResponse {
                        bytes: serde_json::to_vec(&body).unwrap(),
                        source_url: String::new(),
                        retrieved_at: String::new(),
                        continuation: None,
                    })
                }
            })
            .await
            .unwrap();
            assert_eq!(result, expected);
            assert_eq!(
                *calls.lock().unwrap(),
                vec![KrxMarket::Kospi, KrxMarket::Kosdaq]
            );
        }
        let error = verify_market_coverage(|_| async {
            Err(FinanceDataError::CredentialRejected(KRX_SOURCE_ID))
        })
        .await
        .unwrap_err();
        assert!(
            matches!(error, FinanceDataError::ServiceApprovalRequired { services, .. } if services.len() == 2)
        );
        // KRX's own next step names its connection and the market fallback.
        assert!(matches!(
            approval_required(&[KrxMarket::Kosdaq]),
            FinanceDataError::ServiceApprovalRequired { provider, next, .. }
                if provider == KRX_SOURCE_ID
                    && next.contains(r#"{id: "krx.market-data"}"#)
                    && next.contains("approved market")
        ));
        let coverage = verify_market_coverage(|market| async move {
            if market == KrxMarket::Kospi {
                Ok(RawProviderResponse {
                    bytes: br#"{"OutBlock_1":[]}"#.to_vec(),
                    source_url: String::new(),
                    retrieved_at: String::new(),
                    continuation: None,
                })
            } else {
                Err(FinanceDataError::Offline(KRX_SOURCE_ID))
            }
        })
        .await
        .unwrap();
        assert_eq!(
            coverage,
            KrxCoverage {
                kospi: Verified,
                kosdaq: Unavailable
            }
        );
    }

    #[test]
    fn range_validation_skips_weekends_and_caps_the_number_of_dates() {
        let week = request(json!({"symbol": "111111", "start": "2026-02-27", "end": "2026-03-03"}));
        assert_eq!(
            week.dates,
            vec![date("2026-02-27"), date("2026-03-02"), date("2026-03-03")]
        );
        let today = date("2026-10-30");
        let invalid = |value: Value| {
            validate_krx_query(serde_json::from_value(value).unwrap(), today).unwrap_err()
        };
        // 32 weekdays.
        assert!(matches!(
            invalid(json!({"symbol": "111111", "start": "2026-01-01", "end": "2026-02-13"})),
            FinanceDataError::InvalidQuery(_)
        ));
        assert!(matches!(
            invalid(
                json!({"symbol": "111111", "date": "2026-10-30", "start": "2026-10-01", "end": "2026-10-02"})
            ),
            FinanceDataError::InvalidQuery(_)
        ));
        assert!(matches!(
            invalid(json!({"symbol": "111111", "date": "2026-10-31"})),
            FinanceDataError::InvalidQuery(_)
        ));
        assert!(matches!(
            invalid(json!({"symbol": "111111", "date": "2026-10-29", "market": "konex"})),
            FinanceDataError::InvalidQuery(_)
        ));
        assert!(matches!(
            invalid(json!({"symbol": "111111", "date": "2026-10-25"})),
            FinanceDataError::NoData(_)
        ));
        let largest =
            request(json!({"symbol": "111111", "start": "2026-01-01", "end": "2026-02-12"}));
        assert_eq!(largest.dates.len(), MAX_KRX_RANGE_DATES);
    }

    #[tokio::test]
    async fn past_daily_files_are_served_from_the_cache_and_recent_empty_days_are_not() {
        let directory = tempfile::tempdir().unwrap();
        let cache = Arc::new(KrxDailyCache::open(directory.path().join("krx-daily")).unwrap());
        let provider = FixtureKrx::new();
        let paced = AtomicUsize::new(0);
        let request =
            request(json!({"symbol": "111111", "start": "2026-03-02", "end": "2026-03-03"}));

        let first = run(&request, Some(&cache), &provider, &paced)
            .await
            .unwrap();
        assert_eq!(
            first["provenance"]["cache"],
            json!({"hits": 0, "provider_reads": 1})
        );
        // Published holidays are excluded; only the trading day's file is cached.
        assert_eq!(cache.file_count(), 1);

        let second = run(&request, Some(&cache), &provider, &paced)
            .await
            .unwrap();
        assert_eq!(
            second["provenance"]["cache"],
            json!({"hits": 1, "provider_reads": 0})
        );
        assert_eq!(provider.calls().len(), 1);
        assert_eq!(second["data"]["bars"], first["data"]["bars"]);
        assert_eq!(
            second["provenance"]["retrieved_at"],
            first["provenance"]["retrieved_at"]
        );

        // A corrupt entry is discarded and refetched.
        std::fs::write(directory.path().join("krx-daily/kospi-20260303.json"), b"{").unwrap();
        run(&request, Some(&cache), &provider, &paced)
            .await
            .unwrap();
        assert_eq!(provider.calls().len(), 2);

        let file = |records: Vec<Map<String, Value>>| KrxDailyFile {
            schema: KRX_CACHE_SCHEMA.into(),
            market: "kospi".into(),
            date: "2026-10-28".into(),
            source_url: String::new(),
            retrieved_at: String::new(),
            records,
        };
        let today = date("2026-10-30");
        let published = file(vec![Map::new()]);
        assert!(cacheable(&published, date("2026-10-29"), today));
        assert!(!cacheable(&published, today, today));
        assert!(!cacheable(&file(Vec::new()), date("2026-10-29"), today));
        assert!(cacheable(&file(Vec::new()), date("2026-10-20"), today));
    }

    #[test]
    fn cache_eviction_drops_expired_and_oldest_files_beyond_the_bound() {
        let directory = tempfile::tempdir().unwrap();
        let cache = KrxDailyCache::open(directory.path().join("krx-daily")).unwrap();
        let file = KrxDailyFile {
            schema: KRX_CACHE_SCHEMA.into(),
            market: "kospi".into(),
            date: "2026-03-03".into(),
            source_url: "https://data-dbg.krx.co.kr/svc/apis/sto/stk_bydd_trd?basDd=20260303"
                .into(),
            retrieved_at: "2026-10-30T00:00:00.000Z".into(),
            records: Vec::new(),
        };
        let now = SystemTime::now();
        cache
            .store(KrxMarket::Kospi, date("2026-03-03"), &file, now)
            .unwrap();
        assert!(cache
            .load(KrxMarket::Kospi, date("2026-03-03"), now)
            .is_some());
        // Unrelated names in the directory are left alone.
        std::fs::write(directory.path().join("krx-daily/notes.txt"), b"keep").unwrap();
        let later = now + KRX_CACHE_MAX_AGE + Duration::from_secs(60);
        assert!(cache
            .load(KrxMarket::Kospi, date("2026-03-03"), later)
            .is_none());
        assert_eq!(cache.file_count(), 0);
        cache
            .store(KrxMarket::Kosdaq, date("2026-03-04"), &file, now)
            .unwrap();
        cache.evict(later).unwrap();
        assert_eq!(cache.file_count(), 0);
        assert!(directory.path().join("krx-daily/notes.txt").exists());

        // Beyond the file bound, the oldest-written entries go first.
        for (day, age) in [("2026-03-05", 30), ("2026-03-06", 10), ("2026-03-09", 20)] {
            cache
                .store(KrxMarket::Kospi, date(day), &file, now)
                .unwrap();
            std::fs::File::options()
                .write(true)
                .open(cache.path(KrxMarket::Kospi, date(day)))
                .unwrap()
                .set_modified(now - Duration::from_secs(age))
                .unwrap();
        }
        cache.evict_within(now, u64::MAX, 2).unwrap();
        assert_eq!(cache.file_count(), 2);
        assert!(!cache.path(KrxMarket::Kospi, date("2026-03-05")).exists());
        cache.evict_within(now, 0, usize::MAX).unwrap();
        assert_eq!(cache.file_count(), 0);
    }
}
