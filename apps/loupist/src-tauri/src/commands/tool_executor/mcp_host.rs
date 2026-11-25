mod custom;
mod errors;
mod openbb;
mod point_in_time;

use errors::{
    is_argument_failure, is_sec_contact_required, map_mcp_error, mcp_call_error_with_inventory,
    rejected_mcp_result, stopped_mcp_response,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::*;
use crate::{
    fs::artifact_trust::{create_private_directory, ensure_private_directory},
    marketplace::{
        bundled_catalog, MarketplaceEntryDto, MarketplaceFreeState, MarketplaceRuntimeKind,
    },
    mcp::pool::{authority_fingerprint, McpPoolKey, TurnMcpServer},
    mcp::{
        contains_protected_value, filter_tool_providers, tool_to_agent_schema,
        validate_result_provider, BundledMcpRuntime, McpError, McpLaunchConfig, McpSession,
        McpTool, McpToolActivation, ProviderlessToolPolicy,
    },
};

const MCP_CALL_TIMEOUT: Duration = Duration::from_secs(45);
const MCP_SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

type SharedMcpServer = Arc<RwLock<Option<TurnMcpServer>>>;

#[derive(Clone)]
struct McpTurnConnection {
    server: SharedMcpServer,
    notices: Arc<[String]>,
}

fn protected_values(
    credentials: &BTreeMap<String, String>,
    app_data: &std::path::Path,
    scratch: &std::path::Path,
) -> Vec<String> {
    credentials
        .values()
        .cloned()
        .chain([
            app_data.to_string_lossy().into_owned(),
            scratch.to_string_lossy().into_owned(),
        ])
        .collect()
}

#[derive(Default)]
pub(super) struct McpTurnSessions {
    by_server: BTreeMap<String, McpTurnConnection>,
    closed: bool,
}

impl McpTurnSessions {
    fn get(&self, server_id: &str) -> Option<McpTurnConnection> {
        self.by_server.get(server_id).cloned()
    }

    fn insert_shared_if_absent(
        &mut self,
        server_id: &str,
        connection: McpTurnConnection,
    ) -> Result<Option<McpTurnConnection>, ()> {
        if self.closed {
            return Err(());
        }
        if let Some(existing) = self.by_server.get(server_id) {
            return Ok(Some(existing.clone()));
        }
        self.by_server.insert(server_id.to_owned(), connection);
        Ok(None)
    }

    fn remove_if_current(&mut self, server_id: &str, expected: &SharedMcpServer) {
        if self
            .by_server
            .get(server_id)
            .is_some_and(|current| Arc::ptr_eq(&current.server, expected))
        {
            self.by_server.remove(server_id);
        }
    }

    fn close_and_take_all(&mut self) -> Vec<SharedMcpServer> {
        self.closed = true;
        std::mem::take(&mut self.by_server)
            .into_values()
            .map(|connection| connection.server)
            .collect()
    }
}

enum McpServerInstall {
    Installed,
    Occupied {
        response: Value,
        candidate: Box<TurnMcpServer>,
    },
}

struct McpRunAuthority {
    runtime: BundledMcpRuntime,
    enabled_provider_ids: BTreeSet<String>,
    allowed_network_hosts: BTreeSet<String>,
    credentials: BTreeMap<String, String>,
    provider_config: BTreeMap<String, BTreeMap<String, String>>,
    /// Model-facing notes about setup and saved keys that could not be read.
    setup_notices: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpConnectRequest {
    server_id: String,
    /// Runtime tool names a warm Chat had active when its last turn ended.
    /// Activating exactly these again keeps the declared tool prefix stable.
    #[serde(default)]
    activate_tool_names: Vec<String>,
}

const MAX_ACTIVATION_NAMES: usize = 512;

impl McpConnectRequest {
    fn parse(params: Value) -> Result<Self, BrokerError> {
        let request: Self = serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        let valid_name = |name: &String| {
            !name.is_empty()
                && name.len() <= 128
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        };
        if request.activate_tool_names.len() > MAX_ACTIVATION_NAMES
            || !request.activate_tool_names.iter().all(valid_name)
        {
            return Err(BrokerError::Malformed);
        }
        Ok(request)
    }
}

/// The tools a connect activates: the warm Chat's previous set when it named
/// one (control tools are always present, so they are not requested), else
/// the default read set. The runtime ignores names outside its inventory and
/// the host's provider filter drops what the run cannot use.
fn activation_names(
    requested: &[String],
    control_tool_names: &BTreeSet<String>,
    enabled_provider_ids: &BTreeSet<String>,
) -> Vec<String> {
    let restored = requested
        .iter()
        .filter(|name| !control_tool_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if restored.is_empty() {
        openbb::DEFAULT_ACTIVE_TOOLS
            .iter()
            .copied()
            .chain(
                enabled_provider_ids
                    .contains("fred")
                    .then_some("economy_fred_series"),
            )
            .map(str::to_owned)
            .collect()
    } else {
        restored
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct McpToolCallRequest {
    server_id: String,
    tool_name: String,
    arguments: Value,
}

// Data reads share the immutable authority/inventory while the process serves
// them. Control calls, inventory replacement, pool return and shutdown exclude
// those readers for their entire state transition.
enum McpCallGuard<'a> {
    Read(RwLockReadGuard<'a, Option<TurnMcpServer>>),
    Write(RwLockWriteGuard<'a, Option<TurnMcpServer>>),
}
impl McpCallGuard<'_> {
    fn server(&self) -> &TurnMcpServer {
        match self {
            Self::Read(guard) => guard.as_ref(),
            Self::Write(guard) => guard.as_ref(),
        }
        .expect("loaded MCP session")
    }
}

/// A call admitted on this turn's session: what the server receives and what
/// the host records about it.
struct McpCallPlan {
    /// The arguments forwarded, without host-only ones and bounded to any cutoff.
    arguments: Value,
    requested_provider: Option<String>,
    control_tool: bool,
    /// A retained read this recent is reused instead of calling the server.
    max_age: Option<u64>,
    /// The dated route, cutoff and bounds a cutoff read is filtered by.
    dated: Option<(
        &'static point_in_time::DatedRoute,
        chrono::NaiveDate,
        point_in_time::Bounded,
    )>,
    capture_request: Value,
    request_key: String,
}

/// How a forwarded call ended.
enum McpInvocation {
    /// A checked result; the caller refreshes changed inventory exclusively.
    Answered {
        payload: Value,
        tools: Option<Vec<Value>>,
    },
    /// The call failed and the session stays.
    Failed(BrokerError),
    /// The session can no longer be trusted and is discarded.
    Stopped(Option<BrokerError>),
}

/// Empty row containers, including OpenBB models whose fields are all null.
/// Zero and false are data; nonempty structured metadata is not an empty row section.
fn empty_row(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(values) => values.iter().all(empty_row),
        Value::Object(values) => values.values().all(empty_row),
        Value::Bool(_) | Value::Number(_) => false,
    }
}

fn returned_no_rows(payload: &Value) -> bool {
    match payload
        .get("structuredContent")
        .filter(|value| !value.is_null())
    {
        Some(structured) => match structured.get("results") {
            Some(Value::Array(rows)) => rows.iter().all(empty_row),
            Some(Value::Null) => true,
            None => empty_row(structured),
            _ => false,
        },
        None => payload
            .get("content")
            .and_then(Value::as_array)
            .is_none_or(|parts| {
                parts.iter().all(|part| {
                    part["type"] == "text"
                        && part["text"]
                            .as_str()
                            .is_some_and(|text| text.trim().is_empty())
                })
            }),
    }
}

fn describe_empty_read(payload: &mut Value) {
    payload["status"] = json!("ok");
    payload["row_count"] = json!(0);
    payload["next"] = json!("No data was returned for this request. Try another enabled provider or change the request. Do not cite this empty result.");
}

/// Invocation: forwards the admitted call and checks what came back. A result
/// from a provider the call did not admit, or one echoing a credential, stops
/// the session; a cutoff read keeps only rows on or before the cutoff.
async fn invoke_mcp_tool(
    request: &McpToolCallRequest,
    server: &TurnMcpServer,
    plan: &McpCallPlan,
) -> Result<McpInvocation, BrokerError> {
    let failure = |error, provider_error| {
        McpInvocation::Failed(provider_failure(
            error,
            server.tools.get(&request.tool_name),
            plan.requested_provider.as_deref(),
            provider_error,
        ))
    };
    let call = server
        .session
        .call_tool(&request.tool_name, plan.arguments.clone(), MCP_CALL_TIMEOUT)
        .await;
    let mut result = match call {
        Ok(result) => result,
        Err(error) => {
            let terminal = matches!(
                &error,
                McpError::Io(_) | McpError::Protocol | McpError::Stopped | McpError::FrameTooLarge
            );
            let provider_error = matches!(&error, McpError::Remote(_));
            let mapped = map_mcp_error(error);
            if terminal {
                return Ok(McpInvocation::Stopped(Some(mapped)));
            }
            return Ok(failure(mapped, provider_error));
        }
    };
    if result.is_error {
        return Ok(failure(rejected_mcp_result(&result), true));
    }
    if validate_result_provider(
        &result,
        &server.provider_receipt_pointer,
        plan.requested_provider.as_deref(),
        &server.enabled_provider_ids,
    )
    .is_err()
    {
        return Ok(McpInvocation::Stopped(None));
    }
    let original = serde_json::to_value(&result)
        .map_err(|_| BrokerError::Execution("MCP result was invalid".into()))?;
    if contains_protected_value(&original, &server.sensitive_values) {
        return Ok(McpInvocation::Stopped(None));
    }
    let blank = crate::mcp::result_is_blank(&result);
    // A 204 is already empty, including under a cutoff. Do not run the dated
    // row filter, which requires a structured row section.
    let as_of = match plan.dated.filter(|_| !blank) {
        Some((route, cutoff, bounded)) => {
            match point_in_time::filter_result(route, &mut result, cutoff, bounded) {
                Ok(summary) => Some(summary),
                Err(error) => return Ok(McpInvocation::Failed(error)),
            }
        }
        None => None,
    };
    let mut payload = match as_of {
        Some(_) => serde_json::to_value(&result)
            .map_err(|_| BrokerError::Execution("MCP result was invalid".into()))?,
        None => original,
    };
    if blank && !plan.control_tool {
        payload["structuredContent"] = json!({"results": []});
    }
    if let Some(as_of) = as_of {
        payload["as_of"] = as_of;
    }
    Ok(McpInvocation::Answered {
        payload,
        tools: None,
    })
}

impl ToolCapture {
    pub(crate) async fn shutdown_mcp(&self) {
        let sessions = {
            let mut sessions = self.mcp_sessions.lock().await;
            sessions.close_and_take_all()
        };
        let mut servers = Vec::with_capacity(sessions.len());
        for session in sessions {
            if let Some(server) = session.write().await.take() {
                servers.push(server);
            }
        }
        match &self.mcp_pool {
            Some(pool) => {
                pool.release_in_background(servers);
            }
            None => {
                for server in servers {
                    server.discard().await;
                }
            }
        }
    }

    pub(super) async fn mcp_session(&self, server_id: &str) -> Option<SharedMcpServer> {
        self.mcp_sessions
            .lock()
            .await
            .get(server_id)
            .map(|connection| connection.server)
    }

    async fn remove_mcp_session(&self, server_id: &str, expected: &SharedMcpServer) {
        self.mcp_sessions
            .lock()
            .await
            .remove_if_current(server_id, expected);
    }

    pub(super) async fn discard_mcp_session(
        &self,
        server_id: &str,
        expected: &SharedMcpServer,
        server: Option<TurnMcpServer>,
    ) {
        self.remove_mcp_session(server_id, expected).await;
        if let Some(server) = server {
            server.discard().await;
        }
    }

    async fn install_mcp_session(
        &self,
        server_id: &str,
        server: TurnMcpServer,
        notices: Arc<[String]>,
    ) -> Result<McpServerInstall, BrokerError> {
        let candidate = Arc::new(RwLock::new(Some(server)));
        let connection = McpTurnConnection {
            server: candidate.clone(),
            notices,
        };
        loop {
            let existing = self
                .mcp_sessions
                .lock()
                .await
                .insert_shared_if_absent(server_id, connection.clone());
            let existing = match existing {
                Ok(existing) => existing,
                Err(()) => {
                    let candidate = candidate
                        .write()
                        .await
                        .take()
                        .expect("unpublished MCP candidate disappeared");
                    candidate.discard().await;
                    return Err(BrokerError::Execution("MCP turn stopped".into()));
                }
            };
            let Some(existing) = existing else {
                return Ok(McpServerInstall::Installed);
            };
            let response = {
                let server = existing.server.write().await;
                server.as_ref().map(|server| {
                    server_cards(server_id, server)
                        .map(|tools| connect_response(server_id, tools, &existing.notices))
                })
            };
            if let Some(response) = response {
                let candidate = candidate
                    .write()
                    .await
                    .take()
                    .expect("unpublished MCP candidate disappeared");
                return match response {
                    Ok(response) => Ok(McpServerInstall::Occupied {
                        response,
                        candidate: Box::new(candidate),
                    }),
                    Err(error) => {
                        candidate.discard().await;
                        Err(error)
                    }
                };
            }
            self.remove_mcp_session(server_id, &existing.server).await;
        }
    }
}

impl AppToolExecutor {
    pub(super) async fn mcp_connect(&self, params: Value) -> Result<Value, BrokerError> {
        let request = McpConnectRequest::parse(params)?;
        if crate::custom_connections::service::valid_id(&request.server_id) {
            return self.custom_mcp_connect(&request.server_id).await;
        }
        #[cfg(feature = "eval-harness")]
        if crate::eval_harness::replay::enabled() {
            return Err(BrokerError::Execution(
                "eval_replay_unmatched: bundled runtime connect is unavailable during replay"
                    .into(),
            ));
        }
        if let Some(response) = self.connected_mcp_response(&request.server_id).await? {
            return Ok(response);
        }
        let authority = self.mcp_run_authority(&request.server_id).await?;
        if let Some(response) = self.connected_mcp_response(&request.server_id).await? {
            return Ok(response);
        }

        let result = self
            .attach_or_spawn_mcp(&request.server_id, authority, &request.activate_tool_names)
            .await;
        self.record_openbb_connect(&request.server_id, result.is_ok());
        result
    }

    fn record_openbb_connect(&self, server_id: &str, connected: bool) {
        if server_id == openbb::OPENBB_SERVER_ID {
            self.state.mcp_pool.record_openbb_connect(connected);
            if let Some(failed) = &self.capture.price_setup_connect_failed {
                failed.store(!connected, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    async fn connected_mcp_response(&self, server_id: &str) -> Result<Option<Value>, BrokerError> {
        let connected = self.capture.mcp_sessions.lock().await.get(server_id);
        let Some(McpTurnConnection {
            server: session,
            notices,
        }) = connected
        else {
            return Ok(None);
        };
        let mut guard = session.write().await;
        let Some(server) = guard.as_mut() else {
            drop(guard);
            self.capture.remove_mcp_session(server_id, &session).await;
            return Ok(None);
        };
        let cached = match server.session.is_running().await {
            Ok(false) => Ok(None),
            Err(error) => Err(map_mcp_error(error)),
            Ok(true) => match refresh_inventory(server_id, server, false).await {
                Ok(_) => server_cards(server_id, server)
                    .map(|tools| Some(connect_response(server_id, tools, &notices))),
                Err(error) => Err(error),
            },
        };
        if matches!(&cached, Ok(Some(_))) {
            self.record_openbb_connect(server_id, true);
            return cached;
        }
        if cached.is_err() {
            self.record_openbb_connect(server_id, false);
        }
        let stale = guard.take();
        drop(guard);
        self.capture.remove_mcp_session(server_id, &session).await;
        if let Some(stale) = stale {
            stale.discard().await;
        }
        cached
    }

    async fn attach_or_spawn_mcp(
        &self,
        server_id: &str,
        authority: McpRunAuthority,
        activate_tool_names: &[String],
    ) -> Result<Value, BrokerError> {
        let notices: Arc<[String]> = authority.setup_notices.clone().into();
        let activation = if server_id == openbb::OPENBB_SERVER_ID {
            authority.runtime.tool_activation.clone()
        } else {
            None
        };
        let pool_key = self
            .capture
            .mcp_pool
            .as_ref()
            .map(|_| pool_key(&self.loupist_id, server_id, &authority));
        if let (Some(pool), Some(key)) = (&self.capture.mcp_pool, pool_key.as_ref()) {
            if let Some(mut server) = pool.acquire(key).await {
                let prepared = match prepare_acquired_session(server_id, &mut server).await {
                    Ok(tools) => activate_tools(
                        server_id,
                        &mut server,
                        activation.as_ref(),
                        activate_tool_names,
                    )
                    .await
                    .map(|activated| activated.unwrap_or(tools)),
                    Err(error) => Err(error),
                };
                match prepared {
                    Ok(tools) => match self
                        .capture
                        .install_mcp_session(server_id, server, notices.clone())
                        .await?
                    {
                        McpServerInstall::Installed => {
                            return Ok(connect_response(server_id, tools, &notices));
                        }
                        McpServerInstall::Occupied {
                            response,
                            candidate,
                        } => {
                            pool.release(*candidate).await;
                            return Ok(response);
                        }
                    },
                    Err(_) => {
                        server.discard().await;
                    }
                }
            }
        }

        // Start without holding the session map. A cold start takes tens of
        // seconds on a busy Mac; blocking the map would stall shutdown and a
        // second connect. With a pool, the pool owns the start: this turn
        // reuses a start already under way (the app-start one included), and
        // a start it stops waiting for stays for the next turn.
        let app_data_dir = self.state.artifacts.app_data_dir.clone();
        let mut server = match (&self.capture.mcp_pool, pool_key.as_ref()) {
            (Some(pool), Some(key)) => {
                let start = start_bundled_server(
                    server_id.to_owned(),
                    authority,
                    StartScratch::Pool(pool.clone()),
                    app_data_dir,
                );
                pool.lease_fresh(key, start).await?
            }
            _ => {
                let scratch_root = self.capture.mcp_scratch_root.clone().ok_or_else(|| {
                    BrokerError::Execution("MCP run isolation is unavailable".into())
                })?;
                start_bundled_server(
                    server_id.to_owned(),
                    authority,
                    StartScratch::Under(scratch_root),
                    app_data_dir,
                )
                .await?
            }
        };
        server.pool_key = pool_key;
        let cards = match agent_tool_cards(
            server_id,
            server.tools.values(),
            &server.control_tool_names,
            &server.enabled_provider_ids,
            &server.providerless_tool_policy,
        ) {
            Ok(cards) => cards,
            Err(error) => {
                server.discard().await;
                return Err(error);
            }
        };
        let cards = match activate_tools(
            server_id,
            &mut server,
            activation.as_ref(),
            activate_tool_names,
        )
        .await
        {
            Ok(activated) => activated.unwrap_or(cards),
            Err(error) => {
                server.discard().await;
                return Err(error);
            }
        };
        match self
            .capture
            .install_mcp_session(server_id, server, notices.clone())
            .await?
        {
            McpServerInstall::Installed => Ok(connect_response(server_id, cards, &notices)),
            McpServerInstall::Occupied {
                response,
                candidate,
            } => {
                match &self.capture.mcp_pool {
                    Some(pool) => pool.release(*candidate).await,
                    None => (*candidate).discard().await,
                }
                Ok(response)
            }
        }
    }

    /// The display refresh forwards only the fixed ECB route. Use ordinary
    /// provider admission and response validation, omitting dataset retention.
    pub(in crate::commands) async fn host_fx_read(
        &mut self,
        policy: &ToolPolicy,
    ) -> Result<Value, BrokerError> {
        use crate::finance_data::fx::{RATE_PROVIDER, RATE_ROUTE};
        self.ensure_scope(policy)?;
        if !self.capability_ids.contains("openbb.platform") || policy.as_of.is_some() {
            return Err(BrokerError::MethodDenied);
        }
        // Like asset search, own an isolated process and remove even the
        // scratch left by a timed-out startup before the session is installed.
        let scratch = tempfile::Builder::new()
            .prefix("fx-display-")
            .tempdir_in(&self.state.artifacts.app_data_dir)
            .map_err(|_| BrokerError::Execution("FX display isolation unavailable".into()))?;
        let capture = Arc::get_mut(&mut self.capture).ok_or(BrokerError::MethodDenied)?;
        capture.mcp_pool = None;
        capture.mcp_scratch_root = Some(scratch.path().to_owned());
        let operation = async {
            self.host_connect(policy, "openbb", &[RATE_ROUTE]).await?;
            let session = self
                .capture
                .mcp_session("openbb")
                .await
                .ok_or(BrokerError::MethodDenied)?;
            let guard = session.read().await;
            let server = guard.as_ref().ok_or(BrokerError::MethodDenied)?;
            let request = McpToolCallRequest {
                server_id: "openbb".into(),
                tool_name: RATE_ROUTE.into(),
                arguments: json!({"provider":RATE_PROVIDER}),
            };
            let plan = self.admit_mcp_call(&request, server, None)?;
            match invoke_mcp_tool(&request, server, &plan).await? {
                McpInvocation::Answered { payload, .. } => Ok(payload),
                McpInvocation::Failed(error) | McpInvocation::Stopped(Some(error)) => Err(error),
                McpInvocation::Stopped(None) => Err(BrokerError::Execution(
                    "The finance data provider returned an invalid response".into(),
                )),
            }
        };
        let result = tokio::time::timeout(Duration::from_secs(60), operation)
            .await
            .unwrap_or_else(|_| {
                Err(BrokerError::Execution(
                    "FX display refresh timed out".into(),
                ))
            });
        self.host_shutdown().await;
        result
    }

    /// One MCP tool call on this turn's session, in order: admission, a fresh
    /// retained read or the invocation, then provenance and result shaping for
    /// a data read. A session that can no longer be trusted is discarded.
    pub(super) async fn mcp_call(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.mcp_call_with_reuse(policy, params, delivery_id, true)
            .await
    }

    pub(super) async fn mcp_call_with_reuse(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
        allow_reuse: bool,
    ) -> Result<Value, BrokerError> {
        let request: McpToolCallRequest =
            serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        if !self.capture.saved_keys_needing_permission.is_empty() {
            let provider = request.arguments.get("provider").and_then(Value::as_str);
            let catalog = bundled_catalog()
                .map_err(|_| BrokerError::Execution("Marketplace catalog is unavailable".into()))?;
            let entries = catalog.entries.iter().filter(|entry| {
                entry.runtime.server_id.as_deref() == Some(request.server_id.as_str())
            });
            let provider_available = entries.clone().any(|entry| {
                self.capability_enabled(&entry.id)
                    && provider.is_none_or(|provider| {
                        entry.free_state == MarketplaceFreeState::Keyless
                            && entry.runtime.provider_ids.iter().any(|id| id == provider)
                    })
            });
            for entry in entries.filter(|entry| {
                !provider_available
                    && provider.is_none_or(|provider| {
                        entry.runtime.provider_ids.iter().any(|id| id == provider)
                    })
            }) {
                self.require_saved_key_permission(&entry.id)?;
            }
        }
        let cutoff = parse_as_of_date(policy.as_of.as_deref())?;
        if crate::custom_connections::service::valid_id(&request.server_id) {
            if !crate::extension_availability::custom_connections() {
                return Err(BrokerError::MethodDenied);
            }
            // A custom server's rows carry no date the host can check against a cutoff.
            if let Some(cutoff) = cutoff {
                return Err(point_in_time::refusal(&request.tool_name, cutoff));
            }
            return self.custom_mcp_call(request, delivery_id).await;
        }
        #[cfg(feature = "eval-harness")]
        if request.server_id == openbb::OPENBB_SERVER_ID
            && self.capability_ids.contains("openbb.platform")
            && crate::eval_harness::replay::installed_for(&self.state.credential_environment)
        {
            return Err(BrokerError::Execution(
                "eval_replay_unmatched: bundled runtime calls are unavailable during replay".into(),
            ));
        }
        let Some(session) = self.capture.mcp_session(&request.server_id).await else {
            return Ok(stopped_mcp_response(None));
        };
        let (mut session_guard, preflight_tools) =
            match self.lock_mcp_call_session(&request, &session).await {
                Ok(locked) => locked,
                Err(stopped) => return Ok(stopped),
            };
        let server = session_guard.server();
        let plan = match self.admit_mcp_call(&request, server, cutoff) {
            Ok(plan) => plan,
            Err(error) => return mcp_call_error_with_inventory(preflight_tools, error),
        };
        let reused = match plan.max_age {
            Some(max_age) if allow_reuse && !plan.control_tool => self
                .state
                .store
                .dataset_fresh(&self.loupist_id, &plan.request_key, max_age)
                .map_err(command_error)?,
            _ => None,
        };

        let (mut payload, tools, retained) = if let Some((reference, stored, metadata)) = reused {
            let fetched_at = metadata_fetched_at(&metadata);
            (stored, preflight_tools, Some((reference, fetched_at)))
        } else {
            match invoke_mcp_tool(&request, server, &plan).await? {
                McpInvocation::Answered { payload, tools } => {
                    (payload, tools.or(preflight_tools), None)
                }
                McpInvocation::Failed(error) => {
                    return mcp_call_error_with_inventory(preflight_tools, error)
                }
                McpInvocation::Stopped(error) => {
                    drop(session_guard);
                    let stopped = session.write().await.take();
                    self.capture
                        .discard_mcp_session(&request.server_id, &session, stopped)
                        .await;
                    return Ok(stopped_mcp_response(error.as_ref()));
                }
            }
        };
        // Data calls may announce an inventory change too. Upgrade only after
        // releasing the read guard; other active reads finish before replacing
        // inventory or quarantining the session.
        if matches!(&session_guard, McpCallGuard::Read(_))
            && session_guard.server().session.take_tools_changed()
        {
            drop(session_guard);
            let guard = session.write().await;
            if guard.is_none() {
                return Ok(stopped_mcp_response(None));
            }
            session_guard = McpCallGuard::Write(guard);
        }
        let tools = if let McpCallGuard::Write(guard) = &mut session_guard {
            let server = guard.as_mut().expect("loaded MCP session");
            match refresh_inventory(&request.server_id, server, true).await {
                Ok(updated) => updated.or(tools),
                Err(_) => {
                    let stopped = guard.take();
                    drop(session_guard);
                    self.capture
                        .discard_mcp_session(&request.server_id, &session, stopped)
                        .await;
                    return Ok(stopped_mcp_response(None));
                }
            }
        } else {
            tools
        };
        drop(session_guard);
        let payload = if plan.control_tool {
            payload
        } else {
            let reused = retained.is_some();
            let compact = !super::python_reads::nested() && payload.to_string().len() > 16 * 1024;
            let (reference, fetched_at) = match retained {
                Some(retained) => retained,
                None => {
                    let (reference, fetched_at) = self.retain_dataset(
                        &plan.request_key,
                        &payload,
                        json!({"producer":"mcp", "request":plan.capture_request,"publication_policy":"provider-declared; unknown availability is not point-in-time evidence","model_coverage":if compact {"bounded_sample"} else {"complete"},
                            "trust": crate::custom_connections::taint::retained_trust(ToolMethod::McpCall, &plan.capture_request, &self.state.artifacts.mcp_runtimes)}),
                    )?;
                    (reference, Some(fetched_at))
                }
            };
            let mut model_payload = self.shape_mcp_read(
                &mut payload,
                &request,
                &reference,
                fetched_at,
                reused,
                compact,
            )?;
            let result_ref = match self
                .stage_mcp_read(
                    delivery_id,
                    &request,
                    &plan,
                    &payload,
                    model_payload.as_ref(),
                )
                .await
            {
                Ok(result_ref) => result_ref,
                Err(error) => return mcp_call_error_with_inventory(tools, error),
            };
            if returned_no_rows(&payload) {
                // Nothing the answer can rely on: host reads still resolve it
                // by result_ref, but it has no citation_url and is never one
                // of the answer's Sources.
                self.capture
                    .stage(delivery_id, |pending| {
                        pending.unlisted_results.insert(result_ref.clone());
                    })
                    .await;
                describe_empty_read(&mut payload);
                payload["result_ref"] = json!(result_ref);
                if let Some(delivered) = &mut model_payload {
                    describe_empty_read(delivered);
                    delivered["result_ref"] = json!(result_ref);
                }
            } else {
                attach_result_ref(&mut payload, &result_ref);
                if let Some(delivered) = &mut model_payload {
                    attach_result_ref(delivered, &result_ref);
                }
            }
            model_payload.unwrap_or(payload)
        };
        Ok(match tools {
            Some(tools) => json!({ "result": payload, "tools": tools }),
            None => json!({ "result": payload }),
        })
    }

    async fn lock_mcp_call_session<'s>(
        &self,
        request: &McpToolCallRequest,
        session: &'s SharedMcpServer,
    ) -> Result<(McpCallGuard<'s>, Option<Vec<Value>>), Value> {
        let guard = session.read().await;
        if let Some(server) = guard.as_ref() {
            if !server.control_tool_names.contains(&request.tool_name)
                && matches!(server.session.is_running().await, Ok(true))
                && !server.session.take_tools_changed()
            {
                return Ok((McpCallGuard::Read(guard), None));
            }
        }
        drop(guard);
        let (mut guard, tools) = self
            .lock_running_mcp_session(&request.server_id, session)
            .await?;
        // The shared preflight consumed the signal. Refresh under the exclusive
        // guard before admitting against an inventory that may have changed.
        let server = guard.as_mut().expect("loaded MCP session");
        let tools = match refresh_inventory(&request.server_id, server, true).await {
            Ok(updated) => updated.or(tools),
            Err(_) => {
                let stopped = guard.take();
                drop(guard);
                self.capture
                    .discard_mcp_session(&request.server_id, session, stopped)
                    .await;
                return Err(stopped_mcp_response(None));
            }
        };
        Ok((McpCallGuard::Write(guard), tools))
    }

    /// Locks this turn's session for `server_id` and refreshes its inventory.
    /// A missing, stopped or failing server leaves the turn, answered with the
    /// stopped response the caller returns as is.
    async fn lock_running_mcp_session<'s>(
        &self,
        server_id: &str,
        session: &'s SharedMcpServer,
    ) -> Result<
        (
            RwLockWriteGuard<'s, Option<TurnMcpServer>>,
            Option<Vec<Value>>,
        ),
        Value,
    > {
        let mut session_guard = session.write().await;
        let Some(server) = session_guard.as_mut() else {
            drop(session_guard);
            self.capture.remove_mcp_session(server_id, session).await;
            return Err(stopped_mcp_response(None));
        };
        let preflight = match server.session.is_running().await {
            Ok(true) => refresh_inventory(server_id, server, false).await,
            Ok(false) => Err(map_mcp_error(McpError::Stopped)),
            Err(error) => Err(map_mcp_error(error)),
        };
        match preflight {
            Ok(tools) => Ok((session_guard, tools)),
            Err(error) => {
                let stopped = session_guard.take();
                drop(session_guard);
                self.capture
                    .discard_mcp_session(server_id, session, stopped)
                    .await;
                Err(stopped_mcp_response(Some(&error)))
            }
        }
    }

    /// Admission: the tool is in the session's authorized inventory and any
    /// provider it names is enabled for this Loupist. Under an as-of cutoff a
    /// data read must take a dated route, bounded to the cutoff.
    fn admit_mcp_call(
        &self,
        request: &McpToolCallRequest,
        server: &TurnMcpServer,
        cutoff: Option<chrono::NaiveDate>,
    ) -> Result<McpCallPlan, BrokerError> {
        let Some(tool) = server.tools.get(&request.tool_name) else {
            return Err(BrokerError::invalid("tool_name", "Not in this connection's active tool list. Use an active tool, or load this connection again if it stopped."));
        };
        if let Err(error) =
            validate_provider_argument(tool, &request.arguments, &server.enabled_provider_ids)
        {
            return Err(self.explain_provider_denial(request, &server.enabled_provider_ids, error));
        }
        let requested_provider = request
            .arguments
            .get("provider")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| {
                server
                    .providerless_tool_policy
                    .implicit_provider
                    .get(&request.tool_name)
                    .cloned()
            });
        let control_tool = server.control_tool_names.contains(&request.tool_name);
        let local_tool = server
            .providerless_tool_policy
            .local_tools
            .contains(&request.tool_name);
        let data_tool = !control_tool && !local_tool;
        if super::python_reads::nested() && !data_tool {
            return Err(BrokerError::MethodDenied);
        }
        let mut arguments = request.arguments.clone();
        // `max_age` is a host argument added to OpenBB data cards; never forwarded.
        let max_age = if data_tool
            && request.server_id == openbb::OPENBB_SERVER_ID
            && tool.input_schema.pointer("/properties/max_age").is_none()
        {
            take_max_age(&mut arguments)?
        } else {
            None
        };
        // Point-in-time authority: an allowlist of dated routes, everything
        // else that reads provider data is refused under a cutoff.
        let mut dated = None;
        if let (Some(cutoff), true) = (cutoff, data_tool) {
            let Some(route) = point_in_time::dated_route(&request.tool_name) else {
                return Err(point_in_time::refusal(&request.tool_name, cutoff));
            };
            let bounded = point_in_time::bound_arguments(tool, &mut arguments, cutoff)?;
            dated = Some((route, cutoff, bounded));
        }
        let mut capture_request = json!({
            "server_id": request.server_id.clone(),
            "tool_name": request.tool_name.clone(),
            "arguments": arguments.clone(),
        });
        if let Some(cutoff) = cutoff {
            capture_request["as_of"] = json!(cutoff.to_string());
        }
        let request_key = sha256(capture_request.to_string().as_bytes());
        Ok(McpCallPlan {
            arguments,
            requested_provider,
            control_tool,
            max_age,
            dated,
            capture_request,
            request_key,
        })
    }

    /// Result shaping: stamps the retained dataset on the full payload and, for
    /// a large one, returns the bounded summary the model receives instead.
    fn shape_mcp_read(
        &self,
        payload: &mut Value,
        request: &McpToolCallRequest,
        reference: &crate::datasets::DatasetRef,
        fetched_at: Option<String>,
        reused: bool,
        compact: bool,
    ) -> Result<Option<Value>, BrokerError> {
        payload["dataset_ref"] = json!(reference);
        payload["fetched_at"] = json!(fetched_at);
        if reused {
            payload["reused"] = json!(true);
        }
        payload["model_delivery"] = json!({"coverage":if compact {"bounded_sample"} else {"complete"},"retained":"complete"});
        if !compact {
            return Ok(None);
        }
        let description = self
            .state
            .store
            .dataset_describe_omitting(
                &self.loupist_id,
                &reference.dataset_id,
                reference.revision,
                // SEC statements carry about 60 KiB of XBRL source tags
                // here, which left the sample no room for the periods.
                &["/structuredContent/extra/results_metadata"],
            )
            .map_err(command_error)?;
        let mut delivered = json!({"dataset_ref":reference,"fetched_at":payload["fetched_at"],"data_summary":description,"model_delivery":{"coverage":"bounded_sample","retained":"complete"}});
        // A statement read is too large to deliver whole, and its sample
        // shows three periods; the model still sees every period, compact.
        if request.server_id == openbb::OPENBB_SERVER_ID
            && matches!(
                request.tool_name.as_str(),
                "equity_fundamental_income"
                    | "equity_fundamental_balance"
                    | "equity_fundamental_cash"
            )
        {
            if let Some(statements) = super::finance::market_statement_table(payload) {
                delivered["statements"] = statements;
            }
        }
        if let Some(as_of) = payload.get("as_of") {
            delivered["as_of"] = as_of.clone();
        }
        if reused {
            delivered["reused"] = json!(true);
        }
        Ok(Some(delivered))
    }

    /// Provenance: stages the read as this run's result. A named provider
    /// makes it acquired original data; otherwise it is derived.
    async fn stage_mcp_read(
        &self,
        delivery_id: &str,
        request: &McpToolCallRequest,
        plan: &McpCallPlan,
        payload: &Value,
        model_payload: Option<&Value>,
    ) -> Result<String, BrokerError> {
        self.capture
            .stage_run_result_with_delivery(
                delivery_id,
                RunResultProducer {
                    origin: if plan.requested_provider.is_some() {
                        RunResultOrigin::AcquiredOriginal
                    } else {
                        RunResultOrigin::Derived
                    },
                    runtime_id: request.server_id.clone(),
                    tool_name: request.tool_name.clone(),
                    provider: plan.requested_provider.clone(),
                },
                &plan.capture_request,
                payload.clone(),
                Vec::new(),
                model_payload,
            )
            .await
    }

    /// A provider that exists in the runtime but needs setup gets a recovery
    /// step instead of a bare denial.
    fn explain_provider_denial(
        &self,
        request: &McpToolCallRequest,
        enabled_provider_ids: &BTreeSet<String>,
        error: BrokerError,
    ) -> BrokerError {
        let Some(provider) = request.arguments.get("provider").and_then(Value::as_str) else {
            return error;
        };
        if enabled_provider_ids.contains(provider) {
            return error;
        }
        let needs_setup = self
            .state
            .artifacts
            .mcp_runtimes
            .get(&request.server_id)
            .and_then(|runtime| runtime.provider_config_fields.get(provider))
            .is_some_and(|fields| !fields.is_empty());
        if matches!(
            error,
            BrokerError::MethodDenied | BrokerError::InvalidArguments { .. }
        ) && needs_setup
        {
            return BrokerError::invalid("provider", format!("Provider {provider} needs setup. If connection_setup is available, call it with this provider's connection name so the user finishes setup in a card; otherwise tell the user to check this provider in Connections."));
        }
        error
    }

    async fn mcp_run_authority(&self, server_id: &str) -> Result<McpRunAuthority, BrokerError> {
        let catalog = bundled_catalog()
            .map_err(|_| BrokerError::Execution("Marketplace catalog is unavailable".into()))?;
        let entries = catalog
            .entries
            .iter()
            .filter(|entry| {
                self.capability_ids.contains(&entry.id)
                    && entry.runtime.kind == MarketplaceRuntimeKind::BundledMcp
                    && entry.runtime.server_id.as_deref() == Some(server_id)
            })
            .collect::<Vec<_>>();
        if entries.is_empty() {
            if catalog.entries.iter().any(|entry| {
                entry.runtime.server_id.as_deref() == Some(server_id)
                    && self
                        .capture
                        .saved_keys_needing_permission
                        .contains(&entry.id)
            }) {
                return Err(BrokerError::KeychainPermissionRequired);
            }
            return Err(BrokerError::MethodDenied);
        }
        let runtime = self
            .state
            .artifacts
            .mcp_runtimes
            .get(server_id)
            .cloned()
            .ok_or_else(|| BrokerError::Execution("MCP runtime is unavailable".into()))?;
        let requested_provider_ids = entries
            .iter()
            .flat_map(|entry| entry.runtime.provider_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        if requested_provider_ids.is_empty()
            || !requested_provider_ids.is_subset(&runtime.provider_ids)
        {
            return Err(BrokerError::Execution(
                "MCP provider authority is invalid".into(),
            ));
        }
        for entry in &entries {
            let possible_hosts = entry
                .runtime
                .provider_ids
                .iter()
                .filter_map(|provider| runtime.provider_network_hosts.get(provider))
                .flatten()
                .cloned()
                .collect::<BTreeSet<_>>();
            let declared_hosts = entry
                .permissions
                .network_hosts
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>();
            if declared_hosts.is_empty() || declared_hosts != possible_hosts {
                return Err(BrokerError::Execution(
                    "MCP network authority is invalid".into(),
                ));
            }
        }

        let credential_specs = entries
            .iter()
            .filter(|entry| !entry.runtime.credential_mapping.is_empty())
            .map(|entry| {
                (
                    entry.id.clone(),
                    entry.free_state,
                    entry.runtime.credential_mapping.clone(),
                )
            })
            .collect::<Vec<_>>();
        let state = self.state.clone();
        let trace = self.capture.tool_trace.clone();
        let (credentials, revisions, missing) = tokio::task::spawn_blocking(move || {
            use crate::finance_credentials;

            let lookup_failed = || BrokerError::Execution("credential lookup failed".into());
            let mut credentials = BTreeMap::new();
            let mut revisions = BTreeMap::new();
            let mut missing = BTreeMap::new();
            for (entry_id, access, mapping) in credential_specs {
                let required = access != MarketplaceFreeState::Keyless;
                let bundle = match finance_credentials::get_for_state(&state, &entry_id) {
                    Ok(bundle) => {
                        revisions.insert(
                            entry_id.clone(),
                            bundle.as_ref().map(|bundle| bundle.revision().to_owned()),
                        );
                        bundle
                    }
                    Err(error) => {
                        if matches!(error, finance_credentials::FinanceCredentialError::Locked) {
                            if let Some(status) =
                                finance_credentials::refused_read_status(&state, &entry_id)
                            {
                                trace.record_connector_keychain_status(status);
                            }
                        }
                        let saved_key_error = finance_credentials::has_saved_key(&state, &entry_id)
                            .then(|| super::errors::credential_read_error(error));
                        if required || saved_key_error.is_some() {
                            missing.insert(entry_id, saved_key_error);
                        }
                        continue;
                    }
                };
                let Some(bundle) = bundle else {
                    if required {
                        missing.insert(entry_id, None);
                    }
                    continue;
                };
                for (field_id, target_id) in mapping {
                    let secret = bundle.get(&field_id).ok_or_else(lookup_failed)?.to_owned();
                    if credentials.insert(target_id, secret).is_some() {
                        return Err(lookup_failed());
                    }
                }
            }
            Ok((credentials, revisions, missing))
        })
        .await
        .map_err(|_| BrokerError::Execution("credential lookup failed".into()))??;

        // A provider may have a public grant and a separate keyed grant. The
        // public grant still admits that provider's credential-free routes.
        let available_provider_ids = entries
            .iter()
            .filter(|entry| {
                entry.free_state == MarketplaceFreeState::Keyless
                    || !missing.contains_key(&entry.id)
            })
            .flat_map(|entry| entry.runtime.provider_ids.iter().cloned())
            .collect::<BTreeSet<_>>();
        let provider_config = {
            let _guard = crate::marketplace::connector_lifecycle_lock().map_err(command_error)?;
            let config = collect_server_provider_config(
                server_id,
                &catalog.entries,
                &available_provider_ids,
                |entry_id, field_id| match crate::marketplace::connector_config_value(
                    &self.state,
                    entry_id,
                    field_id,
                ) {
                    Ok(value) => Ok(value),
                    Err(error) if error.code == "config_apply_pending" => Ok(None),
                    Err(_) => Err(BrokerError::Execution(
                        "connector setup is unavailable".into(),
                    )),
                },
            )?;
            for (entry, expected) in revisions {
                let current = crate::finance_credentials::revision_for_state(&self.state, &entry)
                    .map_err(|_| BrokerError::MethodDenied)?;
                if current != expected {
                    return Err(BrokerError::MethodDenied);
                }
            }
            config
        };
        // A manifest-keyless provider can still require non-secret operational
        // configuration (SEC contact identity is the current case). Keep it in
        // the platform catalog, but do not expose or authorize it for a run
        // until every manifest-declared config field is present.
        let enabled_provider_ids = configured_provider_ids(
            available_provider_ids.clone(),
            &runtime.provider_config_fields,
            &provider_config,
        );
        let mut setup_notices = setup_notices(
            &catalog.entries,
            server_id,
            available_provider_ids.difference(&enabled_provider_ids),
        );
        let mut needs_setup = !setup_notices.is_empty();
        for entry in entries {
            match missing.get(&entry.id) {
                Some(Some(error)) => {
                    let body = crate::broker::error_body(error);
                    if let Some(message) = body["message"].as_str() {
                        setup_notices.push(format!("{} ({}) — {message}", entry.name, entry.id));
                    }
                }
                Some(None) => {
                    if let [provider] = entry.runtime.provider_ids.as_slice() {
                        setup_notices.push(provider_setup_notice(entry, provider, true));
                        needs_setup = true;
                    }
                }
                None => {}
            }
        }
        if enabled_provider_ids.is_empty() {
            let error = BrokerError::Execution(if setup_notices.is_empty() {
                "MCP provider setup is incomplete".into()
            } else {
                setup_notices.join(" ")
            });
            if !needs_setup {
                if let Some(saved_key_error) = missing.into_values().flatten().next() {
                    let body = crate::broker::error_body(&saved_key_error);
                    return Err(
                        error.with_next(body["next"].as_str().unwrap_or_default().to_owned())
                    );
                }
            }
            return Err(error.with_next(PROVIDER_SETUP_NEXT));
        }
        let allowed_network_hosts = enabled_provider_ids
            .iter()
            .filter_map(|provider| runtime.provider_network_hosts.get(provider))
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>();
        Ok(McpRunAuthority {
            runtime,
            enabled_provider_ids,
            allowed_network_hosts,
            credentials,
            provider_config,
            setup_notices,
        })
    }
}

/// Names each granted provider that cannot run until its connection is set up,
/// so the model can say why instead of silently lacking that source.
fn setup_notices<'a>(
    catalog_entries: &[MarketplaceEntryDto],
    server_id: &str,
    missing: impl Iterator<Item = &'a String>,
) -> Vec<String> {
    missing
        .filter_map(|provider| {
            let entry = catalog_entries.iter().find(|entry| {
                entry.runtime.kind == MarketplaceRuntimeKind::BundledMcp
                    && entry.runtime.server_id.as_deref() == Some(server_id)
                    && !entry.runtime.config_mapping.is_empty()
                    && entry.runtime.provider_ids.as_slice() == [provider.clone()]
            })?;
            Some(provider_setup_notice(entry, provider, false))
        })
        .collect()
}

fn provider_setup_notice(entry: &MarketplaceEntryDto, provider: &str, credential: bool) -> String {
    let fields = entry
        .setup
        .as_ref()
        .map(|setup| {
            if credential {
                setup
                    .credential_fields
                    .iter()
                    .filter(|field| field.required)
                    .map(|field| field.label.as_str())
                    .collect::<Vec<_>>()
            } else {
                setup
                    .config_fields
                    .iter()
                    .map(|field| field.label.as_str())
                    .collect::<Vec<_>>()
            }
        })
        .filter(|labels| !labels.is_empty())
        .map(|labels| labels.join(" and "))
        .unwrap_or_else(|| "its setup".into());
    format!(
        "{name} (provider {provider}) is unavailable until the user adds the {fields}: if connection_setup is available, call it with {{id: \"{id}\"}} and the user finishes it in a card; otherwise point them to Connections. Until then do not present another provider's data as {name} data.",
        name = entry.name,
        id = entry.id,
    )
}

const PROVIDER_SETUP_NEXT: &str = "The providers named above need setup before they can run. If connection_setup is available, call it with the named connection id so the user finishes setup in a card; otherwise tell the user to check that provider in Connections. Use another provider only when its tool schema lists it, and name it when citing.";

fn connect_response(server_id: &str, tools: Vec<Value>, notices: &[String]) -> Value {
    let mut response = json!({ "server_id": server_id, "tools": tools });
    if !notices.is_empty() {
        response["notices"] = json!(notices);
    }
    response
}

/// Activates the warm Chat's previous set, or the common read set so a first
/// question needs no discovery round. Unknown names are ignored by the
/// runtime; a refused activation leaves discovery available instead of
/// failing the connection.
async fn activate_tools(
    server_id: &str,
    server: &mut TurnMcpServer,
    activation: Option<&McpToolActivation>,
    requested: &[String],
) -> Result<Option<Vec<Value>>, BrokerError> {
    let Some(activation) = activation else {
        return Ok(None);
    };
    if !server.control_tool_names.contains(&activation.tool_name) {
        return Ok(None);
    }
    let arguments = Value::Object(serde_json::Map::from_iter([(
        activation.argument_name.clone(),
        json!(activation_names(
            requested,
            &server.control_tool_names,
            &server.enabled_provider_ids
        )),
    )]));
    let result = server
        .session
        .call_tool(&activation.tool_name, arguments, MCP_CALL_TIMEOUT)
        .await
        .map_err(map_mcp_error)?;
    if result.is_error {
        return Ok(None);
    }
    let _ = server.session.take_tools_changed();
    refresh_inventory(server_id, server, true).await
}

fn provider_failure(
    error: BrokerError,
    tool: Option<&McpTool>,
    provider: Option<&str>,
    provider_error: bool,
) -> BrokerError {
    if is_argument_failure(&error)
        || is_sec_contact_required(&error)
        || matches!(
            error,
            BrokerError::EmptyResult | BrokerError::ResultTooLarge { .. }
        )
    {
        return error;
    }
    let source = match &error {
        BrokerError::WithNext { source, .. } => source.as_ref(),
        other => other,
    };
    let alternatives = provider
        .filter(|_| provider_error && matches!(source, BrokerError::Execution(_)))
        .and_then(|provider| openbb::provider_failure_next(tool?, provider));
    let next = alternatives.map(std::borrow::Cow::Owned).or_else(|| {
        if tool.is_some_and(|tool| tool.name == "news_company") {
            Some(openbb::COMPANY_NEWS_UNAVAILABLE.into())
        } else if provider.is_some_and(openbb::is_best_effort) {
            Some(openbb::BEST_EFFORT_FAILURE_NEXT.into())
        } else {
            None
        }
    });
    let Some(next) = next else {
        return error;
    };
    let source = match error {
        BrokerError::WithNext { source, .. } => *source,
        other => other,
    };
    source.with_next(next)
}

fn metadata_fetched_at(metadata: &Value) -> Option<String> {
    metadata
        .get("fetched_at")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn configured_provider_ids(
    requested: BTreeSet<String>,
    required_config: &BTreeMap<String, BTreeSet<String>>,
    provider_config: &BTreeMap<String, BTreeMap<String, String>>,
) -> BTreeSet<String> {
    requested
        .into_iter()
        .filter(|provider_id| {
            required_config.get(provider_id).is_none_or(|required| {
                required.iter().all(|field| {
                    provider_config
                        .get(provider_id)
                        .and_then(|values| values.get(field))
                        .is_some_and(|value| !value.trim().is_empty())
                })
            })
        })
        .collect()
}

/// Non-secret provider config (SEC contact email today) is stored on the
/// sibling Marketplace entry that collects it. A run that enables
/// `openbb.platform` still receives that setup even when `sec.edgar` is
/// not itself enabled for the Loupist.
fn collect_server_provider_config(
    server_id: &str,
    catalog_entries: &[MarketplaceEntryDto],
    requested: &BTreeSet<String>,
    mut read_field: impl FnMut(&str, &str) -> Result<Option<String>, BrokerError>,
) -> Result<BTreeMap<String, BTreeMap<String, String>>, BrokerError> {
    let mut provider_config = BTreeMap::<String, BTreeMap<String, String>>::new();
    for entry in catalog_entries {
        if entry.runtime.kind != MarketplaceRuntimeKind::BundledMcp
            || entry.runtime.server_id.as_deref() != Some(server_id)
            || entry.runtime.config_mapping.is_empty()
        {
            continue;
        }
        let [provider_id] = entry.runtime.provider_ids.as_slice() else {
            return Err(BrokerError::Execution(
                "MCP provider configuration is ambiguous".into(),
            ));
        };
        if !requested.contains(provider_id) {
            continue;
        }
        let mut values = BTreeMap::new();
        let mut complete = true;
        for field_id in entry.runtime.config_mapping.keys() {
            match read_field(&entry.id, field_id)? {
                Some(value) => {
                    values.insert(field_id.clone(), value);
                }
                None => complete = false,
            }
        }
        // An incomplete entry leaves its provider unconfigured; the run drops
        // that provider and reports it instead of failing every provider.
        if !complete {
            continue;
        }
        let dest = provider_config.entry(provider_id.clone()).or_default();
        for (field_id, value) in values {
            if dest.insert(field_id, value).is_some() {
                return Err(BrokerError::Execution(
                    "MCP provider configuration is ambiguous".into(),
                ));
            }
        }
    }
    Ok(provider_config)
}

async fn refresh_inventory(
    server_id: &str,
    server: &mut TurnMcpServer,
    force: bool,
) -> Result<Option<Vec<Value>>, BrokerError> {
    if !force && !server.session.take_tools_changed() {
        return Ok(None);
    }
    let listed = server.session.list_tools().await.map_err(map_mcp_error)?;
    ensure_inventory_contains_no_protected_values(&listed, &server.sensitive_values)?;
    let filtered = filter_inventory(
        &listed,
        &server.control_tool_names,
        &server.enabled_provider_ids,
        &server.providerless_tool_policy,
    )?;
    let cards = agent_tool_cards(
        server_id,
        filtered.values(),
        &server.control_tool_names,
        &server.enabled_provider_ids,
        &server.providerless_tool_policy,
    )?;
    server.tools = filtered;
    Ok(Some(cards))
}

fn ensure_inventory_contains_no_protected_values(
    tools: &[McpTool],
    sensitive_values: &[String],
) -> Result<(), BrokerError> {
    let inventory = serde_json::to_value(tools)
        .map_err(|_| BrokerError::Execution("MCP tool inventory was invalid".into()))?;
    if contains_protected_value(&inventory, sensitive_values) {
        return Err(BrokerError::Execution(
            "MCP tool inventory contained protected private data".into(),
        ));
    }
    Ok(())
}

fn filter_inventory(
    tools: &[McpTool],
    control_tool_names: &BTreeSet<String>,
    enabled_provider_ids: &BTreeSet<String>,
    providerless_tool_policy: &ProviderlessToolPolicy,
) -> Result<BTreeMap<String, McpTool>, BrokerError> {
    let mut filtered = BTreeMap::new();
    let mut dynamic_names = BTreeSet::new();
    for tool in tools {
        let control = control_tool_names.contains(&tool.name);
        let has_provider_parameter = tool
            .input_schema
            .get("properties")
            .and_then(|properties| properties.get("provider"))
            .is_some();
        if !control && !has_provider_parameter {
            let local = providerless_tool_policy.local_tools.contains(&tool.name);
            let implicit_enabled = providerless_tool_policy
                .implicit_provider
                .get(&tool.name)
                .is_some_and(|provider| enabled_provider_ids.contains(provider));
            if !local && !implicit_enabled {
                continue;
            }
        }
        let Some(tool) =
            filter_tool_providers(tool, enabled_provider_ids, control).map_err(map_mcp_error)?
        else {
            continue;
        };
        let schema = tool_to_agent_schema("runtime", &tool).map_err(map_mcp_error)?;
        let dynamic_suffix = schema
            .get("name")
            .and_then(Value::as_str)
            .and_then(|name| name.strip_prefix("mcp__runtime__"))
            .ok_or_else(|| BrokerError::Execution("MCP tool schema is invalid".into()))?;
        if !dynamic_names.insert(dynamic_suffix.to_owned()) {
            return Err(BrokerError::Execution(
                "MCP tool namespace collision".into(),
            ));
        }
        filtered.insert(tool.name.clone(), tool);
    }
    if filtered.is_empty() {
        return Err(BrokerError::Execution(
            "MCP runtime exposed no authorized tools".into(),
        ));
    }
    Ok(filtered)
}

fn agent_tool_cards<'a>(
    server_id: &str,
    tools: impl Iterator<Item = &'a McpTool>,
    control_tool_names: &BTreeSet<String>,
    enabled_provider_ids: &BTreeSet<String>,
    providerless_tool_policy: &ProviderlessToolPolicy,
) -> Result<Vec<Value>, BrokerError> {
    tools
        .map(|tool| {
            let mut card = tool_to_agent_schema(server_id, tool).map_err(map_mcp_error)?;
            if server_id == openbb::OPENBB_SERVER_ID {
                openbb::shape_card(
                    &mut card,
                    tool,
                    control_tool_names.contains(&tool.name),
                    providerless_tool_policy.local_tools.contains(&tool.name),
                    enabled_provider_ids,
                );
            }
            Ok(card)
        })
        .collect()
}

fn server_cards(server_id: &str, server: &TurnMcpServer) -> Result<Vec<Value>, BrokerError> {
    agent_tool_cards(
        server_id,
        server.tools.values(),
        &server.control_tool_names,
        &server.enabled_provider_ids,
        &server.providerless_tool_policy,
    )
}

fn validate_provider_argument(
    tool: &McpTool,
    arguments: &Value,
    enabled_provider_ids: &BTreeSet<String>,
) -> Result<(), BrokerError> {
    let arguments = arguments.as_object().ok_or(BrokerError::Malformed)?;
    let Some(provider_schema) = tool
        .input_schema
        .get("properties")
        .and_then(|value| value.get("provider"))
    else {
        return Ok(());
    };
    let required = tool
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values
                .iter()
                .any(|value| value.as_str() == Some("provider"))
        });
    let provider = arguments.get("provider").and_then(Value::as_str);
    if required && provider.is_none() {
        return Err(BrokerError::invalid(
            "provider",
            "Choose a provider listed in this active tool's schema.",
        ));
    }
    if let Some(provider) = provider {
        let schema_allows = provider_schema
            .get("enum")
            .and_then(Value::as_array)
            .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(provider)));
        if !enabled_provider_ids.contains(provider) || !schema_allows {
            return Err(BrokerError::invalid("provider", "Not enabled for this tool in this run. Choose a provider listed in the active tool schema, or use connection_setup for the provider you need."));
        }
    }
    Ok(())
}

fn pool_key(loupist_id: &str, server_id: &str, authority: &McpRunAuthority) -> McpPoolKey {
    let canonical = json!({
        "allowed_categories": authority.runtime.allowed_categories,
        "allowed_network_hosts": authority.allowed_network_hosts,
        "control_tool_names": authority.runtime.control_tool_names,
        "credentials": authority.credentials,
        "enabled_provider_ids": authority.enabled_provider_ids,
        "executable": authority.runtime.executable,
        "provider_config": authority.provider_config,
    });
    McpPoolKey {
        loupist_id: loupist_id.to_owned(),
        server_id: server_id.to_owned(),
        authority_fingerprint: authority_fingerprint(&canonical),
    }
}

fn bootstrap_settings(authority: &McpRunAuthority) -> Value {
    json!({
        "allowed_categories": authority.runtime.allowed_categories,
        "enabled_provider_ids": authority.enabled_provider_ids,
        "allowed_network_hosts": authority.allowed_network_hosts,
        "provider_config": authority.provider_config,
    })
}

async fn prepare_acquired_session(
    server_id: &str,
    server: &mut TurnMcpServer,
) -> Result<Vec<Value>, BrokerError> {
    if !server.session.is_running().await.map_err(map_mcp_error)? {
        return Err(map_mcp_error(McpError::Stopped));
    }
    let tools = refresh_inventory(server_id, server, true)
        .await?
        .ok_or_else(|| map_mcp_error(McpError::Protocol))?;
    let names = server.tools.keys().cloned().collect::<BTreeSet<_>>();
    if names != server.control_tool_names {
        return Err(BrokerError::Execution(
            "MCP runtime reused inventory did not match its signed control surface".into(),
        ));
    }
    Ok(tools)
}

/// Where a start creates its process's private working directory.
enum StartScratch {
    Pool(McpProcessPool),
    /// A run-owned root, for a turn that keeps its process out of the pool.
    Under(std::path::PathBuf),
}

impl StartScratch {
    fn create(&self, server_id: &str) -> Result<std::path::PathBuf, BrokerError> {
        let failed = || BrokerError::Execution("MCP run isolation failed".into());
        match self {
            Self::Pool(pool) => pool.create_scratch(server_id).map_err(|_| failed()),
            Self::Under(root) => {
                ensure_private_directory(root).map_err(|_| failed())?;
                let scratch = root.join(format!("{server_id}-{}", uuid::Uuid::new_v4().simple()));
                create_private_directory(&scratch).map_err(|_| failed())?;
                Ok(scratch)
            }
        }
    }
}

/// Starts the bundled runtime with `authority` and checks its signed control
/// surface. The process belongs to no Loupist until a turn leases it.
async fn start_bundled_server(
    server_id: String,
    authority: McpRunAuthority,
    scratch: StartScratch,
    app_data_dir: std::path::PathBuf,
) -> Result<TurnMcpServer, BrokerError> {
    let scratch = scratch.create(&server_id)?;
    let sensitive_values = protected_values(&authority.credentials, &app_data_dir, &scratch);
    let providerless_tool_policy = authority.runtime.providerless_tool_policy.clone();
    let provider_receipt_pointer = authority.runtime.provider_receipt_pointer.clone();
    let control_tool_names = authority.runtime.control_tool_names.clone();
    let enabled_provider_ids = authority.enabled_provider_ids.clone();
    let bootstrap = json!({
        "type": "loupist.bootstrap",
        "protocol_version": 1,
        "run_id": format!("mcp:{}", uuid::Uuid::new_v4().simple()),
        "scratch_dir": scratch.to_string_lossy(),
        "credentials": authority.credentials,
        "settings": bootstrap_settings(&authority),
    });
    let launch = McpLaunchConfig {
        server_id: server_id.clone(),
        executable: authority.runtime.executable,
        arguments: Vec::new(),
        private_working_dir: scratch.clone(),
        lease_dir: authority.runtime.lease_dir,
        environment: BTreeMap::new(),
        bootstrap,
    };
    let (session, initial_tools) = match McpSession::spawn(launch).await {
        Ok(started) => started,
        Err(error) => {
            let _ = tokio::fs::remove_dir_all(&scratch).await;
            return Err(map_mcp_error(error));
        }
    };
    let checked = ensure_inventory_contains_no_protected_values(&initial_tools, &sensitive_values)
        .and_then(|()| {
            let initial_tool_names = initial_tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect::<BTreeSet<_>>();
            if initial_tool_names != control_tool_names {
                return Err(BrokerError::Execution(
                    "MCP runtime initial inventory did not match its signed control surface".into(),
                ));
            }
            filter_inventory(
                &initial_tools,
                &control_tool_names,
                &enabled_provider_ids,
                &providerless_tool_policy,
            )
        });
    let tools = match checked {
        Ok(tools) => tools,
        Err(error) => {
            let _ = session.shutdown(MCP_SHUTDOWN_GRACE).await;
            let _ = tokio::fs::remove_dir_all(&scratch).await;
            return Err(error);
        }
    };
    Ok(TurnMcpServer {
        session,
        control_tool_names,
        tools,
        enabled_provider_ids,
        providerless_tool_policy,
        provider_receipt_pointer,
        sensitive_values,
        pool_key: None,
        scratch_dir: Some(scratch),
    })
}

/// Starts the bundled market-data runtime in the background at app start when
/// a Loupist has it enabled, so the first question finds it started or
/// starting instead of paying its cold start. Reads no saved key: a Loupist
/// whose market-data grants need one is skipped, because macOS may ask for the
/// keychain password before the first read after an update. Returns whether a
/// start began.
pub(crate) async fn prewarm_market_data(state: &AppState) -> bool {
    let server_id = openbb::OPENBB_SERVER_ID;
    if !state.artifacts.mcp_runtimes.contains_key(server_id) {
        return false;
    }
    let candidate = {
        let state = state.clone();
        tokio::task::spawn_blocking(move || keyless_market_data_grants(&state, server_id))
            .await
            .ok()
            .flatten()
    };
    let Some((loupist_id, capability_ids)) = candidate else {
        return false;
    };
    let executor = AppToolExecutor::for_host(state, &loupist_id, capability_ids);
    let Ok(authority) = executor.mcp_run_authority(server_id).await else {
        return false;
    };
    // Defense in depth: the grants above map no credential.
    if !authority.credentials.is_empty() {
        return false;
    }
    let key = pool_key(&loupist_id, server_id, &authority);
    let start = start_bundled_server(
        server_id.to_owned(),
        authority,
        StartScratch::Pool(state.mcp_pool.clone()),
        state.artifacts.app_data_dir.clone(),
    );
    state.mcp_pool.prewarm(&key, start).await
}

/// The first available Loupist whose enabled grants reach `server_id`, with
/// those grants, when none of them maps a saved key. Store reads only.
fn keyless_market_data_grants(
    state: &AppState,
    server_id: &str,
) -> Option<(String, BTreeSet<String>)> {
    let catalog = bundled_catalog().ok()?;
    let loupists = state.store.list_loupists().ok()?;
    crate::marketplace::with_connector_lifecycle(state, || {
        for loupist in loupists {
            if state.ensure_loupist_available(&loupist.id).is_err() {
                continue;
            }
            let Ok(bindings) = crate::marketplace::loupist_bindings(state, &loupist.id) else {
                continue;
            };
            let granted = bindings
                .iter()
                .filter(|binding| {
                    binding.enabled && crate::commands::binding_grants_execute(binding)
                })
                .filter_map(|binding| {
                    let entry = catalog
                        .entries
                        .iter()
                        .find(|entry| entry.id == binding.entry_id)?;
                    (entry.runtime.kind == MarketplaceRuntimeKind::BundledMcp
                        && entry.runtime.server_id.as_deref() == Some(server_id))
                    .then_some((binding, entry))
                })
                .collect::<Vec<_>>();
            if granted
                .iter()
                .any(|(_, entry)| entry.free_state != MarketplaceFreeState::Keyless)
            {
                continue;
            }
            let ready = granted
                .into_iter()
                // A normal turn adds optional credentials from its dedicated grants.
                // Prewarming may only start with grants that never open the keychain.
                .filter(|(_, entry)| entry.runtime.credential_mapping.is_empty())
                .filter(|(binding, _)| {
                    crate::marketplace::execute_binding_ready(binding, state).unwrap_or(false)
                })
                .map(|(binding, _)| binding.entry_id.clone())
                .collect::<BTreeSet<_>>();
            if !ready.is_empty() {
                return Ok(Some((loupist.id, ready)));
            }
        }
        Ok(None)
    })
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::{fs, path::PathBuf, sync::Arc};

    #[cfg(all(unix, feature = "eval-harness"))]
    #[tokio::test]
    async fn fx_replay_refuses_bundled_connect_and_unrecorded_platform_call() {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "fx-replay", None);
        let mut executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        executor.capability_ids.insert("openbb.platform".into());
        let _guard = serde_json::from_value::<crate::eval_harness::replay::Replay>(
            json!({"schema":"loupist-connection-replay/1","requests":[]}),
        )
        .unwrap()
        .install(&state.credential_environment)
        .unwrap();
        let error = executor
            .mcp_connect(json!({"server_id":"openbb"}))
            .await
            .unwrap_err();
        assert!(error.to_string().contains("eval_replay_unmatched"));
        let error = executor
            .mcp_call(
                &policy("loupist-a", None),
                json!({"server_id":"openbb","tool_name":"equity_price_quote",
                    "arguments":{"symbol":"EXAMPLE","provider":"yfinance"}}),
                "unrecorded-platform-call",
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("eval_replay_unmatched"));
        assert!(turn.mcp_session("openbb").await.is_none());
        assert!(!turn.mcp_scratch_root.as_ref().unwrap().exists());
    }

    #[cfg(unix)]
    fn fx_text_executor(temporary: &tempfile::TempDir) -> AppToolExecutor {
        let state = AppState::for_test(temporary.path().join("app"));
        let executor = turn_executor(
            temporary,
            &state,
            "loupist-a",
            capture(temporary, "fx-text", None),
        );
        state
            .store
            .create_chat(&crate::commands::tests::chat("mcp-test", "loupist-a", 1))
            .unwrap();
        executor
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn fx_reviewed_argument_refusals_reach_the_broker() {
        let temporary = tempfile::tempdir().unwrap();
        let executor = fx_text_executor(&temporary);
        for (params, message) in [
            (
                json!({"from":"USD"}),
                "to: this turn has no display currency. Pass the target ISO 4217 code in `to`.",
            ),
            // Pi rejects this shape before the broker. Direct callers still
            // receive the request repair when they skip schema validation.
            (
                json!({"from":"USD","amount":1}),
                "request: expected only a `from` currency code and an optional `to` currency code.",
            ),
        ] {
            let error = executor
                .execute_staged(
                    &policy("loupist-a", None),
                    ToolMethod::FxRate,
                    params,
                    "fx-refusal-text",
                )
                .await
                .unwrap_err();
            assert_eq!(
                crate::broker::error_body(&error),
                json!({
                    "code":"malformed_request", "message":message, "retryable":false, "next":""
                })
            );
        }
        assert!(executor.capture.mcp_session("openbb").await.is_none());
        assert!(!temporary.path().join("app/cache/fx-reference").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fx_reviewed_asof_refusal_reaches_the_model_before_any_read() {
        let temporary = tempfile::tempdir().unwrap();
        let executor = fx_text_executor(&temporary);
        let error = executor
            .execute_staged(
                &policy("loupist-a", Some("2026-11-16")),
                ToolMethod::FxRate,
                json!({"from":"USD"}),
                "asof-text",
            )
            .await
            .unwrap_err();
        let body = crate::broker::error_body(&error);
        assert_eq!(body["code"], "as_of_unsupported");
        assert_eq!(body["message"],"tool execution failed: as_of_unsupported: fx_rate supplies the latest rates only; currency_price_historical, a market data route, reads a currency pair's price bars up to the cutoff.");
        assert!(executor.capture.mcp_session("openbb").await.is_none());
        assert!(!temporary.path().join("app/cache/fx-reference").exists());
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn fx_reviewed_unsupported_currency_refusal_reaches_the_model_by_field() {
        let temporary = tempfile::tempdir().unwrap();
        let mut executor = fx_text_executor(&temporary);
        executor.capability_ids.insert("openbb.platform".into());
        executor
            .state
            .fx_reference
            .resolve(chrono::Utc::now(), true, || async {
                Ok(json!({"date":"2099-11-17","USD":1.1,"KRW":1500}))
            })
            .await
            .unwrap();
        for (field, params) in [
            ("from", json!({"from":"XXX","to":"USD"})),
            ("to", json!({"from":"USD","to":"XXX"})),
        ] {
            let error = executor
                .execute_staged(
                    &policy("loupist-a", None),
                    ToolMethod::FxRate,
                    params,
                    "unsupported-text",
                )
                .await
                .unwrap_err();
            assert_eq!(crate::broker::error_body(&error)["message"],format!("{field}: XXX is absent from this European Central Bank reference-rate table; fx_rate accepts EUR, KRW, USD."));
        }
    }
    #[cfg(unix)]
    #[tokio::test]
    async fn fx_reviewed_connection_off_refusal_and_next_reach_the_model() {
        let temporary = tempfile::tempdir().unwrap();
        let executor = fx_text_executor(&temporary);
        let error = executor
            .execute_staged(
                &policy("loupist-a", None),
                ToolMethod::FxRate,
                json!({"from":"USD","to":"KRW"}),
                "off-text",
            )
            .await
            .unwrap_err();
        let body = crate::broker::error_body(&error);
        assert_eq!(body["message"],"tool execution failed: fx_rate reads through the Market data platform connection, which is off for this Loupist, and has no stored rates.");
        assert_eq!(body["next"],"If connection_setup is available, calling it with {id: \"openbb.platform\"} shows the user the connection's setup card; otherwise the user can check the connection in Connections.");
    }

    #[cfg(unix)]
    const FX_SCRIPT: &str = r#"
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$MCP_TEST_TRANSCRIPT"
  id=${line#*\"id\":\"}; id=${id%%\"*}
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"tools":[{"name":"currency_reference_rates","inputSchema":{"type":"object","properties":{"provider":{"enum":["ecb"]}},"required":["provider"]},"annotations":{"readOnlyHint":true}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"structuredContent":{"provider":"ecb","results":{"date":"2099-11-17","EUR":1,"USD":1.1014,"KRW":1470.23}},"isError":false}}\n' "$id" ;;
  esac
done
"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn host_fx_read_checks_scope_and_provider_without_retaining_turn_artifacts() {
        let temporary = tempfile::tempdir().unwrap();
        let mut executor = fx_text_executor(&temporary);
        let binding = policy("loupist-a", None);
        assert!(matches!(
            executor.host_fx_read(&binding).await,
            Err(BrokerError::MethodDenied)
        ));
        executor.capability_ids.insert("openbb.platform".into());
        let provider = spawn_script_server(&temporary, "fx-display", "ecb", None, FX_SCRIPT).await;
        let scratch = provider.server.scratch_dir.clone().unwrap();
        install(&executor.capture, provider.server).await;
        let store = executor.state.store.clone();
        let persisted = || {
            let connection = store.lock_loupist("loupist-a").unwrap();
            let counts =
                ["chat_sessions", "knowledge_saved_price_reads", "datasets"].map(|table| {
                    connection
                        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                            row.get::<_, i64>(0)
                        })
                        .unwrap()
                });
            let chat: String = connection
                .query_row(
                    "SELECT data_json FROM chat_sessions WHERE id = 'mcp-test'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            (counts, chat)
        };
        let before = persisted();
        let raw = executor.host_fx_read(&binding).await.unwrap();
        assert_eq!(raw["structuredContent"]["results"]["KRW"], 1470.23);
        let calls = fs::read_to_string(provider.transcript).unwrap();
        assert_eq!(calls.matches(r#""method":"tools/call""#).count(), 1);
        assert!(calls.contains(r#""name":"currency_reference_rates""#));
        assert!(calls.contains(r#""provider":"ecb""#));
        assert!(executor.capture.mcp_session("openbb").await.is_none());
        assert!(executor.capture.run_results.lock().await.results.is_empty());
        assert!(executor.capture.evidence_refs.lock().await.is_empty());
        assert!(!scratch.exists());
        assert!(!executor.capture.mcp_scratch_root.as_ref().unwrap().exists());
        assert_eq!(before, persisted());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discarded_fx_delivery_restages_the_pinned_pair_and_its_evidence() {
        let temporary = tempfile::tempdir().unwrap();
        let spawned = spawn_script_server(&temporary, "fx-discard", "ecb", None, FX_SCRIPT).await;
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "fx-discard", None);
        install(&turn, spawned.server).await;
        let mut executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        executor.capability_ids.insert("openbb.platform".into());
        state
            .store
            .create_chat(&crate::commands::tests::chat("mcp-test", "loupist-a", 1))
            .unwrap();
        let policy = ToolPolicy {
            display_currency: Some("KRW".into()),
            ..policy("loupist-a", None)
        };
        let first = executor
            .execute_staged(&policy, ToolMethod::FxRate, json!({"from":"USD"}), "fx-a")
            .await
            .unwrap();
        turn.discard_delivery("fx-a").await;
        // Replace the shared service's table; this turn must keep its own revision.
        state
            .fx_reference
            .resolve("2100-01-05T16:00:00Z".parse().unwrap(), true, || async {
                Ok(json!({"date":"2100-01-05", "USD":1.0, "KRW":2000.0}))
            })
            .await
            .unwrap();
        let second = executor
            .execute_staged(
                &policy,
                ToolMethod::FxRate,
                json!({"from":"USD", "to":"KRW"}),
                "fx-b",
            )
            .await
            .unwrap();
        turn.commit_delivery("fx-b").await;
        let citation = second["citation_url"].as_str().unwrap();
        let resolves = evidence::cited_result(&*turn.run_results.lock().await, citation).is_ok();
        let receipts = turn.evidence_refs.lock().await.clone();
        let checks = turn
            .number_checks(
                &format!("[{}]({citation})", second["to_per_from"]),
                &receipts,
            )
            .await
            .checks;
        let reads = fs::read_to_string(&spawned.transcript)
            .unwrap()
            .matches(r#""method":"tools/call""#)
            .count();
        turn.shutdown_mcp().await;
        assert!(
            resolves,
            "delivery B must retain the result its citation names after A was discarded"
        );
        assert_ne!(first["citation_url"], second["citation_url"]);
        assert_eq!(second["revision"], first["revision"]);
        assert_eq!(second["to_per_from"], first["to_per_from"]);
        assert_eq!(reads, 1, "the pinned table must not fetch again");
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].kind, crate::domain::NumberCheckKind::Data);
        assert_eq!(checks[0].status, crate::domain::NumberCheckStatus::Matched);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fx_executor_retains_pairs_pins_revision_and_projects_one_source() {
        let temporary = tempfile::tempdir().unwrap();
        let spawned = spawn_script_server(&temporary, "fx", "ecb", None, FX_SCRIPT).await;
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "fx", None);
        install(&turn, spawned.server).await;
        let mut executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        executor.capability_ids.insert("openbb.platform".into());
        state
            .store
            .create_chat(&crate::commands::tests::chat("mcp-test", "loupist-a", 1))
            .unwrap();
        let policy = ToolPolicy {
            display_currency: Some("KRW".into()),
            ..policy("loupist-a", None)
        };
        let first = executor
            .execute_staged(
                &policy,
                ToolMethod::FxRate,
                json!({"from":"USD"}),
                "fx-first",
            )
            .await
            .unwrap();
        turn.commit_delivery("fx-first").await;
        let changes = state.fx_reference.take_display_changes();
        assert_eq!(
            serde_json::to_value(changes).unwrap(),
            json!([{"state":"ready","revision":"ecb:2099-11-17"}])
        );
        assert_eq!(
            first
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            [
                "citation_url",
                "computed",
                "date",
                "from",
                "legs",
                "revision",
                "source",
                "to",
                "to_per_from"
            ]
        );
        assert_eq!(first["to_per_from"], 1334.87);
        let receipt = turn.evidence_refs.lock().await.last().unwrap().clone();
        assert_eq!(turn.evidence_refs.lock().await.len(), 1);
        assert_eq!(
            receipt.title,
            "USD/KRW · European Central Bank reference rates"
        );
        assert_eq!(receipt.provider.as_deref(), Some("ecb"));
        assert_eq!(receipt.published_at.as_deref(), Some("2099-11-17"));
        assert_eq!(
            receipt.url.as_deref(),
            Some(crate::finance_data::fx::SOURCE_URL)
        );
        let checks = turn
            .number_checks(
                &format!("[1334.87]({})", first["citation_url"].as_str().unwrap()),
                std::slice::from_ref(&receipt),
            )
            .await
            .checks;
        assert_eq!(checks[0].kind, crate::domain::NumberCheckKind::Data);
        assert_eq!(checks[0].status, crate::domain::NumberCheckStatus::Matched);
        let retained = turn
            .run_results
            .lock()
            .await
            .values()
            .find(|result| result.producer.tool_name == "fx_rate")
            .unwrap()
            .clone();
        assert_eq!(retained.upstream_result_refs.len(), 1);
        let cache = temporary.path().join("app/cache/fx-reference/latest.json");
        let mut newer: Value = serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        newer["date"] = json!("2099-11-18");
        newer["revision"] = json!("ecb:2099-11-18");
        newer["per_euro"]["KRW"] = json!(1600);
        fs::write(&cache, serde_json::to_vec(&newer).unwrap()).unwrap();
        let second = executor
            .execute_staged(
                &policy,
                ToolMethod::FxRate,
                json!({"from":"EUR","to":"KRW"}),
                "fx-second",
            )
            .await
            .unwrap();
        assert_eq!(second["revision"], first["revision"]);
        assert_eq!(second["to_per_from"], 1470.23);
        let repeated = executor
            .execute_staged(
                &policy,
                ToolMethod::FxRate,
                json!({"from":"USD","to":"KRW"}),
                "fx-repeat",
            )
            .await
            .unwrap();
        assert_eq!(repeated, first);
        for rate in [first, repeated] {
            let converted = executor
                .execute_staged(
                    &policy,
                    ToolMethod::Calculate,
                    json!({
                        "operation":"convert", "unit":"USD", "to":"KRW",
                        "inputs":{"amount":100,"rate":rate["to_per_from"].to_string()},
                        "input_sources":{"rate":rate["citation_url"]}
                    }),
                    "fx-convert",
                )
                .await
                .unwrap();
            assert_eq!(converted["conversion"]["revision"], rate["revision"]);
            assert_eq!(converted["inputs"][1]["provenance"]["status"], "matched");
            assert!(converted["formulas"][0]
                .as_str()
                .unwrap()
                .ends_with("133,487 KRW"));
        }
        assert_eq!(
            fs::read_to_string(&spawned.transcript)
                .unwrap()
                .matches("\"method\":\"tools/call\"")
                .count(),
            1
        );
        let mut off_turn = capture(&temporary, "fx-off", None);
        Arc::get_mut(&mut off_turn).unwrap().unattended_run_id = Some("fx-unattended".into());
        let off = turn_executor(&temporary, &state, "loupist-a", off_turn);
        let offline = off
            .execute_staged(
                &policy,
                ToolMethod::FxRate,
                json!({"from":"USD","to":"KRW"}),
                "fx-off",
            )
            .await
            .unwrap();
        assert_eq!(offline["behind"]["because"], "connection_off");
        executor.capture.shutdown_mcp().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn fx_cutoff_and_invalid_pairs_make_no_read_and_unattended_is_allowed() {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let executor = turn_executor(
            &temporary,
            &state,
            "loupist-a",
            capture(&temporary, "fx-refuse", None),
        );
        state
            .store
            .create_chat(&crate::commands::tests::chat("mcp-test", "loupist-a", 1))
            .unwrap();
        let error = executor
            .execute_staged(
                &policy("loupist-a", Some("2026-11-16")),
                ToolMethod::FxRate,
                json!({"from":"USD","to":"KRW"}),
                "cutoff",
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("as_of_unsupported"));
        assert!(executor.capture.mcp_session("openbb").await.is_none());
        assert_eq!(
            ToolMethod::FxRate.safety().unattended,
            crate::broker::tool_class::Unattended::Allowed
        );
    }

    #[cfg(unix)]
    const ISOLATION_MCP_SCRIPT: &str = r#"
activated=0
trap 'printf exited > "$MCP_TEST_EXITED"' EXIT
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$MCP_TEST_TRANSCRIPT"
  case "$line" in
    *'"type":"loupist.bootstrap"'*)
      ;;
    *'"method":"initialize"'*)
      id=${line#*\"id\":\"}; id=${id%%\"*}
      printf '{"jsonrpc":"2.0","id":"%s","result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"Loupist MCP isolation test server","version":"1"}}}\n' "$id"
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      id=${line#*\"id\":\"}; id=${id%%\"*}
      if [ "$activated" = 1 ]; then
        printf '{"jsonrpc":"2.0","id":"%s","result":{"tools":[{"name":"activate_tools","description":"activate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"deactivate_tools","description":"deactivate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"equity_quote","description":"quote","inputSchema":{"type":"object","properties":{"symbol":{"type":"string"},"provider":{"type":"string","enum":["fmp","yfinance"]}},"required":["symbol"]},"annotations":{"readOnlyHint":true,"destructiveHint":false}}]}}\n' "$id"
      else
        printf '{"jsonrpc":"2.0","id":"%s","result":{"tools":[{"name":"activate_tools","description":"activate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"deactivate_tools","description":"deactivate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}}]}}\n' "$id"
      fi
      ;;
    *'"method":"tools/call"'*'"name":"activate_tools"'*)
      id=${line#*\"id\":\"}; id=${id%%\"*}
      activated=1
      printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":"activated"}],"structuredContent":{"activated":true},"isError":false}}\n' "$id"
      ;;
    *'"method":"tools/call"'*'"name":"deactivate_tools"'*)
      id=${line#*\"id\":\"}; id=${id%%\"*}
      if [ -n "${MCP_TEST_RESET_DELAY_SECONDS:-}" ]; then
        sleep "$MCP_TEST_RESET_DELAY_SECONDS"
      fi
      activated=0
      printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":"deactivated"}],"structuredContent":{"deactivated":true},"isError":false}}\n' "$id"
      ;;
    *'"method":"tools/call"'*'"name":"equity_quote"'*)
      id=${line#*\"id\":\"}; id=${id%%\"*}
      case "$line" in
        *'"provider":"fmp"'*) provider=fmp ;;
        *) provider=yfinance ;;
      esac
      printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":"quote"}],"structuredContent":{"provider":"%s","price":123},"isError":false}}\n' "$id" "$provider"
      ;;
  esac
done
"#;

    fn market_data_tool(name: &str, providers: &[&str]) -> McpTool {
        McpTool {
            input_schema: json!({"type": "object", "properties": {
                "symbol": {"type": "string"},
                "provider": {"type": "string", "enum": providers},
            }}),
            ..providerless_tool(name)
        }
    }

    fn providerless_tool(name: &str) -> McpTool {
        McpTool {
            name: name.into(),
            title: None,
            description: None,
            input_schema: json!({"type": "object", "properties": {}}),
            output_schema: None,
            annotations: Some(json!({"readOnlyHint": true})),
            metadata: None,
        }
    }

    #[cfg(unix)]
    fn control_tools() -> BTreeSet<String> {
        BTreeSet::from(["activate_tools".into(), "deactivate_tools".into()])
    }

    /// A per-turn server for one provider whose bootstrap carries that provider's secret.
    #[cfg(unix)]
    struct TestServer {
        server: TurnMcpServer,
        transcript: PathBuf,
        exited: PathBuf,
    }

    #[cfg(unix)]
    async fn spawn_server(
        temporary: &tempfile::TempDir,
        name: &str,
        provider: &str,
        reset_delay: Option<Duration>,
    ) -> TestServer {
        spawn_script_server(temporary, name, provider, reset_delay, ISOLATION_MCP_SCRIPT).await
    }

    #[cfg(unix)]
    async fn spawn_script_server(
        temporary: &tempfile::TempDir,
        name: &str,
        provider: &str,
        reset_delay: Option<Duration>,
        script: &str,
    ) -> TestServer {
        let transcript = temporary.path().join(format!("{name}-transcript.jsonl"));
        let exited = temporary.path().join(format!("{name}-exited"));
        let scratch = temporary.path().join(format!("{name}-scratch"));
        let enabled_provider_ids = BTreeSet::from([provider.to_owned()]);
        let secret = format!("{name}-secret");
        let mut environment = BTreeMap::from([
            (
                "MCP_TEST_TRANSCRIPT".into(),
                transcript.to_string_lossy().into_owned(),
            ),
            (
                "MCP_TEST_EXITED".into(),
                exited.to_string_lossy().into_owned(),
            ),
        ]);
        if let Some(delay) = reset_delay {
            environment.insert(
                "MCP_TEST_RESET_DELAY_SECONDS".into(),
                format!("{:.3}", delay.as_secs_f64()),
            );
        }
        let (session, initial_tools) = McpSession::spawn(McpLaunchConfig {
            server_id: "openbb".into(),
            // macOS `/bin/sh` re-execs the selected shell, which would change
            // the executable identity after the lease is registered.
            executable: fs::canonicalize("/bin/bash").unwrap(),
            arguments: vec!["-c".into(), script.into()],
            private_working_dir: scratch.clone(),
            lease_dir: temporary.path().join(format!("{name}-leases")),
            environment,
            bootstrap: json!({
                "type": "loupist.bootstrap",
                "protocol_version": 1,
                "run_id": format!("run-{name}"),
                "scratch_dir": scratch,
                "credentials": { format!("{provider}_api_key"): secret },
                "settings": {"enabled_provider_ids": enabled_provider_ids}
            }),
        })
        .await
        .unwrap();
        let tools = filter_inventory(
            &initial_tools,
            &control_tools(),
            &enabled_provider_ids,
            &ProviderlessToolPolicy::default(),
        )
        .unwrap();
        TestServer {
            server: TurnMcpServer {
                session,
                control_tool_names: control_tools(),
                tools,
                enabled_provider_ids,
                providerless_tool_policy: ProviderlessToolPolicy::default(),
                provider_receipt_pointer: "/structuredContent/provider".into(),
                sensitive_values: vec![secret],
                pool_key: None,
                scratch_dir: Some(scratch),
            },
            transcript,
            exited,
        }
    }

    #[cfg(unix)]
    fn capture(
        temporary: &tempfile::TempDir,
        name: &str,
        pool: Option<crate::mcp::pool::McpProcessPool>,
    ) -> Arc<ToolCapture> {
        Arc::new(ToolCapture {
            mcp_scratch_root: Some(temporary.path().join(format!("{name}-run"))),
            mcp_pool: pool,
            ..ToolCapture::default()
        })
    }

    #[cfg(unix)]
    async fn install(capture: &ToolCapture, server: TurnMcpServer) {
        assert!(matches!(
            capture
                .install_mcp_session("openbb", server, Arc::from([]))
                .await,
            Ok(McpServerInstall::Installed)
        ));
    }

    #[cfg(unix)]
    fn authority(temporary: &tempfile::TempDir, provider: &str, secret: &str) -> McpRunAuthority {
        McpRunAuthority {
            runtime: BundledMcpRuntime {
                executable: fs::canonicalize("/bin/bash").unwrap(),
                lease_dir: temporary.path().join("runtime-leases"),
                allowed_categories: vec!["equity".into()],
                provider_ids: BTreeSet::from(["yfinance".into(), "fmp".into()]),
                provider_network_hosts: BTreeMap::new(),
                provider_config_fields: BTreeMap::new(),
                providerless_tool_policy: ProviderlessToolPolicy::default(),
                provider_receipt_pointer: "/structuredContent/provider".into(),
                tool_activation: None,
                control_tool_names: control_tools(),
            },
            enabled_provider_ids: BTreeSet::from([provider.to_owned()]),
            allowed_network_hosts: BTreeSet::new(),
            credentials: BTreeMap::from([(format!("{provider}_api_key"), secret.to_owned())]),
            provider_config: BTreeMap::new(),
            setup_notices: Vec::new(),
        }
    }

    #[cfg(unix)]
    fn turn_executor(
        temporary: &tempfile::TempDir,
        state: &AppState,
        loupist_id: &str,
        capture: Arc<ToolCapture>,
    ) -> AppToolExecutor {
        let workspace = temporary.path().join(format!("{loupist_id}-workspace"));
        fs::create_dir_all(&workspace).unwrap();
        // Read results are retained as Loupist-owned datasets.
        if state.store.get_loupist(loupist_id).unwrap().is_none() {
            crate::commands::tests::seed_profile(
                state.store.as_ref(),
                &crate::commands::tests::profile(loupist_id, &workspace, 1),
            );
        }
        AppToolExecutor {
            state: state.clone(),
            capture,
            loupist_id: loupist_id.into(),
            capability_ids: BTreeSet::new(),
        }
    }

    #[cfg(unix)]
    fn policy(loupist_id: &str, as_of: Option<&str>) -> ToolPolicy {
        ToolPolicy {
            loupist_id: loupist_id.into(),
            session_id: "mcp-test".into(),
            use_memory: false,
            propose_memory_updates: false,
            memory_proposal_budget: 0,
            as_of: as_of.map(str::to_owned),
            display_currency: None,
        }
    }

    #[cfg(unix)]
    async fn call_as_of(
        executor: &AppToolExecutor,
        tool_name: &str,
        arguments: Value,
        as_of: Option<&str>,
    ) -> Result<Value, BrokerError> {
        let delivery = new_id("mcp-call");
        let result = executor
            .mcp_call(
                &policy(&executor.loupist_id, as_of),
                json!({"server_id": "openbb", "tool_name": tool_name, "arguments": arguments}),
                &delivery,
            )
            .await;
        executor.capture.discard_delivery(&delivery).await;
        result
    }

    #[cfg(unix)]
    async fn call(
        executor: &AppToolExecutor,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value, BrokerError> {
        call_as_of(executor, tool_name, arguments, None).await
    }

    #[cfg(unix)]
    #[cfg(unix)]
    fn tool_names(response: &Value) -> Vec<String> {
        response["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| tool["mcp_name"].as_str().unwrap().to_owned())
            .collect()
    }

    #[cfg(unix)]
    async fn wait_for_exit_marker(path: &std::path::Path) {
        tokio::time::timeout(Duration::from_secs(4), async {
            while !path.is_file() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    #[cfg(unix)]
    async fn wait_for_pool_idle(pool: &crate::mcp::pool::McpProcessPool, expected: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while pool.idle_count().await != expected {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }

    /// Returns a server to the idle pool the way a finished turn does.
    #[cfg(unix)]
    async fn release_to_pool(
        temporary: &tempfile::TempDir,
        name: &str,
        pool: &crate::mcp::pool::McpProcessPool,
        key: &McpPoolKey,
    ) -> PathBuf {
        let mut spawned = spawn_server(temporary, name, "yfinance", None).await;
        spawned.server.pool_key = Some(key.clone());
        let turn = capture(temporary, name, Some(pool.clone()));
        install(&turn, spawned.server).await;
        turn.shutdown_mcp().await;
        spawned.exited
    }

    #[test]
    fn credentials_and_private_paths_are_refused_in_results_and_inventory() {
        let secrets = protected_values(
            &BTreeMap::from([("api_key".into(), "provider-secret-123".into())]),
            std::path::Path::new("/private/app-data"),
            std::path::Path::new("/private/scratch"),
        );
        let mut keyed_echo = serde_json::Map::new();
        keyed_echo.insert("provider-secret-123".into(), json!("credential-shaped key"));
        for echoed in [
            json!({"content": [{"type": "text", "text": "error: provider-secret-123"}]}),
            json!({"content": [{"type": "text", "text": "Read /private/app-data/key.json"}]}),
            json!({"structuredContent": {"path":"/private/scratch/runtime.json"}}),
            Value::Object(keyed_echo),
        ] {
            assert!(contains_protected_value(&echoed, &secrets));
        }
        assert!(!contains_protected_value(
            &json!({"content": [{"type": "text", "text": "quote unavailable"}]}),
            &secrets,
        ));
        let mut tool = providerless_tool("technical_sma");
        tool.description = Some("schema leaked provider-secret-123".into());
        assert!(ensure_inventory_contains_no_protected_values(&[tool], &secrets).is_err());
    }

    #[test]
    fn inventory_exposes_only_manifest_authorized_read_only_tools() {
        let mut unannotated = providerless_tool("technical_ema");
        unannotated.annotations = None;
        let mut destructive = providerless_tool("technical_wma");
        destructive.annotations = Some(json!({"readOnlyHint": true, "destructiveHint": true}));
        let tools = vec![
            providerless_tool("available_tools"),
            providerless_tool("technical_sma"),
            providerless_tool("imf_utils_list_tables"),
            providerless_tool("undeclared_external_tool"),
            unannotated,
            destructive,
        ];
        let control = BTreeSet::from(["available_tools".into()]);
        let policy = ProviderlessToolPolicy {
            local_tools: BTreeSet::from([
                "technical_sma".into(),
                "technical_ema".into(),
                "technical_wma".into(),
            ]),
            implicit_provider: BTreeMap::from([("imf_utils_list_tables".into(), "imf".into())]),
        };
        let exposed = |provider: &str| {
            filter_inventory(
                &tools,
                &control,
                &BTreeSet::from([provider.into()]),
                &policy,
            )
            .unwrap()
            .into_keys()
            .collect::<Vec<_>>()
        };
        assert_eq!(exposed("yfinance"), ["available_tools", "technical_sma"]);
        assert_eq!(
            exposed("imf"),
            ["available_tools", "imf_utils_list_tables", "technical_sma"]
        );
    }

    #[test]
    fn provider_config_fails_closed_until_required_fields_are_configured() {
        let requested = BTreeSet::from(["sec".into(), "yfinance".into()]);
        let required = BTreeMap::from([
            ("sec".into(), BTreeSet::from(["contact_email".into()])),
            ("yfinance".into(), BTreeSet::new()),
        ]);
        assert_eq!(
            configured_provider_ids(requested.clone(), &required, &BTreeMap::new()),
            BTreeSet::from(["yfinance".into()])
        );

        // The OpenBB platform receives the SEC contact email stored on its sibling connector.
        let catalog = bundled_catalog().unwrap();
        let stored = BTreeMap::from([(
            "sec.edgar".to_owned(),
            BTreeMap::from([(
                "contact_email".to_owned(),
                "research@example.com".to_owned(),
            )]),
        )]);
        let config = collect_server_provider_config(
            "openbb",
            &catalog.entries,
            &requested,
            |entry_id, field_id| {
                Ok(stored
                    .get(entry_id)
                    .and_then(|values| values.get(field_id))
                    .cloned())
            },
        )
        .unwrap();
        assert_eq!(config["sec"]["contact_email"], "research@example.com");

        // Without the email the SEC provider is dropped and named, not silently lost,
        // and the other granted providers keep running.
        let missing =
            collect_server_provider_config("openbb", &catalog.entries, &requested, |_, _| Ok(None))
                .unwrap();
        assert!(!missing.contains_key("sec"));
        let required = BTreeMap::from([("sec".into(), BTreeSet::from(["contact_email".into()]))]);
        let requested = BTreeSet::from(["sec".to_owned(), "yfinance".to_owned()]);
        let enabled = configured_provider_ids(requested.clone(), &required, &missing);
        assert_eq!(enabled, BTreeSet::from(["yfinance".to_owned()]));
        let notices = setup_notices(&catalog.entries, "openbb", requested.difference(&enabled));
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("sec"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn one_turn_reuses_its_mcp_process_and_shutdown_ends_it_and_late_servers() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "one-turn", None);
        let spawned = spawn_server(&temporary, "one-turn", "yfinance", None).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let first = executor
            .mcp_connect(json!({"server_id": "openbb"}))
            .await
            .unwrap();
        let second = executor
            .mcp_connect(json!({"server_id": "openbb"}))
            .await
            .unwrap();
        assert_eq!(first, second);
        let transcript = fs::read_to_string(&spawned.transcript).unwrap();
        assert_eq!(transcript.matches("\"method\":\"initialize\"").count(), 1);

        turn.shutdown_mcp().await;
        assert!(turn.mcp_sessions.lock().await.get("openbb").is_none());
        wait_for_exit_marker(&spawned.exited).await;
        // A server whose spawn finishes after the turn stopped is discarded, not installed.
        let late = spawn_server(&temporary, "late", "yfinance", None).await;
        assert!(turn
            .install_mcp_session("openbb", late.server, Arc::from([]))
            .await
            .is_err());
        assert!(turn.mcp_sessions.lock().await.get("openbb").is_none());
        wait_for_exit_marker(&late.exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn turns_and_loupists_do_not_share_activation_provider_or_credential_authority() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn_a = capture(&temporary, "turn-a", None);
        let turn_b = capture(&temporary, "turn-b", None);
        let server_a = spawn_server(&temporary, "turn-a", "yfinance", None).await;
        let server_b = spawn_server(&temporary, "turn-b", "fmp", None).await;
        install(&turn_a, server_a.server).await;
        install(&turn_b, server_b.server).await;
        let executor_a = turn_executor(&temporary, &state, "loupist-a", turn_a.clone());
        let executor_b = turn_executor(&temporary, &state, "loupist-b", turn_b.clone());
        let quote = |provider: &str| json!({"symbol": "TEST", "provider": provider});
        let activate = json!({"tool_names": ["equity_quote"]});

        let activated_a = call(&executor_a, "activate_tools", activate.clone())
            .await
            .unwrap();
        let card = activated_a["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["mcp_name"] == "equity_quote")
            .unwrap()
            .clone();
        assert_eq!(
            card["parameters"]["properties"]["provider"]["enum"],
            json!(["yfinance"])
        );
        // Activation in one turn is invisible to another Loupist's turn.
        let connected_b = executor_b
            .mcp_connect(json!({"server_id": "openbb"}))
            .await
            .unwrap();
        assert!(!tool_names(&connected_b).contains(&"equity_quote".to_owned()));
        assert!(matches!(
            call(&executor_b, "equity_quote", quote("fmp")).await,
            Err(BrokerError::InvalidArguments { .. })
        ));
        assert!(matches!(
            call(&executor_a, "equity_quote", quote("fmp")).await,
            Err(BrokerError::InvalidArguments { .. })
        ));
        call(&executor_a, "equity_quote", quote("yfinance"))
            .await
            .unwrap();
        call(&executor_b, "activate_tools", activate).await.unwrap();
        call(&executor_b, "equity_quote", quote("fmp"))
            .await
            .unwrap();

        let transcript_a = fs::read_to_string(&server_a.transcript).unwrap();
        let transcript_b = fs::read_to_string(&server_b.transcript).unwrap();
        assert!(transcript_a.contains("turn-a-secret") && !transcript_a.contains("turn-b-secret"));
        assert!(transcript_b.contains("turn-b-secret") && !transcript_b.contains("turn-a-secret"));

        turn_a.shutdown_mcp().await;
        turn_b.shutdown_mcp().await;
        wait_for_exit_marker(&server_a.exited).await;
        wait_for_exit_marker(&server_b.exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_pooled_process_is_reused_by_the_next_turn_only_after_its_activation_resets() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let pool = state.mcp_pool.clone();
        let authority = authority(&temporary, "yfinance", "reuse-secret");
        let key = pool_key("loupist-a", "openbb", &authority);
        let turn = capture(&temporary, "reuse-one", Some(pool.clone()));
        let mut spawned = spawn_server(
            &temporary,
            "reuse",
            "yfinance",
            Some(Duration::from_millis(250)),
        )
        .await;
        spawned.server.pool_key = Some(key.clone());
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        call(
            &executor,
            "activate_tools",
            json!({"tool_names": ["equity_quote"]}),
        )
        .await
        .unwrap();
        call(
            &executor,
            "equity_quote",
            json!({"symbol": "TEST", "provider": "yfinance"}),
        )
        .await
        .unwrap();

        turn.shutdown_mcp().await;
        // The process is quarantined while its activation resets in the background.
        assert_eq!(pool.idle_count().await, 0);
        assert!(pool.acquire(&key).await.is_none());
        assert!(!spawned.exited.is_file());
        wait_for_pool_idle(&pool, 1).await;

        let next = capture(&temporary, "reuse-two", Some(pool.clone()));
        let next_executor = turn_executor(&temporary, &state, "loupist-a", next.clone());
        let connected = next_executor
            .attach_or_spawn_mcp("openbb", authority, &[])
            .await
            .unwrap();
        assert_eq!(
            tool_names(&connected),
            ["activate_tools", "deactivate_tools"]
        );
        assert!(matches!(
            call(
                &next_executor,
                "equity_quote",
                json!({"symbol": "TEST", "provider": "yfinance"})
            )
            .await,
            Err(BrokerError::InvalidArguments { .. })
        ));
        assert_eq!(
            fs::read_to_string(&spawned.transcript)
                .unwrap()
                .matches("\"method\":\"initialize\"")
                .count(),
            1
        );
        next.shutdown_mcp().await;
        wait_for_pool_idle(&pool, 1).await;
        pool.shutdown().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_pooled_process_is_not_shared_across_loupists_authorities_or_concurrent_turns() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let pool = state.mcp_pool.clone();
        let yfinance = authority(&temporary, "yfinance", "secret-a");
        let key = pool_key("loupist-a", "openbb", &yfinance);
        let exited = release_to_pool(&temporary, "shared", &pool, &key).await;
        wait_for_pool_idle(&pool, 1).await;
        for other in [
            pool_key("loupist-b", "openbb", &yfinance),
            pool_key(
                "loupist-a",
                "openbb",
                &authority(&temporary, "fmp", "secret-b"),
            ),
            pool_key(
                "loupist-a",
                "openbb",
                &authority(&temporary, "yfinance", "rotated"),
            ),
        ] {
            assert!(pool.acquire(&other).await.is_none());
        }
        let (first, second) = tokio::join!(pool.acquire(&key), pool.acquire(&key));
        let leased = match (first, second) {
            (Some(server), None) | (None, Some(server)) => server,
            (Some(_), Some(_)) => panic!("concurrent acquires shared one pooled MCP process"),
            (None, None) => panic!("pooled MCP process was not reusable"),
        };
        assert!(!exited.is_file());
        leased.discard().await;
        wait_for_exit_marker(&exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_prewarmed_process_serves_the_first_turn_of_any_loupist_with_its_authority() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let pool = state.mcp_pool.clone();
        let granted = authority(&temporary, "yfinance", "prewarm-secret");
        // The app-start prewarm chose another Loupist with the same grants.
        let prewarm_key = pool_key("loupist-first", "openbb", &granted);
        let TestServer {
            server,
            transcript,
            exited,
        } = spawn_server(&temporary, "prewarmed", "yfinance", None).await;
        assert!(pool.prewarm(&prewarm_key, async move { Ok(server) }).await);
        tokio::time::timeout(Duration::from_secs(2), async {
            while pool.fresh_count().await != 1 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        let turn = capture(&temporary, "first-question", Some(pool.clone()));
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let connected = tokio::time::timeout(
            Duration::from_secs(10),
            executor.attach_or_spawn_mcp("openbb", granted, &[]),
        )
        .await
        .expect("the turn reused the prewarmed process instead of starting one")
        .unwrap();
        assert_eq!(
            tool_names(&connected),
            ["activate_tools", "deactivate_tools"]
        );
        assert_eq!(pool.fresh_count().await, 0);
        assert_eq!(
            fs::read_to_string(&transcript)
                .unwrap()
                .matches("\"method\":\"initialize\"")
                .count(),
            1
        );
        // After its first turn the process belongs to that Loupist.
        turn.shutdown_mcp().await;
        wait_for_pool_idle(&pool, 1).await;
        assert!(pool.acquire(&prewarm_key).await.is_none());
        pool.shutdown().await;
        wait_for_exit_marker(&exited).await;
    }

    #[cfg(unix)]
    fn install_manifest_runtime_fixture(state: &mut AppState, temporary: &tempfile::TempDir) {
        let runtime = temporary.path().join("openbb-runtime");
        fs::create_dir(&runtime).unwrap();
        let bundle = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../openbb");
        for file in ["runtime-manifest.json", "uv.lock"] {
            fs::copy(bundle.join(file), runtime.join(file)).unwrap();
        }
        fs::write(runtime.join("loupist-openbb"), []).unwrap();
        Arc::make_mut(&mut state.artifacts).mcp_runtimes = crate::mcp::discover_bundled_runtimes(
            temporary.path(),
            &temporary.path().join("leases"),
        );
    }

    #[cfg(unix)]
    fn credential_fallback_runtime(temporary: &tempfile::TempDir) -> AppState {
        let mut state = AppState::for_persistent_test(temporary.path().join("app"));
        install_manifest_runtime_fixture(&mut state, temporary);
        let runtime = Arc::make_mut(&mut state.artifacts)
            .mcp_runtimes
            .get_mut("openbb")
            .unwrap();
        let controls = runtime
            .control_tool_names
            .iter()
            .map(|name| json!(providerless_tool(name)))
            .collect::<Vec<_>>();
        let mut activated = controls.clone();
        activated.extend([
            json!(market_data_tool("equity_price_quote", &["fmp", "yfinance"])),
            json!(market_data_tool("cftc_cot_search", &["cftc"])),
            json!(market_data_tool("economy_fred_series", &["fred"])),
        ]);
        let source = temporary.path().join("credential-fixture.c");
        let executable = temporary.path().join("credential-fixture");
        // Use a native process so its executable identity matches its lease.
        fs::write(
            &source,
            r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
int main(void) {
    char *line = NULL;
    size_t size = 0;
    int active = 0;
    int fmp_key = 0;
    while (getline(&line, &size, stdin) >= 0) {
        if (strstr(line, "\"type\":\"loupist.bootstrap\"")) {
            if (strstr(line, "\"fred_api_key\"") || strstr(line, "\"cftc_app_token\"")
                    || strstr(line, "\"api.stlouisfed.org\"")) return 1;
            fmp_key = strstr(line, "\"fmp_api_key\":\"fixture-fmp-key\"") != NULL;
        }
        char *id = strstr(line, "\"id\":\"");
        if (!id) continue;
        id += 6;
        char *end = strchr(id, '"');
        if (!end) return 2;
        printf("{\"jsonrpc\":\"2.0\",\"id\":\"%.*s\",\"result\":", (int)(end - id), id);
        if (strstr(line, "\"method\":\"initialize\"")) {
            printf("{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"serverInfo\":{\"name\":\"fixture\",\"version\":\"1\"}}");
        } else if (strstr(line, "\"method\":\"tools/list\"")) {
            printf("{\"tools\":%s}", active ? ACTIVE_TOOLS : CONTROL_TOOLS);
        } else if (strstr(line, "\"name\":\"activate_tools\"")) {
            active = 1;
            printf("{\"content\":[]}");
        } else {
            const char *provider = strstr(line, "\"provider\":\"cftc\"") ? "cftc"
                : strstr(line, "\"provider\":\"fmp\"") && fmp_key ? "fmp" : "yfinance";
            printf("{\"content\":[],\"structuredContent\":{\"provider\":\"%s\",\"results\":[{\"value\":123}]}}", provider);
        }
        puts("}"); fflush(stdout);
    }
    free(line);
    return 0;
}
"#
            .replace("ACTIVE_TOOLS", &json!(json!(activated).to_string()).to_string())
            .replace("CONTROL_TOOLS", &json!(json!(controls).to_string()).to_string()),
        )
        .unwrap();
        let compiled = std::process::Command::new("cc")
            .arg(&source)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        runtime.executable = executable;
        state
    }

    #[cfg(unix)]
    fn verified_test_key(state: &AppState, id: &str, field: &str, value: &str) {
        use crate::finance_credentials as credentials;
        credentials::mutate(state, id, {
            let fields = BTreeMap::from([(field.to_owned(), value.to_owned())]);
            move |state, id| {
                let env = &state.credential_environment;
                credentials::stage(env, id, &fields)?;
                let candidate = credentials::candidate(env, id)?.unwrap();
                credentials::finish_verification(
                    env,
                    id,
                    candidate.revision(),
                    credentials::VerificationOutcome::Verified,
                    1,
                    None,
                )
            }
        })
        .unwrap();
    }

    #[cfg(unix)]
    async fn check_anonymous_tools(executor: &AppToolExecutor, connected: &Value) {
        let tools = tool_names(connected);
        assert!(tools.iter().any(|name| name == "equity_price_quote"));
        assert!(tools.iter().any(|name| name == "cftc_cot_search"));
        assert!(!tools.iter().any(|name| name == "economy_fred_series"));
        for (tool, provider) in [
            ("equity_price_quote", "yfinance"),
            ("cftc_cot_search", "cftc"),
        ] {
            let read = call(executor, tool, json!({"provider": provider}))
                .await
                .unwrap();
            assert_eq!(read["result"]["structuredContent"]["provider"], provider);
            assert_eq!(
                read["result"]["structuredContent"]["results"][0]["value"],
                123
            );
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn security_lookup_failures_leave_public_tools_and_boot_with_readable_keys() {
        use crate::finance_credentials::{test_store, KeychainReadStatus};
        let temporary = tempfile::tempdir().unwrap();
        let state = credential_fallback_runtime(&temporary);
        let environment = &state.credential_environment;
        // The same run must still receive credentials from successful lookups.
        verified_test_key(&state, "fmp.market-data", "api_key", "fixture-fmp-key");
        for (status, diagnostic) in [
            (-25307, KeychainReadStatus::NoDefaultKeychain),
            (-67869, KeychainReadStatus::DatabaseLocked),
            (-25308, KeychainReadStatus::InteractionNotAllowed),
            (-25293, KeychainReadStatus::Other),
        ] {
            for id in ["cftc.commitments", "fred.macro"] {
                test_store::restart(environment, id);
                test_store::fail_with_security_status(environment, id, status);
            }
            let mut executor = turn_executor(
                &temporary,
                &state,
                "loupist-a",
                capture(&temporary, "fallback", None),
            );
            // An already captured grant can outlive removal of its key.
            executor.capability_ids = [
                "openbb.platform",
                "cftc.commitments",
                "fred.macro",
                "fmp.market-data",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            let connected = executor
                .mcp_connect(json!({"server_id":"openbb"}))
                .await
                .unwrap();
            check_anonymous_tools(&executor, &connected).await;
            let notices = connected["notices"].to_string();
            assert!(notices.contains("fred.macro") && notices.contains("FRED API key"));
            assert!(!notices.contains("saved key") && !notices.contains("cftc.commitments"));
            let fmp = call(&executor, "equity_price_quote", json!({"provider":"fmp"}))
                .await
                .unwrap();
            assert_eq!(fmp["result"]["structuredContent"]["provider"], "fmp");
            assert!(!connected.to_string().contains("fixture-fmp-key"));
            assert_eq!(
                executor.capture.tool_trace.connector_keychain_statuses(),
                BTreeSet::from([diagnostic])
            );
            executor.capture.shutdown_mcp().await;
        }
        for id in ["cftc.commitments", "fred.macro"] {
            assert_eq!(test_store::secret_reads(environment, id), 4);
            assert!(state
                .store
                .connector_credential_status(id)
                .unwrap()
                .is_none());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn saved_key_refusal_is_named_on_each_connect_without_claiming_an_optional_key_exists() {
        use crate::finance_credentials::test_store;
        let temporary = tempfile::tempdir().unwrap();
        let state = credential_fallback_runtime(&temporary);
        let environment = &state.credential_environment;
        let loupist_id = "loupist-a";
        let fred = "fred.macro";
        verified_test_key(&state, fred, "api_key", "fixture-fred-key");
        test_store::restart(environment, fred);
        let mut executor = turn_executor(
            &temporary,
            &state,
            loupist_id,
            capture(&temporary, "first", None),
        );
        {
            let guard = crate::marketplace::connector_lifecycle_lock().unwrap();
            crate::marketplace::enable_ready_bindings_locked(&state, fred, &guard).unwrap();
        }
        let reads = test_store::secret_reads(environment, fred);
        // A 0.1.3 refusal could persist this status even with no saved item.
        state
            .store
            .set_connector_credential_status("cftc.commitments", r#""Locked""#)
            .unwrap();
        test_store::fail_with_security_status(environment, fred, -25308);
        test_store::fail_with_security_status(environment, "cftc.commitments", -25308);
        for turn in ["first", "next"] {
            let authority =
                crate::commands::capture_chat_connector_authority(&state, loupist_id).unwrap();
            executor.capability_ids = authority.capability_ids.into_iter().collect();
            assert!(executor.capability_ids.contains(fred));
            assert!(executor.capability_ids.contains("cftc.commitments"));
            let connected = executor
                .mcp_connect(json!({"server_id":"openbb"}))
                .await
                .unwrap();
            check_anonymous_tools(&executor, &connected).await;
            let notices = connected["notices"].as_array().unwrap();
            let saved = notices
                .iter()
                .filter_map(Value::as_str)
                .filter(|notice| notice.contains("macOS did not release the saved key"))
                .collect::<Vec<_>>();
            assert_eq!(saved.len(), 1);
            assert!(saved[0].contains(fred));
            assert!(
                !connected.to_string().contains("fixture-fred-key"),
                "{turn}"
            );
            let cached = executor
                .mcp_connect(json!({"server_id":"openbb"}))
                .await
                .unwrap();
            assert_eq!(cached["notices"], connected["notices"]);
            executor.capture.shutdown_mcp().await;
            executor.capture = capture(&temporary, "next", None);
        }
        assert_eq!(test_store::secret_reads(environment, fred), reads + 1);
        assert!(crate::finance_credentials::has_saved_key(&state, fred));
        assert!(!crate::finance_credentials::has_saved_key(
            &state,
            "cftc.commitments"
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pending_legacy_keys_are_still_protected_and_do_not_block_the_platform() {
        use crate::finance_credentials::test_store;
        let temporary = tempfile::tempdir().unwrap();
        let state = credential_fallback_runtime(&temporary);
        let environment = &state.credential_environment;
        let loupist_id = "loupist-a";
        let mut executor = turn_executor(
            &temporary,
            &state,
            loupist_id,
            capture(&temporary, "legacy", None),
        );
        for (id, field) in [("fred.macro", "api_key"), ("cftc.commitments", "app_token")] {
            verified_test_key(&state, id, field, "fixture-legacy-key");
            {
                let guard = crate::marketplace::connector_lifecycle_lock().unwrap();
                crate::marketplace::enable_ready_bindings_locked(&state, id, &guard).unwrap();
            }
            test_store::restart(environment, id);
            state
                .store
                .set_connector_credential_status(id, r#""PresentUnread""#)
                .unwrap();
        }
        let reads = test_store::backend_calls(environment);
        let authority =
            crate::commands::capture_chat_connector_authority(&state, loupist_id).unwrap();
        assert_eq!(
            authority.saved_keys_needing_permission,
            ["cftc.commitments".to_owned(), "fred.macro".to_owned()].into()
        );
        executor.capability_ids = authority.capability_ids.into_iter().collect();
        Arc::get_mut(&mut executor.capture)
            .unwrap()
            .saved_keys_needing_permission = authority.saved_keys_needing_permission;
        let connected = executor
            .mcp_connect(json!({"server_id":"openbb"}))
            .await
            .unwrap();
        check_anonymous_tools(&executor, &connected).await;
        assert!(matches!(
            call(&executor, "economy_fred_series", json!({"provider":"fred"})).await,
            Err(BrokerError::KeychainPermissionRequired)
        ));
        assert_eq!(test_store::backend_calls(environment), reads);
        executor.capture.shutdown_mcp().await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sec_contact_controls_platform_bootstrap_and_fundamental_cards() {
        let temporary = tempfile::tempdir().unwrap();
        let mut state = AppState::for_test(temporary.path().join("app"));
        install_manifest_runtime_fixture(&mut state, &temporary);
        let executor = AppToolExecutor::for_host(
            &state,
            "fixture",
            BTreeSet::from(["openbb.platform".into()]),
        );
        let listed = [
            market_data_tool(
                "equity_fundamental_income",
                &["fmp", "intrinio", "sec", "yfinance"],
            ),
            market_data_tool(
                "equity_fundamental_cash",
                &["fmp", "intrinio", "sec", "yfinance"],
            ),
            market_data_tool(
                "equity_fundamental_balance",
                &["fmp", "intrinio", "sec", "yfinance"],
            ),
            market_data_tool(
                "equity_fundamental_metrics",
                &["finviz", "fmp", "intrinio", "yfinance"],
            ),
            market_data_tool(
                "equity_fundamental_filings",
                &["fmp", "intrinio", "sec", "tmx"],
            ),
            market_data_tool("regulators_sec_filing_headers", &["sec"]),
        ];

        for contact in [None, Some("fixture@example.test"), None] {
            crate::scoped_config::write_global_connector_settings(
                &state.artifacts.app_data_dir,
                "sec.edgar",
                &contact
                    .into_iter()
                    .map(|value| ("contact_email".into(), value.into()))
                    .collect(),
            )
            .unwrap();
            crate::marketplace::connector_config::reconcile_connector_configs(&state).unwrap();
            let grant = executor.mcp_run_authority("openbb").await.unwrap();
            let settings = bootstrap_settings(&grant);
            let enabled = &grant.enabled_provider_ids;
            let platform = bundled_catalog()
                .unwrap()
                .entries
                .into_iter()
                .find(|entry| entry.id == "openbb.platform")
                .unwrap();
            let expected = platform
                .runtime
                .provider_ids
                .into_iter()
                .filter(|id| id != "sec" || contact.is_some())
                .collect::<BTreeSet<_>>();
            assert_eq!(settings["enabled_provider_ids"], json!(expected));
            assert_eq!(
                settings["allowed_network_hosts"],
                json!(expected
                    .iter()
                    .flat_map(|id| &grant.runtime.provider_network_hosts[id])
                    .collect::<BTreeSet<_>>())
            );
            assert_eq!(
                settings["provider_config"]["sec"]["contact_email"].as_str(),
                contact
            );
            assert_eq!(grant.setup_notices.is_empty(), contact.is_some());
            if contact.is_none() {
                assert!(grant.setup_notices[0]
                    .contains(r#"connection_setup is available, call it with {id: "sec.edgar"}"#));
            }

            let controls = BTreeSet::new();
            let policy = ProviderlessToolPolicy::default();
            let tools = filter_inventory(&listed, &controls, enabled, &policy).unwrap();
            let cards =
                agent_tool_cards("openbb", tools.values(), &controls, enabled, &policy).unwrap();
            assert_eq!(cards.len(), if contact.is_some() { 6 } else { 5 });
            assert_eq!(
                tools.contains_key("regulators_sec_filing_headers"),
                contact.is_some()
            );
            for card in cards {
                let name = card["name"].as_str().unwrap();
                let description = card["description"].as_str().unwrap();
                let choices = card
                    .pointer("/parameters/properties/provider/enum")
                    .unwrap();
                let expected = if name.ends_with("_metrics") {
                    json!(["finviz", "yfinance"])
                } else if name.ends_with("_filings") {
                    if contact.is_some() {
                        json!(["sec", "tmx"])
                    } else {
                        json!(["tmx"])
                    }
                } else if name.ends_with("_filing_headers") {
                    json!(["sec"])
                } else if contact.is_some() {
                    assert!(description.contains("sec preferred"));
                    json!(["sec", "yfinance"])
                } else {
                    assert!(!description.contains("sec preferred"));
                    json!(["yfinance"])
                };
                assert_eq!(choices, &expected, "{name}");
                assert!(card["parameters"]["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("provider")));
            }
            if contact.is_none() {
                assert!(validate_provider_argument(
                    &tools["equity_fundamental_income"],
                    &json!({"provider": "sec"}),
                    enabled,
                )
                .is_err());
            }
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn v012_verified_fred_series_is_active_on_the_first_chat_connection() {
        let temporary = tempfile::tempdir().unwrap();
        let mut state = AppState::for_test(temporary.path().join("app"));
        install_manifest_runtime_fixture(&mut state, &temporary);
        let id = "fred.macro";
        let mut executor = turn_executor(
            &temporary,
            &state,
            "loupist-a",
            capture(&temporary, "first-fred", None),
        );
        let env = &state.credential_environment;
        crate::finance_credentials::stage(
            env,
            id,
            &BTreeMap::from([("api_key".into(), "fixture-fred-key".into())]),
        )
        .unwrap();
        let pending = crate::finance_credentials::candidate(env, id)
            .unwrap()
            .unwrap();
        crate::finance_credentials::finish_verification(
            env,
            id,
            pending.revision(),
            crate::finance_credentials::VerificationOutcome::Verified,
            1,
            None,
        )
        .unwrap();
        {
            let guard = crate::marketplace::connector_lifecycle_lock().unwrap();
            crate::marketplace::enable_ready_bindings_locked(&state, id, &guard).unwrap();
        }
        let first = crate::commands::capture_chat_connector_authority(&state, "loupist-a").unwrap();
        assert!(first.capability_ids.iter().any(|entry| entry == id));
        let profile = crate::agent_harness::AgentRuntimeProfile::new(
            "chat",
            false,
            false,
            &first.capability_ids,
        )
        .unwrap();
        assert!(profile
            .components
            .iter()
            .any(|component| component.id == "openbb"
                && component.entry_ids.iter().any(|entry| entry == id)));
        executor.capability_ids = first.capability_ids.into_iter().collect();
        let grant = executor.mcp_run_authority("openbb").await.unwrap();
        assert!(grant.enabled_provider_ids.contains("fred"));
        assert!(grant.credentials.contains_key("fred_api_key"));
        assert!(
            activation_names(&[], &control_tools(), &grant.enabled_provider_ids)
                .iter()
                .any(|route| route == "economy_fred_series")
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dedicated_cftc_grant_starts_without_an_app_token() {
        let temporary = tempfile::tempdir().unwrap();
        let mut state = AppState::for_test(temporary.path().join("app"));
        let mut runtime = authority(&temporary, "cftc", "unused").runtime;
        runtime.executable = temporary.path().join("missing-runtime");
        runtime.provider_ids.insert("cftc".into());
        runtime.provider_network_hosts.insert(
            "cftc".into(),
            BTreeSet::from(["publicreporting.cftc.gov".into()]),
        );
        Arc::make_mut(&mut state.artifacts)
            .mcp_runtimes
            .insert("openbb".into(), runtime);
        let mut executor = AppToolExecutor::for_host(
            &state,
            "fixture",
            BTreeSet::from(["cftc.commitments".into()]),
        );
        // A saved setup choice for a disconnected sibling must not reach the
        // sidecar, which rejects config for any provider outside this grant.
        crate::scoped_config::write_global_connector_settings(
            &state.artifacts.app_data_dir,
            "tradier.market-data",
            &BTreeMap::from([("account_type".into(), "sandbox".into())]),
        )
        .unwrap();
        crate::marketplace::connector_config::reconcile_connector_configs(&state).unwrap();
        let grant = executor.mcp_run_authority("openbb").await.unwrap();
        assert_eq!(grant.enabled_provider_ids, BTreeSet::from(["cftc".into()]));
        assert!(
            grant.provider_config.is_empty(),
            "{:?}",
            grant.provider_config
        );
        assert!(grant.credentials.is_empty());
        let price_failure = Arc::new(std::sync::atomic::AtomicBool::new(false));
        Arc::get_mut(&mut executor.capture)
            .unwrap()
            .price_setup_connect_failed = Some(price_failure.clone());
        assert!(executor
            .mcp_connect(json!({"server_id": "openbb"}))
            .await
            .is_err());
        assert!(state.mcp_pool.openbb_connect_failed());
        assert!(price_failure.load(std::sync::atomic::Ordering::Relaxed));
        let env = &state.credential_environment;
        let id = "cftc.commitments";
        crate::finance_credentials::stage(
            env,
            id,
            &BTreeMap::from([("app_token".into(), "fixture-token".into())]),
        )
        .unwrap();
        let pending = crate::finance_credentials::candidate(env, id)
            .unwrap()
            .unwrap();
        assert!(executor
            .mcp_run_authority("openbb")
            .await
            .unwrap()
            .credentials
            .is_empty());
        crate::finance_credentials::finish_verification(
            env,
            id,
            pending.revision(),
            crate::finance_credentials::VerificationOutcome::Verified,
            1,
            None,
        )
        .unwrap();
        let grant = executor.mcp_run_authority("openbb").await.unwrap();
        assert_eq!(
            grant.credentials.get("cftc_app_token").map(String::as_str),
            Some("fixture-token")
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_app_start_prewarm_needs_an_enabled_grant_and_reads_no_saved_key() {
        let temporary = tempfile::tempdir().unwrap();
        let mut state = AppState::for_test(temporary.path().join("app"));
        Arc::make_mut(&mut state.artifacts).mcp_runtimes.insert(
            "openbb".into(),
            authority(&temporary, "yfinance", "unused").runtime,
        );
        let workspace = temporary.path().join("loupist-a-workspace");
        fs::create_dir_all(&workspace).unwrap();
        crate::commands::tests::seed_profile(
            state.store.as_ref(),
            &crate::commands::tests::profile("loupist-a", &workspace, 1),
        );
        let grant = |entry_id: &str, enabled: bool| {
            state
                .store
                .save_loupist_capability(&crate::domain::LoupistCapabilityBinding {
                    loupist_id: "loupist-a".into(),
                    entry_id: entry_id.into(),
                    enabled,
                    granted_permissions: vec!["execute".into()],
                    config: BTreeMap::new(),
                    updated_at_ms: 1,
                })
                .unwrap()
        };
        // A new Loupist starts with the keyless platform enabled, so a
        // default install starts market data at launch.
        let (loupist_id, granted) = keyless_market_data_grants(&state, "openbb").unwrap();
        assert_eq!(loupist_id, "loupist-a");
        assert!(granted.contains("openbb.platform"), "{granted:?}");
        // With every market-data grant off, nothing starts.
        let catalog = bundled_catalog().unwrap();
        for binding in crate::marketplace::loupist_bindings(&state, "loupist-a").unwrap() {
            if catalog.entries.iter().any(|entry| {
                entry.id == binding.entry_id && entry.runtime.server_id.as_deref() == Some("openbb")
            }) {
                grant(&binding.entry_id, false);
            }
        }
        assert!(keyless_market_data_grants(&state, "openbb").is_none());
        grant("openbb.platform", true);
        assert_eq!(
            keyless_market_data_grants(&state, "openbb"),
            Some((
                "loupist-a".to_owned(),
                BTreeSet::from(["openbb.platform".to_owned()])
            ))
        );
        // A granted provider with a saved key would make the start read the keychain.
        grant("fmp.market-data", true);
        assert!(keyless_market_data_grants(&state, "openbb").is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_pool_evicts_idle_processes_after_their_ttl_and_beyond_capacity() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let key = pool_key(
            "loupist-a",
            "openbb",
            &authority(&temporary, "yfinance", "pool-secret"),
        );
        let pool_at = |name: &str, ttl: Duration, max_idle: usize| {
            let root = temporary.path().join(name);
            crate::fs::artifact_trust::ensure_private_directory(&root).unwrap();
            crate::mcp::pool::McpProcessPool::with_limits(root, ttl, max_idle)
        };

        let ttl_pool = pool_at("ttl-pool", Duration::from_millis(40), 4);
        let expired = release_to_pool(&temporary, "ttl", &ttl_pool, &key).await;
        wait_for_pool_idle(&ttl_pool, 1).await;
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert!(ttl_pool.acquire(&key).await.is_none());
        wait_for_exit_marker(&expired).await;

        let capped = pool_at("cap-pool", Duration::from_secs(60), 1);
        let oldest = release_to_pool(&temporary, "cap-a", &capped, &key).await;
        wait_for_pool_idle(&capped, 1).await;
        let newest = release_to_pool(&temporary, "cap-b", &capped, &key).await;
        wait_for_exit_marker(&oldest).await;
        assert!(!newest.is_file());
        wait_for_pool_idle(&capped, 1).await;
        capped.acquire(&key).await.unwrap().discard().await;
        wait_for_exit_marker(&newest).await;
    }

    /// Price bars include a post-cutoff row and an undated row; the profile has no date.
    #[cfg(unix)]
    const DATED_MCP_SCRIPT: &str = r#"
trap 'printf exited > "$MCP_TEST_EXITED"' EXIT
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$MCP_TEST_TRANSCRIPT"
  id=${line#*\"id\":\"}; id=${id%%\"*}
  case "$line" in
    *'"type":"loupist.bootstrap"'*)
      ;;
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"Loupist dated test server","version":"1"}}}\n' "$id"
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"tools":[{"name":"activate_tools","description":"activate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"deactivate_tools","description":"deactivate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"equity_price_historical","description":"Get historical price data for a given stock.","inputSchema":{"type":"object","properties":{"symbol":{"type":"string"},"start_date":{"type":"string"},"end_date":{"type":"string"},"provider":{"type":"string","enum":["yfinance"]}},"required":["symbol"]},"annotations":{"readOnlyHint":true,"destructiveHint":false}},{"name":"equity_profile","description":"Get a company profile.","inputSchema":{"type":"object","properties":{"symbol":{"type":"string"},"provider":{"type":"string","enum":["yfinance"]}},"required":["symbol"]},"annotations":{"readOnlyHint":true,"destructiveHint":false}}]}}\n' "$id"
      ;;
    *'"method":"tools/call"'*'"name":"equity_price_historical"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":"bars through 2026-03-04 close 12.5"}],"structuredContent":{"provider":"yfinance","results":[{"date":"2026-03-02","close":10.5},{"date":"2026-03-03T16:00:00-05:00","close":11.5},{"date":"2026-03-04","close":12.5},{"close":13.5}],"chart":{"last":"2026-03-04"}},"isError":false}}\n' "$id"
      ;;
    *'"method":"tools/call"'*'"name":"equity_profile"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":"profile"}],"structuredContent":{"provider":"yfinance","results":[{"symbol":"AAA","name":"Fixture Corp"}]},"isError":false}}\n' "$id"
      ;;
  esac
done
"#;

    #[cfg(unix)]
    #[tokio::test]
    async fn an_as_of_cutoff_bounds_dated_openbb_routes_and_refuses_undated_ones() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "as-of", None);
        let spawned =
            spawn_script_server(&temporary, "as-of", "yfinance", None, DATED_MCP_SCRIPT).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let cutoff = Some("2026-03-03");

        let response = call_as_of(
            &executor,
            "equity_price_historical",
            json!({"symbol": "AAA", "provider": "yfinance", "end_date": "2026-06-30"}),
            cutoff,
        )
        .await
        .unwrap();
        let result = &response["result"];
        let closes = result["structuredContent"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["close"].as_f64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(closes, [10.5, 11.5]);
        assert!(!response.to_string().contains("2026-03-04"), "{response}");
        // Match the row's value, not any "12.5" (a timestamp's seconds can spell it).
        assert!(!response.to_string().contains("\":12.5"), "{response}");
        assert_eq!(result["as_of"]["cutoff"], "2026-03-03");
        assert_eq!(result["as_of"]["post_cutoff_excluded"], 1);
        assert_eq!(result["as_of"]["undated_excluded"], 1);
        // The retained dataset holds only rows known by the cutoff.
        let reference: crate::datasets::DatasetRef =
            serde_json::from_value(result["dataset_ref"].clone()).unwrap();
        let (_, retained, _) = state
            .store
            .dataset("loupist-a", &reference.dataset_id, reference.revision)
            .unwrap();
        assert!(!retained.to_string().contains("2026-03-04"));
        assert!(retained.to_string().contains("11.5"));

        // An undated route is refused before it reaches the provider.
        let refused = call_as_of(
            &executor,
            "equity_profile",
            json!({"symbol": "AAA", "provider": "yfinance"}),
            cutoff,
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(
            refused.contains("as_of_unsupported") && refused.contains("2026-03-03"),
            "{refused}"
        );
        // A start after the cutoff cannot be bounded.
        assert!(call_as_of(
            &executor,
            "equity_price_historical",
            json!({"symbol": "AAA", "provider": "yfinance", "start_date": "2026-04-01"}),
            cutoff,
        )
        .await
        .is_err());

        let transcript = fs::read_to_string(&spawned.transcript).unwrap();
        assert!(transcript.contains(r#""end_date":"2026-03-03""#));
        assert!(!transcript.contains("2026-06-30"));
        assert!(!transcript.contains("2026-04-01"));
        assert!(!transcript.contains(r#""name":"equity_profile""#));

        // Without a cutoff the undated route runs.
        call(
            &executor,
            "equity_profile",
            json!({"symbol": "AAA", "provider": "yfinance"}),
        )
        .await
        .unwrap();
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn max_age_reuses_a_recent_identical_openbb_read_without_forwarding_it() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "max-age", None);
        let spawned =
            spawn_script_server(&temporary, "max-age", "yfinance", None, DATED_MCP_SCRIPT).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let request = json!({"symbol": "AAA", "provider": "yfinance", "max_age": 3600});

        let first = call(&executor, "equity_price_historical", request.clone())
            .await
            .unwrap();
        assert!(first["result"]["fetched_at"].is_string());
        assert!(first["result"].get("reused").is_none());
        let second = call(&executor, "equity_price_historical", request)
            .await
            .unwrap();
        assert_eq!(second["result"]["reused"], true);
        assert_eq!(
            second["result"]["dataset_ref"],
            first["result"]["dataset_ref"]
        );
        assert_eq!(
            second["result"]["fetched_at"],
            first["result"]["fetched_at"]
        );
        // Without max_age the provider is called again.
        call(
            &executor,
            "equity_price_historical",
            json!({"symbol": "AAA", "provider": "yfinance"}),
        )
        .await
        .unwrap();

        let transcript = fs::read_to_string(&spawned.transcript).unwrap();
        assert_eq!(
            transcript
                .matches(r#""name":"equity_price_historical""#)
                .count(),
            2
        );
        assert!(!transcript.contains("max_age"));
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    #[test]
    fn empty_rows_do_not_mistake_zero_false_or_other_data_for_missing_values() {
        for rows in [
            json!([]),
            json!([{}]),
            json!([null, {"price":null, "nested":[]}]),
            json!([{"value":" "}]),
        ] {
            assert!(returned_no_rows(
                &json!({"structuredContent":{"results":rows}})
            ));
        }
        for rows in [
            json!([{"price":0}]),
            json!([{"flag":false}]),
            json!([{"symbol":"AAA"}]),
            json!([{}, {"value":1}]),
            json!({"value":null}),
        ] {
            assert!(!returned_no_rows(
                &json!({"structuredContent":{"results":rows}})
            ));
        }
        assert!(!returned_no_rows(
            &json!({"structuredContent":{"notice":"data"}})
        ));
        assert!(!returned_no_rows(
            &json!({"content":[{"type":"image","data":""}]})
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_blank_provider_reply_keeps_the_session_and_a_host_result_ref() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "blank", None);
        let script = ASSET_REFRESH_SCRIPT.replace(
            "reply \"$id\" ''",
            r#"printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":""}],"isError":false}}\n' "$id""#,
        );
        let spawned = spawn_script_server(&temporary, "blank", "sec", None, &script).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let delivery = new_id("mcp-call");
        let empty = executor.mcp_call(&policy("loupist-a", None), json!({"server_id":"openbb", "tool_name":"etf_info", "arguments":{"symbol":"MORR","provider":"sec"}}), &delivery).await.unwrap();
        executor.capture.commit_delivery(&delivery).await;
        assert_eq!(empty["result"]["status"], "ok", "{empty}");
        assert_eq!(empty["result"]["row_count"], 0);
        assert!(empty["result"]["next"]
            .as_str()
            .unwrap()
            .contains("another enabled provider"));
        assert!(empty["result"]["result_ref"].is_string());
        assert!(empty["result"].get("citation_url").is_none());
        assert!(turn.evidence_refs.lock().await.is_empty());
        let bars = call(
            &executor,
            "equity_price_historical",
            json!({"symbol":"MORR","provider":"sec"}),
        )
        .await
        .unwrap();
        assert!(bars["result"]["citation_url"].is_string(), "{bars}");
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    /// A provider that answered with no rows (cboe before yfinance) is shown
    /// to the model as empty but is not a citable result or a listed Source.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_read_that_returned_no_rows_is_not_a_source_of_the_answer() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "no-rows", None);
        let spawned =
            spawn_script_server(&temporary, "no-rows", "sec", None, ASSET_REFRESH_SCRIPT).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let read = |tool: &'static str| {
            let executor = &executor;
            async move {
                let delivery = new_id("mcp-call");
                let result = executor
                    .mcp_call(
                        &policy("loupist-a", None),
                        json!({"server_id": "openbb", "tool_name": tool, "arguments": {"symbol": "MORR", "provider": "sec"}}),
                        &delivery,
                    )
                    .await
                    .unwrap();
                executor.capture.commit_delivery(&delivery).await;
                result
            }
        };
        let empty = read("etf_holdings").await;
        assert_eq!(empty["result"]["structuredContent"]["results"], json!([]));
        assert!(empty["result"].get("citation_url").is_none(), "{empty}");
        let bars = read("equity_price_historical").await;
        assert!(bars["result"]["citation_url"].is_string(), "{bars}");
        let evidence = turn.evidence_refs.lock().await.clone();
        assert_eq!(
            evidence
                .iter()
                .map(|item| item.tool_name.as_str())
                .collect::<Vec<_>>(),
            ["equity_price_historical"]
        );
        assert!(!returned_no_rows(
            &json!({"content": [{"type": "text", "text": "rows"}], "structuredContent": {"provider": "sec", "results": {"symbol": "MORR"}}})
        ));
        assert!(returned_no_rows(
            &json!({"content": [{"type": "text", "text": " "}], "isError": false})
        ));
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_first_openbb_connect_activates_the_default_read_set() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let pool = state.mcp_pool.clone();
        let mut authority = authority(&temporary, "yfinance", "default-secret");
        authority.runtime.tool_activation = Some(McpToolActivation {
            tool_name: "activate_tools".into(),
            argument_name: "tool_names".into(),
        });
        let key = pool_key("loupist-a", "openbb", &authority);
        let mut spawned = spawn_server(&temporary, "defaults", "yfinance", None).await;
        spawned.server.pool_key = Some(key);
        let earlier = capture(&temporary, "defaults-one", Some(pool.clone()));
        install(&earlier, spawned.server).await;
        earlier.shutdown_mcp().await;
        wait_for_pool_idle(&pool, 1).await;

        let turn = capture(&temporary, "defaults-two", Some(pool.clone()));
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let connected = executor
            .attach_or_spawn_mcp("openbb", authority, &[])
            .await
            .unwrap();
        assert!(tool_names(&connected).contains(&"equity_quote".to_owned()));
        let activation = fs::read_to_string(&spawned.transcript)
            .unwrap()
            .lines()
            .rfind(|line| line.contains(r#""name":"activate_tools""#))
            .unwrap()
            .to_owned();
        for name in openbb::DEFAULT_ACTIVE_TOOLS {
            assert!(activation.contains(name), "{activation}");
        }
        // The activated tool is callable with no discovery round.
        call(
            &executor,
            "equity_quote",
            json!({"symbol": "TEST", "provider": "yfinance"}),
        )
        .await
        .unwrap();
        turn.shutdown_mcp().await;
        wait_for_pool_idle(&pool, 1).await;

        // A warm Chat restores the set its last turn ended with, not the defaults.
        let mut warm_authority = self::authority(&temporary, "yfinance", "default-secret");
        warm_authority.runtime.tool_activation = Some(McpToolActivation {
            tool_name: "activate_tools".into(),
            argument_name: "tool_names".into(),
        });
        let warm = capture(&temporary, "defaults-three", Some(pool.clone()));
        let warm_executor = turn_executor(&temporary, &state, "loupist-a", warm.clone());
        warm_executor
            .attach_or_spawn_mcp(
                "openbb",
                warm_authority,
                &["activate_tools".into(), "equity_quote".into()],
            )
            .await
            .unwrap();
        let restored = fs::read_to_string(&spawned.transcript)
            .unwrap()
            .lines()
            .rfind(|line| line.contains(r#""name":"activate_tools""#))
            .unwrap()
            .to_owned();
        assert!(restored.contains("equity_quote"), "{restored}");
        assert!(!restored.contains("equity_search"), "{restored}");
        warm.shutdown_mcp().await;
        wait_for_pool_idle(&pool, 1).await;
        pool.shutdown().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    #[test]
    fn a_connect_restores_the_requested_set_or_falls_back_to_the_defaults() {
        let control = control_tools();
        let requested = ["activate_tools".to_owned(), "etf_holdings".to_owned()];
        assert_eq!(
            activation_names(&requested, &control, &BTreeSet::new()),
            ["etf_holdings"]
        );
        assert_eq!(
            activation_names(&["activate_tools".to_owned()], &control, &BTreeSet::new()),
            openbb::DEFAULT_ACTIVE_TOOLS
        );
        assert_eq!(
            activation_names(&[], &control, &BTreeSet::new()),
            openbb::DEFAULT_ACTIVE_TOOLS
        );
        let parsed = McpConnectRequest::parse(
            json!({"server_id": "openbb", "activate_tool_names": ["etf_holdings"]}),
        )
        .unwrap();
        assert_eq!(parsed.activate_tool_names, ["etf_holdings"]);
        assert!(McpConnectRequest::parse(json!({"server_id": "openbb"}))
            .unwrap()
            .activate_tool_names
            .is_empty());
        for names in [
            json!(["bad name"]),
            json!([""]),
            json!(["x".repeat(129)]),
            json!(vec!["a"; MAX_ACTIVATION_NAMES + 1]),
        ] {
            assert!(matches!(
                McpConnectRequest::parse(
                    json!({"server_id": "openbb", "activate_tool_names": names})
                ),
                Err(BrokerError::Malformed)
            ));
        }
    }

    #[test]
    fn provider_failure_names_only_the_tools_enabled_alternatives() {
        let generic = || BrokerError::Execution("provider failed".into()).with_next("generic");
        let listed = [market_data_tool(
            "equity_fundamental_income",
            &[
                "sec", "fmp", "yfinance", "finviz", "intrinio", "nasdaq", "tmx", "fred",
            ],
        )];
        let enabled = ["sec", "yfinance", "finviz", "nasdaq", "tmx", "fred"];
        for (enabled, expected) in [
            (&enabled[..], "This provider failed for this request. Call the tool again with another provider listed for it (for example nasdaq, tmx, fred, yfinance), or report that the data is unavailable."),
            (&enabled[..2], "This provider failed for this request. Call the tool again with another provider listed for it (for example yfinance), or report that the data is unavailable."),
            (&enabled[..1], "generic"),
        ] {
            let enabled = enabled.iter().map(|id| (*id).into()).collect();
            let filtered = filter_inventory(&listed, &BTreeSet::new(), &enabled, &Default::default()).unwrap();
            let tool = filtered.get("equity_fundamental_income");
            let error = provider_failure(generic(), tool, Some("sec"), true);
            assert_eq!(crate::broker::error_body(&error)["next"], expected);
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn openbb_offline_marker_crosses_the_stdio_and_executor_boundaries() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "offline", None);
        let script = ISOLATION_MCP_SCRIPT.replace(
            r#""price":123},"isError":false"#,
            r#""loupist_error":{"kind":"offline"}},"isError":true"#,
        );
        let spawned = spawn_script_server(&temporary, "offline", "yfinance", None, &script).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        call(
            &executor,
            "activate_tools",
            json!({"tool_names": ["equity_quote"]}),
        )
        .await
        .unwrap();
        // Once activated, repeated reads must remain typed broker failures.
        for _ in 0..2 {
            let error = call(
                &executor,
                "equity_quote",
                json!({"symbol": "TEST", "provider": "yfinance"}),
            )
            .await
            .unwrap_err();
            let BrokerError::WithNext { source, .. } = error else {
                panic!("expected the scraped-provider recovery hint");
            };
            assert!(matches!(*source, BrokerError::Offline(_)));
        }
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn sec_contact_refusal_crosses_stdio_and_keeps_its_activity_reason() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "sec-contact", None);
        let script = ISOLATION_MCP_SCRIPT
            .replace("fmp", "sec")
            .replace("equity_quote", "equity_fundamental_income")
            .replace(
                r#""text":"quote"}],"structuredContent":{"provider":"%s","price":123},"isError":false"#,
                r#""text":"sec_contact_required"}],"structuredContent":{"loupist_error":{"provider":"%s","kind":"sec_contact_required"}},"isError":true"#,
            );
        let spawned = spawn_script_server(&temporary, "sec-contact", "sec", None, &script).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        call(
            &executor,
            "activate_tools",
            json!({"tool_names": ["equity_fundamental_income"]}),
        )
        .await
        .unwrap();
        let error = call(
            &executor,
            "equity_fundamental_income",
            json!({"symbol": "TSLA", "provider": "sec", "period": "annual", "limit": 4}),
        )
        .await
        .unwrap_err();
        let body = crate::broker::error_body(&error);
        assert_eq!(body["code"], "sec_contact_required");
        assert_eq!(body["message"], "SEC answers only requests that declare a contact, and none is set in this app, so the sec provider was not used.");
        assert_eq!(body["next"], "Call this tool again with another provider listed for it. Tell the user that adding their contact under Settings > Data connections > SEC EDGAR enables SEC data.");
        let mut activity = crate::chat::progress::ChatProgressProjection::new(1);
        activity.start_tool(
            "call-sec",
            "mcp__openbb__equity_fundamental_income",
            &json!({}),
            None,
            2,
        );
        activity.finish_tool_result("call-sec", true, 3, &Value::Null, Some(&body));
        let step = serde_json::to_value(&activity.snapshot().items[0]).unwrap();
        assert_eq!(step["failureReason"], "invalid_input");
        assert_eq!(step["refusal"], "sec_contact_required");
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn python_reads_capture_quarterly_income_in_the_parent_delivery() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let _worker = crate::python_run::PYTHON_WORKER_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let (_workspace, roots) = crate::python_run::ledger::empty_roots().unwrap();
        let turn = Arc::new(ToolCapture {
            native: NativeGate::for_turn(roots),
            ..ToolCapture::default()
        });
        let route = "equity_fundamental_income";
        let script = ISOLATION_MCP_SCRIPT
            .replace("equity_quote", route)
            .replace(
                r#""symbol":{"type":"string"},"provider""#,
                r#""symbol":{"type":"string"},"period":{"type":"string"},"limit":{"type":"integer"},"provider""#,
            )
            .replace(
                r#"    *'"method":"tools/call"'*'"name":"equity_fundamental_income"'*)"#,
                r#"    *'"method":"tools/call"'*'"name":"equity_fundamental_income"'*)
      case "$line" in
        *'"symbol":"AMD"'*) revenue=123 ;;
        *'"symbol":"AVGO"'*) revenue=456 ;;
        *) exit 1 ;;
      esac"#,
            )
            .replace(r#""price":123"#, r#""results":[{"period_ending":"2025-03-31","total_revenue":%s}]"#)
            .replace(r#""$id" "$provider""#, r#""$id" "$provider" "$revenue""#);
        let spawned =
            spawn_script_server(&temporary, "quarterly-income", "yfinance", None, &script).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let policy = policy("loupist-a", None);
        state
            .store
            .create_chat(&crate::commands::tests::chat(
                &policy.session_id,
                "loupist-a",
                1,
            ))
            .unwrap();

        let delivery = "python-income";
        let result = executor
            .execute_for_delivery(
                &policy,
                ToolMethod::PythonRun,
                json!({"code": r#"
import loupist

for symbol in ("AMD", "AVGO"):
    arguments = dict(symbol=symbol, provider="yfinance", period="quarter", limit=4)
    income = loupist.call("mcp__openbb__equity_fundamental_income", **arguments)
    assert income == loupist.call("mcp__openbb__equity_fundamental_income", **arguments)
    print(symbol, income["structuredContent"]["results"][0]["total_revenue"])
"#}),
                delivery,
            )
            .await;
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;

        let result = result.unwrap();
        assert_eq!(result["status"], "completed", "{result}");
        assert_eq!(result["rounds"], 3);
        let stdout = result["stdout"].as_str().unwrap();
        assert!(stdout.contains("AMD 123\nAVGO 456\n"), "{stdout}");
        let reads = result["reads"].as_array().unwrap();
        assert_eq!(reads.len(), 2);
        for read in reads {
            assert_eq!(read["status"], "ok", "{read}");
        }
        let transcript = fs::read_to_string(spawned.transcript).unwrap();
        let income_calls: Vec<_> = transcript
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|call| call["method"] == "tools/call" && call["params"]["name"] == route)
            .map(|call| call["params"]["arguments"].clone())
            .collect();
        assert_eq!(
            income_calls,
            [
                json!({"symbol":"AMD", "provider":"yfinance", "period":"quarter", "limit":4}),
                json!({"symbol":"AVGO", "provider":"yfinance", "period":"quarter", "limit":4}),
            ]
        );

        let parent_ref = result["result_ref"].as_str().unwrap();
        assert!(turn.run_result(parent_ref).await.is_none());
        assert_eq!(
            turn.pending_deliveries.lock().await[delivery]
                .run_results
                .len(),
            3
        );
        executor.commit_delivery(&policy, delivery).await.unwrap();
        let parent = turn.run_result(parent_ref).await.unwrap();
        assert_eq!(parent.upstream_result_refs.len(), 2);
        let mut symbols = BTreeSet::new();
        for reference in parent.upstream_result_refs {
            let read = turn.run_result(&reference).await.unwrap();
            assert_eq!(read.producer.tool_name, route);
            symbols.insert(read.origin.unwrap());
        }
        assert_eq!(symbols, BTreeSet::from(["AMD".into(), "AVGO".into()]));
    }

    /// A market-data server for host refreshes: SEC filings for MORR (a 10-K on
    /// the first call, then a new 10-Q too), an income read that fails, closes,
    /// and `etf_info` that knows only the fund QQQF.
    #[cfg(unix)]
    /// SEC's five-year Apple income statement as the bundled runtime returned
    /// it on 2026-11-23: about 60 KiB of XBRL source tags in
    /// `extra.results_metadata` ahead of the periods. Delivered compacted, the
    /// sample used to show `results` as omitted and no statements table.
    #[cfg(unix)]
    #[tokio::test]
    async fn a_compacted_sec_statement_shows_its_periods_and_every_line_item() {
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/commands/tests/fixtures/sec-income-aapl-annual5.json");
        let recorded: Value = serde_json::from_slice(&fs::read(&fixture).unwrap()).unwrap();
        assert!(
            recorded["structuredContent"]["extra"]["results_metadata"]
                .to_string()
                .len()
                > 32 * 1024
        );
        let script = SEC_STATEMENT_SCRIPT.replace("FIXTURE", &fixture.to_string_lossy());
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "sec-statement", None);
        let spawned = spawn_script_server(&temporary, "sec-statement", "sec", None, &script).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let delivery = new_id("mcp-call");
        let read = executor
            .mcp_call(
                &policy("loupist-a", None),
                json!({"server_id": "openbb", "tool_name": "equity_fundamental_income",
                    "arguments": {"symbol": "AAPL", "period": "annual", "limit": 5, "provider": "sec"}}),
                &delivery,
            )
            .await
            .unwrap();
        executor.capture.commit_delivery(&delivery).await;
        let delivered = &read["result"];
        let sample = &delivered["data_summary"]["sample"]["structuredContent"];
        assert!(
            sample["extra"].get("results_metadata").is_none(),
            "{sample}"
        );
        let periods = sample["results"].as_array().unwrap();
        assert!(!periods.is_empty() && periods[0]["total_revenue"].is_number());
        let statements = &delivered["statements"];
        assert_eq!(statements["provider"], "sec");
        let columns = statements["columns"].as_array().unwrap();
        assert_eq!(columns[0], "period_ending");
        let rows = statements["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 5);
        for line in ["total_revenue", "total_operating_income", "net_income"] {
            let index = columns.iter().position(|column| column == line).unwrap();
            assert!(rows.iter().all(|row| row[index].is_number()), "{line}");
        }
        assert!(delivered.to_string().len() < 32 * 1024);
        // The retained result keeps the provider's bookkeeping whole.
        let reference = &delivered["dataset_ref"];
        let (_, retained, _) = state
            .store
            .dataset(
                "loupist-a",
                reference["dataset_id"].as_str().unwrap(),
                reference["revision"].as_u64().unwrap() as u32,
            )
            .unwrap();
        assert_eq!(
            retained["structuredContent"]["extra"]["results_metadata"],
            recorded["structuredContent"]["extra"]["results_metadata"]
        );
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }

    const SEC_STATEMENT_SCRIPT: &str = r#"
trap 'printf exited > "$MCP_TEST_EXITED"' EXIT
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$MCP_TEST_TRANSCRIPT"
  id=${line#*\"id\":\"}; id=${id%%\"*}
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"Loupist SEC statement test server","version":"1"}}}\n' "$id"
      ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"tools":[{"name":"activate_tools","description":"activate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"deactivate_tools","description":"deactivate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"equity_fundamental_income","description":"income","inputSchema":{"type":"object","properties":{"symbol":{"type":"string"},"period":{"type":"string"},"limit":{"type":"integer"},"provider":{"type":"string","enum":["sec"]}}},"annotations":{"readOnlyHint":true,"destructiveHint":false}}]}}\n' "$id"
      ;;
    *'"method":"tools/call"'*'"name":"equity_fundamental_income"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":' "$id"; tr -d '\n' < 'FIXTURE'; printf '}\n'
      ;;
  esac
done
"#;

    const ASSET_REFRESH_SCRIPT: &str = r#"
trap 'printf exited > "$MCP_TEST_EXITED"' EXIT
tool() { printf '{"name":"%s","description":"%s","inputSchema":{"type":"object","properties":{"symbol":{"type":"string"},"limit":{"type":"integer"},"start_date":{"type":"string"},"period":{"type":"string"},"provider":{"type":"string","enum":["sec"]}}},"annotations":{"readOnlyHint":true,"destructiveHint":false}}' "$1" "$1"; }
reply() { printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":"rows"}],"structuredContent":{"provider":"sec","results":[%s]},"isError":false}}\n' "$1" "$2"; }
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$MCP_TEST_TRANSCRIPT"
  id=${line#*\"id\":\"}; id=${id%%\"*}
  case "$line" in
    *'"type":"loupist.bootstrap"'*)
      ;;
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"Loupist refresh test server","version":"1"}}}\n' "$id"
      ;;
    *'"method":"notifications/initialized"'*)
      ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"tools":[{"name":"activate_tools","description":"activate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},{"name":"deactivate_tools","description":"deactivate","inputSchema":{"type":"object","properties":{"tool_names":{"type":"array"}},"required":["tool_names"]}},%s,%s,%s,%s,%s,%s,%s]}}\n' "$id" "$(tool etf_info)" "$(tool equity_fundamental_filings)" "$(tool equity_fundamental_income)" "$(tool equity_price_historical)" "$(tool equity_fundamental_dividends)" "$(tool etf_historical)" "$(tool etf_holdings)"
      ;;
    *'"method":"tools/call"'*'"symbol":"QQQF"'*'"name":"etf_info"'*)
      reply "$id" '{"symbol":"QQQF","name":"Fictional Fund","net_expense_ratio":0.002}'
      ;;
    *'"method":"tools/call"'*'"symbol":"MORR"'*'"name":"equity_fundamental_filings"'*)
      calls=$(grep '"name":"equity_fundamental_filings"' "$MCP_TEST_TRANSCRIPT" | grep -c '"symbol":"MORR"')
      tenk='{"filing_date":"2026-02-20","report_type":"10-K","report_url":"https://www.sec.gov/Archives/edgar/data/990001/000099000126000004/morr-20251231.htm","accession_number":"0000990001-26-000004","primary_doc_description":"Annual report"}'
      tenq='{"filing_date":"2026-04-30","report_type":"10-Q","report_url":"https://www.sec.gov/Archives/edgar/data/990001/000099000126000011/morr-20260328.htm","accession_number":"0000990001-26-000011","primary_doc_description":"Quarterly report"}'
      if [ "$calls" = 1 ]; then reply "$id" "$tenk"; else reply "$id" "$tenq,$tenk"; fi
      ;;
    *'"method":"tools/call"'*'"name":"equity_fundamental_income"'*)
      printf '{"jsonrpc":"2.0","id":"%s","result":{"content":[{"type":"text","text":"provider unavailable"}],"isError":true}}\n' "$id"
      ;;
    *'"method":"tools/call"'*'"name":"equity_price_historical"'*|*'"method":"tools/call"'*'"name":"etf_historical"'*)
      reply "$id" '{"date":"2026-04-28","close":41.0},{"date":"2026-04-29","close":42.5}'
      ;;
    *'"method":"tools/call"'*)
      reply "$id" ''
      ;;
  esac
done
"#;
}
