//! Explicit allowlist: additions to the product catalog do not grant unattended access.
use crate::broker::ToolMethod;

pub(crate) const REFUSAL: &str = "Unavailable in unattended runs; only pre-approved read-only tools and Memory reads/saves are allowed";

pub(crate) fn allowed_name(name: &str) -> bool {
    matches!(
        name,
        "capability_list"
            | "capability_load"
            | "skills_list"
            | "skills_read"
            | "chat_history"
            | "dataset_describe"
            | "dataset_query"
            | "calculate"
            | "worldbank_series"
            | "defillama_read"
            | "krx_daily_quote"
            | "kis_operation_search"
            | "kis_read"
            | "dart_financials"
            | "dart_filing_search"
            | "dart_filing_read"
            | "dart_company_lookup"
            | "dart_major_report"
            | "dart_ownership"
            | "dart_dividends"
            | "run_results_list"
            | "plan_update"
            | "web_search"
            | "web_fetch"
            | "source_save"
            | "world_search"
            | "world_read"
            | "note_save"
            | "lens_update"
            | "lens_search"
            | "lens_read"
            | "world_job"
            | "conversation_memory"
            | "asset_page_read"
            | "thesis_update"
            | "earnings_snapshot_save"
            | "artifact_list"
            | "artifact_read"
            | "chart_query"
    )
}

pub(crate) fn allowed_method(method: ToolMethod) -> bool {
    // MCP performs its own captured-grant and read-only/Always check before I/O.
    if matches!(method, ToolMethod::McpConnect | ToolMethod::McpCall) {
        return true;
    }
    serde_json::to_value(method)
        .ok()
        .and_then(|v| v.as_str().map(allowed_name))
        .unwrap_or(false)
}

pub(crate) fn enforce(
    store: &crate::store::SqliteStore,
    loupist_id: &str,
    run_id: &str,
    method: ToolMethod,
    params: &serde_json::Value,
) -> Result<(), crate::broker::BrokerError> {
    if allowed_method(method) {
        return Ok(());
    }
    let name = if method == ToolMethod::NativeAuthorize {
        params
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("native")
            .to_owned()
    } else {
        serde_json::to_value(method)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    store
        .scheduled_run_skip_tool(loupist_id, run_id, &name, REFUSAL)
        .map_err(|_| {
            crate::broker::BrokerError::Execution("Could not record refused unattended tool".into())
        })?;
    Err(crate::broker::BrokerError::Execution(REFUSAL.into()))
}
