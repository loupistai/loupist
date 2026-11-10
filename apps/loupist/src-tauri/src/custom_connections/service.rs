use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::{
    auth,
    config::{self, ParsedConnection, TransportConfig},
    credentials::{self, Secrets},
    http::{self, HttpTransport, Reply},
    policy::{self, ApprovalChoice, Approvals, ToolEligibility},
    ConnectionError,
};
use crate::{
    app::AppState,
    fs::artifact_trust::{ensure_private_directory, ensure_private_regular_file},
    mcp::{contains_protected_value, McpLaunchConfig, McpSession, McpTool},
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum AuthState {
    NotRequired,
    SignedIn,
    SignInNeeded,
    InProgress,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ConnectionRecord {
    pub id: String,
    pub name: String,
    pub transport: TransportConfig,
    pub dont_save_to_memory: bool,
    pub pending: bool,
    pub enabled_loupists: BTreeSet<String>,
    pub tools: Vec<McpTool>,
    pub env_keys: Vec<String>,
    pub header_names: Vec<String>,
    pub last_error: Option<String>,
    pub needs_review: bool,
    pub auth_state: AuthState,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ToolCard {
    pub definition_changed: bool,
    pub name: String,
    /// The UI must label this as the server's own text, never host guidance.
    pub server_description: Option<String>,
    pub input_schema: Value,
    pub annotations: Option<Value>,
    pub definition_hash: String,
    pub eligibility: ToolEligibility,
    pub approval: Option<ApprovalChoice>,
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "CustomConnection"))]
pub struct ConnectionView {
    pub id: String,
    pub kind: &'static str,
    pub name: String,
    pub transport: TransportConfig,
    pub command_line: Option<String>,
    pub dont_save_to_memory: bool,
    pub pending: bool,
    pub enabled: bool,
    pub tools: Vec<ToolCard>,
    pub env_keys: Vec<String>,
    pub header_names: Vec<String>,
    pub last_error: Option<String>,
    pub needs_review: bool,
    pub auth_state: AuthState,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct StoredConnections {
    connections: BTreeMap<String, ConnectionRecord>,
    approvals: Approvals,
}

enum Session {
    Http(HttpTransport),
    Stdio(McpSession),
}

pub struct Service {
    root: PathBuf,
    credential_environment: String,
    stored: StoredConnections,
    sessions: BTreeMap<String, Session>,
}

pub type SharedService = Arc<tokio::sync::Mutex<Service>>;

pub fn for_state(state: &AppState) -> Result<SharedService, ConnectionError> {
    static SERVICES: OnceLock<Mutex<BTreeMap<PathBuf, SharedService>>> = OnceLock::new();
    let mut services = SERVICES
        .get_or_init(Default::default)
        .lock()
        .map_err(|_| ConnectionError::Configuration)?;
    let root = state.artifacts.app_data_dir.join("custom-connections");
    if let Some(service) = services.get(&root) {
        return Ok(service.clone());
    }
    let service = Arc::new(tokio::sync::Mutex::new(Service::open(
        root.clone(),
        state.credential_environment.to_string(),
    )?));
    services.insert(root, service.clone());
    Ok(service)
}

fn load_stored(root: &Path) -> Result<StoredConnections, ConnectionError> {
    let file = root.join("connections.json");
    if !file
        .try_exists()
        .map_err(|_| ConnectionError::Configuration)?
    {
        return Ok(StoredConnections::default());
    }
    ensure_private_regular_file(&file).map_err(|_| ConnectionError::Configuration)?;
    let metadata = std::fs::metadata(&file).map_err(|_| ConnectionError::Configuration)?;
    if metadata.len() > 16 * 1024 * 1024 {
        return Err(ConnectionError::Configuration);
    }
    let encoded = std::fs::read(file).map_err(|_| ConnectionError::Configuration)?;
    let stored: StoredConnections =
        serde_json::from_slice(&encoded).map_err(|_| ConnectionError::Configuration)?;
    if stored.connections.len() > 32 {
        return Err(ConnectionError::Configuration);
    }
    for (id, connection) in &stored.connections {
        if !valid_id(id) || &connection.id != id {
            return Err(ConnectionError::Configuration);
        }
        config::validate_name(&connection.name)?;
        match &connection.transport {
            TransportConfig::Http { url } => {
                http::canonical_resource(url)?;
            }
            TransportConfig::Stdio { command, args } => config::validate_command(command, args)?,
        }
    }
    Ok(stored)
}

pub fn records_for_state(state: &AppState) -> Result<Vec<ConnectionRecord>, ConnectionError> {
    Ok(
        load_stored(&state.artifacts.app_data_dir.join("custom-connections"))?
            .connections
            .into_values()
            .collect(),
    )
}

pub fn views_for_state(
    state: &AppState,
    loupist: Option<&str>,
) -> Result<Vec<ConnectionView>, ConnectionError> {
    let root = state.artifacts.app_data_dir.join("custom-connections");
    let service = Service {
        stored: load_stored(&root)?,
        root,
        credential_environment: state.credential_environment.to_string(),
        sessions: BTreeMap::new(),
    };
    Ok(service.list(loupist))
}

/// Every stored secret of every custom connection (headers, environment,
/// client secrets and tokens), for the sharing private-string scan. Values
/// stay in Rust.
pub(crate) fn protected_values_for_state(state: &AppState) -> Vec<String> {
    let Ok(records) = records_for_state(state) else {
        return Vec::new();
    };
    records
        .iter()
        .filter_map(|record| credentials::load(&state.credential_environment, &record.id).ok())
        .flat_map(|secrets| secrets.protected_values())
        .collect()
}

pub fn valid_id(id: &str) -> bool {
    id.strip_prefix("custom-")
        .is_some_and(|suffix| suffix.len() == 32 && suffix.bytes().all(|b| b.is_ascii_hexdigit()))
}

impl Service {
    pub fn open(root: PathBuf, credential_environment: String) -> Result<Self, ConnectionError> {
        ensure_private_directory(&root).map_err(|_| ConnectionError::Configuration)?;
        let mut stored = load_stored(&root)?;
        let mut interrupted = false;
        for connection in stored.connections.values_mut() {
            if connection.auth_state == AuthState::InProgress {
                connection.auth_state = AuthState::SignInNeeded;
                interrupted = true;
            }
        }
        let mut service = Self {
            root,
            credential_environment,
            stored,
            sessions: BTreeMap::new(),
        };
        if interrupted {
            service.persist()?;
        }
        Ok(service)
    }

    fn persist(&mut self) -> Result<(), ConnectionError> {
        let result = self.persist_inner();
        if result.is_err() {
            // A failed persistence operation cannot leave a widened in-memory grant.
            for connection in self.stored.connections.values_mut() {
                connection.pending = true;
            }
            self.sessions.clear();
        }
        result
    }

    fn persist_inner(&self) -> Result<(), ConnectionError> {
        let encoded =
            serde_json::to_vec(&self.stored).map_err(|_| ConnectionError::Configuration)?;
        if encoded.len() > 16 * 1024 * 1024 {
            return Err(ConnectionError::Configuration);
        }
        let mut file = tempfile::NamedTempFile::new_in(&self.root)
            .map_err(|_| ConnectionError::Configuration)?;
        file.write_all(&encoded)
            .map_err(|_| ConnectionError::Configuration)?;
        file.as_file()
            .sync_all()
            .map_err(|_| ConnectionError::Configuration)?;
        file.persist(self.root.join("connections.json"))
            .map_err(|_| ConnectionError::Configuration)?;
        Ok(())
    }

    pub fn list(&self, loupist: Option<&str>) -> Vec<ConnectionView> {
        self.stored
            .connections
            .values()
            .map(|record| self.view(record, loupist))
            .collect()
    }

    pub fn get(&self, id: &str, loupist: Option<&str>) -> Result<ConnectionView, ConnectionError> {
        Ok(self.view(self.record(id)?, loupist))
    }

    fn record(&self, id: &str) -> Result<&ConnectionRecord, ConnectionError> {
        self.stored
            .connections
            .get(id)
            .ok_or(ConnectionError::Configuration)
    }

    fn view(&self, record: &ConnectionRecord, loupist: Option<&str>) -> ConnectionView {
        ConnectionView {
            id: record.id.clone(),
            kind: "custom",
            name: record.name.clone(),
            transport: record.transport.clone(),
            command_line: record.transport.command_line(),
            dont_save_to_memory: record.dont_save_to_memory,
            pending: record.pending,
            enabled: loupist.is_some_and(|g| record.enabled_loupists.contains(g)),
            tools: record
                .tools
                .iter()
                .map(|tool| ToolCard {
                    name: tool.name.clone(),
                    server_description: tool.description.clone(),
                    input_schema: tool.input_schema.clone(),
                    annotations: tool.annotations.clone(),
                    definition_hash: policy::definition_hash(tool),
                    eligibility: policy::eligibility(tool),
                    approval: loupist.and_then(|loupist| {
                        self.stored.approvals.choice(loupist, &record.id, tool)
                    }),
                    definition_changed: loupist.is_some_and(|loupist| {
                        self.stored
                            .approvals
                            .definition_changed(loupist, &record.id, tool)
                    }),
                })
                .collect(),
            env_keys: record.env_keys.clone(),
            header_names: record.header_names.clone(),
            last_error: record.last_error.clone(),
            needs_review: record.needs_review
                || loupist.is_some_and(|loupist| {
                    record.tools.iter().any(|tool| {
                        policy::eligibility(tool) == ToolEligibility::ReadOnly
                            && self
                                .stored
                                .approvals
                                .choice(loupist, &record.id, tool)
                                .is_none()
                    })
                }),
            auth_state: record.auth_state,
        }
    }

    pub fn add_pending(
        &mut self,
        parsed: ParsedConnection,
    ) -> Result<ConnectionView, ConnectionError> {
        if self.stored.connections.len() >= 32 {
            return Err(ConnectionError::Configuration);
        }
        config::validate_name(&parsed.name)?;
        config::validate_secrets(&parsed.secrets)?;
        match &parsed.transport {
            TransportConfig::Http { url } => {
                http::canonical_resource(url)?;
            }
            TransportConfig::Stdio { command, args } => config::validate_command(command, args)?,
        }
        if contains_protected_value(
            &json!({"name":parsed.name,"transport":parsed.transport}),
            &parsed.secrets.protected_values(),
        ) {
            return Err(ConnectionError::Configuration);
        }
        let id = format!("custom-{}", uuid::Uuid::new_v4().simple());
        credentials::save(&self.credential_environment, &id, &parsed.secrets)?;
        let record = ConnectionRecord {
            id: id.clone(),
            name: parsed.name,
            transport: parsed.transport,
            dont_save_to_memory: true,
            pending: true,
            enabled_loupists: BTreeSet::new(),
            tools: Vec::new(),
            env_keys: parsed.secrets.env.keys().cloned().collect(),
            header_names: parsed.secrets.headers.keys().cloned().collect(),
            last_error: None,
            needs_review: true,
            auth_state: AuthState::NotRequired,
        };
        self.stored.connections.insert(id.clone(), record);
        if let Err(error) = self.persist() {
            self.stored.connections.remove(&id);
            let _ = credentials::remove(&self.credential_environment, &id);
            return Err(error);
        }
        self.get(&id, None)
    }

    #[cfg(test)]
    pub async fn confirm(&mut self, id: &str) -> Result<ConnectionView, ConnectionError> {
        self.confirm_for_loupist(id, None).await
    }

    /// Confirming from the Add dialog enables the Loupist that opened it, before tool review.
    /// Another Loupist stays disabled until its own switch is turned on.
    pub async fn confirm_for_loupist(
        &mut self,
        id: &str,
        loupist_id: Option<&str>,
    ) -> Result<ConnectionView, ConnectionError> {
        let view = self.inspect(id, false, true).await?;
        match loupist_id
            .map(str::trim)
            .filter(|loupist| !loupist.is_empty())
        {
            Some(loupist) if !view.pending && view.last_error.is_none() => {
                self.set_enabled(id, loupist, true)
            }
            _ => Ok(view),
        }
    }

    pub async fn probe(&mut self, id: &str) -> Result<ConnectionView, ConnectionError> {
        self.inspect(id, false, false).await
    }

    pub fn begin_sign_in(&mut self, id: &str) -> Result<(), ConnectionError> {
        let record = self
            .stored
            .connections
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?;
        if !matches!(record.transport, TransportConfig::Http { .. }) {
            return Err(ConnectionError::Configuration);
        }
        record.auth_state = AuthState::InProgress;
        self.persist()
    }

    pub fn cancel_sign_in(&mut self, id: &str) -> Result<ConnectionView, ConnectionError> {
        let record = self
            .stored
            .connections
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?;
        record.auth_state = AuthState::SignInNeeded;
        record.last_error = None;
        self.persist()?;
        self.get(id, None)
    }

    pub async fn sign_in(&mut self, id: &str) -> Result<ConnectionView, ConnectionError> {
        self.inspect(id, true, true).await
    }

    async fn inspect(
        &mut self,
        id: &str,
        interactive: bool,
        confirm: bool,
    ) -> Result<ConnectionView, ConnectionError> {
        let result = self.open_session(id, interactive).await;
        let record = self
            .stored
            .connections
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?;
        match result {
            Ok(()) => {
                if confirm {
                    record.pending = false;
                }
                record.last_error = None;
                record.auth_state = if credentials::load(&self.credential_environment, id)?
                    .tokens
                    .is_some()
                {
                    AuthState::SignedIn
                } else {
                    AuthState::NotRequired
                };
            }
            Err(error) => {
                if matches!(
                    error,
                    ConnectionError::Authorization
                        | ConnectionError::ClientIdRequired
                        | ConnectionError::InvalidAuthorization
                ) || record.auth_state == AuthState::InProgress
                {
                    record.auth_state = AuthState::SignInNeeded;
                }
                record.pending = true;
                record.last_error = Some(error.to_string());
                self.sessions.remove(id);
            }
        }
        self.persist()?;
        self.get(id, None)
    }

    pub fn set_memory(
        &mut self,
        id: &str,
        dont_save: bool,
    ) -> Result<ConnectionView, ConnectionError> {
        self.stored
            .connections
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?
            .dont_save_to_memory = dont_save;
        self.persist()?;
        self.get(id, None)
    }

    pub fn set_enabled(
        &mut self,
        id: &str,
        loupist: &str,
        enabled: bool,
    ) -> Result<ConnectionView, ConnectionError> {
        let record = self
            .stored
            .connections
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?;
        if enabled && record.pending {
            return Err(ConnectionError::Configuration);
        }
        if enabled {
            record.enabled_loupists.insert(loupist.into());
        } else {
            record.enabled_loupists.remove(loupist);
        }
        self.persist()?;
        self.get(id, Some(loupist))
    }

    pub async fn remove(&mut self, id: &str) -> Result<(), ConnectionError> {
        let record = self
            .stored
            .connections
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?;
        record.pending = true;
        record.enabled_loupists.clear();
        self.stored.approvals.remove_connection(id);
        self.persist()?;
        if let Some(Session::Stdio(session)) = self.sessions.remove(id) {
            let _ = session.shutdown(Duration::from_secs(1)).await;
        }
        credentials::remove(&self.credential_environment, id)?;
        self.stored.connections.remove(id);
        self.persist()
    }

    pub async fn set_client(
        &mut self,
        id: &str,
        issuer: &str,
        client_id: String,
        client_secret: Option<String>,
    ) -> Result<ConnectionView, ConnectionError> {
        self.record(id)?;
        http::checked_url(issuer)?;
        if client_id.is_empty()
            || client_id.len() > 2048
            || client_secret
                .as_ref()
                .is_some_and(|s| s.is_empty() || s.len() > 16 * 1024)
        {
            return Err(ConnectionError::Configuration);
        }
        let mut secrets = credentials::load(&self.credential_environment, id)?;
        secrets.registration = Some(auth::Registration {
            issuer: issuer.into(),
            client_id,
            token_endpoint_auth_method: if client_secret.is_some() {
                "client_secret_post"
            } else {
                "none"
            }
            .into(),
            client_secret,
            dynamically_registered: false,
        });
        credentials::save(&self.credential_environment, id, &secrets)?;
        self.begin_sign_in(id)?;
        self.sign_in(id).await
    }

    async fn open_session(&mut self, id: &str, interactive: bool) -> Result<(), ConnectionError> {
        if self.sessions.contains_key(id) {
            return self.refresh_inventory(id, interactive).await;
        }
        let record = self.record(id)?.clone();
        let mut secrets = credentials::load(&self.credential_environment, id)?;
        let (session, tools) = match record.transport {
            TransportConfig::Http { url } => {
                let mut transport = HttpTransport::new(&url)?;
                transport.set_headers(&secrets.headers)?;
                authorized_request(&mut transport, "initialize", json!({"protocolVersion":crate::mcp::MCP_PROTOCOL_VERSION,"capabilities":{},"clientInfo":{"name":"Loupist","version":env!("CARGO_PKG_VERSION")}}), &mut secrets, &self.credential_environment, id, interactive).await?;
                authorized_request(
                    &mut transport,
                    "notifications/initialized",
                    json!({}),
                    &mut secrets,
                    &self.credential_environment,
                    id,
                    interactive,
                )
                .await?;
                let mut session = Session::Http(transport);
                let tools = list_tools(
                    &mut session,
                    &mut secrets,
                    &self.credential_environment,
                    id,
                    interactive,
                )
                .await?;
                (session, tools)
            }
            TransportConfig::Stdio { command, args } => {
                let (executable, arguments) = config::launch_command(&command, &args)?;
                let lease_dir = self
                    .root
                    .parent()
                    .ok_or(ConnectionError::Configuration)?
                    .join("process-leases");
                ensure_private_directory(&lease_dir).map_err(|_| ConnectionError::Configuration)?;
                let private_working_dir = self.root.join(id);
                ensure_private_directory(&private_working_dir)
                    .map_err(|_| ConnectionError::Configuration)?;
                let (session, tools) = McpSession::spawn_custom(McpLaunchConfig {
                    server_id: id.into(),
                    executable,
                    arguments,
                    private_working_dir,
                    lease_dir,
                    environment: config::runtime_environment(&secrets),
                    bootstrap: Value::Null,
                })
                .await
                .map_err(|_| ConnectionError::Protocol)?;
                (Session::Stdio(session), tools)
            }
        };
        self.accept_inventory(id, tools, &secrets)?;
        self.sessions.insert(id.into(), session);
        Ok(())
    }

    fn accept_inventory(
        &mut self,
        id: &str,
        tools: Vec<McpTool>,
        secrets: &Secrets,
    ) -> Result<(), ConnectionError> {
        let value = serde_json::to_value(&tools).map_err(|_| ConnectionError::Protocol)?;
        if contains_protected_value(&value, &secrets.protected_values()) {
            return Err(ConnectionError::Protocol);
        }
        self.stored.approvals.recheck(id, &tools);
        let record = self
            .stored
            .connections
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?;
        if record
            .tools
            .iter()
            .map(policy::definition_hash)
            .collect::<BTreeSet<_>>()
            != tools.iter().map(policy::definition_hash).collect()
        {
            record.needs_review = true;
        }
        record.tools = tools;
        self.persist()
    }

    async fn refresh_inventory(
        &mut self,
        id: &str,
        interactive: bool,
    ) -> Result<(), ConnectionError> {
        let mut secrets = credentials::load(&self.credential_environment, id)?;
        let session = self
            .sessions
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?;
        // Re-list before every invocation, including notifications/list_changed.
        // Approvals never outlive a changed descriptor even if a server omits notifications.
        let tools = list_tools(
            session,
            &mut secrets,
            &self.credential_environment,
            id,
            interactive,
        )
        .await?;
        self.accept_inventory(id, tools, &secrets)
    }

    pub async fn connect(
        &mut self,
        loupist: &str,
        id: &str,
        interactive: bool,
    ) -> Result<Vec<Value>, ConnectionError> {
        self.ensure_enabled(loupist, id)?;
        self.open_session(id, interactive).await?;
        let record = self.record(id)?;
        record
            .tools
            .iter()
            .filter(|tool| {
                matches!(
                    self.stored.approvals.choice(loupist, id, tool),
                    Some(ApprovalChoice::Always | ApprovalChoice::AllowOnce)
                )
            })
            .map(|tool| {
                crate::mcp::tool_to_agent_schema(id, &policy::model_tool(&record.name, tool))
                    .map_err(|_| ConnectionError::Protocol)
            })
            .collect()
    }

    /// True while a refused tool still has no approval for this Loupist (or its connection
    /// is pending); false once approved or when the tool or connection is gone.
    pub fn approval_pending(&self, loupist: &str, id: &str, tool_name: &str) -> bool {
        let Ok(record) = self.record(id) else {
            return false;
        };
        if record.pending || !record.enabled_loupists.contains(loupist) {
            return true;
        }
        record
            .tools
            .iter()
            .find(|tool| tool.name == tool_name)
            .is_some_and(|tool| {
                !matches!(
                    self.stored.approvals.choice(loupist, id, tool),
                    Some(ApprovalChoice::Always | ApprovalChoice::AllowOnce)
                )
            })
    }

    fn ensure_enabled(&self, loupist: &str, id: &str) -> Result<(), ConnectionError> {
        let record = self.record(id)?;
        if record.pending || !record.enabled_loupists.contains(loupist) {
            return Err(ConnectionError::ApprovalRequired);
        }
        Ok(())
    }

    pub async fn approve(
        &mut self,
        loupist: &str,
        id: &str,
        name: &str,
        expected_hash: &str,
        choice: ApprovalChoice,
    ) -> Result<ConnectionView, ConnectionError> {
        self.ensure_enabled(loupist, id)?;
        self.open_session(id, false).await?;
        let tool = self
            .record(id)?
            .tools
            .iter()
            .find(|t| t.name == name)
            .cloned()
            .ok_or(ConnectionError::ApprovalRequired)?;
        if policy::definition_hash(&tool) != expected_hash
            || !self.stored.approvals.approve(loupist, id, &tool, choice)
        {
            return Err(ConnectionError::ApprovalRequired);
        }
        let reviewed = self
            .record(id)?
            .tools
            .iter()
            .filter(|tool| policy::eligibility(tool) == ToolEligibility::ReadOnly)
            .all(|tool| self.stored.approvals.choice(loupist, id, tool).is_some());
        self.stored.connections.get_mut(id).unwrap().needs_review = !reviewed;
        self.persist()?;
        self.get(id, Some(loupist))
    }

    pub async fn call(
        &mut self,
        loupist: &str,
        id: &str,
        name: &str,
        arguments: Value,
        unattended: bool,
    ) -> Result<Value, ConnectionError> {
        self.ensure_enabled(loupist, id)?;
        if !arguments.is_object()
            || serde_json::to_vec(&arguments)
                .map_err(|_| ConnectionError::Configuration)?
                .len()
                > 1024 * 1024
        {
            return Err(ConnectionError::Configuration);
        }
        self.open_session(id, !unattended).await?;
        let tool = self
            .record(id)?
            .tools
            .iter()
            .find(|t| t.name == name)
            .cloned()
            .ok_or(ConnectionError::ApprovalRequired)?;
        if unattended && !self.stored.approvals.unattended_allowed(loupist, id, &tool) {
            return Err(ConnectionError::ApprovalRequired);
        }
        if !self.stored.approvals.consume(loupist, id, &tool) {
            return Err(ConnectionError::ApprovalRequired);
        }
        // Consume Allow once durably before any network/process write.
        self.persist()?;
        let mut secrets = credentials::load(&self.credential_environment, id)?;
        let (result, changed) = match self
            .sessions
            .get_mut(id)
            .ok_or(ConnectionError::Configuration)?
        {
            Session::Http(transport) => {
                let result = authorized_request(
                    transport,
                    "tools/call",
                    json!({"name":name,"arguments":arguments}),
                    &mut secrets,
                    &self.credential_environment,
                    id,
                    !unattended,
                )
                .await?;
                (result, transport.tools_changed)
            }
            Session::Stdio(session) => {
                let result = serde_json::to_value(
                    session
                        .call_tool(name, arguments, Duration::from_secs(45))
                        .await
                        .map_err(|_| ConnectionError::Protocol)?,
                )
                .map_err(|_| ConnectionError::Protocol)?;
                (result, session.take_tools_changed())
            }
        };
        if contains_protected_value(&result, &secrets.protected_values()) {
            return Err(ConnectionError::Protocol);
        }
        if changed {
            self.refresh_inventory(id, false).await?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;

async fn list_tools(
    session: &mut Session,
    secrets: &mut Secrets,
    environment: &str,
    id: &str,
    interactive: bool,
) -> Result<Vec<McpTool>, ConnectionError> {
    match session {
        Session::Stdio(session) => session
            .list_tools()
            .await
            .map_err(|_| ConnectionError::Protocol),
        Session::Http(transport) => {
            let mut tools = Vec::new();
            let mut cursors = BTreeSet::new();
            let mut cursor = None;
            let mut bytes = 0usize;
            for _ in 0..64 {
                let params = cursor
                    .as_ref()
                    .map(|c| json!({"cursor":c}))
                    .unwrap_or_else(|| json!({}));
                let page = authorized_request(
                    transport,
                    "tools/list",
                    params,
                    secrets,
                    environment,
                    id,
                    interactive,
                )
                .await?;
                bytes += serde_json::to_vec(&page)
                    .map_err(|_| ConnectionError::Protocol)?
                    .len();
                if bytes > 8 * 1024 * 1024 {
                    return Err(ConnectionError::Protocol);
                }
                cursor = http::parse_inventory_page(&page, &mut tools, &mut cursors)?;
                if cursor.is_none() {
                    transport.tools_changed = false;
                    return Ok(tools);
                }
            }
            Err(ConnectionError::Protocol)
        }
    }
}

async fn authorized_request(
    transport: &mut HttpTransport,
    method: &str,
    params: Value,
    secrets: &mut Secrets,
    environment: &str,
    id: &str,
    interactive: bool,
) -> Result<Value, ConnectionError> {
    let mut scope_attempts = 0;
    let mut refresh_attempted = false;
    let mut authorization_attempts = 0;
    loop {
        let bearer = secrets
            .tokens
            .as_ref()
            .map(|t| (t.resource.as_str(), t.access_token.as_str()));
        let challenge = match transport.request(method, params.clone(), bearer).await? {
            Reply::Result(value) => return Ok(value),
            Reply::Challenge(challenge) => challenge,
        };
        let parsed = auth::parse_challenge(&challenge.headers)?;
        let found = auth::discover(transport.resource(), &parsed).await?;
        if secrets
            .tokens
            .as_ref()
            .is_some_and(|t| t.issuer != found.metadata.issuer)
        {
            // Never transfer a token, refresh token or DCR client secret to a new issuer.
            if let Some(tokens) = secrets.tokens.take() {
                secrets.retired_values.push(tokens.access_token);
                secrets.retired_values.extend(tokens.refresh_token);
            }
            if let Some(registration) = secrets.registration.take() {
                if !registration.dynamically_registered {
                    return Err(ConnectionError::InvalidAuthorization);
                }
                secrets.retired_values.extend(registration.client_secret);
            }
        }
        if challenge.status == reqwest::StatusCode::UNAUTHORIZED && !refresh_attempted {
            refresh_attempted = true;
            if let (Some(registration), Some(tokens)) = (&secrets.registration, &secrets.tokens) {
                if tokens.refresh_token.is_some() {
                    if let Ok(tokens) = auth::refresh(&found.metadata, registration, tokens).await {
                        secrets.replace_tokens(tokens)?;
                        secrets.metadata = Some(found.metadata);
                        credentials::save(environment, id, secrets)?;
                        continue;
                    }
                }
            }
        }
        if !interactive {
            return Err(ConnectionError::Authorization);
        }
        let scopes = if challenge.status == reqwest::StatusCode::FORBIDDEN {
            auth::scope_upgrade(
                secrets
                    .tokens
                    .as_ref()
                    .ok_or(ConnectionError::Authorization)?,
                &challenge,
                &mut scope_attempts,
            )?
        } else {
            if authorization_attempts >= 1 {
                return Err(ConnectionError::Authorization);
            }
            authorization_attempts += 1;
            found.scopes
        };
        let (registration, tokens) = auth::authorize(
            &found.metadata,
            transport.resource(),
            &scopes,
            secrets.registration.as_ref(),
        )
        .await?;
        secrets.registration = Some(registration);
        secrets.metadata = Some(found.metadata);
        secrets.replace_tokens(tokens)?;
        credentials::save(environment, id, secrets)?;
    }
}
