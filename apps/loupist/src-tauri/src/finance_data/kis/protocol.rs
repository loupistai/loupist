use super::*;

pub(in crate::finance_data) fn kis_credentials(
    credentials: &BTreeMap<String, String>,
) -> Result<(&str, &str), FinanceDataError> {
    if credentials.len() != 2 {
        return Err(FinanceDataError::InvalidQuery(
            "KIS requires one app key and one app secret",
        ));
    }
    let app_key = credentials
        .get("app_key")
        .map(String::as_str)
        .ok_or(FinanceDataError::InvalidQuery("KIS app key is required"))?;
    let app_secret = credentials
        .get("app_secret")
        .map(String::as_str)
        .ok_or(FinanceDataError::InvalidQuery("KIS app secret is required"))?;
    validate_api_key(app_key)?;
    validate_api_key(app_secret)?;
    Ok((app_key, app_secret))
}

pub(in crate::finance_data) fn valid_kis_parameter_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

pub(in crate::finance_data) fn valid_kis_profile_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub(in crate::finance_data) fn kis_credential_fingerprint(
    environment: KisEnvironment,
    app_key: &str,
    app_secret: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"loupist/kis-credential-cache/v1\0");
    hasher.update(environment.as_str().as_bytes());
    hasher.update(b"\0");
    hasher.update(app_key.as_bytes());
    hasher.update(b"\0");
    hasher.update(app_secret.as_bytes());
    hex::encode(hasher.finalize())
}

pub(in crate::finance_data) fn deserialize_kis_expires_in<'de, D>(
    deserializer: D,
) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Number(number) => number
            .as_i64()
            .ok_or_else(|| serde::de::Error::custom("KIS token expiry is invalid")),
        Value::String(value) => value
            .parse::<i64>()
            .map_err(|_| serde::de::Error::custom("KIS token expiry is invalid")),
        _ => Err(serde::de::Error::custom("KIS token expiry is invalid")),
    }
}

pub(in crate::finance_data) fn parse_kis_token_response(
    bytes: &[u8],
    environment: KisEnvironment,
    app_key: &str,
    app_secret: &str,
) -> Result<KisAccessToken, FinanceDataError> {
    let response: KisTokenResponse = serde_json::from_slice(bytes).map_err(|_| {
        let root = serde_json::from_slice::<Value>(bytes).ok();
        if root.as_ref().is_some_and(|root| {
            root.get("error_code").is_some()
                || root.get("error_description").is_some()
                || root.get("msg_cd").is_some()
        }) {
            classify_kis_token_error(StatusCode::OK, bytes, app_key, app_secret)
        } else {
            FinanceDataError::InvalidResponse
        }
    })?;
    if response.token_type != "Bearer"
        || response.access_token.len() < 16
        || response.access_token.len() > 4_096
        || response
            .access_token
            .chars()
            .any(|character| character.is_whitespace() || character.is_control())
        || !(60..=172_800).contains(&response.expires_in)
    {
        return Err(FinanceDataError::InvalidResponse);
    }
    Ok(KisAccessToken {
        value: response.access_token,
        environment,
        expires_at: Utc::now() + ChronoDuration::seconds(response.expires_in),
    })
}

pub(in crate::finance_data) fn classify_kis_token_error(
    status: StatusCode,
    bytes: &[u8],
    app_key: &str,
    app_secret: &str,
) -> FinanceDataError {
    let diagnostic = safe_kis_token_diagnostic(bytes, app_key, app_secret);
    let summary = diagnostic.summary().to_ascii_lowercase();
    if status == StatusCode::TOO_MANY_REQUESTS
        || summary.contains("egw00133")
        || summary.contains("1분당")
        || summary.contains("too many")
        || summary.contains("rate limit")
    {
        FinanceDataError::KisRateLimited(diagnostic)
    } else if matches!(
        status,
        StatusCode::OK | StatusCode::BAD_REQUEST | StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        FinanceDataError::KisCredentialRejected(diagnostic)
    } else {
        FinanceDataError::Provider(format!("{KIS_SOURCE_ID} returned HTTP {}", status.as_u16()))
    }
}

pub(in crate::finance_data) fn safe_kis_token_diagnostic(
    bytes: &[u8],
    app_key: &str,
    app_secret: &str,
) -> SafeProviderDiagnostic {
    let root = serde_json::from_slice::<Value>(bytes).ok();
    let code = root
        .as_ref()
        .and_then(|value| value.get("error_code").or_else(|| value.get("msg_cd")))
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        })
        .map(str::to_owned);
    let message = root
        .as_ref()
        .and_then(|value| value.get("error_description").or_else(|| value.get("msg1")))
        .and_then(Value::as_str)
        .map(|value| value.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|value| {
            !value.is_empty()
                && value.len() <= 512
                && !value.contains(app_key)
                && !value.contains(app_secret)
                && !value.chars().any(char::is_control)
        });
    SafeProviderDiagnostic { code, message }
}

/// Provider pages one `kis_read` may merge. Upstream examples stop at ten.
pub(in crate::finance_data) const MAX_KIS_PAGES: usize = 8;
/// Raw bytes across pages; each page keeps its own 8 MiB bound.
pub(in crate::finance_data) const MAX_KIS_TOTAL_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::finance_data) enum KisPageStop {
    Complete,
    PageCap,
    ByteCap,
    MissingCursor,
    RepeatedPage,
    LaterPageFailed,
}

impl KisPageStop {
    fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::PageCap => "page_cap",
            Self::ByteCap => "byte_cap",
            Self::MissingCursor => "missing_cursor",
            Self::RepeatedPage => "repeated_page",
            Self::LaterPageFailed => "later_page_failed",
        }
    }

    fn warning(self) -> Option<String> {
        Some(match self {
            Self::Complete => return None,
            Self::PageCap => format!(
                "KIS has more pages beyond the {MAX_KIS_PAGES}-page bound; narrow the query (dates, filters) to see the rest."
            ),
            Self::ByteCap => {
                "KIS has more pages beyond the 16 MiB response bound; narrow the query to see the rest."
                    .to_owned()
            }
            Self::MissingCursor => {
                "KIS reported more pages without a continuation key; only the pages shown were read."
                    .to_owned()
            }
            Self::RepeatedPage => {
                "KIS repeated the previous page; continuation stopped and later rows may be missing."
                    .to_owned()
            }
            Self::LaterPageFailed => {
                "A later KIS page failed; the result holds the earlier pages only.".to_owned()
            }
        })
    }
}

/// Raw provider pages in request order. Every page passed credential-echo
/// rejection before it was kept.
pub(in crate::finance_data) struct KisPages {
    pub(in crate::finance_data) pages: Vec<RawProviderResponse>,
    pub(in crate::finance_data) stop: KisPageStop,
}

impl KisPages {
    fn first(&self) -> Result<&RawProviderResponse, FinanceDataError> {
        self.pages.first().ok_or(FinanceDataError::InvalidResponse)
    }
}

fn kis_has_more(raw: &RawProviderResponse) -> bool {
    matches!(raw.continuation.as_deref(), Some("F" | "M"))
}

/// Next-page keys: each `CTX_AREA_*`/`CTS` query field is returned in the
/// body under its lowercase name. Header-only operations repeat the query.
fn kis_next_cursor(operation: &KisOperation, bytes: &[u8]) -> Option<Vec<(String, String)>> {
    let continuation = operation.continuation.as_ref()?;
    let root = serde_json::from_slice::<Value>(bytes).ok()?;
    continuation
        .query_fields
        .iter()
        .map(|wire_name| {
            root.get(wire_name.to_ascii_lowercase())
                .and_then(Value::as_str)
                .filter(|value| {
                    value.len() <= MAX_KIS_PARAMETER_CHARS && !value.chars().any(char::is_control)
                })
                .map(|value| (wire_name.clone(), value.to_owned()))
        })
        .collect()
}

pub(in crate::finance_data) fn kis_page_url(
    prepared: &PreparedKisRequest<'_>,
    cursor: &[(String, String)],
) -> Url {
    let mut url = prepared.url.clone();
    let mut pairs = prepared
        .url
        .query_pairs()
        .map(|(key, value)| {
            let value = cursor
                .iter()
                .find(|(wire_name, _)| *wire_name == key)
                .map_or_else(|| value.into_owned(), |(_, next)| next.clone());
            (key.into_owned(), value)
        })
        .collect::<Vec<_>>();
    for (wire_name, value) in cursor {
        if !pairs.iter().any(|(key, _)| key == wire_name) {
            pairs.push((wire_name.clone(), value.clone()));
        }
    }
    url.set_query(None);
    url.query_pairs_mut().extend_pairs(pairs);
    url
}

/// Follows `tr_cont` continuation (`F`/`M` = more) with `tr_cont: N` and the
/// returned cursor keys, bounded by page count, total bytes and pacing.
/// `fetch(url, continuing)` must reject credential echoes itself.
pub(in crate::finance_data) async fn collect_kis_pages<F, Fut>(
    prepared: &PreparedKisRequest<'_>,
    mut fetch: F,
    pacer: &RequestPacer<'_>,
) -> Result<KisPages, FinanceDataError>
where
    F: FnMut(Url, bool) -> Fut,
    Fut: std::future::Future<Output = Result<RawProviderResponse, FinanceDataError>>,
{
    let first = fetch(prepared.url.clone(), false).await?;
    let mut total_bytes = first.bytes.len();
    let mut pages = vec![first];
    let stop = loop {
        let last = pages.last().expect("the first page was kept");
        if prepared.operation.continuation.is_none() || !kis_has_more(last) {
            break KisPageStop::Complete;
        }
        if pages.len() >= MAX_KIS_PAGES {
            break KisPageStop::PageCap;
        }
        if total_bytes >= MAX_KIS_TOTAL_RESPONSE_BYTES {
            break KisPageStop::ByteCap;
        }
        let Some(cursor) = kis_next_cursor(prepared.operation, &last.bytes) else {
            break KisPageStop::MissingCursor;
        };
        pacer().await;
        let next = match fetch(kis_page_url(prepared, &cursor), true).await {
            Ok(next) => next,
            Err(
                FinanceDataError::NetworkSafe(_)
                | FinanceDataError::Network(_)
                | FinanceDataError::RateLimited(_)
                | FinanceDataError::Provider(_),
            ) => break KisPageStop::LaterPageFailed,
            Err(error) => return Err(error),
        };
        match validate_kis_response(&next.bytes, prepared.operation) {
            Ok(_) => {}
            Err(FinanceDataError::RateLimited(_) | FinanceDataError::Provider(_)) => {
                break KisPageStop::LaterPageFailed
            }
            Err(error) => return Err(error),
        }
        if next.bytes == last.bytes {
            break KisPageStop::RepeatedPage;
        }
        total_bytes = total_bytes.saturating_add(next.bytes.len());
        pages.push(next);
    };
    Ok(KisPages { pages, stop })
}

fn kis_container_rows(value: Value) -> Vec<Value> {
    match value {
        Value::Array(rows) => rows,
        Value::Object(row) if row.is_empty() => Vec::new(),
        row @ Value::Object(_) => vec![row],
        _ => Vec::new(),
    }
}

/// Validates and projects every page, then appends row-container rows in page
/// order (as the upstream examples do). A single-object container is a
/// per-request summary and keeps the first page's value; an array container
/// that repeats the first page's content (an account summary sent with every
/// page) is not appended again. Single-page shapes are unchanged.
/// Continuation cursors are transport state and are removed.
fn merge_kis_pages(
    prepared: &PreparedKisRequest<'_>,
    pages: &KisPages,
) -> Result<(Value, bool), FinanceDataError> {
    let operation = prepared.operation;
    let cursor_fields = operation
        .continuation
        .iter()
        .flat_map(|continuation| &continuation.query_fields)
        .map(|wire_name| wire_name.to_ascii_lowercase())
        .collect::<std::collections::BTreeSet<_>>();
    let mut merged: Option<Map<String, Value>> = None;
    let mut first_containers = Map::new();
    let mut dropped_unknown_fields = false;
    for raw in &pages.pages {
        let root = validate_kis_response(&raw.bytes, operation)?;
        let (response, dropped) = sanitize_kis_response(root, operation);
        dropped_unknown_fields |= dropped;
        let Value::Object(mut page) = response else {
            return Err(FinanceDataError::InvalidResponse);
        };
        page.retain(|key, _| !cursor_fields.contains(key));
        let Some(merged) = merged.as_mut() else {
            for key in &operation.response.containers {
                if let Some(value) = page.get(key) {
                    first_containers.insert(key.clone(), value.clone());
                }
            }
            merged = Some(page);
            continue;
        };
        for (key, value) in page {
            if operation.response.containers.contains(&key) {
                match first_containers.get(&key) {
                    Some(Value::Object(_)) => continue,
                    Some(first) if *first == value => continue,
                    _ => {}
                }
                let mut rows = merged
                    .remove(&key)
                    .map(kis_container_rows)
                    .unwrap_or_default();
                rows.extend(kis_container_rows(value));
                merged.insert(key, Value::Array(rows));
            } else {
                merged.insert(key, value);
            }
        }
    }
    let merged = merged.ok_or(FinanceDataError::InvalidResponse)?;
    Ok((Value::Object(merged), dropped_unknown_fields))
}

/// Retrieval clock and, for KRX-listed cash products, the trading session at
/// retrieval time; published KRX holidays are closed (`market_calendar`).
pub(in crate::finance_data) fn kis_market_time(
    product: &str,
    retrieved_at: &str,
) -> Map<String, Value> {
    let local = DateTime::parse_from_rfc3339(retrieved_at)
        .ok()
        .map(|time| time.with_timezone(&chrono_tz::Asia::Seoul));
    let (session, basis) = if matches!(product, "domestic_stock" | "etfetn" | "elw") {
        (
            local.map(|time| krx_equity_session(time.naive_local())),
            "KRX equity hours on trading days in Asia/Seoul: pre-market 08:00-09:00, regular 09:00-15:30, after-hours 15:30-18:00. Holidays KRX has published are closed; days in a year KRX has not published yet are treated as trading days.",
        )
    } else if product.starts_with("overseas_") {
        (None, "Overseas venue; KRX sessions do not apply.")
    } else {
        (None, "Trading sessions are not modeled for this product.")
    };
    let mut fields = Map::new();
    fields.insert("timezone".into(), json!("Asia/Seoul"));
    fields.insert(
        "retrieved_at_local".into(),
        json!(local.map(|time| time.to_rfc3339_opts(SecondsFormat::Millis, false))),
    );
    fields.insert("session_at_retrieval".into(), json!(session));
    fields.insert("session_basis".into(), json!(basis));
    fields
}

pub(in crate::finance_data) fn krx_equity_session(local: chrono::NaiveDateTime) -> &'static str {
    use chrono::Timelike;
    if !crate::market_calendar::is_trading_day("KRX", local.date()) {
        return "closed";
    }
    match local.hour() * 60 + local.minute() {
        480..=539 => "pre_market",
        540..=929 => "regular",
        930..=1079 => "after_hours",
        _ => "closed",
    }
}

struct KisNormalized {
    data: Value,
    returned: usize,
    dropped_unknown_fields: bool,
}

fn normalize_kis_pages(
    prepared: &PreparedKisRequest<'_>,
    pages: &KisPages,
    profile: Option<&KisAccountProfile>,
) -> Result<KisNormalized, FinanceDataError> {
    let (response, dropped_unknown_fields) = merge_kis_pages(prepared, pages)?;
    if contains_sensitive_kis_response_key(&response)
        || profile.is_some_and(|profile| contains_kis_profile_value(&response, profile))
    {
        return Err(FinanceDataError::InvalidResponse);
    }
    let returned = kis_response_record_count(&response);
    if returned == 0 {
        return Err(FinanceDataError::NoData(KIS_SOURCE_ID));
    }
    let data = json!({
        "operation": {
            "id": prepared.operation.id,
            "product": prepared.operation.product,
            "category": prepared.operation.category,
            "name": prepared.operation.name
        },
        "response": response
    });
    Ok(KisNormalized {
        data,
        returned,
        dropped_unknown_fields,
    })
}

fn kis_provenance(
    prepared: &PreparedKisRequest<'_>,
    pages: &KisPages,
    data: &Value,
    revision_semantics: Option<&str>,
) -> Result<Value, FinanceDataError> {
    let first = pages.first()?;
    let mut provenance = Map::new();
    provenance.insert(
        "data_authority".into(),
        json!("Korea Investment & Securities"),
    );
    provenance.insert(
        "provider".into(),
        json!("Korea Investment Open Trading API"),
    );
    provenance.insert("official_source".into(), json!(true));
    provenance.insert(
        "source_origin".into(),
        json!(format!(
            "{}{}",
            prepared.url.origin().ascii_serialization(),
            prepared.operation.path
        )),
    );
    provenance.insert("retrieved_at".into(), json!(first.retrieved_at));
    if let Some(last) = pages.pages.last().filter(|_| pages.pages.len() > 1) {
        provenance.insert("last_page_retrieved_at".into(), json!(last.retrieved_at));
    }
    provenance.insert("normalized_sha256".into(), json!(normalized_sha256(data)?));
    provenance.insert("raw_persisted".into(), json!(false));
    if let Some(semantics) = revision_semantics {
        provenance.insert("revision_semantics".into(), json!(semantics));
    }
    provenance.extend(kis_market_time(
        &prepared.operation.product,
        &first.retrieved_at,
    ));
    Ok(Value::Object(provenance))
}

fn kis_page(pages: &KisPages, returned: usize) -> Value {
    json!({
        "returned": returned,
        "available": returned,
        "truncated": pages.stop != KisPageStop::Complete,
        "provider_pages": pages.pages.len(),
        "page_limit": MAX_KIS_PAGES,
        "stop_reason": pages.stop.as_str()
    })
}

pub(in crate::finance_data) fn normalize_kis_market_response(
    prepared: &PreparedKisRequest<'_>,
    pages: &KisPages,
    profile: Option<&KisAccountProfile>,
) -> Result<Value, FinanceDataError> {
    let normalized = normalize_kis_pages(prepared, pages, profile)?;
    let mut warnings = vec![
        "KIS V1 validates the reviewed request, provider success envelope, and operation-level response-field allowlist, but not request-to-response identity. Treat fields that depend on response identity with caution."
            .to_owned(),
    ];
    warnings.extend(pages.stop.warning());
    if normalized.dropped_unknown_fields {
        warnings.push(
            "KIS returned fields outside the reviewed response schema; those fields were omitted."
                .to_owned(),
        );
    }
    Ok(json!({
        "schema_version": "loupist-kis-result/1",
        "tool": "kis_read",
        "operation": "market.operation",
        "source_id": KIS_SOURCE_ID,
        "query": {
            "provider": KIS_SOURCE_ID,
            "operation_id": prepared.operation.id,
            "params": prepared.tool_params
        },
        "provenance": kis_provenance(prepared, pages, &normalized.data, Some("latest_only"))?,
        "data": normalized.data,
        "quality": {
            "source_class": "official",
            "status": "warn",
            "checks": [
                {"code": "provider_success", "status": "pass"},
                {"code": "response_schema_allowlist", "status": "pass"},
                {"code": "response_identity", "status": "warn"}
            ],
            "warnings": warnings
        },
        "warnings": warnings,
        "page": kis_page(pages, normalized.returned)
    }))
}

pub(in crate::finance_data) fn normalize_kis_account_response(
    prepared: &PreparedKisRequest<'_>,
    pages: &KisPages,
    profile: Option<&KisAccountProfile>,
) -> Result<Value, FinanceDataError> {
    let normalized = normalize_kis_pages(prepared, pages, profile)?;
    let mut warnings = vec![
        "This account result is projected through an operation-level response allowlist and strips account-shaped fields and reflected profile values, but it has no request-to-response identity rule."
            .to_owned(),
    ];
    warnings.extend(pages.stop.warning());
    if normalized.dropped_unknown_fields {
        warnings.push(
            "KIS returned fields outside the reviewed response schema; those fields were omitted."
                .to_owned(),
        );
    }
    Ok(json!({
        "schema_version": "loupist-kis-account-result/1",
        "tool": "kis_read",
        "source_id": KIS_SOURCE_ID,
        "operation": "account.read",
        "query": {
            "provider": KIS_SOURCE_ID,
            "operation_id": prepared.operation.id,
            "params": prepared.tool_params
        },
        "provenance": kis_provenance(prepared, pages, &normalized.data, None)?,
        "data": normalized.data,
        "quality": {
            "status": "warn",
            "checks": [
                {"code": "provider_success", "status": "pass"},
                {"code": "response_schema_allowlist", "status": "pass"},
                {"code": "response_identity", "status": "warn"},
                {"code": "account_identifiers_redacted", "status": "warn"}
            ],
            "warnings": warnings
        },
        "page": kis_page(pages, normalized.returned)
    }))
}

pub(in crate::finance_data) fn validate_kis_response(
    bytes: &[u8],
    operation: &KisOperation,
) -> Result<Value, FinanceDataError> {
    let mismatch = |detail: String| FinanceDataError::KisResponseContract {
        operation: operation.id.clone(),
        detail,
    };
    let root: Value = serde_json::from_slice(bytes)
        .map_err(|_| mismatch("provider body was not valid JSON".to_owned()))?;
    let object = root
        .as_object()
        .ok_or_else(|| mismatch("top-level value was not an object".to_owned()))?;
    let status = object
        .get("rt_cd")
        .and_then(Value::as_str)
        .ok_or_else(|| mismatch("rt_cd was missing or not a string".to_owned()))?;
    if status == "0" {
        validate_kis_success_shape(object, operation)?;
        return Ok(root);
    }
    let code = object
        .get("msg_cd")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let message = object
        .get("msg1")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if [code.as_str(), message.as_str()].iter().any(|value| {
        value.contains("token")
            || value.contains("appkey")
            || value.contains("app key")
            || value.contains("인증")
            || value.contains("접근토큰")
    }) {
        return Err(FinanceDataError::CredentialRejected(KIS_SOURCE_ID));
    }
    if [code.as_str(), message.as_str()].iter().any(|value| {
        value.contains("rate")
            || value.contains("초당")
            || value.contains("호출 횟수")
            || value.contains("too many")
    }) {
        return Err(FinanceDataError::RateLimited(KIS_SOURCE_ID));
    }
    Err(FinanceDataError::Provider(format!(
        "{KIS_SOURCE_ID} rejected reviewed operation {}",
        operation.id
    )))
}

pub(in crate::finance_data) fn validate_kis_success_shape(
    object: &Map<String, Value>,
    operation: &KisOperation,
) -> Result<(), FinanceDataError> {
    let contract = &operation.response;
    let mismatch = |detail: String| FinanceDataError::KisResponseContract {
        operation: operation.id.clone(),
        detail,
    };
    let safe_names = |names: Vec<&str>| {
        let unsupported_kind = names.first().map(|name| {
            if name.is_empty() {
                "blank field name"
            } else if name.chars().all(char::is_whitespace) {
                "whitespace-only field name"
            } else if !name.is_ascii() {
                "non-ASCII field name"
            } else if name.len() > 64 {
                "overlong field name"
            } else {
                "field name with unsupported punctuation"
            }
        });
        let names = names
            .into_iter()
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= 64
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            })
            .take(8)
            .collect::<Vec<_>>();
        if names.is_empty() {
            unsupported_kind
                .unwrap_or("unsupported field name")
                .to_owned()
        } else {
            names.join(", ")
        }
    };
    let allowed_fields = contract
        .allowed_fields
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let unknown_top_level = object
        .keys()
        .filter(|key| {
            !matches!(key.as_str(), "rt_cd" | "msg_cd" | "msg1")
                && !contract
                    .containers
                    .iter()
                    .any(|container| container == *key)
                && !contract.top_level_fields.iter().any(|field| field == *key)
        })
        .map(String::as_str)
        .collect::<Vec<_>>();
    if unknown_top_level
        .iter()
        .any(|field| sensitive_kis_response_key(field))
    {
        return Err(mismatch(format!(
            "unexpected sensitive top-level field(s): {}",
            safe_names(
                unknown_top_level
                    .into_iter()
                    .filter(|field| sensitive_kis_response_key(field))
                    .collect(),
            )
        )));
    }
    let missing_containers = contract
        .containers
        .iter()
        .filter(|container| !object.contains_key(*container))
        .map(String::as_str)
        .collect::<Vec<_>>();
    if !missing_containers.is_empty() {
        return Err(mismatch(format!(
            "missing response container(s): {}",
            safe_names(missing_containers)
        )));
    }
    for field in &contract.top_level_fields {
        if object.get(field).is_none_or(|value| {
            !matches!(
                value,
                Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
            )
        }) {
            return Err(mismatch(format!(
                "top-level field {} was missing or non-scalar",
                safe_names(vec![field])
            )));
        }
    }

    for container in &contract.containers {
        let value = object
            .get(container)
            .expect("the required response containers were checked above");
        let rows = match value {
            Value::Array(rows) => rows.iter().collect::<Vec<_>>(),
            Value::Object(row) if row.is_empty() => Vec::new(),
            Value::Object(_) => vec![value],
            _ => {
                return Err(mismatch(format!(
                    "container {} was not an object or array",
                    safe_names(vec![container])
                )));
            }
        };
        for row in rows {
            let row = row.as_object().ok_or_else(|| {
                mismatch(format!(
                    "container {} included a non-object row",
                    safe_names(vec![container])
                ))
            })?;
            if row.is_empty() {
                return Err(mismatch(format!(
                    "container {} included an empty row",
                    safe_names(vec![container])
                )));
            }
            let unknown_fields = row
                .keys()
                .filter(|key| !allowed_fields.contains(key.as_str()))
                .map(String::as_str)
                .collect::<Vec<_>>();
            if unknown_fields
                .iter()
                .any(|field| sensitive_kis_response_key(field))
            {
                return Err(mismatch(format!(
                    "container {} included unexpected sensitive field(s): {}",
                    safe_names(vec![container]),
                    safe_names(
                        unknown_fields
                            .into_iter()
                            .filter(|field| sensitive_kis_response_key(field))
                            .collect(),
                    )
                )));
            }
            if row.iter().any(|(field, value)| {
                allowed_fields.contains(field.as_str())
                    && !matches!(
                        value,
                        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)
                    )
            }) {
                return Err(mismatch(format!(
                    "container {} included a non-scalar field value",
                    safe_names(vec![container])
                )));
            }
        }
    }
    Ok(())
}

pub(in crate::finance_data) fn reject_kis_credential_echo(
    bytes: &[u8],
    app_key: &str,
    app_secret: &str,
    access_token: &str,
) -> Result<(), FinanceDataError> {
    fn canonical_key(key: &str) -> String {
        key.bytes()
            .filter(|byte| byte.is_ascii_alphanumeric())
            .map(|byte| byte.to_ascii_lowercase() as char)
            .collect()
    }

    fn contains_credential(
        value: &Value,
        app_key: &str,
        app_secret: &str,
        access_token: &str,
    ) -> bool {
        match value {
            Value::Array(values) => values
                .iter()
                .any(|value| contains_credential(value, app_key, app_secret, access_token)),
            Value::Object(object) => object.iter().any(|(key, value)| {
                matches!(
                    canonical_key(key).as_str(),
                    "appkey"
                        | "appsecret"
                        | "accesstoken"
                        | "authorization"
                        | "bearertoken"
                        | "token"
                ) || contains_credential(value, app_key, app_secret, access_token)
            }),
            Value::String(value) => {
                value.contains(app_key)
                    || value.contains(app_secret)
                    || value.contains(access_token)
                    || value.contains(&format!("Bearer {access_token}"))
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => false,
        }
    }

    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    if contains_credential(&root, app_key, app_secret, access_token) {
        return Err(FinanceDataError::InvalidResponse);
    }
    Ok(())
}

pub(in crate::finance_data) fn sanitize_kis_response(
    mut value: Value,
    operation: &KisOperation,
) -> (Value, bool) {
    fn retain_allowed_row(
        value: &mut Value,
        allowed_fields: &std::collections::BTreeSet<&str>,
    ) -> bool {
        match value {
            Value::Array(rows) => {
                let mut dropped = false;
                for row in rows {
                    dropped |= retain_allowed_row(row, allowed_fields);
                }
                dropped
            }
            Value::Object(row) => {
                let before = row.len();
                row.retain(|key, _| {
                    allowed_fields.contains(key.as_str()) && !sensitive_kis_response_key(key)
                });
                row.len() != before
            }
            _ => false,
        }
    }

    let allowed_fields = operation
        .response
        .allowed_fields
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    let mut dropped_unknown_fields = false;
    if let Some(object) = value.as_object_mut() {
        let before = object.len();
        object.retain(|key, _| {
            operation
                .response
                .containers
                .iter()
                .any(|container| container == key)
                || operation
                    .response
                    .top_level_fields
                    .iter()
                    .any(|field| field == key)
        });
        dropped_unknown_fields |= object.len() + 3 < before;
        for container in &operation.response.containers {
            if let Some(rows) = object.get_mut(container) {
                dropped_unknown_fields |= retain_allowed_row(rows, &allowed_fields);
            }
        }
    }
    (value, dropped_unknown_fields)
}

pub(in crate::finance_data) fn sensitive_kis_response_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    let canonical = key
        .bytes()
        .filter(|byte| byte.is_ascii_alphanumeric())
        .map(|byte| byte.to_ascii_lowercase() as char)
        .collect::<String>();
    matches!(
        key.as_str(),
        "cano"
            | "acnt_prdt_cd"
            | "acnt_pwd"
            | "acnt_no"
            | "account_no"
            | "acct_no"
            | "acnt_name"
            | "cust_name"
            | "cust_rncno25"
            | "user_id"
            | "hts_id"
            | "hmid"
            | "odno"
            | "orgn_odno"
            | "ord_gno_brno"
            | "inqr_ip_addr"
            | "ctac_tlno"
    ) || matches!(
        canonical.as_str(),
        "appkey" | "appsecret" | "accesstoken" | "authorization" | "bearertoken" | "token"
    ) || key.contains("address")
        || key.contains("addr")
        || key.contains("email")
        || key.ends_with("_tlno")
}

pub(in crate::finance_data) fn contains_sensitive_kis_response_key(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_sensitive_kis_response_key),
        Value::Object(object) => object.iter().any(|(key, value)| {
            sensitive_kis_response_key(key) || contains_sensitive_kis_response_key(value)
        }),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

pub(in crate::finance_data) fn contains_kis_profile_value(
    value: &Value,
    profile: &KisAccountProfile,
) -> bool {
    match value {
        Value::Array(values) => values
            .iter()
            .any(|value| contains_kis_profile_value(value, profile)),
        Value::Object(object) => object
            .values()
            .any(|value| contains_kis_profile_value(value, profile)),
        Value::String(value) => profile
            .sensitive_echo_values()
            .any(|sensitive| value.contains(sensitive)),
        Value::Null | Value::Bool(_) | Value::Number(_) => false,
    }
}

pub(in crate::finance_data) fn kis_response_record_count(response: &Value) -> usize {
    response
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter(|(key, _)| key.starts_with("output"))
        .map(|(_, value)| match value {
            Value::Array(values) => values.len(),
            Value::Object(object) if !object.is_empty() => 1,
            Value::String(value) if !value.is_empty() => 1,
            Value::Number(_) | Value::Bool(_) => 1,
            _ => 0,
        })
        .sum()
}
