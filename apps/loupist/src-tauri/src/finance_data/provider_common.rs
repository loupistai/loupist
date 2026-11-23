use super::*;

pub(super) fn parse_date(
    value: &str,
    message: &'static str,
) -> Result<NaiveDate, FinanceDataError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| FinanceDataError::InvalidQuery(message))
}

pub(super) fn parse_compact_date(value: &str) -> Result<NaiveDate, FinanceDataError> {
    NaiveDate::parse_from_str(value, "%Y%m%d").map_err(|_| FinanceDataError::InvalidResponse)
}

pub(super) fn validate_date_range(
    start: &str,
    end: &str,
    maximum_days: i64,
) -> Result<(NaiveDate, NaiveDate), FinanceDataError> {
    validate_date_range_at(start, end, maximum_days, Utc::now())
}

fn validate_date_range_at(
    start: &str,
    end: &str,
    maximum_days: i64,
    now: DateTime<Utc>,
) -> Result<(NaiveDate, NaiveDate), FinanceDataError> {
    let start = parse_date(start, "start date is invalid")?;
    let end = parse_date(end, "end date is invalid")?;
    // OpenDART dates are in Seoul, which reaches a new day before UTC.
    let today = now.with_timezone(&chrono_tz::Asia::Seoul).date_naive();
    if start > end || start.year() < 1900 || end > today || (end - start).num_days() > maximum_days
    {
        return Err(FinanceDataError::InvalidQuery(
            "date range is invalid or exceeds the bounded span",
        ));
    }
    Ok((start, end))
}

pub(super) fn validate_fiscal_year(year: i32) -> Result<(), FinanceDataError> {
    if year < 1994 || year > Utc::now().year() + 1 {
        return Err(FinanceDataError::InvalidQuery("fiscal year is invalid"));
    }
    Ok(())
}

pub(super) fn validate_digits(
    value: &str,
    length: usize,
    message: &'static str,
) -> Result<String, FinanceDataError> {
    if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FinanceDataError::InvalidQuery(message));
    }
    Ok(value.to_owned())
}

pub(super) fn dart_report_code(value: &str) -> Result<&'static str, FinanceDataError> {
    match value {
        "annual" => Ok("11011"),
        "q1" => Ok("11013"),
        "half_year" => Ok("11012"),
        "q3" => Ok("11014"),
        _ => Err(FinanceDataError::InvalidQuery(
            "report_period is unsupported",
        )),
    }
}

pub(super) fn validate_dart_forms(forms: &[String]) -> Result<(), FinanceDataError> {
    if forms.len() > 1
        || forms
            .iter()
            .any(|form| form.len() != 1 || !matches!(form.as_bytes()[0], b'A'..=b'J'))
    {
        return Err(FinanceDataError::InvalidQuery(
            "OpenDART accepts at most one disclosure type A through J",
        ));
    }
    Ok(())
}

pub(super) fn canonical_decimal(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(text) => text.replace(',', ""),
        // JSON serializes small/large floats with an exponent. Display expands
        // the already-parsed finite float without a second numeric conversion;
        // integers and provider decimal strings keep their exact digits.
        Value::Number(number) if number.is_f64() => number.as_f64()?.to_string(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    let raw = raw.trim();
    if raw.is_empty() || raw == "." {
        return None;
    }
    let (negative, unsigned) = raw
        .strip_prefix('-')
        .map_or((false, raw), |rest| (true, rest));
    let mut pieces = unsigned.split('.');
    let integer = pieces.next()?;
    let fraction = pieces.next();
    if pieces.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let integer = integer.trim_start_matches('0');
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = fraction.unwrap_or_default().trim_end_matches('0');
    let mut normalized = if fraction.is_empty() {
        integer.to_owned()
    } else {
        format!("{integer}.{fraction}")
    };
    if negative && normalized != "0" {
        normalized.insert(0, '-');
    }
    Some(normalized)
}

pub(super) fn normalized_sha256(value: &Value) -> Result<String, FinanceDataError> {
    let bytes = serde_json::to_vec(&canonicalize_json(value))
        .map_err(|_| FinanceDataError::InvalidResponse)?;
    Ok(hex::encode(Sha256::digest(bytes)))
}

pub(super) fn canonicalize_json(value: &Value) -> Value {
    match value {
        Value::Array(values) => Value::Array(values.iter().map(canonicalize_json).collect()),
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            let mut canonical = Map::new();
            for key in keys {
                canonical.insert(key.clone(), canonicalize_json(&object[key]));
            }
            Value::Object(canonical)
        }
        scalar => scalar.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn finance_result(
    tool: &str,
    operation: &str,
    source_id: &str,
    query: Value,
    data: Value,
    raw: &RawProviderResponse,
    data_authority: &str,
    provider: &str,
    official_source: bool,
    source_class: &str,
    revision_semantics: &str,
    quality_status: &str,
    checks: Value,
    warnings: Vec<String>,
    returned: usize,
    available: usize,
    truncated: bool,
) -> Result<Value, FinanceDataError> {
    Ok(json!({
        "schema_version": "loupist-finance-result/1",
        "tool": tool,
        "operation": operation,
        "source_id": source_id,
        "query": query,
        "data": data,
        "provenance": {
            "data_authority": data_authority,
            "provider": provider,
            "official_source": official_source,
            "source_url": raw.source_url,
            "source_urls": [raw.source_url],
            "retrieved_at": raw.retrieved_at,
            "revision_semantics": revision_semantics
        },
        "quality": {
            "source_class": source_class,
            "status": quality_status,
            "checks": checks,
            "warnings": warnings
        },
        "warnings": warnings,
        "page": {
            "returned": returned,
            "available": available,
            "truncated": truncated
        }
    }))
}

pub(super) fn standard_checks() -> Value {
    json!([
        {"code": "provider_success", "status": "pass"},
        {"code": "response_complete", "status": "pass"},
        {"code": "schema_valid", "status": "pass"}
    ])
}

pub(super) fn validate_ohlcv_strings(
    open: &str,
    high: &str,
    low: &str,
    close: &str,
    volume: &str,
) -> Result<(), FinanceDataError> {
    let parse = |value: &str| {
        value
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
            .ok_or(FinanceDataError::InvalidResponse)
    };
    let open = parse(open)?;
    let high = parse(high)?;
    let low = parse(low)?;
    let close = parse(close)?;
    let volume = parse(volume)?;
    if low > open || low > close || low > high || high < open || high < close || volume < 0.0 {
        return Err(FinanceDataError::InvalidResponse);
    }
    Ok(())
}

/// Visible text of an HTML or XML document, up to `maximum` characters.
pub(super) fn extract_text_within(bytes: &[u8], maximum: usize) -> (String, bool) {
    let decoded = String::from_utf8_lossy(bytes);
    let document = Html::parse_document(&decoded);
    let normalized = document
        .root_element()
        .text()
        .flat_map(str::split_whitespace)
        .collect::<Vec<_>>()
        .join(" ");
    truncate_chars(normalized, maximum)
}

pub(super) fn truncate_chars(text: String, maximum: usize) -> (String, bool) {
    if text.chars().count() <= maximum {
        return (text, false);
    }
    (text.chars().take(maximum).collect(), true)
}

pub(super) fn detect_krx_error(bytes: &[u8]) -> Result<Value, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    let code = root.pointer("/result/code").and_then(Value::as_str);
    match code {
        Some("000") => Ok(root),
        Some("401" | "403") => Err(FinanceDataError::CredentialRejected(KRX_SOURCE_ID)),
        Some("429") => Err(FinanceDataError::RateLimited(KRX_SOURCE_ID)),
        Some(code @ ("400" | "422")) => {
            let field = root
                .pointer("/result/message")
                .and_then(Value::as_str)
                .filter(|message| {
                    message
                        .split(|c: char| !c.is_ascii_alphanumeric())
                        .any(|word| word.eq_ignore_ascii_case("basDd"))
                })
                .map(|_| "date");
            Err(rejected_provider_arguments("KRX", code, field))
        }
        Some(_) => Err(FinanceDataError::Provider(
            "KRX returned a provider status error".into(),
        )),
        None if root.get("OutBlock_1").is_some_and(Value::is_array) => Ok(root),
        None => Err(FinanceDataError::InvalidResponse),
    }
}

fn rejected_provider_arguments(
    provider: &str,
    code: &str,
    field: Option<&str>,
) -> FinanceDataError {
    let diagnostic = match field {
        Some(field) => format!("{provider} rejected the {field} in this request (code {code})."),
        None => format!("{provider} rejected a value in this request (code {code})."),
    };
    FinanceDataError::RejectedArguments {
        diagnostic,
        next: "Check the identifiers and dates sent against this tool's parameter descriptions and read again with corrected values; the same call fails the same way.",
    }
}

/// An OpenDART status code that refuses the request; success (`000`) and
/// no data (`013`) pass. OpenDART sends these in an HTTP 200 body; its 100
/// message names no field (an unknown `reprt_code` is even echoed back as
/// data), so the host validates arguments before a request instead.
fn dart_status_error(status: Option<&str>) -> Result<(), FinanceDataError> {
    match status {
        Some("000" | "013") => Ok(()),
        Some("014") => Err(FinanceDataError::NoData(OPENDART_SOURCE_ID)),
        Some(code @ ("010" | "011" | "012" | "901")) => {
            Err(FinanceDataError::DartKeyRefused(match code {
                "010" => "010",
                "011" => "011",
                "012" => "012",
                _ => "901",
            }))
        }
        Some("020") => Err(FinanceDataError::RateLimited(OPENDART_SOURCE_ID)),
        Some(code @ ("021" | "100")) => Err(rejected_provider_arguments("OpenDART", code, None)),
        Some("800") => Err(FinanceDataError::ProviderMaintenance(OPENDART_SOURCE_ID)),
        Some(_) => Err(FinanceDataError::Provider(
            "OpenDART returned a provider status error".to_owned(),
        )),
        None => Err(FinanceDataError::InvalidResponse),
    }
}

pub(super) fn validate_dart_status_allow_no_data(bytes: &[u8]) -> Result<Value, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    dart_status_error(root.get("status").and_then(Value::as_str))?;
    Ok(root)
}

pub(super) fn validate_dart_status(bytes: &[u8]) -> Result<Value, FinanceDataError> {
    let root = validate_dart_status_allow_no_data(bytes)?;
    if root.get("status").and_then(Value::as_str) == Some("013") {
        return Err(FinanceDataError::NoData(OPENDART_SOURCE_ID));
    }
    Ok(root)
}

/// A document request's reply that is a status rather than a ZIP archive:
/// JSON (`{"status":"013",…}`) or XML (`<result><status>013</status>…`). No
/// data (`013`) is [`FinanceDataError::NoData`], other statuses map as for
/// JSON reads, and a status reply that reports success without a document is
/// invalid. Any other body passes to the archive reader.
pub(super) fn validate_dart_document_status(bytes: &[u8]) -> Result<(), FinanceDataError> {
    let body = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let body = &body[body
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(body.len())..];
    let status = match body.first() {
        Some(b'{') => {
            validate_dart_status(body)?;
            return Err(FinanceDataError::InvalidResponse);
        }
        Some(b'<') => {
            let text = String::from_utf8_lossy(body);
            let status = text
                .split_once("<status>")
                .and_then(|(_, rest)| rest.split_once("</status>"))
                .map(|(status, _)| status.trim().to_owned());
            dart_status_error(status.as_deref())?;
            status
        }
        _ => return Ok(()),
    };
    if status.as_deref() == Some("013") {
        return Err(FinanceDataError::NoData(OPENDART_SOURCE_ID));
    }
    Err(FinanceDataError::InvalidResponse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_filing_ranges_follow_seoul_before_utc_midnight() {
        let now = DateTime::parse_from_rfc3339("2026-11-02T01:00:00+09:00")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(now.date_naive().to_string(), "2026-11-01");
        let (start, end) = validate_date_range_at("2025-11-02", "2026-11-02", 366, now).unwrap();
        assert_eq!(start.to_string(), "2025-11-02");
        assert_eq!(end.to_string(), "2026-11-02");
        for (clock, valid) in [
            ("2026-11-01T23:59:59+09:00", false),
            ("2026-11-02T00:00:00+09:00", true),
            ("2026-11-02T08:59:59+09:00", true),
            ("2026-11-02T09:00:00+09:00", true),
        ] {
            let clock = DateTime::parse_from_rfc3339(clock)
                .unwrap()
                .with_timezone(&Utc);
            assert_eq!(
                validate_date_range_at("2025-11-02", "2026-11-02", 366, clock).is_ok(),
                valid,
                "{clock}"
            );
        }
        // Tomorrow in Seoul is still a future query; the bounded span and
        // ordering rules also apply at the timezone boundary.
        for (start, end) in [
            ("2026-11-02", "2026-11-03"),
            ("2025-10-31", "2026-11-02"),
            ("2026-11-02", "2026-11-01"),
        ] {
            assert!(matches!(
                validate_date_range_at(start, end, 366, now),
                Err(FinanceDataError::InvalidQuery(_))
            ));
        }
    }

    #[test]
    fn request_error_envelopes_stay_distinct_from_auth_and_malformed_replies() {
        let dart = br#"{"status":"100","message":"Field value is not valid."}"#;
        assert!(matches!(
            validate_dart_status(dart),
            Err(FinanceDataError::RejectedArguments { diagnostic, next })
                if diagnostic == "OpenDART rejected a value in this request (code 100)."
                && next == "Check the identifiers and dates sent against this tool's parameter descriptions and read again with corrected values; the same call fails the same way."
        ));
        let krx = br#"{"result":{"code":"400","message":"Bad Request"}}"#;
        for (provider, body) in [
            (OPENDART_SOURCE_ID, dart.as_slice()),
            (KRX_SOURCE_ID, krx.as_slice()),
        ] {
            let raw = RawProviderResponse {
                bytes: body.to_vec(),
                source_url: String::new(),
                retrieved_at: String::new(),
                continuation: None,
            };
            assert!(matches!(
                interpret_provider_status(provider, StatusCode::BAD_REQUEST, raw),
                Err(FinanceDataError::RejectedArguments { .. })
            ));
        }

        assert!(matches!(
            detect_krx_error(krx),
            Err(FinanceDataError::RejectedArguments { diagnostic, .. })
                if diagnostic == "KRX rejected a value in this request (code 400)."
        ));
        let krx_named = br#"{"result":{"code":"400","message":"Invalid basDd"}}"#;
        assert!(matches!(
            detect_krx_error(krx_named),
            Err(FinanceDataError::RejectedArguments { diagnostic, .. })
                if diagnostic == "KRX rejected the date in this request (code 400)."
        ));
        assert!(matches!(
            detect_krx_error(br#"{"result":{"code":"403","message":"Unauthorized API Call"}}"#),
            Err(FinanceDataError::CredentialRejected(KRX_SOURCE_ID))
        ));
        assert!(matches!(
            detect_krx_error(br#"{"result":{"code":"500","message":"error"}}"#),
            Err(FinanceDataError::Provider(_))
        ));
        assert!(matches!(
            detect_krx_error(br#"{"unexpected":true}"#),
            Err(FinanceDataError::InvalidResponse)
        ));
    }

    /// rc1: OpenDART answered a missing document with an XML status, which
    /// the filing read reported as an invalid response.
    #[test]
    fn a_document_reads_xml_status_like_json_status() {
        let xml = |status: &str| {
            format!("<result><status>{status}</status><message>조회된 데이타가 없습니다.</message></result>")
        };
        let outcome =
            |body: &[u8]| validate_dart_document_status(body).map_err(|error| error.to_string());
        let no_data =
            Err("finance data provider returned no matching data: opendart.disclosures".to_owned());
        assert_eq!(outcome(xml("013").as_bytes()), no_data);
        let declared = format!(
            "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}",
            xml(" 013 ")
        );
        assert_eq!(outcome(declared.as_bytes()), no_data);
        assert_eq!(outcome(br#"{"status":"013","message":"no data"}"#), no_data);
        assert_eq!(outcome(br#"  {"status":"013"}"#), no_data);
        for (status, expected) in [
            ("010", "OpenDART refused the saved key (status 010)"),
            ("011", "OpenDART refused the saved key (status 011)"),
            ("012", "OpenDART refused the saved key (status 012)"),
            ("901", "OpenDART refused the saved key (status 901)"),
            ("020", "finance data provider rate limit was reached: opendart.disclosures"),
            ("800", "finance data provider opendart.disclosures is stopped for maintenance"),
            ("100", "finance data request is invalid: OpenDART rejected a value in this request (code 100)."),
        ] {
            assert_eq!(outcome(xml(status).as_bytes()), Err(expected.to_owned()), "{status}");
            let json = format!(r#"{{"status":"{status}"}}"#);
            assert_eq!(outcome(json.as_bytes()), Err(expected.to_owned()), "{status}");
        }
        // A status reply without a document, or without a status, is invalid.
        let invalid = Err("finance data provider returned an invalid response".to_owned());
        assert_eq!(outcome(xml("000").as_bytes()), invalid);
        assert_eq!(outcome(br#"{"status":"000"}"#), invalid);
        assert_eq!(
            outcome(b"<result><message>error</message></result>"),
            invalid
        );
        assert_eq!(outcome(b"<html>bad gateway</html>"), invalid);
        // A ZIP archive passes to the document reader.
        assert_eq!(
            outcome(b"PK\x03\x04\x14\x00\x00\x00\x08\x00\xff\xfe"),
            Ok(())
        );
    }
}
