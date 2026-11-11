pub(crate) mod browser;
pub(crate) mod document;
pub(crate) mod external_browser;
pub(crate) mod favicon;
mod official;
mod search_provider;

pub use search_provider::{
    execute_search, SearchAttemptReceipt, SearchBackend, SearchCancel, SearchDroppedCounts,
    SearchHits, SearchProviderId, SearchRequest, WebSearchPolicy,
};

use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::OnceLock,
    time::{Duration, Instant},
};

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use encoding_rs::Encoding;
use futures_util::StreamExt;
use reqwest::{
    header::{ACCEPT, CONTENT_LENGTH, CONTENT_TYPE, LOCATION, USER_AGENT},
    redirect::Policy,
    Client, Method, StatusCode, Url,
};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::net::lookup_host;
use tokio::sync::Semaphore;

const EXA_MCP_URL: &str = "https://mcp.exa.ai/mcp?tools=web_search_exa";
pub(crate) const SEARCH_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const SEARCH_RETRY_DELAY: Duration = Duration::from_millis(750);
const FETCH_TIMEOUT: Duration = Duration::from_secs(15);
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_REDIRECTS: usize = 5;
const MAX_SEARCH_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_FETCH_RESPONSE_BYTES: usize = 20 * 1024 * 1024;
const MAX_FETCH_PAGE_BYTES: usize = 16 * 1024;
pub const MAX_FETCH_EXTRACTED_BYTES: usize = crate::web::document::MAX_EXTRACTED_MARKDOWN_BYTES;
const MAX_ALTERNATES: usize = 8;
const FETCH_RETRY_AFTER_CAP: Duration = Duration::from_secs(3);
const FETCH_ACCEPT: &str = "text/markdown, text/plain;q=0.9, text/html;q=0.8, application/xhtml+xml;q=0.8, text/csv;q=0.7, text/tab-separated-values;q=0.7, application/json;q=0.7, application/ld+json;q=0.7, application/x-ndjson;q=0.7, application/jsonl;q=0.7, application/xml;q=0.7, text/xml;q=0.7, application/yaml;q=0.7, application/x-yaml;q=0.7, application/pdf;q=0.6, application/zip;q=0.6, application/octet-stream;q=0.5, application/vnd.openxmlformats-officedocument.wordprocessingml.document;q=0.6, application/vnd.openxmlformats-officedocument.spreadsheetml.sheet;q=0.6, application/vnd.openxmlformats-officedocument.presentationml.presentation;q=0.6, application/msword;q=0.6, application/vnd.ms-excel;q=0.6, application/vnd.ms-powerpoint;q=0.6";
const FETCH_OFFICIAL_ACCEPT: &str = "application/json, application/ld+json;q=0.9, */*;q=0.1";
const JS_REQUIRED_PHRASES: &[&str] = &[
    "enable javascript",
    "javascript required",
    "turn on javascript",
    "please enable javascript",
    "browser not supported",
    "requires javascript",
];

#[derive(Debug, Error)]
pub enum WebError {
    #[cfg(feature = "eval-harness")]
    #[error("{0}")]
    Replay(String),
    #[error("web request URL is not allowed")]
    UrlDenied,
    #[error("web request resolved to a non-public address")]
    AddressDenied,
    #[error("web request could not resolve its host")]
    Resolution,
    #[error("web request timed out; try the source again once or choose another source")]
    Timeout,
    #[error("web request transport failed; try the source again once or choose another source")]
    Transport,
    /// The request never reached its host: connect failed or no route. One
    /// host failing does not prove the device is offline.
    #[error("web request could not reach {0}")]
    Offline(String),
    #[error("public web provider is rate-limited; try again shortly")]
    RateLimited { retry_after: Option<Duration> },
    #[error("web search was cancelled")]
    Cancelled,
    #[error("web search provider is unavailable")]
    ProviderUnavailable,
    #[error("{0}")]
    ProviderMessage(String),
    #[error("web provider returned HTTP status {0}")]
    ProviderStatus(u16),
    #[error("web provider request failed")]
    Provider,
    #[error("web provider returned an invalid response")]
    InvalidProviderResponse,
    #[error("web response exceeded its size limit")]
    ResponseTooLarge,
    #[error("web response content type is not supported")]
    UnsupportedContent,
    #[error("web page answered with a bot challenge instead of its content; read another source or the publisher's own document")]
    Challenge,
    #[error("web response bytes do not match the declared content type")]
    TypeMismatch,
    #[error("web document has no extractable text layer")]
    NoTextLayer,
    #[error("web document content offset is outside the extracted text")]
    InvalidContentOffset,
    #[error("web request exceeded its redirect limit")]
    RedirectLimit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebRecency {
    Day,
    Week,
    Month,
    Year,
}

impl WebRecency {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "day" => Some(Self::Day),
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            "year" => Some(Self::Year),
            _ => None,
        }
    }

    fn start_published_date(self) -> NaiveDate {
        let days = match self {
            Self::Day => 1,
            Self::Week => 7,
            Self::Month => 30,
            Self::Year => 365,
        };
        Utc::now().date_naive() - chrono::Duration::days(days)
    }
}

#[derive(Clone, Debug)]
pub struct WebSearchQuery {
    pub query: String,
    pub limit: u8,
    pub recency: Option<WebRecency>,
    pub include_domains: Vec<String>,
    pub exclude_domains: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebSource {
    pub source_id: String,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WebSearchOutput {
    pub selected_provider: String,
    pub retrieved_at: String,
    pub untrusted_content: bool,
    pub results: Vec<WebSourceOutput>,
    pub attempts: Vec<SearchAttemptReceipt>,
    pub received_count: usize,
    pub returned_count: usize,
    pub dropped: SearchDroppedCounts,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct WebSourceOutput {
    pub source_id: String,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub published_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct WebAlternate {
    pub url: String,
    pub kind: &'static str,
}

#[derive(Debug, Serialize)]
pub struct WebFetchOutput {
    pub source_id: String,
    pub content_sha256: String,
    pub title: String,
    pub url: String,
    pub requested_url: String,
    pub representation_url: String,
    pub final_url: String,
    pub status: u16,
    pub content_type: String,
    pub charset: Option<String>,
    pub document_kind: String,
    pub extraction_method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representation_provider: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    pub page_count: Option<u32>,
    pub content: String,
    pub content_offset: usize,
    pub content_bytes: usize,
    pub extracted_bytes: usize,
    pub next_offset: Option<usize>,
    pub extraction_truncated: bool,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub quality_warnings: Vec<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternates: Vec<WebAlternate>,
    pub untrusted_content: bool,
    pub content_class: &'static str,
    pub retrieved_at: String,
    pub published_at: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct WebFetchSnapshot {
    source_id: String,
    content_sha256: String,
    title: String,
    url: String,
    requested_url: String,
    representation_url: String,
    final_url: String,
    status: u16,
    content_type: String,
    charset: Option<String>,
    document_kind: String,
    extraction_method: &'static str,
    representation_provider: Option<&'static str>,
    record_id: Option<String>,
    page_count: Option<u32>,
    content: String,
    extraction_truncated: bool,
    quality_warnings: Vec<&'static str>,
    alternates: Vec<WebAlternate>,
    retrieved_at: String,
    published_at: Option<String>,
}

impl WebFetchSnapshot {
    pub(crate) fn page(&self, content_offset: usize) -> Result<WebFetchOutput, WebError> {
        let (content, next_offset) = content_page(&self.content, content_offset)?;
        Ok(self.output(content, content_offset, next_offset))
    }

    pub(crate) fn matches_source(&self, source_id: &str, content_sha256: &str) -> bool {
        self.source_id == source_id && self.content_sha256 == content_sha256
    }

    /// Select exact bytes from the already-acquired representation. A literal
    /// only locates text; it can never replace or supply the returned content.
    pub(crate) fn literal_passage(
        &self,
        literal: &str,
        occurrence: usize,
    ) -> Option<(WebFetchOutput, usize)> {
        const PASSAGE_BYTES: usize = 8 * 1024;
        const LEADING_CONTEXT_BYTES: usize = 2 * 1024;
        if literal.is_empty() || literal.len() > PASSAGE_BYTES - LEADING_CONTEXT_BYTES {
            return None;
        }
        let (match_offset, _) = self.content.match_indices(literal).nth(occurrence)?;
        let mut start = match_offset.saturating_sub(LEADING_CONTEXT_BYTES);
        while !self.content.is_char_boundary(start) {
            start += 1;
        }
        let mut end = start.saturating_add(PASSAGE_BYTES).min(self.content.len());
        while !self.content.is_char_boundary(end) {
            end -= 1;
        }
        let mut output = self.output(self.content[start..end].to_owned(), start, None);
        // This selection is not a pagination cursor and never grants a new
        // arbitrary offset for the ordinary continuation path.
        output.truncated = self.extraction_truncated || start > 0 || end < self.content.len();
        Some((output, match_offset))
    }

    fn output(
        &self,
        content: String,
        content_offset: usize,
        next_offset: Option<usize>,
    ) -> WebFetchOutput {
        let content_bytes = content.len();
        WebFetchOutput {
            source_id: self.source_id.clone(),
            content_sha256: self.content_sha256.clone(),
            title: self.title.clone(),
            url: self.url.clone(),
            requested_url: self.requested_url.clone(),
            representation_url: self.representation_url.clone(),
            final_url: self.final_url.clone(),
            status: self.status,
            content_type: self.content_type.clone(),
            charset: self.charset.clone(),
            document_kind: self.document_kind.clone(),
            extraction_method: self.extraction_method,
            representation_provider: self.representation_provider,
            record_id: self.record_id.clone(),
            page_count: self.page_count,
            content,
            content_offset,
            content_bytes,
            extracted_bytes: self.content.len(),
            next_offset,
            extraction_truncated: self.extraction_truncated,
            truncated: self.extraction_truncated || next_offset.is_some(),
            quality_warnings: self.quality_warnings.clone(),
            alternates: self.alternates.clone(),
            untrusted_content: true,
            content_class: "untrusted_web",
            retrieved_at: self.retrieved_at.clone(),
            published_at: self.published_at.clone(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct JsonSearchResponse {
    results: Vec<JsonSearchResult>,
}

#[derive(Debug, Deserialize)]
struct JsonSearchResult {
    title: Option<String>,
    url: Option<String>,
    text: Option<String>,
    highlights: Option<Vec<String>>,
    #[serde(rename = "publishedDate")]
    published_date: Option<String>,
}

pub(crate) async fn search_exa_once(query: &WebSearchQuery) -> Result<SearchHits, WebError> {
    let _permit = web_search_semaphore()
        .acquire()
        .await
        .map_err(|_| WebError::Provider)?;
    let endpoint = Url::parse(EXA_MCP_URL).map_err(|_| WebError::UrlDenied)?;
    // Exa's anonymous `web_search_exa` schema currently accepts only query and
    // numResults. Express optional filters in the discovery query, then enforce
    // every check that can be proven from returned URL/date metadata locally.
    let arguments = json!({
        "query": adapted_search_query(query),
        "numResults": provider_result_limit(query)
    });
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "web_search_exa",
            "arguments": arguments
        }
    });
    let response = request_bounded(
        Method::POST,
        endpoint,
        Some(body),
        SEARCH_TIMEOUT,
        MAX_SEARCH_RESPONSE_BYTES,
        "application/json, text/event-stream",
    )
    .await?;
    if !response.status.is_success() {
        return Err(status_error_with_retry(
            response.status,
            response.retry_after,
        ));
    }
    let text = String::from_utf8(response.bytes).map_err(|_| WebError::InvalidProviderResponse)?;
    let provider_text = mcp_text(&text)?;
    let mut sources = parse_search_results(&provider_text)?;
    let mut seen = HashSet::new();
    sources.retain(|source| seen.insert(source.url.clone()));
    Ok(SearchHits {
        sources,
        model: None,
        search_request_count: None,
    })
}

fn status_error_with_retry(status: StatusCode, retry_after: Option<Duration>) -> WebError {
    if status == StatusCode::TOO_MANY_REQUESTS {
        WebError::RateLimited { retry_after }
    } else {
        status_error(status)
    }
}

fn web_search_semaphore() -> &'static Semaphore {
    static SEMAPHORE: OnceLock<Semaphore> = OnceLock::new();
    SEMAPHORE.get_or_init(|| Semaphore::new(1))
}

fn retryable_search_error(error: &WebError) -> bool {
    match error {
        WebError::Resolution
        | WebError::Timeout
        | WebError::Transport
        | WebError::Offline(_)
        | WebError::RateLimited { .. } => true,
        WebError::ProviderStatus(status) => {
            matches!(*status, 408 | 425) || (500..=599).contains(status)
        }
        _ => false,
    }
}

pub(crate) fn fallbackable_search_error(error: &WebError) -> bool {
    retryable_search_error(error)
        || matches!(
            error,
            WebError::Provider
                | WebError::ProviderMessage(_)
                | WebError::ProviderStatus(_)
                | WebError::ProviderUnavailable
                | WebError::InvalidProviderResponse
        )
}

fn status_error(status: StatusCode) -> WebError {
    if status == StatusCode::TOO_MANY_REQUESTS {
        WebError::RateLimited { retry_after: None }
    } else {
        WebError::ProviderStatus(status.as_u16())
    }
}

pub(crate) fn retry_after_from_error(error: &WebError) -> Option<Duration> {
    match error {
        WebError::RateLimited { retry_after } => *retry_after,
        _ => None,
    }
}

fn classify_transport_error(error: reqwest::Error) -> WebError {
    if error.is_connect() && !error.is_timeout() {
        let host = error.url().and_then(|url| url.host_str());
        WebError::Offline(host.unwrap_or("the web source").to_owned())
    } else if error.is_timeout() {
        WebError::Timeout
    } else {
        WebError::Transport
    }
}

fn provider_result_limit(query: &WebSearchQuery) -> u8 {
    if query.recency.is_some()
        || !query.include_domains.is_empty()
        || !query.exclude_domains.is_empty()
    {
        10
    } else {
        query.limit
    }
}

fn adapted_search_query(query: &WebSearchQuery) -> String {
    let mut adapted = query.query.trim().to_owned();
    if let Some(recency) = query.recency {
        adapted.push_str(". Prefer sources published on or after ");
        adapted.push_str(&recency.start_published_date().to_string());
    }
    if !query.include_domains.is_empty() {
        adapted.push_str(". Include only sources from: ");
        adapted.push_str(&query.include_domains.join(", "));
    }
    if !query.exclude_domains.is_empty() {
        adapted.push_str(". Exclude sources from: ");
        adapted.push_str(&query.exclude_domains.join(", "));
    }
    adapted
}

pub(crate) fn apply_search_filters_counted(
    query: &WebSearchQuery,
    sources: &mut Vec<WebSource>,
) -> SearchDroppedCounts {
    let mut dropped = SearchDroppedCounts::default();
    sources.retain(|source| {
        let Ok(url) = Url::parse(&source.url) else {
            dropped.invalid_url += 1;
            return false;
        };
        if validate_source_url(url.as_str()).is_err() {
            dropped.invalid_url += 1;
            return false;
        }
        let Some(host) = url.host_str() else {
            dropped.invalid_url += 1;
            return false;
        };
        let included = query.include_domains.is_empty()
            || query
                .include_domains
                .iter()
                .any(|domain| host_matches_domain(host, domain));
        if !included {
            dropped.include_domain += 1;
            return false;
        }
        let excluded = query
            .exclude_domains
            .iter()
            .any(|domain| host_matches_domain(host, domain));
        if excluded {
            dropped.exclude_domain += 1;
            return false;
        }
        let recent = query.recency.is_none_or(|recency| {
            source
                .published_at
                .as_deref()
                .and_then(parse_published_date)
                .is_none_or(|published| published >= recency.start_published_date())
        });
        if !recent {
            dropped.recency += 1;
            return false;
        }
        true
    });
    dropped
}

fn host_matches_domain(host: &str, domain: &str) -> bool {
    host == domain
        || host
            .strip_suffix(domain)
            .is_some_and(|prefix| prefix.ends_with('.'))
}

fn parse_published_date(value: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|date| date.date_naive())
        .or_else(|| {
            value
                .get(..10)
                .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        })
}

pub(crate) async fn fetch(source: &WebSource) -> Result<WebFetchSnapshot, WebError> {
    let requested = validate_source_url(&source.url)?;
    if let Some(target) = official::match_official(&requested) {
        match fetch_official(source, &target).await {
            Ok(snapshot) => return Ok(snapshot),
            Err(error) if official_allows_generic_fallback(&error) => {
                let mut snapshot = fetch_generic(source, &requested).await?;
                push_warning(&mut snapshot.quality_warnings, "official_fallback");
                return Ok(snapshot);
            }
            Err(error) => return Err(error),
        }
    }
    fetch_generic(source, &requested).await
}

fn official_allows_generic_fallback(error: &WebError) -> bool {
    matches!(
        error,
        WebError::Resolution
            | WebError::Timeout
            | WebError::Transport
            | WebError::Offline(_)
            | WebError::RateLimited { .. }
            | WebError::ProviderStatus(_)
            | WebError::Provider
            | WebError::ProviderUnavailable
            | WebError::InvalidProviderResponse
            | WebError::UnsupportedContent
            | WebError::TypeMismatch
            | WebError::NoTextLayer
    )
}

async fn fetch_official(
    source: &WebSource,
    target: &official::OfficialTarget,
) -> Result<WebFetchSnapshot, WebError> {
    let response =
        get_with_429_retry(target.representation_url.clone(), FETCH_OFFICIAL_ACCEPT).await?;
    if !response.status.is_success() {
        return Err(status_error_with_retry(
            response.status,
            response.retry_after,
        ));
    }
    let value: Value =
        serde_json::from_slice(&response.bytes).map_err(|_| WebError::InvalidProviderResponse)?;
    let record_id =
        official::verified_record_id(target, &value).ok_or(WebError::InvalidProviderResponse)?;
    let markdown =
        official::project_markdown(target, &value).ok_or(WebError::InvalidProviderResponse)?;
    if markdown.trim().is_empty() {
        return Err(WebError::NoTextLayer);
    }
    let (content, extraction_truncated) = truncate_utf8(markdown.trim(), MAX_FETCH_EXTRACTED_BYTES);
    let content_sha256 = hex::encode(Sha256::digest(&response.bytes));
    Ok(WebFetchSnapshot {
        source_id: source.source_id.clone(),
        content_sha256,
        title: markdown_heading(&content).unwrap_or_else(|| source.title.clone()),
        url: source.url.clone(),
        requested_url: source.url.clone(),
        representation_url: target.representation_url.to_string(),
        final_url: response.final_url.to_string(),
        status: response.status.as_u16(),
        content_type: media_type(&response.content_type),
        charset: Some("utf-8".into()),
        document_kind: "json".into(),
        extraction_method: "official_api",
        representation_provider: Some(target.provider.as_str()),
        record_id: Some(record_id.clone()),
        page_count: None,
        content: content.clone(),
        extraction_truncated,
        quality_warnings: quality_warnings(&content, false),
        alternates: Vec::new(),
        retrieved_at: now_rfc3339(),
        published_at: source.published_at.clone(),
    })
}

async fn fetch_generic(source: &WebSource, requested: &Url) -> Result<WebFetchSnapshot, WebError> {
    let response = get_with_429_retry(requested.clone(), FETCH_ACCEPT).await?;
    if response.challenge {
        return Err(WebError::Challenge);
    }
    if !response.status.is_success() {
        return Err(status_error_with_retry(
            response.status,
            response.retry_after,
        ));
    }
    let content_type = media_type(&response.content_type);
    let content_sha256 = hex::encode(Sha256::digest(&response.bytes));
    let bytes = response.bytes;
    let raw_content_type = response.content_type.clone();
    let decode_content_type = content_type.clone();
    let fallback_title = source.title.clone();
    let base_url = response.final_url.clone();
    let decoded = tokio::time::timeout(
        EXTRACT_TIMEOUT,
        tokio::task::spawn_blocking(move || {
            decode_fetched_content(
                &bytes,
                &raw_content_type,
                &decode_content_type,
                &fallback_title,
                &base_url,
            )
        }),
    )
    .await
    .map_err(|_| WebError::Timeout)?
    .map_err(|_| WebError::UnsupportedContent)??;
    Ok(WebFetchSnapshot {
        source_id: source.source_id.clone(),
        content_sha256,
        title: decoded.title,
        url: source.url.clone(),
        requested_url: source.url.clone(),
        representation_url: response.final_url.to_string(),
        final_url: response.final_url.to_string(),
        status: response.status.as_u16(),
        content_type,
        charset: decoded.charset,
        document_kind: decoded.document_kind,
        extraction_method: decoded.extraction_method,
        representation_provider: None,
        record_id: None,
        page_count: decoded.page_count,
        content: decoded.content.clone(),
        extraction_truncated: decoded.extraction_truncated,
        quality_warnings: {
            let mut warnings =
                quality_warnings(&decoded.content, decoded.extraction_method == "readability");
            for warning in decoded.warnings.iter().copied() {
                push_warning(&mut warnings, warning);
            }
            warnings
        },
        alternates: decoded.alternates,
        retrieved_at: now_rfc3339(),
        published_at: source.published_at.clone().or(decoded.published_at),
    })
}

async fn get_with_429_retry(url: Url, accept: &str) -> Result<BoundedResponse, WebError> {
    let started = Instant::now();
    let response = request_bounded(
        Method::GET,
        url.clone(),
        None,
        FETCH_TIMEOUT,
        MAX_FETCH_RESPONSE_BYTES,
        accept,
    )
    .await?;
    if response.status != StatusCode::TOO_MANY_REQUESTS {
        return Ok(response);
    }
    let remaining = FETCH_TIMEOUT.saturating_sub(started.elapsed());
    let Some(delay) = clamp_retry_after(
        response.retry_after.unwrap_or(Duration::from_secs(1)),
        remaining,
    ) else {
        return Err(WebError::RateLimited {
            retry_after: response.retry_after,
        });
    };
    tokio::time::sleep(delay).await;
    let remaining = FETCH_TIMEOUT.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(WebError::Timeout);
    }
    request_bounded(
        Method::GET,
        url,
        None,
        remaining,
        MAX_FETCH_RESPONSE_BYTES,
        accept,
    )
    .await
}

fn media_type(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

fn push_warning(warnings: &mut Vec<&'static str>, warning: &'static str) {
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

fn markdown_heading(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    })
}

struct BoundedResponse {
    status: StatusCode,
    /// `cf-mitigated: challenge`: the body is a bot challenge, not the page.
    challenge: bool,
    content_type: String,
    final_url: Url,
    bytes: Vec<u8>,
    retry_after: Option<Duration>,
}

async fn request_bounded(
    method: Method,
    mut url: Url,
    json_body: Option<Value>,
    timeout: Duration,
    max_bytes: usize,
    accept: &str,
) -> Result<BoundedResponse, WebError> {
    #[cfg(feature = "eval-harness")]
    if let Some(response) = crate::eval_harness::replay::lookup(
        "web",
        &json!({"method": method.as_str(), "url": url.as_str(), "body": json_body}),
    ) {
        let response = response.map_err(WebError::Replay)?;
        let bytes = response.bytes().map_err(WebError::Replay)?;
        if bytes.len() > max_bytes {
            return Err(WebError::Replay("replay response exceeds web limit".into()));
        }
        return Ok(BoundedResponse {
            status: StatusCode::from_u16(response.status).map_err(|_| WebError::Provider)?,
            challenge: false,
            content_type: response.content_type,
            final_url: url,
            bytes,
            retry_after: None,
        });
    }
    // One budget covers every redirect hop and the body, so a redirect chain
    // cannot multiply the caller's timeout.
    let deadline = Instant::now() + timeout;
    for redirect_count in 0..=MAX_REDIRECTS {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(WebError::Timeout);
        }
        #[cfg(test)]
        if let Some(scripted) = script::take(&url, accept) {
            if (300..400).contains(&scripted.status) && scripted.status != 304 {
                if method != Method::GET {
                    return Err(WebError::Provider);
                }
                if redirect_count == MAX_REDIRECTS {
                    return Err(WebError::RedirectLimit);
                }
                let location = scripted.location.ok_or(WebError::Provider)?;
                url = url.join(&location).map_err(|_| WebError::UrlDenied)?;
                validate_url_shape(&url)?;
                continue;
            }
            return Ok(BoundedResponse {
                status: StatusCode::from_u16(scripted.status).map_err(|_| WebError::Provider)?,
                challenge: scripted.challenge,
                content_type: scripted.content_type,
                final_url: url,
                bytes: scripted.body,
                retry_after: parse_retry_after_header(
                    scripted
                        .retry_after
                        .as_deref()
                        .and_then(|value| reqwest::header::HeaderValue::from_str(value).ok())
                        .as_ref(),
                ),
            });
        }
        let (host, addresses) = resolve_public(&url).await?;
        let client = Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(5).min(remaining))
            .timeout(remaining)
            .resolve_to_addrs(&host, &addresses)
            .build()
            .map_err(|_| WebError::Provider)?;
        let mut request = client
            .request(method.clone(), url.clone())
            .header(USER_AGENT, "Loupist-Web/1.0 (+pi-web-access-compatible)")
            .header(ACCEPT, accept);
        if let Some(body) = &json_body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(classify_transport_error)?;
        if response.status().is_redirection() {
            if method != Method::GET {
                return Err(WebError::Provider);
            }
            if redirect_count == MAX_REDIRECTS {
                return Err(WebError::RedirectLimit);
            }
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or(WebError::Provider)?;
            url = url.join(location).map_err(|_| WebError::UrlDenied)?;
            validate_url_shape(&url)?;
            continue;
        }
        if response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length > max_bytes)
        {
            return Err(WebError::ResponseTooLarge);
        }
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let status = response.status();
        let retry_after =
            parse_retry_after_header(response.headers().get(reqwest::header::RETRY_AFTER));
        let challenge = response
            .headers()
            .get("cf-mitigated")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("challenge"));
        let final_url = response.url().clone();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(classify_transport_error)?;
            if bytes.len().saturating_add(chunk.len()) > max_bytes {
                return Err(WebError::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        return Ok(BoundedResponse {
            status,
            challenge,
            content_type,
            final_url,
            bytes,
            retry_after,
        });
    }
    Err(WebError::RedirectLimit)
}

async fn resolve_public(url: &Url) -> Result<(String, Vec<SocketAddr>), WebError> {
    validate_url_shape(url)?;
    let host = url.host_str().ok_or(WebError::UrlDenied)?.to_owned();
    let port = url.port_or_known_default().ok_or(WebError::UrlDenied)?;
    let addresses = lookup_host((host.as_str(), port))
        .await
        .map_err(|_| WebError::Resolution)?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(WebError::Resolution);
    }
    if addresses.iter().any(|address| !is_public_ip(address.ip())) {
        return Err(WebError::AddressDenied);
    }
    Ok((host, addresses))
}

pub fn validate_source_url(value: &str) -> Result<Url, WebError> {
    let url = Url::parse(value).map_err(|_| WebError::UrlDenied)?;
    validate_url_shape(&url)?;
    Ok(url)
}

fn validate_url_shape(url: &Url) -> Result<(), WebError> {
    let port_is_allowed = matches!(
        (url.scheme(), url.port()),
        ("http", None | Some(80)) | ("https", None | Some(443))
    );
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
        || !port_is_allowed
    {
        return Err(WebError::UrlDenied);
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_ipv4(ip),
        IpAddr::V6(ip) => is_public_ipv6(ip),
    }
}

fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0
        || a == 10
        || a == 127
        || (a == 100 && (64..=127).contains(&b))
        || (a == 169 && b == 254)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 0 && c == 2)
        || (a == 192 && b == 88 && c == 99)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || a >= 224)
}

fn is_public_ipv6(ip: Ipv6Addr) -> bool {
    if let Some(mapped) = ip.to_ipv4_mapped() {
        return is_public_ipv4(mapped);
    }
    let segments = ip.segments();
    // Accept only global-unicast space, then remove documentation and IPv4
    // transition prefixes whose embedded address can bypass IPv4 policy.
    (segments[0] & 0xe000) == 0x2000
        && segments[0] != 0x2002
        && !(segments[0] == 0x2001 && matches!(segments[1], 0x0000 | 0x0db8))
}

fn mcp_text(body: &str) -> Result<String, WebError> {
    let candidates = body
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim))
        .chain(std::iter::once(body.trim()));
    for candidate in candidates {
        let Ok(value) = serde_json::from_str::<Value>(candidate) else {
            continue;
        };
        // Exa's keyless endpoint reports throttling inside a 200 response.
        if value["result"]["_meta"]["ai.exa/rateLimited"] == Value::Bool(true) {
            return Err(WebError::RateLimited { retry_after: None });
        }
        if value.get("error").is_some() || value["result"]["isError"] == Value::Bool(true) {
            return Err(WebError::Provider);
        }
        if let Some(text) = value["result"]["content"].as_array().and_then(|items| {
            items.iter().find_map(|item| {
                (item["type"].as_str() == Some("text"))
                    .then(|| item["text"].as_str())
                    .flatten()
                    .filter(|text| !text.trim().is_empty())
            })
        }) {
            return Ok(text.to_owned());
        }
    }
    Err(WebError::InvalidProviderResponse)
}

fn parse_search_results(text: &str) -> Result<Vec<WebSource>, WebError> {
    if let Ok(parsed) = serde_json::from_str::<JsonSearchResponse>(text) {
        let sources = parsed
            .results
            .into_iter()
            .filter_map(|result| {
                let url = result.url?;
                validate_source_url(&url).ok()?;
                let title = result
                    .title
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .map_or_else(|| untitled_label(&url), |title| bounded_text(title, 512));
                let snippet = result
                    .highlights
                    .filter(|items| !items.is_empty())
                    .map(|items| items.join(" "))
                    .or(result.text)
                    .unwrap_or_default();
                let published_at = result
                    .published_date
                    .as_deref()
                    .filter(|value| !value.trim().is_empty() && *value != "N/A")
                    .map(|value| bounded_text(value, 128));
                Some(web_source(
                    title,
                    url,
                    bounded_text(&snippet, 2_000),
                    published_at,
                ))
            })
            .collect::<Vec<_>>();
        return Ok(sources);
    }

    let mut sources = Vec::new();
    for block in text.split("\nTitle: ") {
        let block = block.strip_prefix("Title: ").unwrap_or(block);
        // Metadata lines come before the page text; a "URL:" or "Published:"
        // line quoted inside that text says nothing about this result.
        let header_end = ["\nText: ", "\nHighlights:"]
            .iter()
            .filter_map(|marker| block.find(marker))
            .min()
            .unwrap_or(block.len());
        let header = &block[..header_end];
        let mut lines = header.lines();
        let title = lines.next().unwrap_or("").trim();
        let url = header
            .lines()
            .find_map(|line| line.strip_prefix("URL: "))
            .map(str::trim);
        let Some(url) = url else { continue };
        if validate_source_url(url).is_err() {
            continue;
        }
        let snippet = block
            .split_once("\nText: ")
            .map(|(_, value)| value)
            .or_else(|| block.split_once("\nHighlights:\n").map(|(_, value)| value))
            .unwrap_or("")
            .trim_end_matches("\n---")
            .trim();
        let published_at = header
            .lines()
            .find_map(|line| line.strip_prefix("Published: "))
            .map(str::trim)
            .filter(|value| !value.is_empty() && *value != "N/A")
            .map(|value| bounded_text(value, 128));
        let title = if title.is_empty() {
            untitled_label(url)
        } else {
            bounded_text(title, 512)
        };
        sources.push(web_source(
            title,
            url.to_owned(),
            bounded_text(snippet, 2_000),
            published_at,
        ));
    }
    let normalized = text.trim().to_ascii_lowercase();
    if sources.is_empty()
        && (normalized.contains("no results") || normalized.contains("no relevant results"))
    {
        Ok(Vec::new())
    } else if sources.is_empty() {
        Err(WebError::InvalidProviderResponse)
    } else {
        Ok(sources)
    }
}

/// A result without a title is labeled by its host, not a generic placeholder.
fn untitled_label(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "Untitled source".into())
}

pub fn source_from_url(value: &str) -> Result<WebSource, WebError> {
    let url = validate_source_url(value)?;
    let title = url
        .host_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("Untitled source")
        .to_owned();
    Ok(web_source(title, url.to_string(), String::new(), None))
}

pub fn validate_search_domain(value: &str) -> Result<String, WebError> {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty()
        || value.len() > 253
        || value.contains('/')
        || value.contains(':')
        || value.contains('@')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
        || value.starts_with('.')
        || value.ends_with('.')
        || !value.contains('.')
    {
        return Err(WebError::UrlDenied);
    }
    Ok(value)
}

struct DecodedContent {
    title: String,
    content: String,
    charset: Option<String>,
    document_kind: String,
    extraction_method: &'static str,
    page_count: Option<u32>,
    extraction_truncated: bool,
    alternates: Vec<WebAlternate>,
    published_at: Option<String>,
    /// Decoding notes the content itself cannot show (see `quality_warnings`).
    warnings: Vec<&'static str>,
}

fn decode_fetched_content(
    bytes: &[u8],
    raw_content_type: &str,
    content_type: &str,
    fallback_title: &str,
    base_url: &Url,
) -> Result<DecodedContent, WebError> {
    if crate::web::document::looks_like_pdf(bytes)
        || crate::web::document::looks_like_zip(bytes)
        || crate::web::document::looks_like_ole(bytes)
        || is_document_content_type(content_type)
    {
        let extracted =
            crate::web::document::extract(bytes, content_type, MAX_FETCH_EXTRACTED_BYTES)
                .map_err(document_error)?;
        let extraction_method = if extracted.kind == crate::web::document::DocumentKind::Pdf {
            "pdf"
        } else if extracted.kind == crate::web::document::DocumentKind::Html {
            "readability"
        } else if extracted.kind == crate::web::document::DocumentKind::PlainText {
            if content_type == "text/markdown" {
                "direct_markdown"
            } else {
                "direct_text"
            }
        } else {
            "office"
        };
        return Ok(DecodedContent {
            title: fallback_title.to_owned(),
            content: extracted.markdown.trim().to_owned(),
            charset: None,
            document_kind: extracted.kind.as_str().to_owned(),
            extraction_method,
            page_count: extracted.page_count,
            extraction_truncated: extracted.truncated,
            alternates: Vec::new(),
            published_at: None,
            warnings: Vec::new(),
        });
    }
    if !is_text_content_type(content_type)
        && !matches!(content_type, "" | "application/octet-stream")
    {
        return Err(WebError::UnsupportedContent);
    }
    let declared_html = matches!(content_type, "text/html" | "application/xhtml+xml");
    let sniff_html = matches!(content_type, "" | "application/octet-stream")
        && crate::web::document::looks_like_html(bytes);
    let html = declared_html || sniff_html;
    let decoded = decode_web_text(bytes, raw_content_type, html, base_url.host_str())?;
    let text = decoded.text;
    if !is_probably_text(&text) {
        return Err(WebError::UnsupportedContent);
    }
    let mut warnings = Vec::new();
    if decoded.replaced {
        warnings.push("decoding_replaced");
    }
    let (title, content, extraction_method, alternates, published_at) = if html {
        let document = Html::parse_document(&text);
        if is_challenge_page(&document, &text) {
            return Err(WebError::Challenge);
        }
        let page = crate::web::document::extract_html_page(&text, fallback_title);
        // Prefer the explicit Open Graph / microdata tags; then the page's
        // JSON-LD publication the readability parser found.
        let published_at = html_publication_time(&document).or_else(|| {
            page.published_time
                .as_deref()
                .and_then(normalized_publication_time)
        });
        if is_script_shell(&document, &page.markdown) {
            warnings.push("javascript_required");
        }
        (
            page.title,
            page.markdown,
            "readability",
            collect_alternates(&document, base_url),
            published_at,
        )
    } else {
        let extraction_method = if content_type == "text/markdown" {
            "direct_markdown"
        } else {
            "direct_text"
        };
        (
            fallback_title.to_owned(),
            text,
            extraction_method,
            Vec::new(),
            None,
        )
    };
    if content.trim().is_empty() {
        return Err(WebError::NoTextLayer);
    }
    let (content, extraction_truncated) = truncate_utf8(content.trim(), MAX_FETCH_EXTRACTED_BYTES);
    Ok(DecodedContent {
        title,
        content,
        charset: Some(decoded.charset),
        document_kind: if html {
            "html".to_owned()
        } else {
            text_document_kind(content_type).to_owned()
        },
        extraction_method,
        page_count: None,
        extraction_truncated,
        alternates,
        published_at,
        warnings,
    })
}

fn html_publication_time(document: &Html) -> Option<String> {
    let selector = Selector::parse(
        "meta[property='article:published_time'], meta[itemprop='datePublished'], time[itemprop='datePublished']",
    )
    .ok()?;
    document.select(&selector).take(16).find_map(|element| {
        let value = element
            .value()
            .attr("content")
            .or_else(|| element.value().attr("datetime"))?
            .trim();
        normalized_publication_time(value)
    })
}

/// An RFC 3339 time or a bare `YYYY-MM-DD` date, normalized; anything else
/// is unknown rather than guessed.
fn normalized_publication_time(value: &str) -> Option<String> {
    let value = value.trim();
    if value.len() > 128 {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(value)
        .or_else(|_| chrono::DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%z"))
        .map(|time| time.to_rfc3339())
        .ok()
        .or_else(|| {
            (value.len() == 10)
                .then(|| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
                .flatten()
                .map(|date| date.to_string())
        })
}

/// Cloudflare's "Just a moment..." interstitial: a script challenge that a
/// host-side reader can never pass, so its text is not the page.
fn is_challenge_page(document: &Html, text: &str) -> bool {
    let title = Selector::parse("title")
        .ok()
        .and_then(|selector| document.select(&selector).next())
        .map(|title| title.text().collect::<String>())
        .unwrap_or_default();
    title.trim().eq_ignore_ascii_case("just a moment...")
        && (text.contains("window._cf_chl_opt") || text.contains("/cdn-cgi/challenge-platform/"))
}

/// A page whose readable text is tiny but that ships several executable
/// scripts is a client-rendered shell; its extracted text is not the page.
/// JSON-LD and template scripts do not run, so they do not count.
fn is_script_shell(document: &Html, extracted: &str) -> bool {
    let visible = extracted
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    if visible >= 500 {
        return false;
    }
    let Ok(selector) = Selector::parse("script") else {
        return false;
    };
    let executable = document
        .select(&selector)
        .filter(|script| {
            let kind = script
                .value()
                .attr("type")
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            kind.is_empty()
                || kind == "module"
                || kind.ends_with("/javascript")
                || kind.ends_with("/ecmascript")
        })
        .count();
    executable > 3
}

fn quality_warnings(content: &str, html_extract: bool) -> Vec<&'static str> {
    let mut warnings = Vec::new();
    let trimmed = content.trim();
    let non_whitespace = trimmed
        .chars()
        .filter(|character| !character.is_whitespace())
        .count();
    if non_whitespace < 100 {
        warnings.push("very_short");
    }
    let lower = trimmed.to_ascii_lowercase();
    if JS_REQUIRED_PHRASES
        .iter()
        .any(|phrase| lower.contains(phrase))
    {
        warnings.push("javascript_required");
    }
    if html_extract && is_navigation_heavy(trimmed) {
        warnings.push("navigation_heavy");
    }
    warnings
}

fn is_navigation_heavy(content: &str) -> bool {
    let lines = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.len() > 10 {
        let short_lines = lines
            .iter()
            .filter(|line| line.trim().chars().count() < 40)
            .count();
        if (short_lines as f64) / (lines.len() as f64) > 0.7 {
            return true;
        }
    }
    let link_count = content.matches("](").count();
    let words = content.split_whitespace().count().max(1);
    link_count.saturating_mul(4) > words
}

fn collect_alternates(document: &Html, base_url: &Url) -> Vec<WebAlternate> {
    let mut seen = HashSet::new();
    let mut markdown = Vec::new();
    let mut documents = Vec::new();
    if let Ok(selector) = Selector::parse("link[rel][href]") {
        for element in document.select(&selector) {
            let rel = element.value().attr("rel").unwrap_or("");
            if !rel
                .split_whitespace()
                .any(|token| token.eq_ignore_ascii_case("alternate"))
            {
                continue;
            }
            let Some(href) = element.value().attr("href") else {
                continue;
            };
            let type_attr = element
                .value()
                .attr("type")
                .unwrap_or("")
                .to_ascii_lowercase();
            let kind =
                if type_attr.contains("markdown") || href.to_ascii_lowercase().ends_with(".md") {
                    Some("markdown")
                } else {
                    alternate_document_kind(href, &type_attr)
                };
            if let Some(kind) = kind {
                if let Some(alternate) = resolve_alternate(base_url, href, kind, &mut seen) {
                    if kind == "markdown" {
                        markdown.push(alternate);
                    } else {
                        documents.push(alternate);
                    }
                }
            }
        }
    }
    if let Ok(selector) = Selector::parse("a[href]") {
        for element in document.select(&selector) {
            let Some(href) = element.value().attr("href") else {
                continue;
            };
            let type_attr = element
                .value()
                .attr("type")
                .unwrap_or("")
                .to_ascii_lowercase();
            if let Some(kind) = alternate_document_kind(href, &type_attr) {
                if let Some(alternate) = resolve_alternate(base_url, href, kind, &mut seen) {
                    documents.push(alternate);
                }
            }
        }
    }
    markdown
        .into_iter()
        .chain(documents)
        .take(MAX_ALTERNATES)
        .collect()
}

fn alternate_document_kind(href: &str, type_attr: &str) -> Option<&'static str> {
    let path = href
        .split(['?', '#'])
        .next()
        .unwrap_or(href)
        .to_ascii_lowercase();
    if type_attr.contains("markdown") || path.ends_with(".md") {
        Some("markdown")
    } else if type_attr.contains("pdf") || path.ends_with(".pdf") {
        Some("pdf")
    } else if type_attr.contains("word")
        || type_attr.contains("excel")
        || type_attr.contains("powerpoint")
        || type_attr.contains("officedocument")
        || path.ends_with(".doc")
        || path.ends_with(".docx")
        || path.ends_with(".xls")
        || path.ends_with(".xlsx")
        || path.ends_with(".ppt")
        || path.ends_with(".pptx")
    {
        Some("office")
    } else {
        None
    }
}

fn resolve_alternate(
    base_url: &Url,
    href: &str,
    kind: &'static str,
    seen: &mut HashSet<String>,
) -> Option<WebAlternate> {
    let resolved = base_url.join(href).ok()?;
    validate_url_shape(&resolved).ok()?;
    let url = resolved.to_string();
    if url == base_url.as_str() || !seen.insert(url.clone()) {
        return None;
    }
    Some(WebAlternate { url, kind })
}

fn document_error(error: crate::web::document::DocumentError) -> WebError {
    match error {
        crate::web::document::DocumentError::NoTextLayer => WebError::NoTextLayer,
        crate::web::document::DocumentError::ArchiveLimit => WebError::ResponseTooLarge,
        crate::web::document::DocumentError::TypeMismatch => WebError::TypeMismatch,
        _ => WebError::UnsupportedContent,
    }
}

struct DecodedText {
    text: String,
    charset: String,
    /// Some bytes were not valid in the chosen encoding and became U+FFFD.
    replaced: bool,
}

/// Decode web text the way a browser picks its encoding: a byte-order mark,
/// then the HTTP `charset`, then (HTML only) a `<meta>` charset in the first
/// 1024 bytes, then UTF-8. Text that declares nothing and is not UTF-8 —
/// common for Korean and Japanese pages and CSV downloads — gets
/// chardetng's guess, hinted by the host's top-level domain.
fn decode_web_text(
    bytes: &[u8],
    content_type: &str,
    html: bool,
    host: Option<&str>,
) -> Result<DecodedText, WebError> {
    if let Some((encoding, bom_length)) = Encoding::for_bom(bytes) {
        return decode_with(encoding, &bytes[bom_length..]).ok_or(WebError::UnsupportedContent);
    }
    let header = content_type.split(';').skip(1).find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        name.trim().eq_ignore_ascii_case("charset").then(|| {
            value
                .trim()
                .trim_matches(|character| matches!(character, '\'' | '"'))
        })
    });
    // An unknown label is ignored, as browsers do, rather than failing the page.
    let declared = header
        .and_then(|label| Encoding::for_label(label.as_bytes()))
        .or_else(|| html.then(|| html_meta_charset(bytes)).flatten());
    if let Some(encoding) = declared {
        if let Some(decoded) = decode_with(encoding, bytes) {
            return Ok(decoded);
        }
    } else if let Ok(text) = std::str::from_utf8(bytes) {
        return Ok(DecodedText {
            text: text.to_owned(),
            charset: "utf-8".into(),
            replaced: false,
        });
    }
    // Nothing declared and not UTF-8, or a declaration the bytes contradict.
    let mut detector = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
    detector.feed(bytes, true);
    let tld = host
        .and_then(|host| host.trim_end_matches('.').rsplit('.').next())
        .map(str::as_bytes);
    let guessed = detector.guess(tld, chardetng::Utf8Detection::Allow);
    if Some(guessed) == declared {
        return Err(WebError::UnsupportedContent);
    }
    decode_with(guessed, bytes).ok_or(WebError::UnsupportedContent)
}

/// Decode with replacement. A handful of broken bytes should not lose a page,
/// but text that is mostly replacement characters was not in this encoding.
fn decode_with(encoding: &'static Encoding, bytes: &[u8]) -> Option<DecodedText> {
    let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
    if had_errors {
        let mut total = 0usize;
        let mut replaced = 0usize;
        for character in text.chars() {
            total += 1;
            replaced += usize::from(character == char::REPLACEMENT_CHARACTER);
        }
        if replaced.saturating_mul(200) > total.max(1) {
            return None;
        }
    }
    Some(DecodedText {
        text: text.into_owned(),
        charset: encoding.name().to_ascii_lowercase(),
        replaced: had_errors,
    })
}

/// The `<meta charset>` or `<meta http-equiv content="...; charset=...">`
/// label in the first 1024 bytes (the WHATWG prescan window).
fn html_meta_charset(bytes: &[u8]) -> Option<&'static Encoding> {
    let head = bytes[..bytes.len().min(1024)].to_ascii_lowercase();
    let mut cursor = 0;
    while let Some(start) = find_bytes(&head[cursor..], b"<meta") {
        let tag_start = cursor + start + 5;
        let tag_end = head[tag_start..]
            .iter()
            .position(|byte| *byte == b'>')
            .map_or(head.len(), |end| tag_start + end);
        let tag = &head[tag_start..tag_end];
        cursor = tag_end;
        let Some(position) = find_bytes(tag, b"charset") else {
            continue;
        };
        let value = tag[position + 7..].trim_ascii_start();
        let Some(value) = value.strip_prefix(b"=") else {
            continue;
        };
        let value = value.trim_ascii_start();
        let value = value
            .strip_prefix(b"\"")
            .or_else(|| value.strip_prefix(b"'"))
            .unwrap_or(value);
        let length = value
            .iter()
            .position(|byte| byte.is_ascii_whitespace() || b";/>\"'".contains(byte))
            .unwrap_or(value.len());
        if let Some(encoding) = Encoding::for_label(&value[..length]) {
            // A meta UTF-16 label inside ASCII-compatible bytes means UTF-8.
            return Some(if encoding == encoding_rs::X_USER_DEFINED {
                encoding_rs::WINDOWS_1252
            } else {
                encoding.output_encoding()
            });
        }
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn is_probably_text(value: &str) -> bool {
    !value.contains('\0')
        && !value
            .chars()
            .any(|character| character.is_control() && !character.is_whitespace())
}

fn is_text_content_type(content_type: &str) -> bool {
    content_type.starts_with("text/")
        || matches!(
            content_type,
            "application/json"
                | "application/ld+json"
                | "application/x-ndjson"
                | "application/jsonl"
                | "application/xml"
                | "application/csv"
                | "application/yaml"
                | "application/x-yaml"
        )
        || content_type.ends_with("+json")
        || content_type.ends_with("+xml")
}

fn text_document_kind(content_type: &str) -> &'static str {
    if matches!(content_type, "text/csv" | "application/csv") {
        "csv"
    } else if content_type == "text/tab-separated-values" {
        "tsv"
    } else if content_type == "text/markdown" {
        "markdown"
    } else if content_type == "application/json"
        || matches!(
            content_type,
            "application/ld+json" | "application/x-ndjson" | "application/jsonl"
        )
        || content_type.ends_with("+json")
    {
        "json"
    } else if matches!(content_type, "application/xml" | "text/xml")
        || content_type.ends_with("+xml")
    {
        "xml"
    } else if matches!(
        content_type,
        "application/yaml" | "application/x-yaml" | "text/yaml"
    ) {
        "yaml"
    } else {
        "text"
    }
}

fn content_page(content: &str, offset: usize) -> Result<(String, Option<usize>), WebError> {
    if offset >= content.len() || !content.is_char_boundary(offset) {
        return Err(WebError::InvalidContentOffset);
    }
    let mut end = offset
        .saturating_add(MAX_FETCH_PAGE_BYTES)
        .min(content.len());
    while end > offset && !content.is_char_boundary(end) {
        end -= 1;
    }
    let next_offset = (end < content.len()).then_some(end);
    Ok((content[offset..end].to_owned(), next_offset))
}

fn is_document_content_type(content_type: &str) -> bool {
    matches!(
        content_type,
        "application/pdf"
            | "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            | "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            | "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            | "application/msword"
            | "application/vnd.ms-excel"
            | "application/vnd.ms-powerpoint"
    )
}

pub(crate) fn parse_retry_after_header(
    value: Option<&reqwest::header::HeaderValue>,
) -> Option<Duration> {
    let raw = value?.to_str().ok()?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(seconds) = raw.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    httpdate::parse_http_date(raw).ok().map(|date| {
        date.duration_since(std::time::SystemTime::now())
            .unwrap_or_default()
    })
}

pub(crate) fn clamp_retry_after(requested: Duration, remaining: Duration) -> Option<Duration> {
    let budget = FETCH_RETRY_AFTER_CAP.min(remaining);
    (!remaining.is_zero() && requested <= budget).then_some(requested)
}

pub(crate) fn web_source(
    title: String,
    url: String,
    snippet: String,
    published_at: Option<String>,
) -> WebSource {
    let digest = Sha256::digest(url.as_bytes());
    WebSource {
        source_id: format!("web:{}", hex::encode(&digest[..12])),
        title,
        url,
        snippet,
        published_at,
    }
}

fn bounded_text(value: &str, max_bytes: usize) -> String {
    truncate_utf8(value.trim(), max_bytes).0
}

fn truncate_utf8(value: &str, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value.to_owned(), false);
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_owned(), true)
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

#[cfg(test)]
pub(crate) mod script {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone)]
    pub struct ScriptedResponse {
        pub status: u16,
        pub content_type: String,
        pub body: Vec<u8>,
        pub retry_after: Option<String>,
        pub location: Option<String>,
        pub challenge: bool,
    }

    struct Route {
        needle: String,
        responses: Vec<ScriptedResponse>,
        next: usize,
    }

    struct ScriptState {
        owner: std::thread::ThreadId,
        routes: Vec<Route>,
        requests: Vec<(String, String)>,
    }

    static STATE: Mutex<Option<ScriptState>> = Mutex::new(None);
    static RUN: Mutex<()> = Mutex::new(());

    pub struct Guard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl Drop for Guard {
        fn drop(&mut self) {
            *STATE.lock().unwrap_or_else(|error| error.into_inner()) = None;
        }
    }

    pub fn install(routes: Vec<(&str, Vec<ScriptedResponse>)>) -> Guard {
        let run = RUN.lock().unwrap_or_else(|error| error.into_inner());
        *STATE.lock().unwrap_or_else(|error| error.into_inner()) = Some(ScriptState {
            owner: std::thread::current().id(),
            routes: routes
                .into_iter()
                .map(|(needle, responses)| Route {
                    needle: needle.to_owned(),
                    responses,
                    next: 0,
                })
                .collect(),
            requests: Vec::new(),
        });
        Guard(run)
    }

    pub fn take(url: &Url, accept: &str) -> Option<ScriptedResponse> {
        let mut state = STATE.lock().ok()?;
        let state = state.as_mut()?;
        // Scripted tests use a current-thread runtime. RUN serializes their
        // guards, but unrelated unguarded tests can still enter this transport.
        if state.owner != std::thread::current().id() {
            return None;
        }
        state
            .requests
            .push((url.as_str().to_owned(), accept.to_owned()));
        let url_s = url.as_str().to_owned();
        for route in &mut state.routes {
            if url_s.contains(&route.needle) {
                if route.next >= route.responses.len() {
                    return Some(ScriptedResponse {
                        challenge: false,
                        status: 500,
                        content_type: "text/plain".into(),
                        body: b"script exhausted".to_vec(),
                        retry_after: None,
                        location: None,
                    });
                }
                let response = route.responses[route.next].clone();
                route.next += 1;
                return Some(response);
            }
        }
        None
    }

    pub fn requests() -> Vec<(String, String)> {
        STATE
            .lock()
            .ok()
            .and_then(|state| {
                state
                    .as_ref()
                    .filter(|state| state.owner == std::thread::current().id())
                    .map(|state| state.requests.clone())
            })
            .unwrap_or_default()
    }

    pub fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> ScriptedResponse {
        ScriptedResponse {
            challenge: false,
            status: 200,
            content_type: content_type.to_owned(),
            body: body.into(),
            retry_after: None,
            location: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_unreachable_host_is_named_without_claiming_the_device_is_offline() {
        // A closed local port refuses the connection before any HTTP exchange.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let error = reqwest::Client::new()
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap_err();
        let classified = classify_transport_error(error);
        assert!(matches!(&classified, WebError::Offline(host) if host == "127.0.0.1"));
        assert_eq!(
            classified.to_string(),
            "web request could not reach 127.0.0.1"
        );
    }

    #[test]
    fn rejects_private_and_special_addresses() {
        for value in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "192.168.1.1",
            "100.64.0.1",
            "192.0.2.1",
            "::1",
            "fc00::1",
            "fe80::1",
            "2001:db8::1",
            "2002:c0a8:0101::1",
            "2001:0:c0a8:0101::1",
            "::ffff:192.168.1.1",
            "64:ff9b::192.168.1.1",
        ] {
            assert!(!is_public_ip(value.parse().unwrap()), "{value}");
        }
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
        assert!(is_public_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    #[test]
    fn source_urls_are_http_without_credentials_or_unusual_ports() {
        assert!(validate_source_url("https://example.com/report").is_ok());
        assert!(validate_source_url("file:///etc/passwd").is_err());
        assert!(validate_source_url("https://user:secret@example.com/").is_err());
        assert!(validate_source_url("https://example.com:8443/").is_err());
        assert!(validate_source_url("http://example.com:443/").is_err());
        assert!(validate_source_url("https://example.com:80/").is_err());
    }

    #[test]
    fn parses_exa_mcp_search_responses() {
        let rpc = r#"{"result":{"content":[{"type":"text","text":"Title: First report\nURL: https://example.com/one\nText: First snippet\n---\nTitle: Second report\nURL: https://example.org/two\nHighlights:\nSecond snippet\n---"}]}}"#;
        let sse = format!("event: message\ndata: {rpc}\n\n");
        let parsed = parse_search_results(&mcp_text(&sse).unwrap()).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "First report");
        assert_eq!(parsed[0].snippet, "First snippet");
        for empty in [r#"{"results":[]}"#, "No relevant results found."] {
            assert!(parse_search_results(empty).unwrap().is_empty());
        }

        // Only the result header carries metadata; quoted page text cannot
        // date a result or replace its URL, and an untitled hit names its host.
        let quoted = "Title: \nURL: https://news.example.com/a\nPublished: 2026-03-01T00:00:00.000Z\nText: The memo said\nPublished: 1999-01-01\nURL: https://evil.example/\n---";
        let parsed = parse_search_results(quoted).unwrap();
        assert_eq!(parsed[0].url, "https://news.example.com/a");
        assert_eq!(parsed[0].title, "news.example.com");
        assert_eq!(
            parsed[0].published_at.as_deref(),
            Some("2026-03-01T00:00:00.000Z")
        );
        let undated = parse_search_results(
            "Title: Memo\nURL: https://example.com/m\nText: Published: 2020-01-01\n---",
        )
        .unwrap();
        assert_eq!(undated[0].published_at, None);
        let json = parse_search_results(
            r#"{"results":[{"url":"https://ir.example.com/q3","title":" "}]}"#,
        )
        .unwrap();
        assert_eq!(json[0].title, "ir.example.com");
    }

    #[test]
    fn exa_rate_limit_inside_a_successful_response_is_rate_limited() {
        let throttled = r#"{"jsonrpc":"2.0","id":1,"result":{"content":[{"type":"text","text":"You've hit Exa's free MCP rate limit."}],"isError":true,"_meta":{"ai.exa/rateLimited":true}}}"#;
        assert!(matches!(
            mcp_text(throttled),
            Err(WebError::RateLimited { retry_after: None })
        ));
    }

    #[test]
    fn retries_only_transient_search_failures() {
        assert!(matches!(
            status_error(StatusCode::TOO_MANY_REQUESTS),
            WebError::RateLimited { .. }
        ));
        assert!(retryable_search_error(&WebError::RateLimited {
            retry_after: None
        }));
        assert!(retryable_search_error(&WebError::ProviderStatus(503)));
        for permanent in [
            WebError::ProviderStatus(400),
            WebError::ProviderStatus(403),
            WebError::UrlDenied,
        ] {
            assert!(!retryable_search_error(&permanent));
        }
        assert!(fallbackable_search_error(&WebError::ProviderStatus(400)));
        assert!(fallbackable_search_error(
            &WebError::InvalidProviderResponse
        ));
    }

    #[test]
    fn decodes_declared_charsets_and_only_textual_octet_streams() {
        let base = Url::parse("https://example.com/report").unwrap();
        let legacy = decode_fetched_content(
            b"caf\xe9",
            "text/plain; charset=windows-1252",
            "text/plain",
            "fallback",
            &base,
        )
        .unwrap();
        assert_eq!(legacy.content, "café");

        let markdown = decode_fetched_content(
            b"<note>\n# Heading\n\nMarkdown that starts with a tag stays Markdown.",
            "text/markdown; charset=utf-8",
            "text/markdown",
            "note",
            &base,
        )
        .unwrap();
        assert_eq!(markdown.document_kind, "markdown");
        assert!(markdown.content.starts_with("<note>"));

        let download = decode_fetched_content(
            b"symbol,value\nLOUPIST,42",
            "application/octet-stream",
            "application/octet-stream",
            "report.csv",
            &base,
        )
        .unwrap();
        assert!(download.content.contains("LOUPIST,42"));
        assert!(decode_fetched_content(
            b"\x01\x02\x03",
            "application/octet-stream",
            "application/octet-stream",
            "binary",
            &base,
        )
        .is_err());
    }

    #[test]
    fn decodes_undeclared_legacy_korean_pages_and_tolerates_a_few_bad_bytes() {
        let (korean, _, _) =
            encoding_rs::EUC_KR.encode("삼성전자 2025년 영업이익은 12.166조 원입니다.");
        let base = Url::parse("https://finance.example.co.kr/report").unwrap();
        // A `<meta>` charset in an HTML page served without a header charset.
        let mut page = b"<html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=euc-kr\"><title>Report</title></head><body><article><p>".to_vec();
        page.extend_from_slice(&korean);
        page.extend_from_slice(b"</p></article></body></html>");
        let html =
            decode_fetched_content(&page, "text/html", "text/html", "fallback", &base).unwrap();
        assert_eq!(html.charset.as_deref(), Some("euc-kr"));
        assert!(html.content.contains("영업이익은 12.166조 원"));
        assert!(html.warnings.is_empty());

        // A CSV download with no charset anywhere: the detector reads it.
        let mut csv = b"name,value\n".to_vec();
        csv.extend_from_slice(&korean);
        csv.extend_from_slice(b",1\n");
        let download =
            decode_fetched_content(&csv, "text/csv", "text/csv", "data.csv", &base).unwrap();
        assert!(
            download.content.contains("삼성전자"),
            "{}",
            download.content
        );

        // An unknown header label is ignored rather than failing the page.
        let unknown = decode_fetched_content(
            "plain text 한국어".as_bytes(),
            "text/plain; charset=x-not-a-charset",
            "text/plain",
            "fallback",
            &base,
        )
        .unwrap();
        assert_eq!(unknown.content, "plain text 한국어");

        // One stray byte in otherwise valid UTF-8 is replaced and reported.
        let mut mostly = "영업이익 ".repeat(200).into_bytes();
        mostly.push(0xff);
        let lossy = decode_fetched_content(
            &mostly,
            "text/plain; charset=utf-8",
            "text/plain",
            "fallback",
            &base,
        )
        .unwrap();
        assert!(lossy.content.contains('\u{FFFD}'));
        assert_eq!(lossy.warnings, ["decoding_replaced"]);
    }

    #[tokio::test]
    async fn bot_challenges_fail_clearly_instead_of_returning_the_interstitial() {
        let interstitial = "<html><head><title>Just a moment...</title></head><body><script>window._cf_chl_opt={cType:'managed'};</script><p>Checking your browser.</p></body></html>";
        let mut by_header = script::ok("text/html", b"<html><body>blocked</body></html>".to_vec());
        by_header.status = 403;
        by_header.challenge = true;
        let _guard = script::install(vec![
            (
                "https://blocked.example.com/body",
                vec![script::ok("text/html", interstitial.as_bytes().to_vec())],
            ),
            ("https://blocked.example.com/header", vec![by_header]),
        ]);
        for path in ["body", "header"] {
            let source = source_from_url(&format!("https://blocked.example.com/{path}")).unwrap();
            assert!(
                matches!(fetch(&source).await, Err(WebError::Challenge)),
                "{path}"
            );
        }
    }

    #[test]
    fn client_rendered_shells_warn_but_short_static_pages_do_not() {
        let base = Url::parse("https://app.example.com/").unwrap();
        let scripts = "<script src=\"/a.js\"></script><script type=\"module\" src=\"/b.js\"></script><script>boot()</script><script src=\"/c.js\"></script>";
        let shell = format!("<html><head><title>Quote</title>{scripts}</head><body><div id=\"root\">Loading quote</div></body></html>");
        let decoded =
            decode_fetched_content(shell.as_bytes(), "text/html", "text/html", "Quote", &base)
                .unwrap();
        assert!(decoded.warnings.contains(&"javascript_required"));

        let data_only = "<script type=\"application/ld+json\">{}</script>".repeat(6);
        let static_page = format!("<html><head><title>Notice</title>{data_only}</head><body><article><p>The annual meeting is on 3 March.</p></article></body></html>");
        let decoded = decode_fetched_content(
            static_page.as_bytes(),
            "text/html",
            "text/html",
            "Notice",
            &base,
        )
        .unwrap();
        assert!(!decoded.warnings.contains(&"javascript_required"));
    }

    #[test]
    fn meta_charset_prescan_reads_both_forms_and_maps_utf16_to_utf8() {
        assert_eq!(
            html_meta_charset(b"<!doctype html><META CHARSET='Shift_JIS'>"),
            Some(encoding_rs::SHIFT_JIS)
        );
        assert_eq!(
            html_meta_charset(b"<meta name=x><meta charset=utf-16>"),
            Some(encoding_rs::UTF_8)
        );
        assert_eq!(html_meta_charset(b"<meta name=\"charset\">"), None);
        let late = format!("{}<meta charset=euc-kr>", " ".repeat(1024));
        assert_eq!(html_meta_charset(late.as_bytes()), None);
    }

    #[test]
    fn pages_large_extracted_text_on_utf8_boundaries() {
        let content = "한".repeat(100_000);
        let mut reconstructed = String::new();
        let mut cursor = Some(0);
        while let Some(offset) = cursor {
            let (page, following) = content_page(&content, offset).unwrap();
            assert!(page.len() <= MAX_FETCH_PAGE_BYTES);
            reconstructed.push_str(&page);
            cursor = following;
        }
        assert_eq!(reconstructed, content);
        assert!(matches!(
            content_page(&content, 1),
            Err(WebError::InvalidContentOffset)
        ));
    }

    #[tokio::test]
    #[ignore = "requires public network access"]
    async fn live_public_fetch_smoke() {
        let source = web_source(
            "Example Domain".into(),
            "https://example.com/".into(),
            String::new(),
            None,
        );
        let output = fetch(&source).await.unwrap().page(0).unwrap();
        assert_eq!(output.title, "Example Domain");
        assert!(!output.content.trim().is_empty());
        assert!(output.untrusted_content);
    }

    #[test]
    fn quality_warnings_flag_short_js_and_navigation_pages() {
        assert_eq!(
            quality_warnings("Please enable JavaScript to continue.", true),
            vec!["very_short", "javascript_required"]
        );
        let nav = (0..20)
            .map(|index| format!("[Home {index}](/p{index})"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(quality_warnings(&nav, true).contains(&"navigation_heavy"));
        assert!(quality_warnings(&"Useful paragraph. ".repeat(40), true).is_empty());
    }

    #[tokio::test]
    async fn markdown_content_negotiation_seals_exact_bytes() {
        let body = b"# Note\n\nExact markdown bytes.";
        let _guard = script::install(vec![(
            "https://example.com/note",
            vec![script::ok("text/markdown; charset=utf-8", body.to_vec())],
        )]);
        let source = web_source(
            "Note".into(),
            "https://example.com/note".into(),
            String::new(),
            None,
        );
        let output = fetch(&source).await.unwrap().page(0).unwrap();
        assert_eq!(output.extraction_method, "direct_markdown");
        assert_eq!(output.content_sha256, hex::encode(Sha256::digest(body)));
        let requests = script::requests();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].1.starts_with("text/markdown"));
    }

    #[tokio::test]
    async fn html_alternates_are_returned_and_not_auto_fetched() {
        let html = r#"<html><head><title>Spec</title><link rel="alternate" type="text/markdown" href="https://example.com/spec.md"></head><body><article><h1>Spec</h1><p>The specification text is long enough to avoid a short warning.</p><p><a href="/spec.pdf">PDF</a></p></article></body></html>"#;
        let _guard = script::install(vec![(
            "https://example.com/spec",
            vec![script::ok("text/html", html.as_bytes().to_vec())],
        )]);
        let source = web_source(
            "Spec".into(),
            "https://example.com/spec".into(),
            String::new(),
            None,
        );
        let output = fetch(&source).await.unwrap().page(0).unwrap();
        assert_eq!(output.extraction_method, "readability");
        assert_eq!(output.alternates.len(), 2);
        assert_eq!(output.alternates[0].url, "https://example.com/spec.md");
        assert_eq!(output.alternates[1].kind, "pdf");
        assert_eq!(script::requests().len(), 1);
    }

    #[tokio::test]
    async fn get_429_retries_once_honoring_retry_after() {
        let body = b"ok after retry";
        let _guard = script::install(vec![(
            "https://example.com/limited",
            vec![
                script::ScriptedResponse {
                    challenge: false,
                    status: 429,
                    content_type: "text/plain".into(),
                    body: b"slow down".to_vec(),
                    retry_after: Some("0".into()),
                    location: None,
                },
                script::ok("text/plain; charset=utf-8", body.to_vec()),
            ],
        )]);
        let source = web_source(
            "Limited".into(),
            "https://example.com/limited".into(),
            String::new(),
            None,
        );
        let output = fetch(&source).await.unwrap().page(0).unwrap();
        assert_eq!(output.content, "ok after retry");
        assert_eq!(script::requests().len(), 2);
    }

    #[tokio::test]
    async fn get_429_preserves_the_second_responses_retry_after() {
        let _guard = script::install(vec![(
            "https://example.com/limited-again",
            ["0", "7"]
                .into_iter()
                .map(|seconds| script::ScriptedResponse {
                    challenge: false,
                    status: 429,
                    content_type: "text/plain".into(),
                    body: Vec::new(),
                    retry_after: Some(seconds.into()),
                    location: None,
                })
                .collect(),
        )]);
        let source = web_source(
            "Limited".into(),
            "https://example.com/limited-again".into(),
            String::new(),
            None,
        );
        let error = fetch(&source).await.err().unwrap();
        assert_eq!(retry_after_from_error(&error), Some(Duration::from_secs(7)));
        assert_eq!(script::requests().len(), 2);
    }

    #[tokio::test]
    async fn get_429_does_not_shorten_the_servers_requested_wait() {
        let _guard = script::install(vec![(
            "https://example.com/long-wait",
            vec![
                script::ScriptedResponse {
                    challenge: false,
                    status: 429,
                    content_type: "text/plain".into(),
                    body: Vec::new(),
                    retry_after: Some("9".into()),
                    location: None,
                },
                script::ok("text/plain", b"too early".to_vec()),
            ],
        )]);
        let source = web_source(
            "Limited".into(),
            "https://example.com/long-wait".into(),
            String::new(),
            None,
        );
        let error = fetch(&source).await.err().unwrap();
        assert_eq!(retry_after_from_error(&error), Some(Duration::from_secs(9)));
        assert_eq!(script::requests().len(), 1);
    }

    #[test]
    fn retry_after_clamp_rejects_waits_outside_the_remaining_budget() {
        for (requested, remaining, expected) in [
            (0, 0, None),
            (0, 1, Some(0)),
            (2, 10, Some(2)),
            (3, 3, Some(3)),
            (4, 10, None),
            (2, 1, None),
        ] {
            assert_eq!(
                clamp_retry_after(
                    Duration::from_secs(requested),
                    Duration::from_secs(remaining)
                ),
                expected.map(Duration::from_secs)
            );
        }
    }

    #[tokio::test]
    async fn web_fetch_preserves_declared_article_publication_without_inventing_dates() {
        for (metadata, expected) in [
            (
                r#"<meta property="article:published_time" content="2026-04-25T09:00:00Z">"#,
                Some("2026-04-25T09:00:00+00:00"),
            ),
            (
                r#"<meta property="article:modified_time" content="2026-04-26T09:00:00Z">"#,
                None,
            ),
            (
                r#"<meta property="article:published_time" content="not-a-date">"#,
                None,
            ),
            (
                r#"<script type="application/ld+json">{"@context":"https://schema.org","@type":"NewsArticle","headline":"Report","datePublished":"2026-04-24T18:30:00+0900","dateModified":"2026-04-27T09:00:00Z"}</script>"#,
                Some("2026-04-24T18:30:00+09:00"),
            ),
            (
                r#"<script type="application/ld+json">{"@context":"https://schema.org","@type":"NewsArticle","dateModified":"2026-04-27T09:00:00Z"}</script>"#,
                None,
            ),
        ] {
            let html = format!("<html><head><title>Report</title>{metadata}</head><body><article><p>The original report describes revenue for the completed quarter.</p></article></body></html>");
            let _guard = script::install(vec![(
                "https://example.com/publication-test",
                vec![script::ok("text/html", html.as_bytes().to_vec())],
            )]);
            let source = web_source(
                "Report".into(),
                "https://example.com/publication-test".into(),
                String::new(),
                None,
            );
            let output = fetch(&source).await.unwrap().page(0).unwrap();
            assert_eq!(output.published_at.as_deref(), expected);
            assert!(output.content.contains("revenue for the completed quarter"));
        }
    }

    #[tokio::test]
    async fn wikipedia_official_parse_seals_representation_url_and_bytes() {
        let body = br#"{"parse":{"title":"Ada Lovelace","displaytitle":"Ada Lovelace","text":"<p>Ada Lovelace wrote the first published computer algorithm for the Analytical Engine.</p>"}}"#;
        let _guard = script::install(vec![(
            "en.wikipedia.org/w/api.php",
            vec![script::ok("application/json", body.to_vec())],
        )]);
        let source = web_source(
            "Ada Lovelace".into(),
            "https://en.wikipedia.org/wiki/Ada_Lovelace".into(),
            String::new(),
            None,
        );
        let output = fetch(&source).await.unwrap().page(0).unwrap();
        assert_eq!(output.extraction_method, "official_api");
        assert_eq!(output.representation_provider, Some("wikipedia"));
        assert_eq!(output.record_id.as_deref(), Some("Ada Lovelace"));
        assert!(output.representation_url.contains("/w/api.php"));
        assert_eq!(output.content_sha256, hex::encode(Sha256::digest(body)));
        assert!(output.content.contains("Analytical Engine"));
        assert_eq!(script::requests().len(), 1);
    }

    #[tokio::test]
    async fn wikipedia_redirect_uses_the_official_projection_in_one_request() {
        let body = br#"{"parse":{"title":"United States","redirects":[{"from":"USA","to":"United States"}],"text":"<p>The United States article body is projected from the official API.</p>"}}"#;
        let _guard = script::install(vec![
            (
                "en.wikipedia.org/w/api.php",
                vec![script::ok("application/json", body.to_vec())],
            ),
            (
                "https://en.wikipedia.org/wiki/USA",
                vec![script::ok(
                    "text/plain",
                    b"Unexpected generic fallback".to_vec(),
                )],
            ),
        ]);
        let source = web_source(
            "USA".into(),
            "https://en.wikipedia.org/wiki/USA".into(),
            String::new(),
            None,
        );
        let output = fetch(&source).await.unwrap().page(0).unwrap();
        assert_eq!(output.extraction_method, "official_api");
        assert_eq!(output.record_id.as_deref(), Some("United States"));
        assert_eq!(output.content_sha256, hex::encode(Sha256::digest(body)));
        assert!(output.content.contains("United States article body"));
        assert_eq!(script::requests().len(), 1);
    }

    #[tokio::test]
    async fn official_identity_mismatch_falls_back_once_to_generic_fetch() {
        let html = r#"<html><head><title>USA</title></head><body><article><p>The original article body stays visible after official identity mismatch.</p></article></body></html>"#;
        let _guard = script::install(vec![
            (
                "en.wikipedia.org/w/api.php",
                vec![script::ok(
                    "application/json",
                    br#"{"parse":{"title":"United States","text":"<p>Wrong page</p>"}}"#.to_vec(),
                )],
            ),
            (
                "https://en.wikipedia.org/wiki/USA",
                vec![script::ok("text/html", html.as_bytes().to_vec())],
            ),
        ]);
        let source = web_source(
            "USA".into(),
            "https://en.wikipedia.org/wiki/USA".into(),
            String::new(),
            None,
        );
        let output = fetch(&source).await.unwrap().page(0).unwrap();
        assert_eq!(output.extraction_method, "readability");
        assert!(output.quality_warnings.contains(&"official_fallback"));
        assert!(output.content.contains("original article body"));
        assert!(!output.content.contains("Wrong page"));
        let urls = script::requests()
            .into_iter()
            .map(|(url, _)| url)
            .collect::<Vec<_>>();
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("action=parse"));
        assert!(Url::parse(&urls[0])
            .unwrap()
            .query_pairs()
            .any(|(key, value)| key == "redirects" && value == "1"));
        assert_eq!(urls[1], "https://en.wikipedia.org/wiki/USA");
    }

    #[tokio::test]
    async fn official_security_failure_does_not_fallback_to_generic_fetch() {
        let _guard = script::install(vec![
            (
                "en.wikipedia.org/w/api.php",
                vec![script::ScriptedResponse {
                    challenge: false,
                    status: 302,
                    content_type: String::new(),
                    body: Vec::new(),
                    retry_after: None,
                    location: Some("http://10.0.0.1/private".into()),
                }],
            ),
            (
                "https://en.wikipedia.org/wiki/USA",
                vec![script::ok("text/html", b"must not be fetched".to_vec())],
            ),
        ]);
        let source = web_source(
            "USA".into(),
            "https://en.wikipedia.org/wiki/USA".into(),
            String::new(),
            None,
        );
        assert!(matches!(fetch(&source).await, Err(WebError::AddressDenied)));
        let urls = script::requests()
            .into_iter()
            .map(|(url, _)| url)
            .collect::<Vec<_>>();
        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("action=parse"));
        assert_eq!(urls[1], "http://10.0.0.1/private");
    }
}
