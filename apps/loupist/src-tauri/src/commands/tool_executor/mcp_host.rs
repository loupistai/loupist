mod custom;
mod errors;
mod openbb;
mod point_in_time;

use errors::{
    map_mcp_error, mcp_call_error_with_inventory, rejected_mcp_result, stopped_mcp_response,
};

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};

use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Mutex, MutexGuard};

use super::*;
use crate::{
    fs::artifact_trust::{create_private_directory, ensure_private_directory},
    marketplace::{bundled_catalog, MarketplaceEntryDto, MarketplaceRuntimeKind},
    mcp::pool::{authority_fingerprint, McpPoolKey, TurnMcpServer},
    mcp::{
        contains_protected_value, filter_tool_providers, tool_to_agent_schema,
        validate_result_provider, BundledMcpRuntime, McpError, McpLaunchConfig, McpSession,
        McpTool, McpToolActivation, ProviderlessToolPolicy,
    },
};

const MCP_CALL_TIMEOUT: Duration = Duration::from_secs(45);
const MCP_SHUTDOWN_GRACE: Duration = Duration::from_secs(2);

type SharedMcpServer = Arc<Mutex<Option<TurnMcpServer>>>;

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
    by_server: BTreeMap<String, SharedMcpServer>,
    closed: bool,
}

impl McpTurnSessions {
    fn get(&self, server_id: &str) -> Option<SharedMcpServer> {
        self.by_server.get(server_id).cloned()
    }

    fn insert_shared_if_absent(
        &mut self,
        server_id: &str,
        server: SharedMcpServer,
    ) -> Result<Option<SharedMcpServer>, ()> {
        if self.closed {
            return Err(());
        }
        if let Some(existing) = self.by_server.get(server_id) {
            return Ok(Some(existing.clone()));
        }
        self.by_server.insert(server_id.to_owned(), server);
        Ok(None)
    }

    fn remove_if_current(&mut self, server_id: &str, expected: &SharedMcpServer) {
        if self
            .by_server
            .get(server_id)
            .is_some_and(|current| Arc::ptr_eq(current, expected))
        {
            self.by_server.remove(server_id);
        }
    }

    fn close_and_take_all(&mut self) -> Vec<SharedMcpServer> {
        self.closed = true;
        std::mem::take(&mut self.by_server).into_values().collect()
    }
}

enum McpServerInstall {
    Installed,
    Occupied {
        tools: Vec<Value>,
        candidate: Box<TurnMcpServer>,
    },
}

struct McpRunAuthority {
    runtime: BundledMcpRuntime,
    enabled_provider_ids: BTreeSet<String>,
    allowed_network_hosts: BTreeSet<String>,
    credentials: BTreeMap<String, String>,
    provider_config: BTreeMap<String, BTreeMap<String, String>>,
    /// Model-facing notes for granted providers that cannot run until set up.
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
fn activation_names(requested: &[String], control_tool_names: &BTreeSet<String>) -> Vec<String> {
    let restored = requested
        .iter()
        .filter(|name| !control_tool_names.contains(*name))
        .cloned()
        .collect::<Vec<_>>();
    if restored.is_empty() {
        openbb::DEFAULT_ACTIVE_TOOLS
            .iter()
            .map(|name| (*name).to_owned())
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
    /// A checked result, with the inventory refreshed after the call.
    Answered {
        payload: Value,
        tools: Option<Vec<Value>>,
    },
    /// The call failed and the session stays.
    Failed(BrokerError),
    /// The session can no longer be trusted and is discarded.
    Stopped(Option<BrokerError>),
}

/// Invocation: forwards the admitted call and checks what came back. A result
/// from a provider the call did not admit, or one echoing a credential, stops
/// the session; a cutoff read keeps only rows on or before the cutoff.
async fn invoke_mcp_tool(
    request: &McpToolCallRequest,
    server: &mut TurnMcpServer,
    plan: &McpCallPlan,
) -> Result<McpInvocation, BrokerError> {
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
            let mapped = map_mcp_error(error);
            if terminal {
                return Ok(McpInvocation::Stopped(Some(mapped)));
            }
            return Ok(McpInvocation::Failed(provider_failure(
                mapped,
                plan.requested_provider.as_deref(),
                &server.enabled_provider_ids,
            )));
        }
    };
    if result.is_error {
        return Ok(McpInvocation::Failed(provider_failure(
            rejected_mcp_result(&result),
            plan.requested_provider.as_deref(),
            &server.enabled_provider_ids,
        )));
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
    let as_of = match plan.dated {
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
    if let Some(as_of) = as_of {
        payload["as_of"] = as_of;
    }
    let list_changed = server.session.take_tools_changed();
    let refresh = plan.control_tool || list_changed;
    Ok(
        match refresh_inventory(&request.server_id, server, refresh).await {
            Ok(tools) => McpInvocation::Answered { payload, tools },
            Err(_) => McpInvocation::Stopped(None),
        },
    )
}

impl ToolCapture {
    pub(crate) async fn shutdown_mcp(&self) {
        let sessions = {
            let mut sessions = self.mcp_sessions.lock().await;
            sessions.close_and_take_all()
        };
        let mut servers = Vec::with_capacity(sessions.len());
        for session in sessions {
            if let Some(server) = session.lock().await.take() {
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

    async fn mcp_session(&self, server_id: &str) -> Option<SharedMcpServer> {
        self.mcp_sessions.lock().await.get(server_id)
    }

    async fn remove_mcp_session(&self, server_id: &str, expected: &SharedMcpServer) {
        self.mcp_sessions
            .lock()
            .await
            .remove_if_current(server_id, expected);
    }

    async fn discard_mcp_session(
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
    ) -> Result<McpServerInstall, BrokerError> {
        let candidate = Arc::new(Mutex::new(Some(server)));
        loop {
            let existing = self
                .mcp_sessions
                .lock()
                .await
                .insert_shared_if_absent(server_id, candidate.clone());
            let existing = match existing {
                Ok(existing) => existing,
                Err(()) => {
                    let candidate = candidate
                        .lock()
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
            let tools = {
                let existing = existing.lock().await;
                existing
                    .as_ref()
                    .map(|server| server_cards(server_id, server))
            };
            if let Some(tools) = tools {
                let candidate = candidate
                    .lock()
                    .await
                    .take()
                    .expect("unpublished MCP candidate disappeared");
                return match tools {
                    Ok(tools) => Ok(McpServerInstall::Occupied {
                        tools,
                        candidate: Box::new(candidate),
                    }),
                    Err(error) => {
                        candidate.discard().await;
                        Err(error)
                    }
                };
            }
            self.remove_mcp_session(server_id, &existing).await;
        }
    }
}

impl AppToolExecutor {
    pub(super) async fn mcp_connect(&self, params: Value) -> Result<Value, BrokerError> {
        let request = McpConnectRequest::parse(params)?;
        if crate::custom_connections::service::valid_id(&request.server_id) {
            return self.custom_mcp_connect(&request.server_id).await;
        }
        if let Some(tools) = self.connected_mcp_tools(&request.server_id).await? {
            return Ok(json!({
                "server_id": request.server_id,
                "tools": tools,
            }));
        }
        let authority = self.mcp_run_authority(&request.server_id).await?;
        if let Some(tools) = self.connected_mcp_tools(&request.server_id).await? {
            return Ok(json!({
                "server_id": request.server_id,
                "tools": tools,
            }));
        }

        self.attach_or_spawn_mcp(&request.server_id, authority, &request.activate_tool_names)
            .await
    }

    async fn connected_mcp_tools(
        &self,
        server_id: &str,
    ) -> Result<Option<Vec<Value>>, BrokerError> {
        let Some(session) = self.capture.mcp_session(server_id).await else {
            return Ok(None);
        };
        let mut guard = session.lock().await;
        let Some(server) = guard.as_mut() else {
            drop(guard);
            self.capture.remove_mcp_session(server_id, &session).await;
            return Ok(None);
        };
        let cached = match server.session.is_running().await {
            Ok(false) => Ok(None),
            Err(error) => Err(map_mcp_error(error)),
            Ok(true) => match refresh_inventory(server_id, server, false).await {
                Ok(_) => server_cards(server_id, server).map(Some),
                Err(error) => Err(error),
            },
        };
        if matches!(&cached, Ok(Some(_))) {
            return cached;
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
        let notices = authority.setup_notices.clone();
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
                    Ok(tools) => match self.capture.install_mcp_session(server_id, server).await? {
                        McpServerInstall::Installed => {
                            return Ok(connect_response(server_id, tools, &notices));
                        }
                        McpServerInstall::Occupied { tools, candidate } => {
                            pool.release(*candidate).await;
                            return Ok(connect_response(server_id, tools, &notices));
                        }
                    },
                    Err(_) => {
                        server.discard().await;
                    }
                }
            }
        }

        let scratch = match &self.capture.mcp_pool {
            Some(pool) => pool
                .create_scratch(&self.loupist_id, server_id)
                .map_err(|_| BrokerError::Execution("MCP run isolation failed".into()))?,
            None => {
                let scratch_root = self.capture.mcp_scratch_root.as_ref().ok_or_else(|| {
                    BrokerError::Execution("MCP run isolation is unavailable".into())
                })?;
                ensure_private_directory(scratch_root)
                    .map_err(|_| BrokerError::Execution("MCP run isolation failed".into()))?;
                let scratch =
                    scratch_root.join(format!("{}-{}", server_id, uuid::Uuid::new_v4().simple()));
                create_private_directory(&scratch)
                    .map_err(|_| BrokerError::Execution("MCP run isolation failed".into()))?;
                scratch
            }
        };
        let sensitive_values = protected_values(
            &authority.credentials,
            &self.state.artifacts.app_data_dir,
            &scratch,
        );
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
            "settings": {
                "allowed_categories": authority.runtime.allowed_categories.clone(),
                "enabled_provider_ids": enabled_provider_ids.clone(),
                "allowed_network_hosts": authority.allowed_network_hosts,
                "provider_config": authority.provider_config,
            }
        });
        let launch = McpLaunchConfig {
            server_id: server_id.to_owned(),
            executable: authority.runtime.executable,
            arguments: Vec::new(),
            private_working_dir: scratch.clone(),
            lease_dir: authority.runtime.lease_dir,
            environment: BTreeMap::new(),
            bootstrap,
        };
        // Spawn without holding the session map. OpenBB cold start is several
        // seconds; blocking the map would stall shutdown and a second connect.
        let (session, initial_tools) = McpSession::spawn(launch).await.map_err(map_mcp_error)?;
        if let Err(error) =
            ensure_inventory_contains_no_protected_values(&initial_tools, &sensitive_values)
        {
            let _ = session.shutdown(MCP_SHUTDOWN_GRACE).await;
            let _ = tokio::fs::remove_dir_all(&scratch).await;
            return Err(error);
        }
        let initial_tool_names = initial_tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<BTreeSet<_>>();
        if initial_tool_names != control_tool_names {
            let _ = session.shutdown(MCP_SHUTDOWN_GRACE).await;
            let _ = tokio::fs::remove_dir_all(&scratch).await;
            return Err(BrokerError::Execution(
                "MCP runtime initial inventory did not match its signed control surface".into(),
            ));
        }
        let tools = filter_inventory(
            &initial_tools,
            &control_tool_names,
            &enabled_provider_ids,
            &providerless_tool_policy,
        )?;
        let cards = agent_tool_cards(
            server_id,
            tools.values(),
            &control_tool_names,
            &enabled_provider_ids,
            &providerless_tool_policy,
        )?;
        let mut server = TurnMcpServer {
            session,
            control_tool_names,
            tools,
            enabled_provider_ids,
            providerless_tool_policy,
            provider_receipt_pointer,
            sensitive_values,
            pool_key,
            scratch_dir: Some(scratch),
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
        match self.capture.install_mcp_session(server_id, server).await? {
            McpServerInstall::Installed => Ok(connect_response(server_id, cards, &notices)),
            McpServerInstall::Occupied { tools, candidate } => {
                (*candidate).discard().await;
                Ok(connect_response(server_id, tools, &notices))
            }
        }
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
        let request: McpToolCallRequest =
            serde_json::from_value(params).map_err(|_| BrokerError::Malformed)?;
        let cutoff = parse_as_of_date(policy.as_of.as_deref())?;
        if crate::custom_connections::service::valid_id(&request.server_id) {
            // A custom server's rows carry no date the host can check against a cutoff.
            if let Some(cutoff) = cutoff {
                return Err(point_in_time::refusal(&request.tool_name, cutoff));
            }
            return self.custom_mcp_call(request, delivery_id).await;
        }
        let Some(session) = self.capture.mcp_session(&request.server_id).await else {
            return Ok(stopped_mcp_response(None));
        };
        let (mut session_guard, preflight_tools) = match self
            .lock_running_mcp_session(&request.server_id, &session)
            .await
        {
            Ok(locked) => locked,
            Err(stopped) => return Ok(stopped),
        };
        let server = session_guard
            .as_mut()
            .expect("loaded MCP session disappeared after refresh");
        let plan = match self.admit_mcp_call(&request, server, cutoff) {
            Ok(plan) => plan,
            Err(error) => return mcp_call_error_with_inventory(preflight_tools, error),
        };
        let reused = match plan.max_age {
            Some(max_age) if !plan.control_tool => self
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
                    let stopped = session_guard.take();
                    drop(session_guard);
                    self.capture
                        .discard_mcp_session(&request.server_id, &session, stopped)
                        .await;
                    return Ok(stopped_mcp_response(error.as_ref()));
                }
            }
        };
        let payload = if plan.control_tool {
            payload
        } else {
            let reused = retained.is_some();
            let compact = payload.to_string().len() > 16 * 1024;
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
            let mut model_payload =
                self.shape_mcp_read(&mut payload, &reference, fetched_at, reused, compact)?;
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
            attach_result_ref(&mut payload, &result_ref);
            if let Some(delivered) = &mut model_payload {
                attach_result_ref(delivered, &result_ref);
            }
            model_payload.unwrap_or(payload)
        };
        Ok(match tools {
            Some(tools) => json!({ "result": payload, "tools": tools }),
            None => json!({ "result": payload }),
        })
    }

    /// Locks this turn's session for `server_id` and refreshes its inventory.
    /// A missing, stopped or failing server leaves the turn, answered with the
    /// stopped response the caller returns as is.
    async fn lock_running_mcp_session<'s>(
        &self,
        server_id: &str,
        session: &'s SharedMcpServer,
    ) -> Result<(MutexGuard<'s, Option<TurnMcpServer>>, Option<Vec<Value>>), Value> {
        let mut session_guard = session.lock().await;
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
            .dataset_describe(&self.loupist_id, &reference.dataset_id, reference.revision)
            .map_err(command_error)?;
        let mut delivered = json!({"dataset_ref":reference,"fetched_at":payload["fetched_at"],"data_summary":description,"model_delivery":{"coverage":"bounded_sample","retained":"complete"}});
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

    /// The provider a host read uses: the requested one when this Loupist enabled
    /// it and the tool accepts it, else the first preferred provider that is.
    pub(super) async fn host_provider(
        &self,
        server_id: &str,
        tool: &str,
        requested: Option<&str>,
        preferred: &[&str],
    ) -> Option<String> {
        let session = self.capture.mcp_session(server_id).await?;
        let guard = session.lock().await;
        let server = guard.as_ref()?;
        let schema = server
            .tools
            .get(tool)?
            .input_schema
            .pointer("/properties/provider")?;
        let accepted: Vec<String> = schema
            .get("enum")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let allowed = |provider: &str| {
            server.enabled_provider_ids.contains(provider)
                && (accepted.is_empty() || accepted.iter().any(|item| item == provider))
        };
        if let Some(requested) = requested.filter(|provider| allowed(provider)) {
            return Some(requested.to_owned());
        }
        preferred
            .iter()
            .find(|provider| allowed(provider))
            .map(|provider| (*provider).to_owned())
            .or_else(|| {
                accepted
                    .into_iter()
                    .find(|provider| server.enabled_provider_ids.contains(provider))
            })
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
        let runtime = self
            .state
            .artifacts
            .mcp_runtimes
            .get(server_id)
            .cloned()
            .ok_or_else(|| BrokerError::Execution("MCP runtime is unavailable".into()))?;
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
            return Err(BrokerError::MethodDenied);
        }
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
            .map(|entry| (entry.id.clone(), entry.runtime.credential_mapping.clone()))
            .collect::<Vec<_>>();
        let credential_environment = self.state.credential_environment.clone();
        let credentials = tokio::task::spawn_blocking(move || {
            let mut credentials = BTreeMap::new();
            for (entry_id, mapping) in credential_specs {
                let bundle = crate::finance_credentials::get(&credential_environment, &entry_id)
                    .map_err(|_| ())?
                    .ok_or(())?;
                for (field_id, target_id) in mapping {
                    let secret = bundle.get(&field_id).ok_or(())?.to_owned();
                    if credentials.insert(target_id, secret).is_some() {
                        return Err(());
                    }
                }
            }
            Ok::<_, ()>(credentials)
        })
        .await
        .map_err(|_| BrokerError::Execution("credential lookup failed".into()))?
        .map_err(|_| BrokerError::Execution("credential lookup failed".into()))?;

        let provider_config =
            collect_server_provider_config(server_id, &catalog.entries, |entry_id, field_id| {
                crate::marketplace::connector_config_value(&self.state, entry_id, field_id)
                    .map_err(|_| BrokerError::Execution("connector setup is unavailable".into()))
            })?;
        // A manifest-keyless provider can still require non-secret operational
        // configuration (SEC contact identity is the current case). Keep it in
        // the platform catalog, but do not expose or authorize it for a run
        // until every manifest-declared config field is present.
        let enabled_provider_ids = configured_provider_ids(
            requested_provider_ids.clone(),
            &runtime.provider_config_fields,
            &provider_config,
        );
        let setup_notices = setup_notices(
            &catalog.entries,
            server_id,
            requested_provider_ids.difference(&enabled_provider_ids),
        );
        if enabled_provider_ids.is_empty() {
            return Err(BrokerError::Execution(if setup_notices.is_empty() {
                "MCP provider setup is incomplete".into()
            } else {
                setup_notices.join(" ")
            })
            .with_next(PROVIDER_SETUP_NEXT));
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
            let fields = entry
                .setup
                .as_ref()
                .map(|setup| {
                    setup
                        .config_fields
                        .iter()
                        .map(|field| field.label.clone())
                        .collect::<Vec<_>>()
                })
                .filter(|labels| !labels.is_empty())
                .map(|labels| labels.join(" and "))
                .unwrap_or_else(|| "its setup".into());
            Some(format!(
                "{name} (provider {provider}) is unavailable until the user adds the {fields}: if connection_setup is available, call it with {{id: \"{id}\"}} and the user finishes it in a card; otherwise point them to Connections. Until then do not present another provider's data as {name} data.",
                name = entry.name,
                id = entry.id,
            ))
        })
        .collect()
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
        json!(activation_names(requested, &server.control_tool_names)),
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

/// Scraped providers get a passive health signal: the first failure in a
/// turn tells the model to switch to an official provider instead of retrying.
fn provider_failure(
    error: BrokerError,
    provider: Option<&str>,
    enabled_provider_ids: &BTreeSet<String>,
) -> BrokerError {
    if !provider.is_some_and(openbb::is_best_effort) {
        return error;
    }
    let source = match error {
        BrokerError::WithNext { source, .. } => *source,
        other => other,
    };
    source.with_next(openbb::best_effort_failure_next(enabled_provider_ids))
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
                required.is_empty()
                    || provider_config.get(provider_id).is_some_and(|values| {
                        required.iter().all(|field| values.contains_key(field))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    use std::{fs, path::PathBuf, sync::Arc};

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
            capture.install_mcp_session("openbb", server).await,
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
            configured_provider_ids(requested, &required, &BTreeMap::new()),
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
        let config =
            collect_server_provider_config("openbb", &catalog.entries, |entry_id, field_id| {
                Ok(stored
                    .get(entry_id)
                    .and_then(|values| values.get(field_id))
                    .cloned())
            })
            .unwrap();
        assert_eq!(config["sec"]["contact_email"], "research@example.com");

        // Without the email the SEC provider is dropped and named, not silently lost,
        // and the other granted providers keep running.
        let missing =
            collect_server_provider_config("openbb", &catalog.entries, |_, _| Ok(None)).unwrap();
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
            .install_mcp_session("openbb", late.server)
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
        assert!(!response.to_string().contains("12.5"), "{response}");
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
        assert_eq!(activation_names(&requested, &control), ["etf_holdings"]);
        assert_eq!(
            activation_names(&["activate_tools".to_owned()], &control),
            openbb::DEFAULT_ACTIVE_TOOLS
        );
        assert_eq!(
            activation_names(&[], &control),
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
    fn a_failed_scraped_provider_points_to_an_enabled_official_one() {
        let generic = || BrokerError::Execution("provider failed".into()).with_next("generic");
        let enabled = BTreeSet::from(["yfinance".to_owned(), "tmx".to_owned()]);
        let BrokerError::WithNext { next, .. } =
            provider_failure(generic(), Some("yfinance"), &enabled)
        else {
            panic!("expected recovery guidance");
        };
        assert!(next.contains("for example tmx"), "{next}");
        assert!(!next.contains("sec"), "{next}");
        let BrokerError::WithNext { next, .. } = provider_failure(generic(), Some("sec"), &enabled)
        else {
            panic!("expected recovery guidance");
        };
        assert_eq!(next, "generic");
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

    /// A market-data server for host refreshes: SEC filings for MORR (a 10-K on
    /// the first call, then a new 10-Q too), an income read that fails, closes,
    /// and `etf_info` that knows only the fund QQQF.
    #[cfg(unix)]
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

    /// `refresh_asset`'s own path, no model: the grant check, per-source
    /// statuses, the first-refresh fund lookup, statuses reset when the class
    /// changes, and the eval regression (finance-asset-page.md §8) that a
    /// watched company shows a dated, sourced delta after a new filing.
    #[cfg(unix)]
    #[tokio::test]
    async fn asset_refresh_checks_grants_records_statuses_and_dated_sourced_filings() {
        use crate::commands::assets::{refresh_with, with_assets};
        use crate::knowledge::assets;
        let _guard = crate::mcp::MCP_PROCESS_TEST_LOCK.lock().await;
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        let turn = capture(&temporary, "refresh", None);
        let spawned =
            spawn_script_server(&temporary, "refresh", "sec", None, ASSET_REFRESH_SCRIPT).await;
        install(&turn, spawned.server).await;
        let executor = turn_executor(&temporary, &state, "loupist-a", turn.clone());
        let openbb: BTreeSet<String> = crate::marketplace::bundled_catalog()
            .unwrap()
            .entries
            .into_iter()
            .filter(|entry| entry.runtime.server_id.as_deref() == Some("openbb"))
            .map(|entry| entry.id)
            .take(1)
            .collect();
        let page = |symbol: &str, name: &str| {
            with_assets(&state, "loupist-a", true, |tx| {
                assets::ensure_page(
                    tx,
                    "loupist-a",
                    &assets::AssetIdentity {
                        symbol: symbol.into(),
                        market: "NASDAQ".into(),
                        name: name.into(),
                    },
                    None,
                    crate::commands::now_ms(),
                )
            })
            .unwrap()
        };
        let view = |asset_id: &str| {
            let context = crate::commands::assets::view_context(&state).unwrap();
            with_assets(&state, "loupist-a", false, |tx| {
                assets::page_view(tx, "loupist-a", asset_id, 0, &context)
            })
            .unwrap()
        };
        let failures = |asset_id: &str| {
            view(asset_id)
                .checks
                .failures
                .into_iter()
                .map(|failure| (failure.kind, failure.reason))
                .collect::<Vec<_>>()
        };
        let calls = || {
            fs::read_to_string(&spawned.transcript)
                .unwrap_or_default()
                .matches(r#""method":"tools/call""#)
                .count()
        };
        let morrow = page("MORR", "Morrow Components");
        let refresh = |asset_id: String, grants: BTreeSet<String>| {
            let state = state.clone();
            let executor = &executor;
            async move {
                // No reuse window, so each refresh reaches the server.
                let reads = crate::commands::assets::HostReads {
                    executor,
                    capabilities: &grants,
                    max_age: None,
                };
                refresh_with(
                    &state,
                    "loupist-a",
                    &asset_id,
                    assets::ObservationOrigin::Refresh,
                    reads,
                    None,
                )
                .await
                .unwrap()
            }
        };

        // Without a grant nothing is read, and every check says which connection to turn on.
        assert_eq!(refresh(morrow.asset_id.clone(), BTreeSet::new()).await, 0);
        assert_eq!(calls(), 0);
        let waiting = failures(&morrow.asset_id);
        assert_eq!(waiting.len(), 3, "{waiting:?}");
        assert!(waiting
            .iter()
            .all(|(_, reason)| reason == "needs_connection"));
        assert!(view(&morrow.asset_id).refreshed_at_ms.is_some());

        // Granted: a baseline, the fund lookup says "not a fund", the failed read is named.
        let first_at = crate::commands::now_ms();
        assert_eq!(refresh(morrow.asset_id.clone(), openbb.clone()).await, 0);
        let checked = view(&morrow.asset_id);
        assert_eq!(
            (checked.class, checked.class_source),
            (assets::AssetClass::Equity, assets::ClassSource::Lookup)
        );
        assert_eq!(
            failures(&morrow.asset_id),
            [("results".to_owned(), "failed".to_owned())]
        );
        // A new 10-Q is one new row; an identical refresh adds nothing.
        assert_eq!(refresh(morrow.asset_id.clone(), openbb.clone()).await, 1);
        assert_eq!(refresh(morrow.asset_id.clone(), openbb.clone()).await, 0);
        let delta = with_assets(&state, "loupist-a", false, |tx| {
            assets::deltas(tx, "loupist-a", Some(&morrow.asset_id), first_at + 1)
        })
        .unwrap()
        .remove(0);
        let filings: Vec<_> = delta
            .rows
            .iter()
            .filter(|row| row.kind == "filing")
            .collect();
        assert_eq!(filings.len(), 1, "{:?}", delta.rows);
        let row = filings[0];
        assert_eq!(
            (row.fact_date.as_str(), row.facts["form"].as_str()),
            ("2026-04-30", Some("10-Q"))
        );
        assert_eq!(
            row.evidence[0].url.as_deref(),
            Some("https://www.sec.gov/Archives/edgar/data/990001/000099000126000011/morr-20260328.htm")
        );
        assert!(row.evidence[0].dataset.is_some());
        assert_eq!(row.evidence[0].provider.as_deref(), Some("sec"));
        // The 10-Q is a quarterly report whose sections are not compared yet.
        assert_eq!(row.sections.as_deref(), Some(&[][..]));

        // A fund found by the lookup becomes a fund page; the equity checks'
        // statuses (the failed results read) no longer apply to it.
        let fund = page("QQQF", "Fictional Fund");
        refresh(fund.asset_id.clone(), openbb.clone()).await;
        let checked = view(&fund.asset_id);
        assert_eq!(
            (checked.class, checked.class_source),
            (assets::AssetClass::Fund, assets::ClassSource::Lookup)
        );
        assert!(
            failures(&fund.asset_id).is_empty(),
            "{:?}",
            failures(&fund.asset_id)
        );
        assert_eq!(checked.checks.kinds, ["price", "fees", "holdings"]);
        turn.shutdown_mcp().await;
        wait_for_exit_marker(&spawned.exited).await;
    }
}
