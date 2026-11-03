use super::*;

pub(super) fn bounded_chart_query_end(
    rows: &[Vec<Value>],
    offset: usize,
    requested_end: usize,
) -> Result<usize, BrokerError> {
    let mut bytes = 2_usize;
    let mut end = offset;
    for row in &rows[offset..requested_end] {
        let row_bytes = serde_json::to_vec(row)
            .map_err(|_| BrokerError::Execution("chart rows could not be serialized".into()))?
            .len();
        let separator = usize::from(end > offset);
        let next = bytes
            .checked_add(separator)
            .and_then(|value| value.checked_add(row_bytes))
            .ok_or_else(|| BrokerError::Execution("chart row window is too large".into()))?;
        if next > MAX_CHART_QUERY_BYTES {
            break;
        }
        bytes = next;
        end += 1;
    }
    if end == offset && offset < requested_end {
        return Err(BrokerError::Execution(
            "one chart row exceeds the query response limit".into(),
        ));
    }
    Ok(end)
}

pub(super) fn chart_edit_token(revision: &ChatArtifactRevision) -> String {
    sha256(
        format!(
            "{}:{}:{}",
            revision.artifact_id, revision.revision, revision.digest
        )
        .as_bytes(),
    )
}

pub(super) fn exact_object<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, BrokerError> {
    let object = value
        .as_object()
        .ok_or_else(|| BrokerError::invalid("arguments", "expected a JSON object"))?;
    if let Some(key) = required.iter().find(|key| !object.contains_key(**key)) {
        return Err(BrokerError::invalid(*key, "required field is missing"));
    }
    if let Some(key) = object
        .keys()
        .find(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return Err(BrokerError::invalid(key, "unexpected field; omit it"));
    }
    Ok(object)
}

pub(super) fn parse_as_of_date(
    value: Option<&str>,
) -> Result<Option<chrono::NaiveDate>, BrokerError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map(Some)
        .map_err(|_| BrokerError::invalid("as_of", "expected a calendar date in YYYY-MM-DD format"))
}

pub(super) fn effective_as_of(
    policy: &ToolPolicy,
    params: &Value,
) -> Result<Option<chrono::NaiveDate>, BrokerError> {
    let tool = parse_as_of_date(params.get("as_of").and_then(Value::as_str))?;
    let turn = parse_as_of_date(policy.as_of.as_deref())?;
    Ok(match (turn, tool) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (left, right) => left.or(right),
    })
}

/// Upper bound for `max_age`: thirty days, in seconds.
pub(super) const MAX_AGE_SECONDS_LIMIT: u64 = 30 * 24 * 60 * 60;

/// Removes the host-handled `max_age` argument (whole seconds) before a
/// provider sees the request.
pub(super) fn take_max_age(params: &mut Value) -> Result<Option<u64>, BrokerError> {
    let object = params.as_object_mut().ok_or(BrokerError::Malformed)?;
    match object.remove("max_age") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|seconds| *seconds <= MAX_AGE_SECONDS_LIMIT)
            .map(Some)
            .ok_or_else(|| {
                BrokerError::invalid(
                    "max_age",
                    format!("expected whole seconds from 0 to {MAX_AGE_SECONDS_LIMIT}"),
                )
            }),
    }
}

pub(super) fn take_as_of(params: &mut Value) -> Result<Option<chrono::NaiveDate>, BrokerError> {
    let object = params.as_object_mut().ok_or(BrokerError::Malformed)?;
    let value = object.remove("as_of");
    parse_as_of_date(value.as_ref().and_then(Value::as_str))
}

pub(super) fn observation_date(value: &Value) -> Option<chrono::NaiveDate> {
    const KEYS: &[&str] = &[
        "date",
        "period",
        "filed_at",
        "filing_date",
        "available_at",
        "end",
        "report_date",
        "rcept_dt",
    ];
    for key in KEYS {
        let Some(text) = value.get(*key).and_then(Value::as_str) else {
            continue;
        };
        if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d") {
            return Some(date);
        }
        if text.len() == 4 {
            if let Ok(year) = text.parse::<i32>() {
                return chrono::NaiveDate::from_ymd_opt(year, 12, 31);
            }
        }
        if text.len() == 8 {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(text, "%Y%m%d") {
                return Some(date);
            }
        }
    }
    None
}

pub(super) fn record_date(value: Option<&str>) -> Option<chrono::NaiveDate> {
    let value = value?;
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc).date_naive())
        .or_else(|| observation_date(&json!({ "date": value })))
}

pub(super) fn is_after_cutoff(as_of: Option<&str>, cutoff: chrono::NaiveDate) -> bool {
    record_date(as_of).is_none_or(|date| date > cutoff)
}

// Only provider-declared publication evidence can establish point-in-time availability.
// A period end or observation date can precede both first publication and revisions.
fn publication_date(value: &Value) -> Option<chrono::NaiveDate> {
    [
        "available_at",
        "accepted_at",
        "filed_at",
        "filing_date",
        "rcept_dt",
    ]
    .iter()
    .find_map(|key| record_date(value.get(*key).and_then(Value::as_str)))
}

/// OpenDART receipt numbers begin with the receipt date (YYYYMMDD).
fn receipt_date(value: &Value) -> Option<chrono::NaiveDate> {
    let receipt = value.get("filing_id").and_then(Value::as_str)?;
    if receipt.len() != 14 || !receipt.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    chrono::NaiveDate::parse_from_str(&receipt[..8], "%Y%m%d").ok()
}

const DATED_SECTIONS: [&str; 5] = ["observations", "rows", "bars", "facts", "filings"];

/// What a row's date proves about availability at a cutoff.
#[derive(Clone, Copy)]
enum CutoffBasis {
    /// Provider-declared publication; observation dates alone prove nothing.
    Publication,
    /// A completed daily bar, public at the close of its own date.
    Observation,
}

/// Keeps only rows with provider-declared publication on or before the cutoff.
pub(super) fn apply_as_of_cutoff(
    output: &mut Value,
    cutoff: chrono::NaiveDate,
) -> Result<(), BrokerError> {
    apply_cutoff(output, cutoff, CutoffBasis::Publication)
}

/// Keeps only completed daily bars dated on or before the cutoff, the rule
/// OpenBB price history follows.
pub(super) fn apply_observation_cutoff(
    output: &mut Value,
    cutoff: chrono::NaiveDate,
) -> Result<(), BrokerError> {
    apply_cutoff(output, cutoff, CutoffBasis::Observation)
}

fn apply_cutoff(
    output: &mut Value,
    cutoff: chrono::NaiveDate,
    basis: CutoffBasis,
) -> Result<(), BrokerError> {
    // A single-document read is dated by its filing; anything without a dated
    // section cannot show what was known at the cutoff and fails closed.
    let has_rows = output.get("data").is_some_and(|data| {
        DATED_SECTIONS
            .iter()
            .any(|field| data.get(*field).is_some_and(Value::is_array))
    });
    if !has_rows {
        let filed = output
            .pointer("/data/filing")
            .and_then(|filing| publication_date(filing).or_else(|| receipt_date(filing)));
        if filed.is_none_or(|date| date > cutoff) {
            return Err(BrokerError::Execution(format!(
                "as_of_unsupported: this result was not published on or before the requested as-of cutoff {cutoff}"
            )));
        }
    }
    let mut excluded = 0_usize;
    let mut unknown = 0_usize;
    if let Some(data) = output.get_mut("data") {
        for field in DATED_SECTIONS {
            let Some(rows) = data.get_mut(field).and_then(Value::as_array_mut) else {
                continue;
            };
            let before = rows.len();
            rows.retain(|row| {
                let known = match basis {
                    CutoffBasis::Publication => publication_date(row).map(|published| {
                        published <= cutoff
                            && observation_date(row).is_none_or(|date| date <= cutoff)
                    }),
                    CutoffBasis::Observation => observation_date(row).map(|date| date <= cutoff),
                };
                known.unwrap_or_else(|| {
                    unknown += 1;
                    false
                })
            });
            excluded += before.saturating_sub(rows.len());
        }
    }
    if let Some(page) = output.get_mut("page").and_then(Value::as_object_mut) {
        if let Some(returned) = page.get("returned").and_then(Value::as_u64) {
            page.insert(
                "returned".into(),
                json!(returned.saturating_sub(excluded as u64)),
            );
        }
    }
    let quality = output
        .get_mut("quality")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| BrokerError::Execution("provider result had no quality".into()))?;
    if excluded > 0 {
        quality.insert("status".into(), json!("warn"));
    }
    let (basis_label, unknown_key, unknown_text) = match basis {
        CutoffBasis::Publication => (
            "publication_date",
            "unknown_publication_excluded",
            "unknown publication time",
        ),
        CutoffBasis::Observation => ("observation_date", "undated_excluded", "no date"),
    };
    let mut summary = serde_json::Map::new();
    summary.insert("cutoff".into(), json!(cutoff.to_string()));
    summary.insert("basis".into(), json!(basis_label));
    summary.insert(
        "post_cutoff_excluded".into(),
        json!(excluded.saturating_sub(unknown)),
    );
    summary.insert(unknown_key.into(), json!(unknown));
    summary.insert("action".into(), json!("excluded"));
    quality.insert("as_of".into(), Value::Object(summary));
    if let Some(checks) = quality.get_mut("checks").and_then(Value::as_array_mut) {
        checks.push(json!({
            "code": "as_of_cutoff",
            "status": if excluded == 0 { "pass" } else { "warn" },
            "detail": format!("{excluded} observations unavailable at {cutoff} were excluded ({unknown} with {unknown_text})")
        }));
    }
    if excluded > 0 {
        if let Some(warnings) = output.get_mut("warnings").and_then(Value::as_array_mut) {
            warnings.push(json!(format!(
                "excluded {excluded} observations unavailable at as-of {cutoff}; {unknown} have {unknown_text}"
            )));
        }
    }
    let empty = output
        .get("data")
        .and_then(|data| {
            DATED_SECTIONS
                .iter()
                .find_map(|field| data.get(*field).and_then(Value::as_array))
        })
        .is_some_and(|rows| rows.is_empty());
    if empty {
        return Err(BrokerError::Execution(match basis {
            CutoffBasis::Publication => {
                "as_of_empty: no observations have verified publication at the requested as-of cutoff".into()
            }
            CutoffBasis::Observation => {
                format!(
                    "as_of_empty: no observations are dated on or before the requested as-of cutoff {cutoff}"
                )
            }
        }));
    }
    Ok(())
}

pub(super) fn validate_provider_result(
    output: &Value,
    expected_tool: &str,
    expected_source_id: &str,
) -> Result<(), BrokerError> {
    if output.get("schema_version").and_then(Value::as_str) != Some("loupist-finance-result/1")
        || output.get("tool").and_then(Value::as_str) != Some(expected_tool)
        || output.get("source_id").and_then(Value::as_str) != Some(expected_source_id)
    {
        return Err(BrokerError::Execution(
            "provider result did not match its declared contract".into(),
        ));
    }
    output
        .get("provenance")
        .and_then(Value::as_object)
        .ok_or_else(|| BrokerError::Execution("provider result had no provenance".into()))?;
    output
        .get("quality")
        .and_then(|quality| quality.get("status"))
        .and_then(Value::as_str)
        .filter(|status| matches!(*status, "pass" | "warn"))
        .ok_or_else(|| {
            BrokerError::Execution("provider result quality status was invalid".into())
        })?;
    Ok(())
}

#[cfg(test)]
mod as_of_tests {
    use super::*;

    #[test]
    fn as_of_cutoff_excludes_unknown_publication_and_later_revisions() {
        let mut output = json!({
            "data": {
                "observations": [
                    {"date": "2024-06-01", "available_at": "2024-06-02T12:00:00Z", "value": 1},
                    {"date": "2024-06-01", "available_at": "2024-07-01T12:00:00Z", "value": 2},
                    {"value": 3}
                ]
            },
            "page": {"returned": 3, "available": 3},
            "quality": {"status": "pass", "checks": []},
            "warnings": []
        });
        apply_as_of_cutoff(
            &mut output,
            chrono::NaiveDate::from_ymd_opt(2024, 6, 30).unwrap(),
        )
        .unwrap();
        assert_eq!(output["data"]["observations"].as_array().unwrap().len(), 1);
        assert_eq!(output["quality"]["as_of"]["post_cutoff_excluded"], 1);
        assert_eq!(output["page"]["returned"], 1);
        assert_eq!(
            output["quality"]["as_of"]["unknown_publication_excluded"],
            1
        );
        let cutoff = chrono::NaiveDate::from_ymd_opt(2024, 6, 30).unwrap();
        assert!(!is_after_cutoff(Some("2024-06-30T23:59:59Z"), cutoff));
        assert!(is_after_cutoff(Some("2024-07-01T00:00:00Z"), cutoff));
        assert!(is_after_cutoff(Some("2025-01-01"), cutoff));
        assert!(is_after_cutoff(None, cutoff));
    }

    #[test]
    fn a_single_filing_read_is_dated_by_its_receipt_and_undated_shapes_fail_closed() {
        let cutoff = chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap();
        let read = |receipt: &str| {
            json!({
                "data": {"filing": {"filing_id": receipt}, "document": {"text": "filing text"}},
                "quality": {"status": "pass", "checks": []},
                "warnings": []
            })
        };
        let mut earlier = read("20260305990308");
        apply_as_of_cutoff(&mut earlier, cutoff).unwrap();
        assert_eq!(earlier["data"]["document"]["text"], "filing text");
        assert_eq!(earlier["quality"]["as_of"]["cutoff"], "2026-03-15");
        assert!(apply_as_of_cutoff(&mut read("20260318990308"), cutoff).is_err());
        assert!(apply_as_of_cutoff(&mut read("not-a-receipt"), cutoff).is_err());
        let mut undated = json!({"data": {"quote": {"price": "1"}}, "quality": {"status": "pass"}});
        assert!(apply_as_of_cutoff(&mut undated, cutoff).is_err());
    }

    #[test]
    fn daily_bars_are_known_at_the_close_of_their_own_date() {
        let cutoff = chrono::NaiveDate::from_ymd_opt(2026, 3, 3).unwrap();
        let bars = || {
            json!({
                "data": {"bars": [
                    {"date": "2026-03-02", "close": "72500"},
                    {"date": "2026-03-03", "close": "73400"},
                    {"date": "2026-03-04", "close": "74650"},
                    {"close": "1"}
                ]},
                "page": {"returned": 4},
                "quality": {"status": "pass", "checks": []},
                "warnings": []
            })
        };
        // Publication evidence alone would drop every bar.
        assert!(apply_as_of_cutoff(&mut bars(), cutoff).is_err());
        let mut output = bars();
        apply_observation_cutoff(&mut output, cutoff).unwrap();
        let closes = output["data"]["bars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|bar| bar["close"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(closes, ["72500", "73400"]);
        assert_eq!(output["page"]["returned"], 2);
        assert_eq!(output["quality"]["as_of"]["basis"], "observation_date");
        assert_eq!(output["quality"]["as_of"]["post_cutoff_excluded"], 1);
        assert_eq!(output["quality"]["as_of"]["undated_excluded"], 1);
        let mut later = json!({"data": {"bars": [{"date": "2026-03-04", "close": "74650"}]},
            "quality": {"status": "pass", "checks": []}, "warnings": []});
        let error = apply_observation_cutoff(&mut later, cutoff).unwrap_err();
        assert!(error.to_string().contains("as-of cutoff"), "{error}");
    }
}
