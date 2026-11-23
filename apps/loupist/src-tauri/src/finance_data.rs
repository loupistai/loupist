mod dart_events;
mod defillama;
mod entity;
pub(crate) mod fx;
mod kis;
mod krx;
pub(crate) mod openfigi;
mod percent_fields;
mod provider_common;
mod provider_korea;
mod vendor;
mod world_bank;

pub(crate) use defillama::DEFILLAMA_SOURCE_ID;
pub(crate) use entity::DartResolution;
pub(crate) use kis::KisAccountProfile;
use kis::*;
pub(crate) use percent_fields::is_percent_field;
use provider_common::*;
pub(crate) use vendor::{native_vendor, serving_vendor, VendorCredential};

/// The `dart_major_report` kind a 주요사항보고서 title names, if it is one the tool reads.
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use chrono::{DateTime, Datelike, Duration as ChronoDuration, NaiveDate, SecondsFormat, Utc};
use reqwest::{redirect::Policy, Client, RequestBuilder, StatusCode, Url};
use scraper::Html;
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::{json, Map, Value};
pub(crate) use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::sync::Mutex;

pub(crate) const WORLD_BANK_SOURCE_ID: &str = "world-bank.indicators";
const WORLD_BANK_API_ROOT: &str = "https://api.worldbank.org/v2";
pub(crate) const OPENDART_SOURCE_ID: &str = "opendart.disclosures";
pub(super) const OPENDART_API_ROOT: &str = "https://opendart.fss.or.kr";
pub(crate) const KRX_SOURCE_ID: &str = "krx.market-data";
const KRX_API_ROOT: &str = "https://data-dbg.krx.co.kr";
pub(crate) const KIS_SOURCE_ID: &str = "koreainvestment.market-data";
const KIS_REAL_API_ROOT: &str = "https://openapi.koreainvestment.com:9443";
const KIS_DEMO_API_ROOT: &str = "https://openapivts.koreainvestment.com:29443";
const KIS_TOKEN_PATH: &str = "/oauth2/tokenP";
const KIS_UPSTREAM_COMMIT: &str = "b093e42ba32d1df5f5ddad7a71cb715cbc800832";
const KIS_MANIFEST_SHA256: &str =
    "39550150b13f20bcb01838809bdf644672faa5da271e4c78afaacba31c4477ce";
const KIS_EXCLUDED_WRITE_OPERATION_IDS: [&str; 18] = [
    "domestic_bond.buy",
    "domestic_bond.order_rvsecncl",
    "domestic_bond.sell",
    "domestic_futureoption.order",
    "domestic_futureoption.order_rvsecncl",
    "domestic_stock.order_cash",
    "domestic_stock.order_credit",
    "domestic_stock.order_resv",
    "domestic_stock.order_resv_rvsecncl",
    "domestic_stock.order_rvsecncl",
    "overseas_futureoption.order",
    "overseas_futureoption.order_rvsecncl",
    "overseas_stock.daytime_order",
    "overseas_stock.daytime_order_rvsecncl",
    "overseas_stock.order",
    "overseas_stock.order_resv",
    "overseas_stock.order_resv_ccnl",
    "overseas_stock.order_rvsecncl",
];
const KIS_MANIFEST_JSON: &str = include_str!("../../marketplace/kis-read-api-v1.json");
const MAX_KIS_PARAMETER_CHARS: usize = 1_024;
const MAX_KIS_TOKEN_CACHE_ENTRIES: usize = 4;
const KIS_TOKEN_ISSUANCE_COOLDOWN: ChronoDuration = ChronoDuration::seconds(60);
const MAX_KIS_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_KIS_OPERATION_SEARCH_RESULTS: usize = 20;
const MAX_PROVIDER_BYTES: usize = 2 * 1024 * 1024;
pub(super) const MAX_LARGE_PROVIDER_BYTES: usize = 32 * 1024 * 1024;
pub(super) const MAX_DECOMPRESSED_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
const MAX_DOCUMENT_TEXT_CHARS: usize = 200_000;
const MAX_FACT_ROWS: usize = 500;
const MAX_FILING_ROWS: usize = 100;
const MAX_YEAR_SPAN: i32 = 120;

/// Waits for the provider's shared request spacing before each additional
/// upstream request inside one tool call (range dates, continuation pages).
/// The first request of a call is already paced by the tool executor.
pub(crate) type RequestPacer<'a> =
    dyn Fn() -> futures_util::future::BoxFuture<'a, ()> + Send + Sync + 'a;

/// Spacing for host-initiated reads that bypass the tool executor's pace.
pub(crate) fn fixed_request_spacing() -> futures_util::future::BoxFuture<'static, ()> {
    Box::pin(tokio::time::sleep(Duration::from_millis(500)))
}

#[derive(Debug, Error)]
pub enum FinanceDataError {
    #[error("finance data request is invalid: {0}")]
    InvalidQuery(&'static str),
    #[error("finance data request is invalid: {diagnostic}")]
    RejectedArguments {
        diagnostic: String,
        next: &'static str,
    },
    #[error("finance data provider request failed")]
    Network(#[source] reqwest::Error),
    #[error("finance data provider network request failed: {0}")]
    NetworkSafe(&'static str),
    /// The request never reached the provider: DNS, connect or no route.
    /// One provider failing does not prove the device is offline.
    #[error("finance data provider {0} could not be reached")]
    Offline(&'static str),
    /// The provider did not answer within the request's time limit.
    #[error("finance data provider {0} did not answer in time")]
    Timeout(&'static str),
    #[error("finance data provider credential was rejected: {0}")]
    CredentialRejected(&'static str),
    /// OpenDART refused the saved key; the field is its status code (010
    /// unregistered, 011 not usable, 012 this address is not allowed, 901
    /// expired with the holder's data retention period).
    #[error("OpenDART refused the saved key (status {0})")]
    DartKeyRefused(&'static str),
    /// OpenDART is stopped for maintenance (status 800).
    #[error("finance data provider {0} is stopped for maintenance")]
    ProviderMaintenance(&'static str),
    /// OpenDART's company list is still downloading for a name or ticker.
    #[error("OpenDART's company list is still being prepared")]
    DirectoryPreparing,
    /// The provider approves services separately and this key lacks some.
    /// `next` is the provider's own instruction for getting them approved.
    #[error("{provider} needs approval for {}", .services.join(", "))]
    ServiceApprovalRequired {
        provider: &'static str,
        services: Vec<&'static str>,
        next: &'static str,
    },
    #[error("KIS credential was rejected: {0}")]
    KisCredentialRejected(SafeProviderDiagnostic),
    #[error("finance data provider rate limit was reached: {0}")]
    RateLimited(&'static str),
    #[error("KIS token verification is temporarily limited: {0}")]
    KisRateLimited(SafeProviderDiagnostic),
    #[error("finance data provider returned no matching data: {0}")]
    NoData(&'static str),
    #[error("finance data provider rejected the request: {0}")]
    Provider(String),
    #[cfg(feature = "eval-harness")]
    #[error("{0}")]
    Replay(String),
    #[error("finance data provider returned an invalid response")]
    InvalidResponse,
    #[error("KIS response contract mismatch for {operation}: {detail}")]
    KisResponseContract { operation: String, detail: String },
    #[error("finance data configuration is invalid: {0}")]
    Configuration(String),
}

impl From<reqwest::Error> for FinanceDataError {
    fn from(error: reqwest::Error) -> Self {
        // Provider URLs can carry credentials in their query. Keep those out
        // of both model-facing Display text and developer Debug diagnostics.
        Self::Network(error.without_url())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SafeProviderDiagnostic {
    code: Option<String>,
    message: Option<String>,
}

impl SafeProviderDiagnostic {
    pub(crate) fn summary(&self) -> String {
        match (&self.code, &self.message) {
            (Some(code), Some(message)) => format!("{code}: {message}"),
            (Some(code), None) => code.clone(),
            (None, Some(message)) => message.clone(),
            (None, None) => "No provider diagnostic was returned.".to_owned(),
        }
    }

    pub(crate) fn bounded(code: &str, message: &str) -> Self {
        Self {
            code: Some(code.to_owned()),
            message: Some(message.to_owned()),
        }
    }
}

impl FinanceDataError {
    /// Classifies a failed send without keeping the transport error text.
    pub(crate) fn from_send(error: &reqwest::Error, provider: &'static str) -> Self {
        if error.is_timeout() {
            Self::Timeout(provider)
        } else if error.is_connect() {
            Self::Offline(provider)
        } else {
            Self::NetworkSafe(provider)
        }
    }

    /// The same failure for another waiter. A transport error keeps only its
    /// host-owned class, as `finance_error` would.
    pub(crate) fn duplicate(&self) -> Self {
        match self {
            Self::InvalidQuery(message) => Self::InvalidQuery(message),
            Self::RejectedArguments { diagnostic, next } => Self::RejectedArguments {
                diagnostic: diagnostic.clone(),
                next,
            },
            Self::Network(_) => Self::NetworkSafe(OPENDART_SOURCE_ID),
            Self::NetworkSafe(provider) => Self::NetworkSafe(provider),
            Self::Offline(provider) => Self::Offline(provider),
            Self::Timeout(provider) => Self::Timeout(provider),
            Self::CredentialRejected(provider) => Self::CredentialRejected(provider),
            Self::DartKeyRefused(status) => Self::DartKeyRefused(status),
            Self::ProviderMaintenance(provider) => Self::ProviderMaintenance(provider),
            Self::DirectoryPreparing => Self::DirectoryPreparing,
            Self::ServiceApprovalRequired {
                provider,
                services,
                next,
            } => Self::ServiceApprovalRequired {
                provider,
                services: services.clone(),
                next,
            },
            Self::KisCredentialRejected(diagnostic) => {
                Self::KisCredentialRejected(diagnostic.clone())
            }
            Self::RateLimited(provider) => Self::RateLimited(provider),
            Self::KisRateLimited(diagnostic) => Self::KisRateLimited(diagnostic.clone()),
            Self::NoData(provider) => Self::NoData(provider),
            Self::Provider(message) => Self::Provider(message.clone()),
            #[cfg(feature = "eval-harness")]
            Self::Replay(message) => Self::Replay(message.clone()),
            Self::InvalidResponse => Self::InvalidResponse,
            Self::KisResponseContract { operation, detail } => Self::KisResponseContract {
                operation: operation.clone(),
                detail: detail.clone(),
            },
            Self::Configuration(message) => Self::Configuration(message.clone()),
        }
    }
}

/// A request URL as a locator: every query parameter a native vendor declares
/// as its credential is removed, so no result, Source, citation or export can
/// carry one even when a caller passes the request URL by mistake.
pub(crate) fn public_locator(url: &str) -> String {
    let Ok(mut parsed) = Url::parse(url) else {
        return url.to_owned();
    };
    let credential = |name: &str| {
        vendor::credential_query_parameters().any(|parameter| parameter.eq_ignore_ascii_case(name))
    };
    if !parsed.query_pairs().any(|(name, _)| credential(&name)) {
        return url.to_owned();
    }
    let kept = parsed
        .query_pairs()
        .filter(|(name, _)| !credential(name))
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    parsed.set_query(None);
    if !kept.is_empty() {
        parsed.query_pairs_mut().extend_pairs(kept);
    }
    parsed.to_string()
}

/// Whether a locator still carries a native vendor's credential parameter.
pub(crate) fn locator_carries_credential(url: &str) -> bool {
    Url::parse(url).is_ok_and(|parsed| {
        parsed.query_pairs().any(|(name, _)| {
            vendor::credential_query_parameters()
                .any(|parameter| parameter.eq_ignore_ascii_case(&name))
        })
    })
}

impl std::fmt::Display for SafeProviderDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.summary())
    }
}

#[derive(Default)]
struct KisTokenCache {
    tokens: BTreeMap<String, KisAccessToken>,
    issuance_blocked_until: BTreeMap<String, DateTime<Utc>>,
}

#[derive(Clone)]
pub struct FinanceDataService {
    client: Client,
    /// The client for OpenDART, whose servers need the system's TLS (see `new`).
    dart_client: Client,
    kis_manifest: Arc<KisManifest>,
    kis_tokens: Arc<Mutex<KisTokenCache>>,
    kis_tokens_invalidated: Arc<std::sync::atomic::AtomicBool>,
    entity_directories: Arc<Mutex<entity::EntityDirectoryCache>>,
    dart_directory: entity::DartDirectorySettings,
    /// Recently read filing archives by receipt number (see `opendart_document`).
    dart_documents: Arc<Mutex<RecentDocuments>>,
    dart_fiscal_calendars: Arc<Mutex<provider_korea::DartFiscalCalendarCache>>,
    krx_daily_cache: Option<Arc<krx::KrxDailyCache>>,
}

/// Filing archives by receipt number, with when each was fetched.
type RecentDocuments = std::collections::VecDeque<(String, DateTime<Utc>, RawProviderResponse)>;

#[derive(Clone)]
struct RawProviderResponse {
    bytes: Vec<u8>,
    source_url: String,
    retrieved_at: String,
    continuation: Option<String>,
}

impl FinanceDataService {
    pub fn new() -> Result<Self, FinanceDataError> {
        let kis_manifest = Arc::new(KisManifest::parse(KIS_MANIFEST_JSON)?);
        let builder = || {
            Client::builder()
                .https_only(true)
                .redirect(Policy::none())
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(20))
                .user_agent(concat!("Loupist/", env!("CARGO_PKG_VERSION")))
        };
        let client = builder().use_rustls_tls().build()?;
        // OpenDART offers only TLS 1.2 with RSA key exchange, which rustls does not implement,
        // so its requests use the system's TLS where the app is built for macOS.
        #[cfg(target_os = "macos")]
        let dart_client = builder().use_native_tls().build()?;
        #[cfg(not(target_os = "macos"))]
        let dart_client = client.clone();
        Ok(Self {
            client,
            dart_client,
            kis_manifest,
            kis_tokens: Arc::new(Mutex::new(KisTokenCache::default())),
            kis_tokens_invalidated: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            entity_directories: Arc::new(Mutex::new(entity::EntityDirectoryCache::default())),
            dart_directory: entity::DartDirectorySettings::default(),
            dart_documents: Arc::default(),
            dart_fiscal_calendars: Arc::new(Mutex::new(
                provider_korea::DartFiscalCalendarCache::default(),
            )),
            krx_daily_cache: None,
        })
    }

    /// Keeps validated KRX daily market files under `directory`. Without it
    /// (tests, evaluation replay) every KRX read goes to the provider.
    pub fn with_krx_daily_cache(mut self, directory: PathBuf) -> Self {
        self.krx_daily_cache = krx::KrxDailyCache::open(directory).map(Arc::new);
        self
    }

    pub async fn verify_credential(
        &self,
        entry_id: &str,
        credentials: &BTreeMap<String, String>,
        provider_context: Option<&str>,
    ) -> Result<Option<crate::finance_credentials::KrxCoverage>, FinanceDataError> {
        match native_vendor(entry_id).map(|vendor| vendor.credential) {
            Some(VendorCredential::OpenDartKey) => {
                let secret = single_api_key(credentials)?;
                validate_api_key(secret)?;
                let today = Utc::now().date_naive().format("%Y%m%d").to_string();
                let mut public = fixed_url(OPENDART_API_ROOT, &["api", "list.json"])?;
                public
                    .query_pairs_mut()
                    .append_pair("bgn_de", &today)
                    .append_pair("end_de", &today)
                    .append_pair("page_count", "1");
                let mut request = public.clone();
                request.query_pairs_mut().append_pair("crtfc_key", secret);
                let raw = self
                    .fetch_provider(
                        self.dart_client.get(request),
                        OPENDART_SOURCE_ID,
                        public.to_string(),
                        MAX_PROVIDER_BYTES,
                    )
                    .await?;
                validate_dart_status_allow_no_data(&raw.bytes)?;
            }
            Some(VendorCredential::KrxKey) => {
                let secret = single_api_key(credentials)?;
                validate_api_key(secret)?;
                return self.verify_krx_coverage(secret).await.map(Some);
            }
            Some(VendorCredential::KisAppKey) => {
                let (app_key, app_secret) = kis_credentials(credentials)?;
                let environment = KisEnvironment::parse(provider_context.ok_or(
                    FinanceDataError::InvalidQuery("KIS environment is required"),
                )?)?;
                // Verification must always reach the selected authority. An
                // execution token cached for the same pair cannot promote a
                // newly staged credential candidate. Cache the freshly issued
                // token afterward so Chat does not burn the one-per-minute
                // issuance budget immediately after Save & verify.
                let token = self
                    .issue_kis_token(environment, app_key, app_secret)
                    .await?;
                self.remember_kis_access_token(environment, app_key, app_secret, token)
                    .await;
            }
            Some(VendorCredential::Keyless | VendorCredential::OptionalOpenFigiKey) | None => {
                return Err(FinanceDataError::InvalidQuery(
                    "credential provider is not supported",
                ))
            }
        }
        Ok(None)
    }

    async fn fetch_provider(
        &self,
        request: RequestBuilder,
        provider: &'static str,
        source_url: String,
        maximum_bytes: usize,
    ) -> Result<RawProviderResponse, FinanceDataError> {
        let (status, raw) = self
            .send_provider_bounded(request, provider, source_url, maximum_bytes)
            .await?;
        interpret_provider_status(provider, status, raw)
    }

    async fn send_provider_bounded(
        &self,
        request: RequestBuilder,
        provider: &'static str,
        source_url: String,
        maximum_bytes: usize,
    ) -> Result<(StatusCode, RawProviderResponse), FinanceDataError> {
        #[cfg(feature = "eval-harness")]
        let replay_key = if crate::eval_harness::replay::enabled() {
            let built = request
                .try_clone()
                .ok_or(FinanceDataError::InvalidResponse)?
                .build()?;
            let mut url = built.url().clone();
            let public_query = url
                .query_pairs()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_ref(),
                        "crtfc_key" | "api_key" | "appkey" | "appsecret" | "access_token"
                    )
                })
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect::<Vec<_>>();
            url.set_query(None);
            if !public_query.is_empty() {
                url.query_pairs_mut().extend_pairs(public_query);
            }
            let mut key = json!({"url": url.as_str()});
            if provider == KIS_SOURCE_ID {
                key["tr_id"] = json!(built.headers().get("tr_id").and_then(|v| v.to_str().ok()));
                // Continuation pages repeat the first page's URL for header-only
                // operations; first-page keys stay unchanged.
                if let Some(next) = built
                    .headers()
                    .get("tr_cont")
                    .and_then(|v| v.to_str().ok())
                    .filter(|v| !v.is_empty())
                {
                    key["tr_cont"] = json!(next);
                }
            }
            key
        } else {
            Value::Null
        };
        #[cfg(feature = "eval-harness")]
        if let Some(response) = crate::eval_harness::replay::lookup(provider, &replay_key) {
            let response = response.map_err(FinanceDataError::Replay)?;
            let bytes = response.bytes().map_err(FinanceDataError::Replay)?;
            if bytes.len() > maximum_bytes {
                return Err(FinanceDataError::Replay(
                    "eval_replay_invalid: replay response exceeds provider limit".into(),
                ));
            }
            return Ok((
                StatusCode::from_u16(response.status)
                    .map_err(|_| FinanceDataError::InvalidResponse)?,
                RawProviderResponse {
                    bytes,
                    source_url: public_locator(&source_url),
                    retrieved_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                    continuation: response.continuation,
                },
            ));
        }
        let mut response = request
            .send()
            .await
            .map_err(|error| FinanceDataError::from_send(&error, provider))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > maximum_bytes as u64)
        {
            return Err(FinanceDataError::Provider(format!(
                "{provider} response exceeded the bounded size"
            )));
        }
        let continuation = response
            .headers()
            .get("tr_cont")
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        let mut bytes = Vec::with_capacity(
            response
                .content_length()
                .unwrap_or(64 * 1024)
                .min(maximum_bytes as u64) as usize,
        );
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|error| FinanceDataError::from_send(&error, provider))?
        {
            if bytes
                .len()
                .checked_add(chunk.len())
                .is_none_or(|length| length > maximum_bytes)
            {
                return Err(FinanceDataError::Provider(format!(
                    "{provider} response exceeded the bounded size"
                )));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((
            status,
            RawProviderResponse {
                bytes,
                source_url: public_locator(&source_url),
                retrieved_at: Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
                continuation,
            },
        ))
    }
}

fn interpret_provider_status(
    provider: &'static str,
    status: StatusCode,
    raw: RawProviderResponse,
) -> Result<RawProviderResponse, FinanceDataError> {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            Err(FinanceDataError::CredentialRejected(provider))
        }
        StatusCode::TOO_MANY_REQUESTS => Err(FinanceDataError::RateLimited(provider)),
        status if !status.is_success() => {
            // These APIs can send the same diagnostic on HTTP 200 or 4xx.
            let diagnosed = match provider {
                OPENDART_SOURCE_ID if status.is_client_error() => {
                    validate_dart_document_status(&raw.bytes).err()
                }
                KRX_SOURCE_ID if status.is_client_error() => detect_krx_error(&raw.bytes).err(),
                _ => None,
            };
            Err(diagnosed
                .filter(|error| !matches!(error, FinanceDataError::InvalidResponse))
                .unwrap_or_else(|| {
                    FinanceDataError::Provider(format!(
                        "{provider} returned HTTP {}",
                        status.as_u16()
                    ))
                }))
        }
        _ => Ok(raw),
    }
}

fn strict_query<T: DeserializeOwned>(
    params: Value,
    message: &'static str,
) -> Result<T, FinanceDataError> {
    serde_json::from_value(params).map_err(|_| FinanceDataError::InvalidQuery(message))
}

pub(super) fn fixed_url(root: &str, segments: &[&str]) -> Result<Url, FinanceDataError> {
    let mut url = Url::parse(root).map_err(|_| FinanceDataError::InvalidResponse)?;
    url.path_segments_mut()
        .map_err(|_| FinanceDataError::InvalidResponse)?
        .extend(segments);
    Ok(url)
}

pub(super) fn validate_api_key(secret: &str) -> Result<(), FinanceDataError> {
    if secret.len() < 8
        || secret.len() > 512
        || secret
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err(FinanceDataError::InvalidQuery(
            "provider API key has an invalid shape",
        ));
    }
    Ok(())
}

fn single_api_key(credentials: &BTreeMap<String, String>) -> Result<&str, FinanceDataError> {
    if credentials.len() != 1 {
        return Err(FinanceDataError::InvalidQuery(
            "provider requires exactly one API key",
        ));
    }
    credentials
        .get("api_key")
        .map(String::as_str)
        .ok_or(FinanceDataError::InvalidQuery(
            "provider requires the api_key credential",
        ))
}

#[cfg(test)]
mod tests;
