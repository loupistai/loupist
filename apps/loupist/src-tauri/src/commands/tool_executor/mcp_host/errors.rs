//! Fixed, host-owned diagnostics and recovery guidance. Never expose remote error bodies.
use crate::{broker::BrokerError, mcp::McpError};
use serde_json::{json, Value};

pub(super) fn mcp_call_error_with_inventory(
    tools: Option<Vec<Value>>,
    error: BrokerError,
) -> Result<Value, BrokerError> {
    // Preserve the typed broker code even when activation refreshed tools.
    if is_offline(&error) || is_argument_failure(&error) || is_sec_contact_required(&error) {
        return Err(error);
    }
    let message = safe_mcp_error_summary(&error);
    match tools {
        Some(tools) => Ok(
            json!({ "call_error": true, "message": message, "next": mcp_error_next(&error), "tools": tools }),
        ),
        None => Err(error),
    }
}

fn is_offline(error: &BrokerError) -> bool {
    match error {
        BrokerError::Offline(_) => true,
        BrokerError::WithNext { source, .. } => is_offline(source),
        _ => false,
    }
}

pub(super) fn is_argument_failure(error: &BrokerError) -> bool {
    match error {
        BrokerError::InvalidArguments { .. } => true,
        BrokerError::WithNext { source, .. } => is_argument_failure(source),
        _ => false,
    }
}

pub(super) fn is_sec_contact_required(error: &BrokerError) -> bool {
    match error {
        BrokerError::SecContactRequired(_) => true,
        BrokerError::WithNext { source, .. } => is_sec_contact_required(source),
        _ => false,
    }
}

const MCP_REQUEST_REJECTED: &str = "The MCP provider rejected the request";
const MCP_REQUEST_NEXT: &str = "Check the active tool schema, provider, symbol, and date range. Change the request before trying again; if it still fails, report that this data is unavailable.";

pub(super) fn invalid_mcp_arguments() -> BrokerError {
    BrokerError::invalid("", MCP_REQUEST_REJECTED).with_next(MCP_REQUEST_NEXT)
}

fn safe_mcp_error_summary(error: &BrokerError) -> String {
    error
        .to_string()
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(240)
        .collect()
}

fn mcp_error_next(error: &BrokerError) -> Option<&str> {
    match error {
        BrokerError::WithNext { next, .. } => Some(next.as_ref()),
        _ => None,
    }
}

pub(super) fn stopped_mcp_response(error: Option<&BrokerError>) -> Value {
    json!({
        "call_error": true,
        "session_stopped": true,
        "message": error.map(safe_mcp_error_summary),
        "next": format!("Load this capability again before calling its tools. {}", error.and_then(mcp_error_next).unwrap_or("If it stops again, finish with the available evidence and report that the connection is unavailable.")),
    })
}

pub(super) fn map_mcp_error(error: McpError) -> BrokerError {
    let (message, next) = match error {
        McpError::MissingExecutable | McpError::UntrustedExecutable => (
            "The bundled MCP runtime is missing or could not be verified",
            "Ask the user to repair or reinstall Loupist. Do not install a replacement runtime through Chat.",
        ),
        McpError::VersionMismatch => (
            "The bundled MCP runtime uses an unsupported protocol version",
            "Ask the user to update or repair Loupist before using this connection again.",
        ),
        McpError::Timeout => (
            "The MCP request timed out",
            "Reduce the requested date range, symbols, or page size. Do not repeat the identical request in a loop.",
        ),
        // Only the bundled runtime reaches this mapping, before any request ran.
        McpError::StartupTimeout => (
            "Market data was still starting and did not become ready in time",
            "This computer may be busy. Do not load it again in this turn: continue with your other sources, and tell the user that market data was still starting and that they can ask again shortly.",
        ),
        McpError::FrameTooLarge => (
            "The MCP response exceeded the size limit",
            "Load this capability again, then request a smaller date range or page. Do not repeat the oversized request.",
        ),
        McpError::ToolLimit => (
            "The MCP tool inventory exceeded the size limit",
            "Ask the user to update or repair Loupist. Repeating discovery cannot reduce this inventory.",
        ),
        McpError::Lease(_) => (
            "The MCP runtime could not acquire its private process ownership",
            "Ask the user to restart Loupist before reconnecting. Do not modify process ownership files through Chat.",
        ),
        McpError::Io(_) | McpError::Protocol | McpError::Stopped => (
            "The MCP runtime stopped or lost its connection",
            "Load this capability again to reconnect. If it stops again, finish with the available evidence and report that the connection is unavailable.",
        ),
        // Remote diagnostics may contain credentials or private paths. Only
        // a fixed category and its host-owned recovery advice may leave Rust.
        McpError::InvalidParams => return invalid_mcp_arguments(),
        McpError::Remote(text) => return rejected_mcp_text(&text),
    };
    BrokerError::Execution(message.into()).with_next(next)
}

/// What the agent is told when Yahoo Finance cannot serve the price as traded.
const YAHOO_ADJUSTMENT_EXPECTED: &str = "splits_only or splits_and_dividends. Yahoo Finance restates its prices for later share splits, so it gives the price as traded only for a daily bar with no split after it. Read the adjusted price and say that it is adjusted, or read the price as traded from a connected source that serves unadjusted prices.";

pub(super) fn rejected_mcp_result(result: &crate::mcp::McpCallResult) -> BrokerError {
    let marker = result
        .structured_content
        .as_ref()
        .and_then(|value| value.get("loupist_error"));
    match marker.and_then(|marker| marker["kind"].as_str()) {
        Some("invalid_destination") => {
            let field = result
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/loupist_error/field"))
                .and_then(Value::as_str);
            let (field, expected) = match field {
                Some("bill_url") => (
                    "bill_url",
                    "A bill as congress/type/number, such as 118/hr/1, or its address under https://api.congress.gov/v3/bill/. No other address is read.",
                ),
                Some("amendment_url") => (
                    "amendment_url",
                    "An amendment as congress/type/number, such as 119/hamdt/2, or its address under https://api.congress.gov/v3/amendment/. No other address is read.",
                ),
                Some("congress_gov") => (
                    "congress_gov",
                    "Document addresses on https://www.congress.gov/, or for bill text also https://www.govinfo.gov/, as the text_urls tool returns them. No other site is read.",
                ),
                _ => (
                    "url",
                    "A filing address on https://www.sec.gov/. No other site is read; web_fetch reads other public pages.",
                ),
            };
            return BrokerError::invalid(field, expected);
        }
        Some("invalid_arguments") => {
            if marker.is_some_and(|marker| marker["provider"] == "fred") {
                let field = marker
                    .filter(|marker| marker["field"] == "symbol")
                    .map_or("", |_| "symbol");
                return BrokerError::invalid(field, MCP_REQUEST_REJECTED)
                    .with_next("Find the series id with economy_fred_search, then read it again.");
            }
            return invalid_mcp_arguments();
        }
        Some("unsupported_adjustment") => {
            return BrokerError::invalid("adjustment", YAHOO_ADJUSTMENT_EXPECTED);
        }
        Some("offline") => return BrokerError::Offline("market data provider".into()),
        Some("sec_contact_required")
            if marker.is_some_and(|marker| marker["provider"] == "sec") =>
        {
            return BrokerError::SecContactRequired("SEC answers only requests that declare a contact, and none is set in this app, so the sec provider was not used.")
                .with_next("Call this tool again with another provider listed for it. Tell the user that adding their contact under Settings > Data connections > SEC EDGAR enables SEC data.");
        }
        Some("alpha_vantage_paid_plan_required") => {
            return BrokerError::Execution("This needs a paid Alpha Vantage plan.".into());
        }
        _ => {}
    }
    rejected_mcp_text(&result_diagnostic(result).to_string())
}

/// MCP tools also put JSON errors inside text content blocks. Decode one
/// layer and use the result only for classification.
fn result_diagnostic(result: &crate::mcp::McpCallResult) -> Value {
    let content = result
        .content
        .iter()
        .map(|part| {
            if part["type"] == "text" {
                if let Some(value) = part["text"]
                    .as_str()
                    .and_then(|text| serde_json::from_str::<Value>(text).ok())
                {
                    return value;
                }
            }
            part.clone()
        })
        .collect::<Vec<_>>();
    json!({
        "content": content,
        "structuredContent": result.structured_content,
    })
}

fn rejected_mcp_text(diagnostic: &str) -> BrokerError {
    let lower = diagnostic.to_ascii_lowercase();
    let structured = serde_json::from_str::<Value>(diagnostic).ok();
    let has_status = |status| {
        structured
            .as_ref()
            .is_some_and(|value| mcp_has_status(value, status))
    };
    let (message, next) = if has_status(429)
        || [
            "rate limit",
            "rate_limit",
            "too many requests",
            "http 429",
            "status code 429",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        (
            "The MCP provider reported a rate limit",
            "Stop calling this provider for now. Report the limit and use already available evidence; do not retry the same request this turn.",
        )
    } else if has_status(401)
        || has_status(403)
        || [
            "unauthorized",
            "forbidden",
            "authentication",
            "invalid api key",
            "invalid_api_key",
            "http 401",
            "http 403",
            "status code 401",
            "status code 403",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    {
        (
            "The MCP provider rejected authentication or access",
            "If connection_setup is available, call it with this provider's connection name so the user checks and verifies it in a card, including its subscription access; otherwise tell the user to check it in Connections. Do not request a key in Chat or retry with guessed credentials.",
        )
    } else if structured
        .as_ref()
        .is_some_and(structured_argument_rejection)
    {
        return invalid_mcp_arguments();
    } else {
        (MCP_REQUEST_REJECTED, MCP_REQUEST_NEXT)
    };
    BrokerError::Execution(message.into()).with_next(next)
}

fn structured_argument_rejection(value: &Value) -> bool {
    [400, 422, -32602]
        .into_iter()
        .any(|code| mcp_has_status(value, code))
}

pub(super) fn rejected_custom_arguments(value: &Value) -> Option<BrokerError> {
    let result = serde_json::from_value::<crate::mcp::McpCallResult>(value.clone()).ok()?;
    (result.is_error && structured_argument_rejection(&result_diagnostic(&result)))
        .then(invalid_mcp_arguments)
}

fn mcp_has_status(value: &Value, status: i64) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            (matches!(
                key.as_str(),
                "status" | "status_code" | "statusCode" | "code"
            ) && (value.as_i64() == Some(status)
                || value.as_str().and_then(|text| text.parse().ok()) == Some(status)))
                || mcp_has_status(value, status)
        }),
        Value::Array(values) => values.iter().any(|value| mcp_has_status(value, status)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openbb_offline_results_keep_the_typed_broker_failure_after_activation() {
        let result: crate::mcp::McpCallResult = serde_json::from_value(json!({
            "isError": true,
            "content": [{"type":"text", "text":"private provider diagnostic"}],
            "structuredContent": {"loupist_error": {"kind": "offline"}},
        }))
        .unwrap();
        for tools in [
            None,
            Some(vec![json!({"name":"mcp__openbb__equity_price_historical"})]),
        ] {
            let error =
                mcp_call_error_with_inventory(tools, rejected_mcp_result(&result)).unwrap_err();
            assert!(matches!(error, BrokerError::Offline(_)));
            assert!(!error.to_string().contains("private provider diagnostic"));
        }
    }

    #[test]
    fn yahoo_adjustment_refusal_keeps_the_same_providers_supported_options() {
        let result = serde_json::from_value(json!({
            "isError": true, "content": [],
            "structuredContent": {"loupist_error": {"kind": "unsupported_adjustment"}},
        }))
        .unwrap();
        let error = super::super::provider_failure(
            rejected_mcp_result(&result),
            None,
            Some("yfinance"),
            true,
        );
        assert!(
            matches!(error, BrokerError::InvalidArguments { field, expected }
            if field == "adjustment" && expected == YAHOO_ADJUSTMENT_EXPECTED)
        );
    }

    #[test]
    fn refused_destination_is_an_argument_error_without_remote_text() {
        let result = serde_json::from_value(json!({
            "isError": true, "content": [],
            "structuredContent": {"loupist_error": {"kind": "invalid_destination", "field": "bill_url"}},
        })).unwrap();
        assert!(
            matches!(rejected_mcp_result(&result), BrokerError::InvalidArguments { field, .. } if field == "bill_url")
        );
    }

    #[test]
    fn a_paid_alpha_request_names_the_plan_without_reconnecting() {
        let result = serde_json::from_value(json!({
            "isError": true,
            "content": [{"type":"text", "text":"fixture-private-detail"}],
            "structuredContent": {"loupist_error": {"kind": "alpha_vantage_paid_plan_required"}},
        }))
        .unwrap();
        let error = rejected_mcp_result(&result);
        assert!(error.to_string().contains("paid Alpha Vantage plan"));
        assert!(!error.to_string().contains("fixture-private-detail"));
        assert!(mcp_error_next(&error).is_none());
    }

    #[test]
    fn mcp_failures_offer_recovery_without_exposing_provider_or_transport_details() {
        let private = "fixture-private-detail";
        let rate_limited: crate::mcp::McpCallResult = serde_json::from_value(json!({
            "isError": true,
            "content": [{"type":"text", "text":json!({"status_code":429,"private":private}).to_string()}],
        }))
        .unwrap();
        let auth = map_mcp_error(McpError::Remote(format!(
            "HTTP 401 Unauthorized: {private}"
        )));
        let rate = rejected_mcp_result(&rate_limited);
        assert_ne!(mcp_error_next(&auth), mcp_error_next(&rate));
        let tools = vec![json!({"name":"mcp__openbb__available_tools"})];
        for error in [auth, rate, map_mcp_error(McpError::Remote(private.into()))] {
            assert!(mcp_error_next(&error).is_some());
            let response = mcp_call_error_with_inventory(Some(tools.clone()), error).unwrap();
            assert_eq!(response["tools"], json!(tools));
            assert!(!response.to_string().contains(private));
        }
        for error in [
            map_mcp_error(McpError::Io(std::io::Error::other(private))),
            map_mcp_error(McpError::Timeout),
        ] {
            let response = stopped_mcp_response(Some(&error));
            assert_eq!(response["session_stopped"], true);
            assert!(response["next"]
                .as_str()
                .unwrap()
                .contains(mcp_error_next(&error).unwrap()));
            assert!(!response.to_string().contains(private));
        }
    }

    #[test]
    fn structured_input_refusals_keep_the_broker_argument_type() {
        let tools = vec![json!({"name":"economy_fred_search"})];
        let result = serde_json::from_value(json!({
            "isError": true, "content": [{"type":"text","text":"private-canary"}],
            "structuredContent": {"loupist_error": {
                "kind":"invalid_arguments", "provider":"fred", "field":"symbol"
            }},
        }))
        .unwrap();
        let error =
            mcp_call_error_with_inventory(Some(tools), rejected_mcp_result(&result)).unwrap_err();
        assert!(is_argument_failure(&error));
        assert!(error.to_string().starts_with("symbol:"));
        assert_eq!(
            mcp_error_next(&error),
            Some("Find the series id with economy_fred_search, then read it again."),
        );
        assert!(!error.to_string().contains("private-canary"));

        for content in [
            json!({"code":-32602, "message":"private-canary"}),
            json!({"status_code":422, "detail":"private-canary"}),
        ] {
            let mut wire = json!({
                "isError": true, "content": [{"type":"text","text":content.to_string()}],
            });
            let error = rejected_custom_arguments(&wire).unwrap();
            assert!(is_argument_failure(&error));
            assert!(!error.to_string().contains("private-canary"));
            wire["isError"] = json!(false);
            assert!(rejected_custom_arguments(&wire).is_none());
        }
    }

    #[test]
    fn a_runtime_that_never_finished_starting_says_market_data_was_starting() {
        let error = map_mcp_error(McpError::StartupTimeout);
        let message = error.to_string();
        assert!(
            message.contains("Market data was still starting"),
            "{message}"
        );
        assert!(!message.contains("timed out"), "{message}");
        let next = mcp_error_next(&error).unwrap();
        // Nothing was requested yet, so request-narrowing advice does not apply.
        assert!(!next.contains("date range"), "{next}");
        assert!(next.contains("Do not load it again in this turn"), "{next}");
    }
}
