use super::*;

pub(super) fn validate_macro_query(
    params: Value,
) -> Result<ValidatedMacroDataQuery, FinanceDataError> {
    let query: MacroDataQuery = serde_json::from_value(params)
        .map_err(|_| FinanceDataError::InvalidQuery("expected the exact macro data schema"))?;
    if query.provider != WORLD_BANK_SOURCE_ID {
        return Err(FinanceDataError::InvalidQuery(
            "provider is not installed or enabled",
        ));
    }
    let economy = query.economy.trim().to_ascii_uppercase();
    if !(2..=3).contains(&economy.len())
        || !economy.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Err(FinanceDataError::InvalidQuery(
            "economy must be one bounded World Bank economy code",
        ));
    }
    let indicator = query.indicator.trim().to_ascii_uppercase();
    if !(3..=64).contains(&indicator.len())
        || !indicator
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
        || indicator.starts_with('.')
        || indicator.ends_with('.')
    {
        return Err(FinanceDataError::InvalidQuery(
            "indicator must be one canonical World Bank indicator code",
        ));
    }
    let latest_allowed_year = Utc::now().year() + 1;
    if query.start_year < 1900
        || query.end_year > latest_allowed_year
        || query.start_year > query.end_year
        || query.end_year - query.start_year + 1 > MAX_YEAR_SPAN
    {
        return Err(FinanceDataError::InvalidQuery(
            "year range is invalid or exceeds the bounded span",
        ));
    }
    Ok(ValidatedMacroDataQuery {
        economy,
        indicator,
        start_year: query.start_year,
        end_year: query.end_year,
    })
}

pub(super) fn world_bank_url(query: &ValidatedMacroDataQuery) -> Result<Url, FinanceDataError> {
    let mut url = Url::parse(WORLD_BANK_API_ROOT).map_err(|_| FinanceDataError::InvalidResponse)?;
    url.path_segments_mut()
        .map_err(|_| FinanceDataError::InvalidResponse)?
        .extend([
            "country",
            query.economy.as_str(),
            "indicator",
            query.indicator.as_str(),
        ]);
    url.query_pairs_mut()
        .append_pair("format", "json")
        .append_pair("date", &query.date_range())
        .append_pair(
            "per_page",
            &(query.end_year - query.start_year + 1).to_string(),
        );
    Ok(url)
}

pub(super) fn normalize_world_bank_response(
    query: &ValidatedMacroDataQuery,
    source_url: &str,
    retrieved_at: &str,
    bytes: &[u8],
) -> Result<Value, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    let envelope = root.as_array().ok_or(FinanceDataError::InvalidResponse)?;
    if let Some(error) = world_bank_error(query, &root) {
        return Err(error);
    }
    if envelope.len() != 2 {
        return Err(FinanceDataError::InvalidResponse);
    }
    let metadata = envelope[0]
        .as_object()
        .ok_or(FinanceDataError::InvalidResponse)?;
    let pages = metadata
        .get("pages")
        .and_then(Value::as_u64)
        .ok_or(FinanceDataError::InvalidResponse)?;
    if pages > 1 {
        return Err(FinanceDataError::InvalidResponse);
    }
    let provider_updated_at = metadata
        .get("lastupdated")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let records = envelope[1]
        .as_array()
        .ok_or(FinanceDataError::InvalidResponse)?;
    if records.len() > MAX_YEAR_SPAN as usize {
        return Err(FinanceDataError::InvalidResponse);
    }

    let first = records.first().and_then(Value::as_object);
    let indicator_name = first
        .and_then(|record| record.get("indicator"))
        .and_then(|value| value.get("value"))
        .and_then(Value::as_str)
        .unwrap_or(&query.indicator);
    let country_name = first
        .and_then(|record| record.get("country"))
        .and_then(|value| value.get("value"))
        .and_then(Value::as_str)
        .unwrap_or(&query.economy);
    let country_id = first
        .and_then(|record| record.get("countryiso3code"))
        .and_then(Value::as_str)
        .unwrap_or(&query.economy);
    let source = first.and_then(|record| record.get("source"));
    let source_id = source
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str);
    let source_name = source
        .and_then(|value| value.get("value"))
        .and_then(Value::as_str);
    let unit = first
        .and_then(|record| record.get("unit"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let mut observations = Vec::with_capacity(records.len());
    let mut periods = std::collections::BTreeSet::new();
    let mut non_null_observations = 0_usize;
    for record in records {
        let object = record
            .as_object()
            .ok_or(FinanceDataError::InvalidResponse)?;
        let response_indicator = object
            .get("indicator")
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str)
            .ok_or(FinanceDataError::InvalidResponse)?;
        let response_economy = object
            .get("country")
            .and_then(|value| value.get("id"))
            .and_then(Value::as_str);
        let response_iso3 = object.get("countryiso3code").and_then(Value::as_str);
        if response_indicator != query.indicator
            || ![response_economy, response_iso3]
                .into_iter()
                .flatten()
                .any(|value| value.eq_ignore_ascii_case(&query.economy))
        {
            return Err(FinanceDataError::InvalidResponse);
        }
        let period = object
            .get("date")
            .and_then(Value::as_str)
            .ok_or(FinanceDataError::InvalidResponse)?;
        let year = period
            .parse::<i32>()
            .ok()
            .filter(|year| (*year >= query.start_year) && (*year <= query.end_year))
            .ok_or(FinanceDataError::InvalidResponse)?;
        if !periods.insert(year) {
            return Err(FinanceDataError::InvalidResponse);
        }
        let raw_value = object
            .get("value")
            .filter(|value| value.is_number() || value.is_null())
            .ok_or(FinanceDataError::InvalidResponse)?;
        let value = if raw_value.is_null() {
            Value::Null
        } else {
            non_null_observations += 1;
            Value::String(canonical_decimal(raw_value).ok_or(FinanceDataError::InvalidResponse)?)
        };
        observations.push(json!({
            "date": period,
            "period": period,
            "value": value,
            "decimal": object.get("decimal").and_then(Value::as_i64),
            "observation_status": object.get("obs_status").and_then(Value::as_str)
        }));
    }
    observations.sort_by(|left, right| {
        left["period"]
            .as_str()
            .unwrap_or_default()
            .cmp(right["period"].as_str().unwrap_or_default())
    });
    if observations.is_empty() || non_null_observations == 0 {
        return Err(FinanceDataError::NoData(WORLD_BANK_SOURCE_ID));
    }

    let available = observations.len();
    let data = json!({
        "series": {
            "indicator_id": query.indicator,
            "indicator_name": indicator_name,
            "unit": unit,
            "economy_id": country_id,
            "economy_name": country_name,
            "provider_source_id": source_id,
            "provider_source_name": source_name,
            "provider_updated_at": provider_updated_at
        },
        "observations": observations
    });
    let raw = RawProviderResponse {
        bytes: Vec::new(),
        source_url: source_url.to_owned(),
        retrieved_at: retrieved_at.to_owned(),
        continuation: None,
    };
    finance_result(
        "worldbank_series",
        "macro.series",
        WORLD_BANK_SOURCE_ID,
        json!({
            "provider": WORLD_BANK_SOURCE_ID,
            "economy": query.economy,
            "indicator": query.indicator,
            "start_year": query.start_year,
            "end_year": query.end_year
        }),
        data,
        &raw,
        "World Bank",
        "World Bank Indicators API v2",
        true,
        "official",
        "latest_only",
        "pass",
        json!([
            {"code": "provider_success", "status": "pass"},
            {"code": "response_complete", "status": "pass"},
            {"code": "schema_valid", "status": "pass"},
            {"code": "response_identity", "status": "pass"},
            {"code": "requested_range_coverage", "status": "pass"},
            {"code": "unique_periods", "status": "pass"},
            {"code": "non_null_observation", "status": "pass"}
        ]),
        vec![
            "World Bank indicator values can be revised; this exact response is not a historical vintage."
                .to_owned(),
        ],
        available,
        available,
        false,
    )
}

// World Bank returns some API errors inside HTTP 200. Use the documented ID
// for classification and carry only recognized diagnostic text into the broker.
fn world_bank_error(query: &ValidatedMacroDataQuery, root: &Value) -> Option<FinanceDataError> {
    let messages = root.pointer("/0/message")?.as_array()?;
    let mut errors = messages.iter().filter_map(|entry| {
        let id = entry.get("id")?.as_str()?;
        let key = entry.get("key")?.as_str()?;
        let value = entry.get("value")?.as_str()?;
        Some((id, key, value))
    });
    let (id, key, value) = errors.next()?;
    if !matches!(id, "115" | "120") {
        return Some(FinanceDataError::Provider(
            "World Bank returned an API status error".into(),
        ));
    }

    let date = query.date_range();
    let parameter = key
        .strip_prefix(r#"Parameter ""#)
        .and_then(|name| name.strip_suffix(r#"" has an invalid value."#));
    let named_value = match parameter {
        Some(name @ "indicator") => Some((name, query.indicator.as_str())),
        Some(name @ "country") => Some((name, query.economy.as_str())),
        Some(name @ "date") => Some((name, date.as_str())),
        _ => None,
    };
    let detail = match (id, key, value) {
        (
            "120",
            "Invalid value",
            "The provided parameter value is not valid"
            | "The provided parameter value is not valid.",
        )
        | (
            "115",
            "Missing required parameter",
            "Parameters which are required have not been sent.",
        ) => {
            format!(" {key}: {value}")
        }
        (
            "120",
            _,
            "The provided parameter value is not valid"
            | "The provided parameter value is not valid.",
        ) if named_value.is_some() => format!(" {key} {value}"),
        _ => String::new(),
    };
    let diagnostic = match named_value {
        Some((field, sent)) => format!("World Bank rejected the {field} sent ({sent}).{detail}"),
        None => format!(
            "World Bank rejected one of the values sent (series {}, country {}, date {date}) and did not say which.{detail}",
            query.indicator, query.economy,
        ),
    };
    Some(FinanceDataError::RejectedArguments {
        diagnostic,
        next: "Check the series code and the ISO country code before reading again; web_search finds World Bank indicator codes.",
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct MacroDataQuery {
    pub(super) provider: String,
    pub(super) economy: String,
    pub(super) indicator: String,
    pub(super) start_year: i32,
    pub(super) end_year: i32,
}

pub(super) struct ValidatedMacroDataQuery {
    pub(super) economy: String,
    pub(super) indicator: String,
    pub(super) start_year: i32,
    pub(super) end_year: i32,
}

impl ValidatedMacroDataQuery {
    fn date_range(&self) -> String {
        format!("{}:{}", self.start_year, self.end_year)
    }
}

impl FinanceDataService {
    pub async fn macro_data(&self, params: Value) -> Result<Value, FinanceDataError> {
        let query = validate_macro_query(params)?;
        let url = world_bank_url(&query)?;
        let (status, raw) = self
            .send_provider_bounded(
                self.client.get(url.clone()),
                WORLD_BANK_SOURCE_ID,
                url.to_string(),
                MAX_PROVIDER_BYTES,
            )
            .await?;
        decode_world_bank_read(&query, status, &raw)
    }
}

fn decode_world_bank_read(
    query: &ValidatedMacroDataQuery,
    status: StatusCode,
    raw: &RawProviderResponse,
) -> Result<Value, FinanceDataError> {
    if status.is_client_error() {
        if let Some(error) = serde_json::from_slice(&raw.bytes)
            .ok()
            .and_then(|root| world_bank_error(query, &root))
        {
            return Err(error);
        }
    }
    if !status.is_success() {
        return Err(FinanceDataError::Provider(format!(
            "World Bank HTTP {}",
            status.as_u16()
        )));
    }
    normalize_world_bank_response(query, &raw.source_url, &raw.retrieved_at, &raw.bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_bank_invalid_parameter_is_typed_for_success_and_client_error_status() {
        // Recorded C-013 response for a nonexistent World Bank series.
        let recorded = br#"[{"message":[{"id":"120","key":"Invalid value","value":"The provided parameter value is not valid"}]}]"#;
        let query = ValidatedMacroDataQuery {
            indicator: "SP.POP.AUDITNOEXIST".into(),
            economy: "JPN".into(),
            start_year: 2024,
            end_year: 2024,
        };
        let mut raw = RawProviderResponse {
            bytes: recorded.to_vec(),
            source_url: "https://api.worldbank.org/v2/country/JPN/indicator/SP.POP.AUDITNOEXIST"
                .into(),
            retrieved_at: "2026-11-21T00:00:00Z".into(),
            continuation: None,
        };
        for status in [StatusCode::OK, StatusCode::NOT_FOUND] {
            let error = decode_world_bank_read(&query, status, &raw).unwrap_err();
            assert!(matches!(error, FinanceDataError::RejectedArguments {
                diagnostic, next,
            } if diagnostic == "World Bank rejected one of the values sent (series SP.POP.AUDITNOEXIST, country JPN, date 2024:2024) and did not say which. Invalid value: The provided parameter value is not valid"
                && next == "Check the series code and the ISO country code before reading again; web_search finds World Bank indicator codes."));
        }
        raw.bytes = serde_json::to_vec(&json!([{"message": [{
            "id": "120",
            "key": r#"Parameter "date" has an invalid value."#,
            "value": "The provided parameter value is not valid.",
        }]}]))
        .unwrap();
        assert!(matches!(
            decode_world_bank_read(&query, StatusCode::NOT_FOUND, &raw),
            Err(FinanceDataError::RejectedArguments { diagnostic, .. })
                if diagnostic == r#"World Bank rejected the date sent (2024:2024). Parameter "date" has an invalid value. The provided parameter value is not valid."#
        ));
        raw.bytes =
            br#"[{"message":[{"id":"120","key":"private-reply","value":"private-reply"}]}]"#
                .to_vec();
        assert!(
            matches!(decode_world_bank_read(&query, StatusCode::OK, &raw),
            Err(FinanceDataError::RejectedArguments { diagnostic, .. }) if !diagnostic.contains("private-reply"))
        );
        raw.bytes = br#"{"unexpected":true}"#.to_vec();
        assert!(matches!(
            decode_world_bank_read(&query, StatusCode::OK, &raw),
            Err(FinanceDataError::InvalidResponse)
        ));
    }
}
