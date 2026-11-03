//! Point-in-time authority for bundled OpenBB reads.
//!
//! A turn's `as_of` cutoff is enforced here, not by the model. Only routes on
//! the dated allowlist run under a cutoff: their rows carry a date that bounds
//! when the value became known, so the request is bounded and later rows are
//! removed before the result reaches the model or a retained dataset. Every
//! other data route is refused under a cutoff (fail closed). Local analytics
//! compute on caller-supplied inputs and never fetch, so they stay available.
use chrono::NaiveDate;
use serde_json::{json, Map, Value};

use crate::{broker::BrokerError, mcp::McpCallResult, mcp::McpTool};

/// What a route's row date proves about availability at a cutoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum DateBasis {
    /// A completed bar or event observed on that date; public by its end.
    Observation,
    /// The provider-declared filing date.
    Publication,
}

impl DateBasis {
    fn label(self) -> &'static str {
        match self {
            Self::Observation => "observation_date",
            Self::Publication => "publication_date",
        }
    }
}

#[derive(Debug)]
pub(super) struct DatedRoute {
    pub tool: &'static str,
    /// Row fields tried in order; the first parseable date decides the row.
    pub date_fields: &'static [&'static str],
    pub basis: DateBasis,
    /// Values may be restated for corporate actions after the cutoff.
    pub adjusted_values: bool,
}

/// The only OpenBB data routes allowed under a historical cutoff.
pub(super) const DATED_ROUTES: &[DatedRoute] = &[
    DatedRoute {
        tool: "equity_price_historical",
        date_fields: &["date"],
        basis: DateBasis::Observation,
        adjusted_values: true,
    },
    DatedRoute {
        tool: "etf_historical",
        date_fields: &["date"],
        basis: DateBasis::Observation,
        adjusted_values: true,
    },
    DatedRoute {
        tool: "index_price_historical",
        date_fields: &["date"],
        basis: DateBasis::Observation,
        adjusted_values: false,
    },
    DatedRoute {
        tool: "crypto_price_historical",
        date_fields: &["date"],
        basis: DateBasis::Observation,
        adjusted_values: false,
    },
    DatedRoute {
        tool: "currency_price_historical",
        date_fields: &["date"],
        basis: DateBasis::Observation,
        adjusted_values: false,
    },
    DatedRoute {
        tool: "derivatives_futures_historical",
        date_fields: &["date"],
        basis: DateBasis::Observation,
        adjusted_values: false,
    },
    // A dividend is declared before its ex-date, so ex-date <= cutoff bounds
    // its announcement. Amounts can still be split-adjusted later.
    DatedRoute {
        tool: "equity_fundamental_dividends",
        date_fields: &["ex_dividend_date"],
        basis: DateBasis::Observation,
        adjusted_values: true,
    },
    DatedRoute {
        tool: "equity_fundamental_filings",
        date_fields: &["filing_date"],
        basis: DateBasis::Publication,
        adjusted_values: false,
    },
];

pub(super) fn dated_route(tool_name: &str) -> Option<&'static DatedRoute> {
    DATED_ROUTES.iter().find(|route| route.tool == tool_name)
}

pub(super) const AS_OF_REFUSAL_NEXT: &str = "Under this cutoff use a dated OpenBB route (price history, dividends or filings), a native tool that accepts as_of, or evidence retained before the cutoff. Say that this data cannot be shown as it was known then; do not answer from later data or model memory.";
const AS_OF_EMPTY_NEXT: &str = "Nothing in this result was known by the cutoff. Widen the date range before the cutoff or report that no data was available then.";

/// Refusal for a data route that cannot prove availability at the cutoff.
pub(super) fn refusal(tool_name: &str, cutoff: NaiveDate) -> BrokerError {
    BrokerError::Execution(format!(
        "as_of_unsupported: {tool_name} cannot show what was known on {cutoff}; its rows carry no date Loupist can verify against the as-of cutoff"
    ))
    .with_next(AS_OF_REFUSAL_NEXT)
}

/// How a request was bounded to the cutoff.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct Bounded {
    /// `end_date` was set to the cutoff.
    pub end_date: bool,
    /// `adjustment` was forced to the unadjusted option.
    pub unadjusted: bool,
}

const UNADJUSTED: &[&str] = &["unadjusted", "raw"];

/// The unadjusted value the declared `adjustment` parameter accepts for this
/// provider: its per-provider choices when the schema lists them, otherwise a
/// value every alternative of the parameter accepts.
fn unadjusted_option(tool: &McpTool, provider: Option<&str>) -> Option<&'static str> {
    fn values(schema: &Value) -> Option<Vec<&str>> {
        schema
            .get("enum")
            .or_else(|| schema.get("choices"))
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect())
    }
    let property = tool.input_schema.pointer("/properties/adjustment")?;
    let accepted: Vec<Vec<&str>> = match provider
        .and_then(|provider| property.get(provider))
        .and_then(values)
    {
        Some(choices) => vec![choices],
        None => match property.get("anyOf").and_then(Value::as_array) {
            Some(alternatives) => alternatives.iter().filter_map(values).collect(),
            None => values(property).into_iter().collect(),
        },
    };
    UNADJUSTED.iter().copied().find(|option| {
        !accepted.is_empty() && accepted.iter().all(|values| values.contains(option))
    })
}

/// Bounds the request to the cutoff using only parameters the tool declares.
/// Adjusted prices are restated for splits and dividends after the cutoff, so
/// an unadjusted option is forced when the provider offers one.
pub(super) fn bound_arguments(
    tool: &McpTool,
    arguments: &mut Value,
    cutoff: NaiveDate,
) -> Result<Bounded, BrokerError> {
    let declared = |name: &str| {
        tool.input_schema
            .get("properties")
            .and_then(|properties| properties.get(name))
            .is_some()
    };
    let unadjusted = unadjusted_option(tool, arguments.get("provider").and_then(Value::as_str));
    let object = arguments.as_object_mut().ok_or(BrokerError::Malformed)?;
    if let Some(option) = unadjusted {
        object.insert("adjustment".into(), json!(option));
    }
    let mut result = Bounded {
        end_date: false,
        unadjusted: unadjusted.is_some(),
    };
    if declared("start_date") {
        if let Some(start) = object.get("start_date").and_then(Value::as_str) {
            if parse_date(start).is_some_and(|start| start > cutoff) {
                return Err(BrokerError::invalid(
                    "start_date",
                    format!("must be on or before the as-of cutoff {cutoff}"),
                ));
            }
        }
    }
    if !declared("end_date") {
        return Ok(result);
    }
    result.end_date = match object.get("end_date") {
        None | Some(Value::Null) => true,
        Some(Value::String(end)) => parse_date(end).is_none_or(|end| end > cutoff),
        Some(_) => return Err(BrokerError::invalid("end_date", "expected YYYY-MM-DD")),
    };
    if result.end_date {
        object.insert("end_date".into(), json!(cutoff.to_string()));
    }
    Ok(result)
}

/// Removes rows after the cutoff from the canonical structured result and
/// rebuilds the text block from it, so no unfiltered copy survives.
pub(super) fn filter_result(
    route: &DatedRoute,
    result: &mut McpCallResult,
    cutoff: NaiveDate,
    bounded: Bounded,
) -> Result<Value, BrokerError> {
    let refused = || {
        BrokerError::Execution(format!(
            "as_of_unsupported: {} returned no dated rows to check against the as-of cutoff {cutoff}",
            route.tool
        ))
        .with_next(AS_OF_REFUSAL_NEXT)
    };
    let structured = result
        .structured_content
        .as_mut()
        .and_then(Value::as_object_mut)
        .ok_or_else(refused)?;
    let rows = structured
        .get_mut("results")
        .and_then(Value::as_array_mut)
        .ok_or_else(refused)?;
    let before = rows.len();
    let mut unknown = 0_usize;
    rows.retain(|row| match row_date(row, route.date_fields) {
        Some(date) => date <= cutoff,
        None => {
            unknown += 1;
            false
        }
    });
    let kept = rows.len();
    // Charts are rendered from the unfiltered provider result.
    structured.remove("chart");
    let excluded = before - kept;
    if kept == 0 {
        return Err(BrokerError::Execution(format!(
            "as_of_empty: {} had no rows dated on or before the as-of cutoff {cutoff} ({excluded} later or undated rows were removed)",
            route.tool
        ))
        .with_next(AS_OF_EMPTY_NEXT));
    }
    let text = serde_json::to_string(&Value::Object(structured.clone()))
        .map_err(|_| BrokerError::Execution("MCP result was invalid".into()))?;
    result.content = vec![json!({"type": "text", "text": text})];
    let mut summary = Map::new();
    summary.insert("cutoff".into(), json!(cutoff.to_string()));
    summary.insert("basis".into(), json!(route.basis.label()));
    summary.insert("rows_kept".into(), json!(kept));
    summary.insert(
        "post_cutoff_excluded".into(),
        json!(excluded.saturating_sub(unknown)),
    );
    summary.insert("undated_excluded".into(), json!(unknown));
    summary.insert("end_date_bounded".into(), json!(bounded.end_date));
    if bounded.unadjusted {
        summary.insert("adjustment".into(), json!("unadjusted"));
        summary.insert(
            "note".into(),
            json!("Prices are unadjusted: the as-of cutoff forced the unadjusted option so splits or dividends after the cutoff cannot restate them."),
        );
    } else if route.adjusted_values {
        summary.insert(
            "note".into(),
            json!("Values may be adjusted for splits or dividends that happened after the cutoff."),
        );
    }
    Ok(Value::Object(summary))
}

fn row_date(row: &Value, fields: &[&str]) -> Option<NaiveDate> {
    fields
        .iter()
        .find_map(|field| row.get(*field).and_then(Value::as_str).and_then(parse_date))
}

/// Accepts a calendar date or a timestamp; a timestamp keeps its own local date.
fn parse_date(value: &str) -> Option<NaiveDate> {
    let head = value.get(..10)?;
    let date = NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()?;
    let rest = &value[10..];
    (rest.is_empty() || rest.starts_with('T') || rest.starts_with(' ')).then_some(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(properties: Value) -> McpTool {
        McpTool {
            name: "equity_price_historical".into(),
            title: None,
            description: None,
            input_schema: json!({"type": "object", "properties": properties}),
            output_schema: None,
            annotations: Some(json!({"readOnlyHint": true})),
            metadata: None,
        }
    }

    fn cutoff() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 3, 3).unwrap()
    }

    #[test]
    fn requests_are_bounded_to_the_cutoff_through_declared_parameters_only() {
        let dated = tool(json!({"symbol": {}, "start_date": {}, "end_date": {}}));
        let mut open = json!({"symbol": "AAA"});
        assert!(
            bound_arguments(&dated, &mut open, cutoff())
                .unwrap()
                .end_date
        );
        assert_eq!(open["end_date"], "2026-03-03");
        let mut later = json!({"symbol": "AAA", "end_date": "2026-06-30"});
        assert!(
            bound_arguments(&dated, &mut later, cutoff())
                .unwrap()
                .end_date
        );
        assert_eq!(later["end_date"], "2026-03-03");
        let mut earlier = json!({"symbol": "AAA", "end_date": "2026-02-01"});
        assert!(
            !bound_arguments(&dated, &mut earlier, cutoff())
                .unwrap()
                .end_date
        );
        assert_eq!(earlier["end_date"], "2026-02-01");
        let mut future_start = json!({"symbol": "AAA", "start_date": "2026-04-01"});
        assert!(bound_arguments(&dated, &mut future_start, cutoff()).is_err());
        // A route without date parameters is filtered only.
        let undated = tool(json!({"symbol": {}}));
        let mut filings = json!({"symbol": "AAA"});
        assert_eq!(
            bound_arguments(&undated, &mut filings, cutoff()).unwrap(),
            Bounded::default()
        );
        assert_eq!(filings, json!({"symbol": "AAA"}));
    }

    /// OpenBB's `adjustment` parameter as the bundled runtime declares it:
    /// a union of provider enums plus per-provider choices.
    fn adjusted_history() -> McpTool {
        tool(
            json!({"symbol": {}, "end_date": {}, "provider": {"enum": ["tmx", "yfinance"]},
            "adjustment": {
                "anyOf": [
                    {"enum": ["splits_only", "splits_and_dividends", "unadjusted"], "type": "string"},
                    {"enum": ["splits_only", "splits_and_dividends"], "type": "string"}
                ],
                "tmx": {"choices": ["splits_only", "splits_and_dividends", "unadjusted"]},
                "yfinance": {"choices": ["splits_only", "splits_and_dividends"]},
                "default": "splits_only"
            }}),
        )
    }

    fn history_result() -> McpCallResult {
        McpCallResult {
            content: vec![],
            structured_content: Some(json!({"results": [
                {"date": "2026-03-02", "close": 10.0},
                {"date": "2026-03-04", "close": 11.0}
            ]})),
            is_error: false,
            metadata: None,
        }
    }

    #[test]
    fn a_cutoff_forces_unadjusted_prices_where_the_provider_offers_them() {
        let route = dated_route("equity_price_historical").unwrap();
        let history = adjusted_history();
        let mut tmx =
            json!({"symbol": "AAA", "provider": "tmx", "adjustment": "splits_and_dividends"});
        let bounded = bound_arguments(&history, &mut tmx, cutoff()).unwrap();
        assert_eq!(
            bounded,
            Bounded {
                end_date: true,
                unadjusted: true
            }
        );
        assert_eq!(tmx["adjustment"], "unadjusted");
        let summary = filter_result(route, &mut history_result(), cutoff(), bounded).unwrap();
        assert_eq!(summary["adjustment"], "unadjusted");
        assert!(summary["note"]
            .as_str()
            .unwrap()
            .starts_with("Prices are unadjusted"));

        // yfinance has no unadjusted option; the request is kept and the note says so.
        let mut yfinance = json!({"symbol": "AAA", "provider": "yfinance"});
        let bounded = bound_arguments(&history, &mut yfinance, cutoff()).unwrap();
        assert!(!bounded.unadjusted);
        assert!(yfinance.get("adjustment").is_none());
        let summary = filter_result(route, &mut history_result(), cutoff(), bounded).unwrap();
        assert!(summary.get("adjustment").is_none());
        assert!(summary["note"]
            .as_str()
            .unwrap()
            .contains("may be adjusted"));

        // Without per-provider choices, only an option every alternative accepts is forced.
        let plain = tool(json!({"adjustment": {"enum": ["splits_only", "unadjusted"]}}));
        let mut request = json!({"symbol": "AAA"});
        assert!(
            bound_arguments(&plain, &mut request, cutoff())
                .unwrap()
                .unadjusted
        );
        assert_eq!(request["adjustment"], "unadjusted");
        let mut request = json!({"symbol": "AAA", "provider": "unknown"});
        assert!(
            !bound_arguments(&history, &mut request, cutoff())
                .unwrap()
                .unadjusted
        );
    }

    #[test]
    fn dates_and_timestamps_keep_their_calendar_day() {
        assert_eq!(parse_date("2026-03-03"), Some(cutoff()));
        assert_eq!(parse_date("2026-03-03T15:30:00-05:00"), Some(cutoff()));
        assert_eq!(parse_date("2026-03-03 15:30:00"), Some(cutoff()));
        assert_eq!(parse_date("2026-03-031"), None);
        assert_eq!(parse_date("March 3"), None);
    }
}
