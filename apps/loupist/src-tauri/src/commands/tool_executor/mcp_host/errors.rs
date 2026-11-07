//! Fixed, host-owned diagnostics and recovery guidance. Never expose remote error bodies.
use crate::{broker::BrokerError, mcp::McpError};
use serde_json::{json, Value};

pub(super) fn mcp_call_error_with_inventory(
    tools: Option<Vec<Value>>,
    error: BrokerError,
) -> Result<Value, BrokerError> {
    // Offline reads must remain broker failures so the turn records their
    // typed code, even if activation also refreshed the tool inventory.
    if is_offline(&error) {
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
        McpError::Remote(text) => return rejected_mcp_text(&text),
    };
    BrokerError::Execution(message.into()).with_next(next)
}

pub(super) fn rejected_mcp_result(result: &crate::mcp::McpCallResult) -> BrokerError {
    if result
        .structured_content
        .as_ref()
        .and_then(|value| value.pointer("/loupist_error/kind"))
        == Some(&json!("offline"))
    {
        return BrokerError::Offline("market data provider".into());
    }
    // MCP tools commonly put a JSON error inside a text content block.
    // Decode one layer only; never return this provider-controlled body.
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
    let diagnostic = serde_json::to_string(&json!({
        "content": content,
        "structuredContent": result.structured_content,
    }))
    .unwrap_or_default();
    rejected_mcp_text(&diagnostic)
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
            "Call connection_setup with this provider's connection name so the user checks and verifies it in a card, including its subscription access. Do not request a key in Chat or retry with guessed credentials.",
        )
    } else {
        (
            "The MCP provider rejected the request",
            "Check the active tool schema, provider, symbol, and date range. Change the request before trying again; if it still fails, report that this data is unavailable.",
        )
    };
    BrokerError::Execution(message.into()).with_next(next)
}

fn mcp_has_status(value: &Value, status: u64) -> bool {
    match value {
        Value::Object(fields) => fields.iter().any(|(key, value)| {
            (matches!(
                key.as_str(),
                "status" | "status_code" | "statusCode" | "code"
            ) && (value.as_u64() == Some(status)
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
}
