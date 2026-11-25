use super::*;
use crate::finance_data::provider_common::canonical_decimal;
use serde::Serialize;
use std::collections::VecDeque;

const PATH: &str = "/uapi/domestic-stock/v1/ksdinfo/dividend";
const TR_ID: &str = "HHKDB669102C0";
const TOOL: &str = "kis_dividend_calendar";
const ROW_CEILING: usize = 100;
const MAX_PAGES: usize = 10;
/// KIS serves the KSD calendar on real connections only.
const REAL_ONLY: &str = "kis_dividend_calendar reads KSD's calendar only on a real Korea Investment connection, and this one is demo; use another connected dividend source, such as OpenDART's dividend reads, or say the calendar is unavailable";
const QUERY_RULE: &str = "kis_dividend_calendar takes start and end as YYYY-MM-DD record dates, both included, start first and at most 366 days in all, and symbol as a six-character domestic short code such as 005930";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DividendQuery {
    start: String,
    end: String,
    symbol: Option<String>,
    #[serde(default)]
    kind: DividendKind,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DividendKind {
    #[default]
    All,
    Annual,
    Interim,
}

#[derive(Clone, Copy, Debug)]
struct DateWindow {
    start: NaiveDate,
    end: NaiveDate,
}

impl DateWindow {
    fn as_json(self) -> Value {
        json!({"start": self.start.to_string(), "end": self.end.to_string()})
    }

    fn halves(self) -> Option<(Self, Self)> {
        let days = (self.end - self.start).num_days();
        if days == 0 {
            return None;
        }
        let middle = self.start + ChronoDuration::days(days / 2);
        Some((
            Self {
                end: middle,
                ..self
            },
            Self {
                start: middle.succ_opt()?,
                ..self
            },
        ))
    }
}

#[derive(Serialize)]
struct DividendRow {
    record_date: String,
    symbol: String,
    name: String,
    dividend_kind: Option<String>,
    cash_per_share: Option<String>,
    cash_dividend_rate: Option<String>,
    stock_dividend_rate: Option<String>,
    cash_pay_date: Option<String>,
    stock_pay_date: Option<String>,
}

impl FinanceDataService {
    pub(crate) async fn kis_dividend_calendar(
        &self,
        params: Value,
        environment: &str,
        app_key: &str,
        app_secret: &str,
    ) -> Result<Value, FinanceDataError> {
        if KisEnvironment::parse(environment)? != KisEnvironment::Real {
            return Err(FinanceDataError::InvalidQuery(REAL_ONLY));
        }
        let query: DividendQuery = strict_query(params, QUERY_RULE)?;
        let date = |value| {
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .map_err(|_| FinanceDataError::InvalidQuery(QUERY_RULE))
        };
        let window = DateWindow {
            start: date(&query.start)?,
            end: date(&query.end)?,
        };
        if !(0..=365).contains(&(window.end - window.start).num_days())
            || window.start.year() < 1
            || window.end.year() > 9999
            || query
                .symbol
                .as_deref()
                .is_some_and(|symbol| !valid_symbol(symbol))
        {
            return Err(FinanceDataError::InvalidQuery(QUERY_RULE));
        }
        collect_calendar(window, query.symbol.as_deref(), |window| {
            let query = &query;
            async move {
                self.fetch_kis_get(
                    KisGet {
                        url: dividend_url(query, window)?,
                        tr_id: TR_ID,
                        continuation: None,
                    },
                    KisEnvironment::Real,
                    app_key,
                    app_secret,
                )
                .await
            }
        })
        .await
    }
}

fn dividend_url(query: &DividendQuery, window: DateWindow) -> Result<Url, FinanceDataError> {
    let mut url = Url::parse(KIS_REAL_API_ROOT).map_err(|_| FinanceDataError::InvalidResponse)?;
    url.set_path(PATH);
    let kind = match query.kind {
        DividendKind::All => "0",
        DividendKind::Annual => "1",
        DividendKind::Interim => "2",
    };
    url.query_pairs_mut()
        .append_pair("CTS", "")
        .append_pair("GB1", kind)
        .append_pair("F_DT", &window.start.format("%Y%m%d").to_string())
        .append_pair("T_DT", &window.end.format("%Y%m%d").to_string())
        .append_pair("SHT_CD", query.symbol.as_deref().unwrap_or(""))
        .append_pair("HIGH_GB", "");
    Ok(url)
}

async fn collect_calendar<F, Fut>(
    window: DateWindow,
    symbol: Option<&str>,
    mut fetch: F,
) -> Result<Value, FinanceDataError>
where
    F: FnMut(DateWindow) -> Fut,
    Fut: std::future::Future<Output = Result<RawProviderResponse, FinanceDataError>>,
{
    let mut pending = VecDeque::from([window]);
    let mut saturated_dates = Vec::new();
    let mut rows = Vec::new();
    let mut retrieved_at = None;
    let mut provider_pages = 0;
    for _ in 0..MAX_PAGES {
        let Some(window) = pending.pop_front() else {
            break;
        };
        let raw = fetch(window).await?;
        provider_pages += 1;
        let result = parse_page(&raw.bytes, window, symbol)?;
        retrieved_at.get_or_insert(raw.retrieved_at);
        // KIS documents no tr_cont paging for this endpoint. A response at
        // its observed 100-row boundary is re-read in smaller date windows.
        if result.len() >= ROW_CEILING {
            if let Some((earlier, later)) = window.halves() {
                pending.push_front(later);
                pending.push_front(earlier);
                continue;
            }
            saturated_dates.push(window.start.to_string());
        }
        rows.extend(result);
    }
    rows.sort_unstable_by(|left, right| {
        (&left.record_date, &left.symbol).cmp(&(&right.record_date, &right.symbol))
    });
    let truncated = !pending.is_empty() || !saturated_dates.is_empty();
    let pending_windows: Vec<_> = pending.into_iter().map(DateWindow::as_json).collect();
    // The description explains record and payment dates and publication;
    // the warnings say how to read what this call left out.
    let mut warnings = Vec::new();
    if !pending_windows.is_empty() {
        warnings.push(format!(
            "Only part of the range was read: call again with each window in page.pending_windows as start and end{}.",
            symbol.map_or_else(String::new, |symbol| format!(" and symbol {symbol}"))
        ));
    }
    if !saturated_dates.is_empty() {
        warnings.push(format!(
            "KSD returned its most rows per request ({ROW_CEILING}) for record date(s) {}, so some dividends of that date may be missing: read that date again with symbol set to each issue you need.",
            saturated_dates.join(", ")
        ));
    }
    Ok(json!({
        "schema_version": "loupist-finance-result/1", "tool": TOOL, "source_id": KIS_SOURCE_ID,
        "data": {"window": window.as_json(), "symbol": symbol, "rows": rows},
        "page": {"returned": rows.len(), "provider_pages": provider_pages,
                 "truncated": truncated, "pending_windows": pending_windows, "saturated_dates": saturated_dates},
        "quality": {"status": if truncated { "warn" } else { "pass" }},
        "warnings": warnings,
        "provenance": {"source_url": format!("{KIS_REAL_API_ROOT}{PATH}"), "retrieved_at": retrieved_at},
    }))
}

fn parse_page(
    bytes: &[u8],
    window: DateWindow,
    symbol: Option<&str>,
) -> Result<Vec<DividendRow>, FinanceDataError> {
    let root: Value =
        serde_json::from_slice(bytes).map_err(|_| FinanceDataError::InvalidResponse)?;
    let object = root.as_object().ok_or(FinanceDataError::InvalidResponse)?;
    match object.get("rt_cd").and_then(Value::as_str) {
        Some("0") => {}
        Some(_) => return Err(kis_read_failure(object, TOOL)),
        None => return Err(FinanceDataError::InvalidResponse),
    }
    let rows = root["output1"]
        .as_array()
        .ok_or(FinanceDataError::InvalidResponse)?;
    rows.iter()
        .map(|row| {
            let get = |key| {
                row[key]
                    .as_str()
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
            };
            let record_date = get("record_date")
                .and_then(parse_date)
                .filter(|date| (window.start..=window.end).contains(date))
                .ok_or(FinanceDataError::InvalidResponse)?;
            let row_symbol = get("sht_cd")
                .filter(|code| valid_symbol(code) && symbol.is_none_or(|symbol| *code == symbol))
                .ok_or(FinanceDataError::InvalidResponse)?;
            let name = get("isin_name")
                .filter(|name| name.len() <= 384 && !name.chars().any(char::is_control))
                .ok_or(FinanceDataError::InvalidResponse)?;
            let decimal = |key| get(key).and_then(|text| canonical_decimal(&json!(text)));
            Ok(DividendRow {
                record_date: record_date.to_string(),
                symbol: row_symbol.to_owned(),
                name: name.to_owned(),
                dividend_kind: get("divi_kind").map(str::to_owned),
                cash_per_share: decimal("per_sto_divi_amt"),
                cash_dividend_rate: decimal("divi_rate"),
                stock_dividend_rate: decimal("stk_divi_rate"),
                cash_pay_date: get("divi_pay_dt")
                    .and_then(parse_date)
                    .map(|date| date.to_string()),
                stock_pay_date: get("stk_div_pay_dt")
                    .and_then(parse_date)
                    .map(|date| date.to_string()),
            })
        })
        .collect()
}

fn parse_date(value: &str) -> Option<NaiveDate> {
    ["%Y%m%d", "%Y/%m/%d"]
        .into_iter()
        .find_map(|format| NaiveDate::parse_from_str(value, format).ok())
}

fn valid_symbol(value: &str) -> bool {
    value.len() == 6
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start: &str, end: &str) -> DateWindow {
        DateWindow {
            start: parse_date(start).unwrap(),
            end: parse_date(end).unwrap(),
        }
    }

    fn page(date: &str, count: usize) -> RawProviderResponse {
        let rows = (0..count).map(|index| json!({
            "record_date": date, "sht_cd": format!("{index:06}"), "isin_name":"합성 종목",
            "per_sto_divi_amt":"000000000600", "divi_pay_dt":"2026/04/17", "stk_div_pay_dt":"",
        })).collect::<Vec<_>>();
        RawProviderResponse {
            bytes: serde_json::to_vec(&json!({"rt_cd":"0", "output1":rows})).unwrap(),
            source_url: format!("{KIS_REAL_API_ROOT}{PATH}"),
            retrieved_at: "2026-11-24T00:00:00.000Z".into(),
            continuation: None,
        }
    }

    #[test]
    fn dividend_query_uses_record_dates_and_required_blank_keys() {
        let window = window("20260328", "20260331");
        let query = DividendQuery {
            start: window.start.to_string(),
            end: window.end.to_string(),
            symbol: Some("00123A".into()),
            kind: DividendKind::Annual,
        };
        let url = dividend_url(&query, window).unwrap();
        assert_eq!(url.path(), PATH);
        assert_eq!(url.host_str(), Some("openapi.koreainvestment.com"));
        assert_eq!(
            url.query(),
            Some("CTS=&GB1=1&F_DT=20260328&T_DT=20260331&SHT_CD=00123A&HIGH_GB=")
        );
        assert_eq!(TR_ID, "HHKDB669102C0");
    }

    #[tokio::test]
    async fn demo_dividends_fail_before_any_token_request() {
        let service = FinanceDataService::new().unwrap();
        let result = service
            .kis_dividend_calendar(
                json!({"start":"2026-03-28","end":"2026-03-31"}),
                "demo",
                "",
                "",
            )
            .await;
        assert!(matches!(
            result,
            Err(FinanceDataError::InvalidQuery(REAL_ONLY))
        ));
    }

    #[tokio::test]
    async fn dividend_pages_partition_dates_and_report_single_day_saturation() {
        let mut windows = Vec::new();
        let mut replies = VecDeque::from([
            page("20260328", ROW_CEILING),
            page("20260328", 1),
            page("20260329", ROW_CEILING),
        ]);
        let result = collect_calendar(window("20260328", "20260329"), None, |window| {
            windows.push((window.start.to_string(), window.end.to_string()));
            std::future::ready(Ok(replies.pop_front().unwrap()))
        })
        .await
        .unwrap();
        assert_eq!(
            json!(windows),
            json!([
                ["2026-03-28", "2026-03-29"],
                ["2026-03-28", "2026-03-28"],
                ["2026-03-29", "2026-03-29"],
            ])
        );
        assert_eq!(result["page"]["provider_pages"], 3);
        assert_eq!(result["page"]["returned"], ROW_CEILING + 1);
        assert_eq!(result["page"]["saturated_dates"], json!(["2026-03-29"]));
        assert_eq!(result["quality"]["status"], "warn");
        assert_eq!(
            result["warnings"],
            json!(["KSD returned its most rows per request (100) for record date(s) 2026-03-29, so some dividends of that date may be missing: read that date again with symbol set to each issue you need."])
        );
        let row = &result["data"]["rows"][0];
        assert_eq!(row["record_date"], "2026-03-28");
        assert_eq!(row["cash_pay_date"], "2026-04-17");
        assert_eq!(row["cash_per_share"], "600");
        assert!(row["stock_pay_date"].is_null());
    }
}
