//! Explicit allowlist: each broker method's row in `ToolMethod::safety`
//! decides whether unattended runs may call it, so adding a tool to the
//! product catalog does not grant unattended access by itself.
use crate::broker::{tool_class::Unattended, ToolMethod};

pub(crate) const REFUSAL: &str = "Unavailable in unattended runs; only pre-approved read-only tools and Memory reads/saves are allowed";

/// A model-facing tool name. Capability loading has no broker method.
pub(crate) fn allowed_name(name: &str) -> bool {
    match ToolMethod::from_tool_name(name) {
        Some(method) => method.safety().unattended == Unattended::Allowed,
        None => name == "capability_load",
    }
}

pub(crate) fn allowed_method(method: ToolMethod) -> bool {
    match method.safety().unattended {
        // MCP performs its own captured-grant and read-only/Always check before I/O.
        Unattended::Allowed | Unattended::GrantChecked => true,
        Unattended::Refused => false,
    }
}

pub(crate) fn enforce(
    store: &crate::store::SqliteStore,
    loupist_id: &str,
    scheduled_run_id: Option<&str>,
    method: ToolMethod,
    params: &serde_json::Value,
) -> Result<(), crate::broker::BrokerError> {
    if allowed_method(method) {
        return Ok(());
    }
    if let Some(run_id) = scheduled_run_id {
        let name = if method == ToolMethod::NativeAuthorize {
            params
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("native")
                .to_owned()
        } else {
            method.tool_name()
        };
        store
            .scheduled_run_skip_tool(loupist_id, run_id, &name, REFUSAL)
            .map_err(|_| {
                crate::broker::BrokerError::Execution(
                    "Could not record refused unattended tool".into(),
                )
            })?;
    }
    Err(crate::broker::BrokerError::Execution(REFUSAL.into()))
}
