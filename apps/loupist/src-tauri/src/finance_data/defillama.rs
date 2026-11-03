//! DefiLlama (`defillama.tvl`): dated daily value locked for a protocol or a
//! chain, and stablecoin circulating supply. Keyless, fixed hosts.
use super::*;

pub(crate) const DEFILLAMA_SOURCE_ID: &str = "defillama.tvl";
const DEFILLAMA_API_ROOT: &str = "https://api.llama.fi";
const DEFILLAMA_STABLECOINS_ROOT: &str = "https://stablecoins.llama.fi";
const DEFAULT_DAYS: i64 = 365;
const MAX_DAYS: i64 = 3 * 366;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum LlamaRoute {
    Protocol,
    Chain,
    Stablecoin,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LlamaQuery {
    route: LlamaRoute,
    id: String,
    start: Option<String>,
    end: Option<String>,
}

#[derive(Debug)]
pub(super) struct ValidatedLlamaQuery {
    pub(super) route: LlamaRoute,
    pub(super) id: String,
    pub(super) start: NaiveDate,
    pub(super) end: NaiveDate,
}

pub(super) fn validate_llama_query(params: Value) -> Result<ValidatedLlamaQuery, FinanceDataError> {
    let query: LlamaQuery = strict_query(
        params,
        "expected route, id and optional start and end dates",
    )?;
    let id = query.id.trim().to_owned();
    let valid = match query.route {
        LlamaRoute::Protocol => {
            (1..=64).contains(&id.len())
                && id.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'.')
                })
        }
        LlamaRoute::Chain => {
            (1..=48).contains(&id.len())
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'-' | b'.' | b'_'))
        }
        LlamaRoute::Stablecoin => {
            (1..=6).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
        }
    };
    if !valid {
        return Err(FinanceDataError::InvalidQuery(
            "id must be a DefiLlama protocol slug, chain name or numeric stablecoin id",
        ));
    }
    let today = Utc::now().date_naive();
    let end = query
        .end
        .as_deref()
        .map(|value| parse_date(value, "end date is invalid"))
        .transpose()?
        .unwrap_or(today);
    let start = query
        .start
        .as_deref()
        .map(|value| parse_date(value, "start date is invalid"))
        .transpose()?
        .unwrap_or(end - ChronoDuration::days(DEFAULT_DAYS));
    if start > end || end > today || (end - start).num_days() > MAX_DAYS {
        return Err(FinanceDataError::InvalidQuery(
            "date range is invalid or exceeds three years",
        ));
    }
    Ok(ValidatedLlamaQuery {
        route: query.route,
        id,
        start,
        end,
    })
}

pub(super) fn llama_url(query: &ValidatedLlamaQuery) -> Result<Url, FinanceDataError> {
    Ok(match query.route {
        LlamaRoute::Protocol => fixed_url(DEFILLAMA_API_ROOT, &["protocol", &query.id])?,
        LlamaRoute::Chain => {
            fixed_url(DEFILLAMA_API_ROOT, &["v2", "historicalChainTvl", &query.id])?
        }
        LlamaRoute::Stablecoin => {
            let mut url = fixed_url(DEFILLAMA_STABLECOINS_ROOT, &["stablecoincharts", "all"])?;
            url.query_pairs_mut().append_pair("stablecoin", &query.id);
            url
        }
    })
}

fn day(value: &Value) -> Option<NaiveDate> {
    let seconds = match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }?;
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|time| time.date_naive())
}

/// Where a route keeps one day's USD value inside its row.
type ValueOf = fn(&Value) -> Option<&Value>;

/// Daily rows `{date, value}` in USD from any of the three routes.
pub(super) fn normalize_llama_response(
    query: &ValidatedLlamaQuery,
    raw: &RawProviderResponse,
) -> Result<Value, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(&raw.bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    let (series, value_of, name): (&Vec<Value>, ValueOf, Option<&str>) = match query.route {
        LlamaRoute::Protocol => (
            root.get("tvl")
                .and_then(Value::as_array)
                .ok_or(FinanceDataError::InvalidResponse)?,
            |row| row.get("totalLiquidityUSD"),
            root.get("name").and_then(Value::as_str),
        ),
        LlamaRoute::Chain => (
            root.as_array().ok_or(FinanceDataError::InvalidResponse)?,
            |row| row.get("tvl"),
            None,
        ),
        LlamaRoute::Stablecoin => (
            root.as_array().ok_or(FinanceDataError::InvalidResponse)?,
            |row| {
                row.pointer("/totalCirculatingUSD/peggedUSD")
                    .or_else(|| row.pointer("/totalCirculating/peggedUSD"))
            },
            None,
        ),
    };
    let mut by_day = BTreeMap::new();
    for row in series {
        let date = row
            .get("date")
            .and_then(day)
            .ok_or(FinanceDataError::InvalidResponse)?;
        if date < query.start || date > query.end {
            continue;
        }
        let value = value_of(row)
            .and_then(canonical_decimal)
            .ok_or(FinanceDataError::InvalidResponse)?;
        // The latest point of a day wins (the current day can repeat).
        by_day.insert(date, value);
    }
    if by_day.is_empty() {
        return Err(FinanceDataError::NoData(DEFILLAMA_SOURCE_ID));
    }
    let rows = by_day
        .into_iter()
        .map(|(date, value)| json!({"date": date.to_string(), "value": value}))
        .collect::<Vec<_>>();
    let (metric, unit) = match query.route {
        LlamaRoute::Stablecoin => ("circulating_supply", "USD"),
        _ => ("total_value_locked", "USD"),
    };
    let count = rows.len();
    finance_result(
        "defillama_read",
        "crypto.tvl",
        DEFILLAMA_SOURCE_ID,
        json!({
            "route": match query.route { LlamaRoute::Protocol => "protocol", LlamaRoute::Chain => "chain", LlamaRoute::Stablecoin => "stablecoin" },
            "id": query.id,
            "start": query.start.to_string(),
            "end": query.end.to_string()
        }),
        json!({
            "series": {"id": query.id, "name": name, "metric": metric, "unit": unit, "frequency": "daily"},
            "rows": rows
        }),
        raw,
        "DefiLlama",
        "DefiLlama open API",
        false,
        "aggregator",
        "latest_only",
        "pass",
        standard_checks(),
        vec!["DefiLlama recomputes past values when it adds or corrects sources; this response is not a historical vintage.".to_owned()],
        count,
        count,
        false,
    )
}

impl FinanceDataService {
    pub async fn defillama_read(&self, params: Value) -> Result<Value, FinanceDataError> {
        let query = validate_llama_query(params)?;
        let url = llama_url(&query)?;
        let raw = self
            .fetch_provider(
                self.client.get(url.clone()),
                DEFILLAMA_SOURCE_ID,
                url.to_string(),
                MAX_LARGE_PROVIDER_BYTES,
            )
            .await?;
        normalize_llama_response(&query, &raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(body: &str) -> RawProviderResponse {
        RawProviderResponse {
            bytes: body.as_bytes().to_vec(),
            source_url: "https://api.llama.fi/fixture".into(),
            retrieved_at: "2026-11-01T00:00:00.000Z".into(),
            continuation: None,
        }
    }

    fn query(route: &str, id: &str, start: &str, end: &str) -> ValidatedLlamaQuery {
        validate_llama_query(json!({"route": route, "id": id, "start": start, "end": end})).unwrap()
    }

    #[test]
    fn protocol_chain_and_stablecoin_become_dated_daily_rows() {
        let protocol = query("protocol", "fictional-lend", "2026-10-01", "2026-10-03");
        let output = normalize_llama_response(
            &protocol,
            &raw(r#"{"name":"Fictional Lend","tvl":[{"date":1790726400,"totalLiquidityUSD":1.0},{"date":1790812800,"totalLiquidityUSD":1250000000.5},{"date":1790899200,"totalLiquidityUSD":1000000000},{"date":1790935200,"totalLiquidityUSD":990000000}]}"#),
        )
        .unwrap();
        assert_eq!(output["tool"], "defillama_read");
        assert_eq!(output["data"]["series"]["name"], "Fictional Lend");
        assert_eq!(
            output["data"]["rows"],
            json!([{"date":"2026-10-01","value":"1250000000.5"},{"date":"2026-10-02","value":"990000000"}])
        );
        let chain = query("chain", "Ethereum", "2026-10-01", "2026-10-01");
        assert_eq!(
            normalize_llama_response(&chain, &raw(r#"[{"date":1790812800,"tvl":51000000000}]"#))
                .unwrap()["data"]["rows"][0]["value"],
            "51000000000"
        );
        let stable = query("stablecoin", "2", "2026-10-01", "2026-10-01");
        let output = normalize_llama_response(
            &stable,
            &raw(r#"[{"date":"1790812800","totalCirculatingUSD":{"peggedUSD":73000000000.25}}]"#),
        )
        .unwrap();
        assert_eq!(output["data"]["series"]["metric"], "circulating_supply");
        assert_eq!(output["data"]["rows"][0]["value"], "73000000000.25");
        assert!(normalize_llama_response(&chain, &raw(r#"[{"date":1,"tvl":1}]"#)).is_err());
        assert!(normalize_llama_response(&chain, &raw(r#"{"tvl":[]}"#)).is_err());
    }

    #[test]
    fn queries_are_bounded_to_known_routes_ids_and_hosts() {
        for bad in [
            json!({"route":"protocol","id":"../etc"}),
            json!({"route":"protocol","id":"Aave"}),
            json!({"route":"stablecoin","id":"usdc"}),
            json!({"route":"yields","id":"x"}),
            json!({"route":"chain","id":"Ethereum","start":"2020-01-01","end":"2026-01-01"}),
            json!({"route":"chain","id":"Ethereum","extra":1}),
        ] {
            assert!(validate_llama_query(bad.clone()).is_err(), "{bad}");
        }
        for (route, id, host) in [
            ("protocol", "lido", "api.llama.fi"),
            ("chain", "zkSync Era", "api.llama.fi"),
            ("stablecoin", "1", "stablecoins.llama.fi"),
        ] {
            let url = llama_url(&validate_llama_query(json!({"route": route, "id": id})).unwrap())
                .unwrap();
            assert_eq!((url.scheme(), url.host_str()), ("https", Some(host)));
        }
    }
}
