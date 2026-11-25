//! DefiLlama (`defillama.tvl`): dated daily value locked for a protocol or a
//! chain, and stablecoin circulating supply. Keyless, fixed hosts.
use super::*;

pub(crate) const DEFILLAMA_SOURCE_ID: &str = "defillama.tvl";
pub(super) const DEFILLAMA_API_ROOT: &str = "https://api.llama.fi";
pub(super) const DEFILLAMA_STABLECOINS_ROOT: &str = "https://stablecoins.llama.fi";
const DEFAULT_DAYS: i64 = 365;
const MAX_DAYS: i64 = 3 * 366;
/// `protocol/{slug}` has no date filter: it sends the whole history with every
/// chain's and token's breakdown, of which only `name` and `tvl` are read.
/// Curve DEX's reply was 72.9 MB decoded (22.5 MB gzip) on 2026-11-24, over
/// the 32 MiB other large reads allow; most protocols are a few MB.
const MAX_PROTOCOL_BYTES: usize = 128 * 1024 * 1024;

/// The two fields a protocol reply is read for. Every other field, the
/// per-chain and per-token histories included, is skipped while parsing
/// rather than built into a JSON tree.
#[derive(Deserialize)]
struct ProtocolHistory {
    name: Option<Value>,
    tvl: Vec<Value>,
}

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
    if !valid || matches!(id.as_str(), "." | "..") {
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
    let protocol: ProtocolHistory;
    let root: Value;
    let (series, value_of, name): (&Vec<Value>, ValueOf, Option<&str>) = match query.route {
        LlamaRoute::Protocol => {
            protocol = serde_json::from_slice(&raw.bytes)
                .map_err(|_| FinanceDataError::InvalidResponse)?;
            (
                &protocol.tvl,
                |row| row.get("totalLiquidityUSD"),
                protocol.name.as_ref().and_then(Value::as_str),
            )
        }
        LlamaRoute::Chain => {
            root = serde_json::from_slice(&raw.bytes)
                .map_err(|_| FinanceDataError::InvalidResponse)?;
            (
                root.as_array().ok_or(FinanceDataError::InvalidResponse)?,
                |row| row.get("tvl"),
                None,
            )
        }
        LlamaRoute::Stablecoin => {
            root = serde_json::from_slice(&raw.bytes)
                .map_err(|_| FinanceDataError::InvalidResponse)?;
            (
                root.as_array().ok_or(FinanceDataError::InvalidResponse)?,
                // `totalCirculatingUSD` holds one USD value keyed by the coin's peg
                // (`peggedUSD`, `peggedEUR`, `peggedJPY` and so on).
                |row| {
                    row.get("totalCirculatingUSD")
                        .and_then(Value::as_object)
                        .filter(|values| values.len() == 1)
                        .and_then(|values| values.values().next())
                        .or_else(|| row.pointer("/totalCirculating/peggedUSD"))
                },
                None,
            )
        }
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

fn max_reply_bytes(route: LlamaRoute) -> usize {
    match route {
        LlamaRoute::Protocol => MAX_PROTOCOL_BYTES,
        LlamaRoute::Chain | LlamaRoute::Stablecoin => MAX_LARGE_PROVIDER_BYTES,
    }
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
                max_reply_bytes(query.route),
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

    /// Recorded 2026-11-23 from `stablecoincharts/all`: the last rows for
    /// EURC (id 50, pegged to the euro) and USDT (id 1).
    #[test]
    fn a_stablecoin_pegged_to_another_currency_reads_its_usd_value() {
        let eurc = query("stablecoin", "50", "2026-11-22", "2026-11-23");
        let output = normalize_llama_response(
            &eurc,
            &raw(r#"[{"date":"1795305600","totalCirculating":{"peggedEUR":459130476},"totalUnreleased":{"peggedEUR":48921712},"totalCirculatingUSD":{"peggedEUR":503064679}},{"date":"1795392000","totalCirculating":{"peggedEUR":455652552.85},"totalUnreleased":{"peggedEUR":48710582.89},"totalCirculatingUSD":{"peggedEUR":499695928.61},"totalMintedUSD":{"peggedEUR":0},"totalBridgedToUSD":{"peggedEUR":0}}]"#),
        )
        .unwrap();
        assert_eq!(
            output["data"]["rows"],
            json!([{"date":"2026-11-22","value":"503064679"},{"date":"2026-11-23","value":"499695928.61"}])
        );
        assert_eq!(output["data"]["series"]["unit"], "USD");
        let usdt = query("stablecoin", "1", "2026-11-23", "2026-11-23");
        let output = normalize_llama_response(
            &usdt,
            &raw(r#"[{"date":"1795392000","totalCirculating":{"peggedUSD":174807834143.05},"totalUnreleased":{"peggedUSD":9036467344.84},"totalCirculatingUSD":{"peggedUSD":174598064742.07},"totalMintedUSD":{"peggedUSD":0},"totalBridgedToUSD":{"peggedUSD":0}}]"#),
        )
        .unwrap();
        assert_eq!(output["data"]["rows"][0]["value"], "174598064742.07");
    }

    /// Recorded 2026-11-24 from `protocol/curve-dex` (72.9 MB), trimmed: every
    /// top-level field in its order, two chains, the last points of each
    /// history and three tokens per row. Per-chain `tvl` arrays come before
    /// the protocol's own, as in the real reply.
    #[test]
    fn a_recorded_protocol_reply_reads_only_its_own_daily_tvl() {
        let curve = query("protocol", "curve-dex", "2026-11-22", "2026-11-24");
        let output = normalize_llama_response(
            &curve,
            &raw(include_str!("defillama-protocol-fixture.json")),
        )
        .unwrap();
        assert_eq!(output["data"]["series"]["name"], "Curve DEX");
        assert_eq!(output["data"]["series"]["metric"], "total_value_locked");
        // The day's later point (06:18 UTC) replaces its midnight point.
        assert_eq!(
            output["data"]["rows"],
            json!([
                {"date":"2026-11-22","value":"988940256"},
                {"date":"2026-11-23","value":"998099192"},
                {"date":"2026-11-24","value":"1000188776"}
            ])
        );
        assert_eq!(output["page"]["returned"], 3);
    }

    /// The whole history is downloaded whatever the dates, so the protocol
    /// route allows more than other large reads and parses only what it uses.
    #[test]
    fn a_protocol_reply_larger_than_other_large_reads_is_read() {
        assert!(max_reply_bytes(LlamaRoute::Protocol) > 72_920_946);
        for route in [LlamaRoute::Chain, LlamaRoute::Stablecoin] {
            assert_eq!(max_reply_bytes(route), MAX_LARGE_PROVIDER_BYTES);
        }
        let padding = "x".repeat(MAX_LARGE_PROVIDER_BYTES);
        let body = format!(
            r#"{{"name":"Large DEX","chainTvls":{{"Ethereum":{{"tvl":[{{"date":1795305600,"totalLiquidityUSD":7}}],"tokens":[{{"date":1795305600,"tokens":{{"PAD":"{padding}"}}}}]}}}},"tvl":[{{"date":1795305600,"totalLiquidityUSD":5}}],"tokens":[]}}"#
        );
        assert!(body.len() > MAX_LARGE_PROVIDER_BYTES);
        assert!(body.len() <= max_reply_bytes(LlamaRoute::Protocol));
        let large = query("protocol", "large-dex", "2026-11-22", "2026-11-22");
        let output = normalize_llama_response(&large, &raw(&body)).unwrap();
        assert_eq!(
            output["data"]["rows"],
            json!([{"date":"2026-11-22","value":"5"}])
        );
        assert!(normalize_llama_response(&large, &raw(r#"{"name":"No history"}"#)).is_err());
    }

    #[test]
    fn queries_are_bounded_to_known_routes_ids_and_hosts() {
        for bad in [
            json!({"route":"protocol","id":"../etc"}),
            json!({"route":"protocol","id":"."}),
            json!({"route":"protocol","id":".."}),
            json!({"route":"chain","id":"."}),
            json!({"route":"chain","id":".."}),
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
