pub(crate) mod tools;

use async_trait::async_trait;
use futures_util::{stream::FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    future::Future,
    io,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex as StdMutex,
    },
    time::Duration,
};
use thiserror::Error;
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::{watch, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
    time::timeout,
};
use uuid::Uuid;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1,
        },
        GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use crate::chat::artifacts::MAX_CHAT_TURN_ARTIFACTS;

const PROTOCOL: &str = "loupist-tool/1";
// Memory proposals may contain up to 2 MiB of Markdown. Keep framing headroom
// for typed target metadata and JSON escaping.
const MAX_FRAME_BYTES: usize = 5 * 1024 * 1024;
pub const MAX_MEMORY_PROPOSALS: u8 = 8;
// A multi-company comparison can retain several primary sources per company.
// Keep retention bounded without exhausting it after the first company.
pub const MAX_CHAT_TURN_SOURCES: u8 = 16;
const MAX_CONCURRENT_CONNECTIONS: usize = 64;
const RESPONSE_WRITE_TIMEOUT: Duration = Duration::from_secs(10);
const DELIVERY_ACK_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(not(test))]
const AUTHENTICATION_READ_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(test)]
const AUTHENTICATION_READ_TIMEOUT: Duration = Duration::from_millis(100);

type SharedTransactionCardinality = Arc<StdMutex<TransactionCardinality>>;
type ConnectionFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

#[cfg(windows)]
struct WindowsPipeSecurity {
    descriptor: *mut std::ffi::c_void,
}

#[cfg(windows)]
// The descriptor is immutable after construction and LocalFree permits the
// owning allocation to be released from any thread.
unsafe impl Send for WindowsPipeSecurity {}

#[cfg(windows)]
impl WindowsPipeSecurity {
    fn current_user_and_system() -> io::Result<Self> {
        let user_sid = current_user_sid_string()?;
        let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;{user_sid})");
        let encoded = sddl.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: `encoded` is a live, NUL-terminated SDDL string and the output
        // receives one LocalAlloc-owned self-relative security descriptor.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                encoded.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { descriptor })
    }
}

#[cfg(windows)]
impl Drop for WindowsPipeSecurity {
    fn drop(&mut self) {
        // SAFETY: the conversion API returned this LocalAlloc allocation and
        // this value is its unique owner.
        unsafe { LocalFree(self.descriptor) };
    }
}

#[cfg(windows)]
fn current_user_sid_string() -> io::Result<String> {
    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: the pseudo process handle is always valid and `token` is writable.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let result = (|| {
        let mut needed = 0_u32;
        // The first call intentionally discovers the required buffer size.
        unsafe { GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut needed) };
        if needed < std::mem::size_of::<TOKEN_USER>() as u32 {
            return Err(io::Error::last_os_error());
        }
        let words = (needed as usize).div_ceil(std::mem::size_of::<usize>());
        let mut buffer = vec![0_usize; words];
        if unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: GetTokenInformation initialized a suitably aligned TOKEN_USER
        // and its SID remains backed by `buffer` for this conversion call.
        let sid = unsafe { (*(buffer.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        let mut sid_text = std::ptr::null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut sid_text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let converted = (|| {
            let mut length = 0_usize;
            // SID strings are bounded by the SID format; retain a defensive cap
            // before creating the borrowed UTF-16 slice.
            while length <= 256 && unsafe { *sid_text.add(length) } != 0 {
                length += 1;
            }
            if length > 256 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "current user SID string is invalid",
                ));
            }
            // SAFETY: the conversion API returned at least `length + 1` UTF-16
            // units ending in NUL.
            String::from_utf16(unsafe { std::slice::from_raw_parts(sid_text, length) })
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "SID is not UTF-16"))
        })();
        // SAFETY: ConvertSidToStringSidW allocated this string with LocalAlloc.
        unsafe { LocalFree(sid_text.cast()) };
        converted
    })();
    // SAFETY: OpenProcessToken returned this uniquely owned handle.
    unsafe { CloseHandle(token) };
    result
}

#[cfg(windows)]
fn create_windows_pipe_server(
    path: &PathBuf,
    first: bool,
    security: &WindowsPipeSecurity,
) -> io::Result<NamedPipeServer> {
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.descriptor,
        bInheritHandle: 0,
    };
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first)
        .reject_remote_clients(true);
    // Keep Tokio's Win32 `PIPE_UNLIMITED_INSTANCES` default. Each accepted
    // connection remains backed by an OS-accounted pipe handle, while the
    // broker does not make long-running independent tool calls wait behind an
    // arbitrary four-handler ceiling.
    // SAFETY: `attributes` and its immutable descriptor remain live throughout
    // CreateNamedPipeW; Windows copies the security descriptor into the object.
    unsafe {
        options.create_with_security_attributes_raw(
            path,
            (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToolPolicy {
    pub loupist_id: String,
    pub session_id: String,
    pub use_memory: bool,
    pub propose_memory_updates: bool,
    #[serde(default = "default_memory_proposal_budget")]
    pub memory_proposal_budget: u8,
    #[serde(default)]
    pub as_of: Option<String>,
}

const fn default_memory_proposal_budget() -> u8 {
    1
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolMethod {
    NativeAuthorize,
    ScheduleList,
    ScheduleRead,
    ScheduleCreate,
    ScheduleHandoff,
    ScheduleWatch,
    ScheduleBriefing,
    ScheduleUpdate,
    ScheduleSetStatus,
    ScheduleDelete,
    ScheduleRunNow,
    ScheduleStop,
    ChatHistory,
    DatasetDescribe,
    DatasetQuery,
    DatasetExport,
    DatasetImport,
    Calculate,

    WorldbankSeries,
    DefillamaRead,
    KrxDailyQuote,
    KisOperationSearch,
    KisRead,
    DartFinancials,
    DartFilingSearch,
    DartFilingRead,
    DartCompanyLookup,
    DartMajorReport,
    DartOwnership,
    DartDividends,
    McpConnect,
    McpCall,
    RunResultsList,
    PlanUpdate,
    WebSearch,
    WebFetch,
    SourceSave,
    WorldSearch,
    WorldRead,
    NoteSave,
    JournalSave,
    JournalReview,
    LensUpdate,
    PrincipleSave,
    WorldSchemaUpdate,
    LensSearch,
    LensRead,
    WorldJob,
    ConversationMemory,
    AssetPageRead,
    ThesisUpdate,
    EarningsSnapshotSave,
    ArtifactList,
    ArtifactRead,
    ArtifactPublish,
    ChartQuery,
    ChartPublish,
    GridPublish,
    WorkspaceCapture,
}

impl ToolMethod {
    fn parse(value: &str) -> Option<Self> {
        // MCP administration is host-only; all model-facing routes come from
        // the shared catalog, never from client-supplied schemas or aliases.
        match value {
            "native.authorize" => Some(Self::NativeAuthorize),
            "mcp.connect" => Some(Self::McpConnect),
            "mcp.call" => Some(Self::McpCall),
            _ => crate::broker::tools::TOOL_CATALOG
                .tools
                .iter()
                .find(|tool| tool.method.as_deref() == Some(value))
                .and_then(|tool| serde_json::from_value(serde_json::json!(tool.name)).ok()),
        }
    }

    fn permission(self) -> crate::broker::tools::ToolPermission {
        use crate::broker::tools::{ToolPermission, TOOL_CATALOG};
        if matches!(
            self,
            Self::NativeAuthorize | Self::McpConnect | Self::McpCall
        ) {
            return ToolPermission::Always;
        }
        let name = serde_json::to_value(self).expect("tool name serializes");
        TOOL_CATALOG
            .tools
            .iter()
            .find(|tool| name.as_str() == Some(tool.name.as_str()))
            .expect("broker method must have a tool contract")
            .permission
    }
}

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("Outbound access is paused for this turn because it read content from a custom connection. The user can ask again in a new message.")]
    OutboundPaused,
    #[error("Commands, outbound writes and new websites are paused for the rest of this turn because it read third-party text (web pages, search results, news, filings, provider descriptions, or saved Sources and datasets holding them).")]
    UntrustedTextPaused,
    #[error("This custom tool needs your approval in Connections before it can run.")]
    CustomApprovalRequired {
        connection_id: String,
        tool_name: String,
    },
    #[error("{provider} needs approval for {}", .services.join(", "))]
    ProviderApprovalRequired {
        provider: String,
        services: Vec<String>,
    },
    #[error("tool broker I/O failed: {0}")]
    Io(#[from] io::Error),
    #[error("tool is not in the Loupist allowlist")]
    MethodDenied,
    #[error("Loupist Memory use is disabled for this turn")]
    MemoryDisabled,
    #[error("memory proposals are disabled for this turn")]
    ProposalDisabled,
    #[error("tool request is malformed")]
    Malformed,
    #[error("{field}: {expected}")]
    InvalidArguments { field: String, expected: String },
    #[error("{source}")]
    WithNext {
        source: Box<BrokerError>,
        next: &'static str,
    },
    #[error("tool broker authentication failed")]
    Authentication,
    #[error("tool frame exceeded the size limit")]
    FrameTooLarge,
    #[error("tool execution failed: {0}")]
    Execution(String),
    /// The read never reached its server (DNS, connect or no route). One host
    /// failing does not prove the device is offline, so the text names it.
    #[error("{0} could not be reached; the network or this provider may be unavailable")]
    Offline(String),
    #[error("transaction cardinality for this run is exhausted")]
    BudgetExceeded,
}

impl BrokerError {
    pub(crate) fn invalid(field: impl Into<String>, expected: impl Into<String>) -> Self {
        Self::InvalidArguments {
            field: field.into(),
            expected: expected.into(),
        }
    }

    pub(crate) fn with_next(self, next: &'static str) -> Self {
        Self::WithNext {
            source: Box::new(self),
            next,
        }
    }
}

#[async_trait]
pub trait ToolExecutor: Send + Sync + 'static {
    fn enforce_turn_policy(&self, _method: ToolMethod, _params: &Value) -> Result<(), BrokerError> {
        Ok(())
    }

    fn tool_trace(&self) -> Option<Arc<ToolTraceCapture>> {
        None
    }

    async fn execute(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
    ) -> Result<Value, BrokerError>;

    async fn execute_for_delivery(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
        _delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.execute(policy, method, params).await
    }

    /// `call_id` is the model tool-call id. It stays outside tool params so a
    /// schema that rejects unknown fields is unchanged. The default ignores it.
    async fn execute_attributed(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
        delivery_id: &str,
        _call_id: Option<String>,
    ) -> Result<Value, BrokerError> {
        self.execute_for_delivery(policy, method, params, delivery_id)
            .await
    }

    async fn commit_delivery(
        &self,
        _policy: &ToolPolicy,
        _delivery_id: &str,
    ) -> Result<(), BrokerError> {
        Ok(())
    }

    async fn discard_delivery(&self, _policy: &ToolPolicy, _delivery_id: &str) {}
}

#[derive(Deserialize)]
struct BrokerRequest {
    protocol: String,
    id: String,
    token: String,
    method: String,
    params: Value,
    #[serde(default)]
    call_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerDeliveryAck {
    protocol: String,
    id: String,
    delivered: bool,
}

#[derive(Serialize)]
struct BrokerCommitBarrier<'a> {
    protocol: &'static str,
    id: &'a str,
    committed: bool,
}

#[derive(Serialize)]
struct BrokerResponse {
    protocol: &'static str,
    id: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

struct BoundedJsonBuffer {
    bytes: Vec<u8>,
    limit: usize,
    overflowed: bool,
}

impl BoundedJsonBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::with_capacity(limit.min(64 * 1024)),
            limit,
            overflowed: false,
        }
    }
}

impl io::Write for BoundedJsonBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|size| size > self.limit)
        {
            self.overflowed = true;
            return Err(io::Error::other("bounded broker frame exceeded"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub struct ToolBrokerHandle {
    pub socket_path: PathBuf,
    token: String,
    shutdown: Option<watch::Sender<bool>>,
    task: Option<JoinHandle<()>>,
    identity_lease: Option<ToolBrokerIdentityLease>,
}

/// Process-lifetime authority for sequential, turn-scoped tool brokers.
///
/// The endpoint and token remain stable so a future long-lived Pi process can
/// keep one broker identity, while each broker start still receives a fresh
/// policy, executor, and transaction-cardinality state. This type deliberately
/// does not implement `Debug` or serialization because its token is authority.
#[derive(Clone)]
pub struct ToolBrokerIdentity {
    inner: Arc<ToolBrokerIdentityInner>,
}

struct ToolBrokerIdentityInner {
    socket_path: PathBuf,
    token: String,
    leased: AtomicBool,
}

struct ToolBrokerIdentityLease {
    identity: Arc<ToolBrokerIdentityInner>,
}

impl Drop for ToolBrokerIdentityLease {
    fn drop(&mut self) {
        self.identity.leased.store(false, Ordering::Release);
    }
}

impl ToolBrokerIdentity {
    pub fn new(socket_path: PathBuf) -> Self {
        Self {
            inner: Arc::new(ToolBrokerIdentityInner {
                socket_path,
                token: new_broker_token(),
                leased: AtomicBool::new(false),
            }),
        }
    }

    pub fn socket_path(&self) -> &PathBuf {
        &self.inner.socket_path
    }

    pub fn token(&self) -> &str {
        &self.inner.token
    }

    pub async fn start(
        &self,
        policy: ToolPolicy,
        executor: Arc<dyn ToolExecutor>,
    ) -> Result<ToolBrokerHandle, BrokerError> {
        start_tool_broker_with_identity(self, policy, executor).await
    }

    fn acquire(&self) -> Result<ToolBrokerIdentityLease, BrokerError> {
        self.inner
            .leased
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                BrokerError::Io(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "tool broker identity is already active",
                ))
            })?;
        Ok(ToolBrokerIdentityLease {
            identity: self.inner.clone(),
        })
    }
}

#[derive(Debug, Default)]
struct TransactionCardinality {
    journals: u8,
    evidence_creations: u8,
    proposals: u8,
    schema_updates: u8,
    artifact_publishes: u8,
}

impl TransactionCardinality {
    fn consume(&mut self, method: ToolMethod, policy: &ToolPolicy) -> Result<(), BrokerError> {
        match method {
            ToolMethod::NoteSave if self.journals >= 1 => {
                return Err(BrokerError::BudgetExceeded);
            }
            ToolMethod::SourceSave if self.evidence_creations >= MAX_CHAT_TURN_SOURCES => {
                return Err(BrokerError::BudgetExceeded);
            }
            ToolMethod::LensUpdate | ToolMethod::PrincipleSave
                if self.proposals >= policy.memory_proposal_budget =>
            {
                return Err(BrokerError::BudgetExceeded);
            }
            ToolMethod::WorldSchemaUpdate if self.schema_updates >= 1 => {
                return Err(BrokerError::BudgetExceeded);
            }
            ToolMethod::ArtifactPublish | ToolMethod::ChartPublish | ToolMethod::GridPublish
                if self.artifact_publishes >= MAX_CHAT_TURN_ARTIFACTS as u8 =>
            {
                return Err(BrokerError::BudgetExceeded);
            }
            _ => {}
        }
        match method {
            ToolMethod::WorldSchemaUpdate => self.schema_updates += 1,
            ToolMethod::NoteSave => self.journals += 1,
            ToolMethod::SourceSave => self.evidence_creations += 1,
            ToolMethod::LensUpdate | ToolMethod::PrincipleSave => self.proposals += 1,
            ToolMethod::ArtifactPublish | ToolMethod::ChartPublish | ToolMethod::GridPublish => {
                self.artifact_publishes += 1
            }
            _ => {}
        }
        Ok(())
    }

    fn rollback(&mut self, method: ToolMethod) {
        match method {
            ToolMethod::NoteSave => self.journals = self.journals.saturating_sub(1),
            ToolMethod::SourceSave => {
                self.evidence_creations = self.evidence_creations.saturating_sub(1)
            }
            ToolMethod::LensUpdate | ToolMethod::PrincipleSave => {
                self.proposals = self.proposals.saturating_sub(1)
            }
            ToolMethod::WorldSchemaUpdate => {
                self.schema_updates = self.schema_updates.saturating_sub(1)
            }
            ToolMethod::ArtifactPublish | ToolMethod::ChartPublish | ToolMethod::GridPublish => {
                self.artifact_publishes = self.artifact_publishes.saturating_sub(1)
            }
            _ => {}
        }
    }
}

#[derive(Debug)]
struct TransactionReservation {
    cardinality: SharedTransactionCardinality,
    method: ToolMethod,
    committed: bool,
}

#[derive(Debug)]
struct ExecutedRequest {
    id: String,
    result: Value,
    method: ToolMethod,
    reservation: TransactionReservation,
}

impl TransactionReservation {
    fn reserve(
        cardinality: SharedTransactionCardinality,
        method: ToolMethod,
        policy: &ToolPolicy,
    ) -> Result<Self, BrokerError> {
        cardinality
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .consume(method, policy)?;
        Ok(Self {
            cardinality,
            method,
            committed: false,
        })
    }

    fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for TransactionReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.cardinality
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .rollback(self.method);
        }
    }
}

impl ToolBrokerHandle {
    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn shutdown(mut self) -> Result<(), BrokerError> {
        #[cfg(feature = "e2e")]
        eprintln!("Loupist E2E broker explicit shutdown requested");
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.send_replace(true);
        }
        let task_result = if let Some(mut task) = self.task.take() {
            match timeout(Duration::from_secs(2), &mut task).await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(BrokerError::Io(io::Error::other(format!(
                    "tool broker task failed: {error}"
                )))),
                Err(_) => {
                    // Connection handlers are owned as futures inside this task.
                    // Awaiting its abort therefore drops every in-flight tool
                    // future before shutdown reports the bounded failure.
                    task.abort();
                    let _ = task.await;
                    Err(BrokerError::Io(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "tool broker task did not drain after shutdown",
                    )))
                }
            }
        } else {
            Ok(())
        };
        let endpoint_result = remove_broker_endpoint(&self.socket_path);
        // Keep the identity leased until both the accept task and every
        // connection handler have drained and the accepting endpoint is gone.
        self.identity_lease.take();
        task_result?;
        endpoint_result
    }
}

impl Drop for ToolBrokerHandle {
    fn drop(&mut self) {
        #[cfg(feature = "e2e")]
        eprintln!("Loupist E2E broker handle dropped");
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.send_replace(true);
        }
        if let Some(mut task) = self.task.take() {
            let identity_lease = self.identity_lease.take();
            let socket_path = self.socket_path.clone();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    if timeout(Duration::from_secs(2), &mut task).await.is_err() {
                        task.abort();
                        let _ = task.await;
                    }
                    let _ = remove_broker_endpoint(&socket_path);
                    drop(identity_lease);
                });
            } else {
                task.abort();
                let _ = remove_broker_endpoint(&socket_path);
                drop(identity_lease);
            }
            return;
        }
        let _ = remove_broker_endpoint(&self.socket_path);
        self.identity_lease.take();
    }
}

#[cfg(unix)]
fn remove_broker_endpoint(endpoint: &PathBuf) -> Result<(), BrokerError> {
    match std::fs::remove_file(endpoint) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(BrokerError::Io(error)),
    }
}

#[cfg(windows)]
fn remove_broker_endpoint(_endpoint: &PathBuf) -> Result<(), BrokerError> {
    // Named pipes disappear when their final server handle closes.
    Ok(())
}

/// Resolves a run-scoped broker endpoint for the current platform.
///
/// Unix uses the caller-provided filesystem path. Windows named pipes live in
/// a separate namespace, so a stable digest keeps the endpoint short and free
/// of user-controlled separators. The returned path must be passed unchanged
/// both to [`start_tool_broker`] and to the Pi environment.
pub fn tool_broker_endpoint(logical_path: PathBuf) -> PathBuf {
    #[cfg(unix)]
    {
        logical_path
    }
    #[cfg(windows)]
    {
        use sha2::{Digest, Sha256};

        let digest = hex::encode(Sha256::digest(logical_path.to_string_lossy().as_bytes()));
        PathBuf::from(format!(r"\\.\pipe\loupist-tool-{}", &digest[..32]))
    }
}

pub async fn start_tool_broker(
    socket_path: PathBuf,
    policy: ToolPolicy,
    executor: Arc<dyn ToolExecutor>,
) -> Result<ToolBrokerHandle, BrokerError> {
    ToolBrokerIdentity::new(socket_path)
        .start(policy, executor)
        .await
}

async fn start_tool_broker_with_identity(
    identity: &ToolBrokerIdentity,
    policy: ToolPolicy,
    executor: Arc<dyn ToolExecutor>,
) -> Result<ToolBrokerHandle, BrokerError> {
    if policy.memory_proposal_budget > MAX_MEMORY_PROPOSALS
        || (policy.propose_memory_updates && policy.memory_proposal_budget == 0)
    {
        return Err(BrokerError::Malformed);
    }
    let identity_lease = identity.acquire()?;
    let socket_path = identity.socket_path().clone();
    let (shutdown, shutdown_receiver) = watch::channel(false);
    #[cfg(unix)]
    let task = {
        use std::os::unix::fs::PermissionsExt;
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::symlink_metadata(&socket_path) {
            Ok(_) => {
                return Err(BrokerError::Io(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "tool broker endpoint already exists",
                )));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(BrokerError::Io(error)),
        }
        let listener = UnixListener::bind(&socket_path)?;
        std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
        let server_token = identity.token().to_owned();
        let task_token = server_token.clone();
        let task_shutdown = shutdown.clone();
        let mut shutdown_receiver = shutdown_receiver;
        let task = tokio::spawn(async move {
            let cardinality = Arc::new(StdMutex::new(TransactionCardinality::default()));
            let connection_slots = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
            let policy = Arc::new(policy);
            let mut handlers = FuturesUnordered::new();
            loop {
                tokio::select! {
                    biased;
                    _ = shutdown_requested(&mut shutdown_receiver) => break,
                    _ = handlers.next(), if !handlers.is_empty() => {}
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _)) => {
                            if let Ok(permit) = connection_slots.clone().try_acquire_owned() {
                                handlers.push(connection_handler(
                                    stream,
                                    task_token.clone(),
                                    policy.clone(),
                                    cardinality.clone(),
                                    executor.clone(),
                                    task_shutdown.subscribe(),
                                    permit,
                                ));
                            }
                        }
                        Err(_) => break,
                    },
                }
            }
            task_shutdown.send_replace(true);
            drain_connection_handlers(&mut handlers).await;
        });
        (server_token, task)
    };

    #[cfg(windows)]
    let task = {
        if !socket_path.to_string_lossy().starts_with(r"\\.\pipe\") {
            return Err(BrokerError::Malformed);
        }
        let security = WindowsPipeSecurity::current_user_and_system()?;
        let server = create_windows_pipe_server(&socket_path, true, &security)?;
        let server_token = identity.token().to_owned();
        let task_token = server_token.clone();
        let task_path = socket_path.clone();
        let task_shutdown = shutdown.clone();
        let mut shutdown_receiver = shutdown_receiver;
        let task = tokio::spawn(async move {
            let cardinality = Arc::new(StdMutex::new(TransactionCardinality::default()));
            let connection_slots = Arc::new(Semaphore::new(MAX_CONCURRENT_CONNECTIONS));
            let policy = Arc::new(policy);
            let mut handlers = FuturesUnordered::new();
            let mut server = server;
            loop {
                let connected = tokio::select! {
                    biased;
                    _ = shutdown_requested(&mut shutdown_receiver) => break,
                    _ = handlers.next(), if !handlers.is_empty() => continue,
                    connected = server.connect() => connected,
                };
                if connected.is_err() {
                    break;
                }
                let next = create_windows_pipe_server(&task_path, false, &security);
                if let Ok(permit) = connection_slots.clone().try_acquire_owned() {
                    handlers.push(connection_handler(
                        server,
                        task_token.clone(),
                        policy.clone(),
                        cardinality.clone(),
                        executor.clone(),
                        task_shutdown.subscribe(),
                        permit,
                    ));
                }
                match next {
                    Ok(next) => server = next,
                    Err(_) => break,
                }
            }
            task_shutdown.send_replace(true);
            drain_connection_handlers(&mut handlers).await;
        });
        (server_token, task)
    };

    let (token, task) = task;

    Ok(ToolBrokerHandle {
        socket_path,
        token,
        shutdown: Some(shutdown),
        task: Some(task),
        identity_lease: Some(identity_lease),
    })
}

fn connection_handler<S>(
    stream: S,
    expected_token: String,
    policy: Arc<ToolPolicy>,
    cardinality: SharedTransactionCardinality,
    executor: Arc<dyn ToolExecutor>,
    shutdown: watch::Receiver<bool>,
    _permit: OwnedSemaphorePermit,
) -> ConnectionFuture
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    Box::pin(async move {
        // Do not cancel an in-flight handler when the accept loop shuts
        // down. After the client ACKs a result, dropping the write of the
        // commit barrier makes delivery indeterminate (Pi exit 71). Hung
        // tools are still bounded by ToolBrokerHandle::shutdown's abort.
        let _ = shutdown;
        let result =
            handle_connection(stream, &expected_token, &policy, cardinality, executor).await;
        #[cfg(feature = "e2e")]
        if let Err(error) = result {
            eprintln!("Loupist E2E broker handler failed: {error}");
        }
        #[cfg(not(feature = "e2e"))]
        let _ = result;
    })
}

async fn shutdown_requested(shutdown: &mut watch::Receiver<bool>) {
    if *shutdown.borrow_and_update() {
        return;
    }
    loop {
        if shutdown.changed().await.is_err() || *shutdown.borrow_and_update() {
            return;
        }
    }
}

async fn drain_connection_handlers(handlers: &mut FuturesUnordered<ConnectionFuture>) {
    while handlers.next().await.is_some() {}
}

async fn handle_connection<S>(
    stream: S,
    expected_token: &str,
    policy: &ToolPolicy,
    cardinality: SharedTransactionCardinality,
    executor: Arc<dyn ToolExecutor>,
) -> Result<(), BrokerError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    let read = timeout(
        AUTHENTICATION_READ_TIMEOUT,
        (&mut reader)
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "broker authentication timed out"))??;
    if read == 0 || bytes.last() != Some(&b'\n') {
        return Err(BrokerError::Malformed);
    }
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(BrokerError::FrameTooLarge);
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    let request: BrokerRequest =
        serde_json::from_slice(&bytes).map_err(|_| BrokerError::Malformed)?;
    // Never collect the authentication token or unauthenticated requests.
    let trace = if constant_time_eq(request.token.as_bytes(), expected_token.as_bytes()) {
        executor.tool_trace().map(|capture| {
            let spec = crate::broker::tools::TOOL_CATALOG
                .tools
                .iter()
                .find(|tool| tool.method.as_deref() == Some(request.method.as_str()));
            let name = if request.method == "mcp.call" {
                request
                    .params
                    .get("tool_name")
                    .and_then(Value::as_str)
                    .unwrap_or(&request.method)
            } else {
                spec.map_or(request.method.as_str(), |tool| tool.name.as_str())
            };
            let component = spec.and_then(|tool| tool.component.as_deref()).or_else(|| {
                matches!(request.method.as_str(), "mcp.call" | "mcp.connect")
                    .then(|| request.params.get("server_id").and_then(Value::as_str))
                    .flatten()
            });
            capture.begin(
                request.call_id.as_deref(),
                name,
                component,
                Some(&request.params),
            )
        })
    } else {
        None
    };
    let delivery_id = new_broker_token();
    let execution_result = {
        let execution = execute_request(
            request,
            expected_token,
            policy,
            cardinality,
            executor.clone(),
            &delivery_id,
        );
        tokio::pin!(execution);
        tokio::select! {
            biased;
            result = &mut execution => Some(result),
            _ = wait_for_peer_disconnect(&mut reader) => None,
        }
    };
    let Some(execution_result) = execution_result else {
        executor.discard_delivery(policy, &delivery_id).await;
        return Ok(());
    };
    let (response, mut transaction) = match execution_result {
        Ok(executed) => (
            BrokerResponse {
                protocol: PROTOCOL,
                id: executed.id,
                ok: true,
                result: Some(executed.result),
                error: None,
            },
            Some((executed.method, executed.reservation)),
        ),
        Err((id, error)) => {
            executor.discard_delivery(policy, &delivery_id).await;
            (
                BrokerResponse {
                    protocol: PROTOCOL,
                    id,
                    ok: false,
                    result: None,
                    error: Some(error_body(&error)),
                },
                None,
            )
        }
    };
    let response_id = response.id.clone();
    let result_bytes = response.result.as_ref().and_then(trace_result_size);
    let mut trace_error = response.error.as_ref().map(trace_error);
    let (encoded, delivered_success) = match encode_response_or_bounded_error(response) {
        Ok(encoded) => encoded,
        Err(error) => {
            executor.discard_delivery(policy, &delivery_id).await;
            return Err(error);
        }
    };
    if !delivered_success {
        // This is the actual bounded envelope sent to the model, including a
        // frame-too-large substitution. Never retain its result payload.
        trace_error = serde_json::from_slice::<Value>(&encoded)
            .ok()
            .and_then(|response| response.get("error").cloned());
        executor.discard_delivery(policy, &delivery_id).await;
        transaction.take();
    }
    if let Some(trace) = trace {
        trace.finish(trace_error.as_ref(), result_bytes);
    }
    if let Err(error) = write_response(&mut writer, &encoded).await {
        executor.discard_delivery(policy, &delivery_id).await;
        return Err(error);
    }
    if let Err(error) = read_delivery_ack(&mut reader, &response_id).await {
        executor.discard_delivery(policy, &delivery_id).await;
        return Err(error);
    }
    if let Some((_, reservation)) = transaction {
        if let Err(error) = executor.commit_delivery(policy, &delivery_id).await {
            executor.discard_delivery(policy, &delivery_id).await;
            return Err(error);
        }
        reservation.commit();
    }
    let committed = encode_commit_barrier(&response_id)?;
    write_response(&mut writer, &committed).await?;
    // The explicit committed frame is the client-visible terminal boundary.
    // A later half-close anomaly cannot roll back the already acknowledged and
    // committed delivery, so shutdown remains best-effort.
    let _ = shutdown_writer(&mut writer).await;
    Ok(())
}

fn encode_commit_barrier(id: &str) -> Result<Vec<u8>, BrokerError> {
    let mut bytes = serde_json::to_vec(&BrokerCommitBarrier {
        protocol: PROTOCOL,
        id,
        committed: true,
    })
    .map_err(|_| BrokerError::Malformed)?;
    if bytes.len() >= MAX_FRAME_BYTES {
        return Err(BrokerError::FrameTooLarge);
    }
    bytes.push(b'\n');
    Ok(bytes)
}

async fn write_response<W>(writer: &mut W, bytes: &[u8]) -> Result<(), BrokerError>
where
    W: AsyncWrite + Unpin,
{
    timeout(RESPONSE_WRITE_TIMEOUT, async {
        writer.write_all(bytes).await?;
        writer.flush().await
    })
    .await
    .map_err(|_| {
        BrokerError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            "broker response write timed out",
        ))
    })??;
    Ok(())
}

async fn read_delivery_ack<R>(
    reader: &mut BufReader<R>,
    expected_id: &str,
) -> Result<(), BrokerError>
where
    R: AsyncRead + Unpin,
{
    let mut bytes = Vec::new();
    let read = timeout(
        DELIVERY_ACK_TIMEOUT,
        reader
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut bytes),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "broker delivery ACK timed out"))??;
    if read == 0 || bytes.last() != Some(&b'\n') {
        return Err(BrokerError::Malformed);
    }
    if bytes.len() > MAX_FRAME_BYTES {
        return Err(BrokerError::FrameTooLarge);
    }
    bytes.pop();
    if bytes.last() == Some(&b'\r') {
        bytes.pop();
    }
    let ack: BrokerDeliveryAck =
        serde_json::from_slice(&bytes).map_err(|_| BrokerError::Malformed)?;
    if ack.protocol != PROTOCOL || ack.id != expected_id || !ack.delivered {
        return Err(BrokerError::Malformed);
    }
    Ok(())
}

async fn shutdown_writer<W>(writer: &mut W) -> Result<(), BrokerError>
where
    W: AsyncWrite + Unpin,
{
    timeout(RESPONSE_WRITE_TIMEOUT, writer.shutdown())
        .await
        .map_err(|_| {
            BrokerError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "broker response shutdown timed out",
            ))
        })??;
    Ok(())
}

async fn wait_for_peer_disconnect<R>(reader: &mut R)
where
    R: AsyncRead + Unpin,
{
    let mut byte = [0_u8; 1];
    let _ = reader.read(&mut byte).await;
}

fn new_broker_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn execute_request(
    request: BrokerRequest,
    expected_token: &str,
    policy: &ToolPolicy,
    cardinality: SharedTransactionCardinality,
    executor: Arc<dyn ToolExecutor>,
    delivery_id: &str,
) -> Result<ExecutedRequest, (String, BrokerError)> {
    let id = request.id;
    if id.is_empty() || id.len() > 256 || id.contains(['\n', '\r', '\0']) {
        return Err(("invalid-request-id".into(), BrokerError::Malformed));
    }
    if request.protocol != PROTOCOL {
        return Err((id, BrokerError::Malformed));
    }
    if !constant_time_eq(request.token.as_bytes(), expected_token.as_bytes()) {
        return Err((id, BrokerError::Authentication));
    }
    let method = ToolMethod::parse(&request.method).ok_or_else(|| {
        let error = BrokerError::MethodDenied;
        (id.clone(), error)
    })?;
    enforce_policy(policy, method).map_err(|error| (id.clone(), error))?;
    executor
        .enforce_turn_policy(method, &request.params)
        .map_err(|error| (id.clone(), error))?;
    let reservation = TransactionReservation::reserve(cardinality, method, policy)
        .map_err(|error| (id.clone(), error))?;
    if request.call_id.as_ref().is_some_and(|call_id| {
        call_id.is_empty() || call_id.len() > 256 || call_id.contains(['\n', '\r', '\0'])
    }) {
        return Err((id, BrokerError::Malformed));
    }
    let result = executor
        .execute_attributed(policy, method, request.params, delivery_id, request.call_id)
        .await
        .map_err(|error| (id.clone(), error))?;
    Ok(ExecutedRequest {
        id,
        result,
        method,
        reservation,
    })
}

fn encode_response(response: &BrokerResponse) -> Result<Vec<u8>, BrokerError> {
    let mut writer = BoundedJsonBuffer::new(MAX_FRAME_BYTES - 1);
    if serde_json::to_writer(&mut writer, response).is_err() {
        return Err(if writer.overflowed {
            BrokerError::FrameTooLarge
        } else {
            BrokerError::Malformed
        });
    }
    writer.bytes.push(b'\n');
    Ok(writer.bytes)
}

fn encode_response_or_bounded_error(
    response: BrokerResponse,
) -> Result<(Vec<u8>, bool), BrokerError> {
    match encode_response(&response) {
        Ok(bytes) => Ok((bytes, response.ok)),
        Err(BrokerError::FrameTooLarge) => encode_response(&BrokerResponse {
            protocol: PROTOCOL,
            id: response.id,
            ok: false,
            result: None,
            error: Some(json!({
                "code": "frame_too_large",
                "message": "tool response exceeded the bounded broker frame",
                "retryable": false,
                "next": "The result was too large. Request a smaller page or file instead of repeating this call."
            })),
        })
        .map(|bytes| (bytes, false)),
        Err(error) => Err(error),
    }
}

fn enforce_policy(policy: &ToolPolicy, method: ToolMethod) -> Result<(), BrokerError> {
    use crate::broker::tools::ToolPermission;
    match method.permission() {
        ToolPermission::MemoryRead if !policy.use_memory => Err(BrokerError::MemoryDisabled),
        ToolPermission::MemoryWrite if !policy.propose_memory_updates => {
            Err(BrokerError::ProposalDisabled)
        }
        _ => Ok(()),
    }
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

fn error_body(error: &BrokerError) -> Value {
    if let BrokerError::ProviderApprovalRequired { provider, services } = error {
        return json!({"code":"needs_approval", "message":error.to_string(), "retryable":false,
            "next":"Check this key and request the missing service approvals in the provider's portal, then verify it in Connections. Use an approved market or another source meanwhile.",
            "connection_id":provider, "services":services, "reason":"provider_service_approval"});
    }
    if let BrokerError::CustomApprovalRequired {
        connection_id,
        tool_name,
    } = error
    {
        return json!({"code":"needs_approval", "message":error.to_string(), "retryable":false,
            "next":"The user can review this connection in Settings before starting a new message.",
            "connection_id":connection_id, "tool_name":tool_name, "reason":"needs_approval"});
    }
    if let BrokerError::WithNext { source, next } = error {
        let mut body = error_body(source);
        body["next"] = json!(next);
        return body;
    }
    let (next, retryable) = match error {
        BrokerError::CustomApprovalRequired { .. } | BrokerError::ProviderApprovalRequired { .. } => unreachable!("handled above"),
        BrokerError::OutboundPaused => (crate::custom_connections::taint::OUTBOUND_REFUSAL, false),
        BrokerError::UntrustedTextPaused => (
            crate::custom_connections::taint::UNTRUSTED_TEXT_REFUSAL,
            false,
        ),
        BrokerError::WithNext { .. } => unreachable!("handled above"),
        BrokerError::MethodDenied => (
            "This tool is not available. Load the capability that provides it, or use a tool that is already active.",
            false,
        ),
        BrokerError::MemoryDisabled => (
            "Do not retry. Use the conversation and attached context, or tell the user Memory needs to be on.",
            false,
        ),
        BrokerError::ProposalDisabled => (
            "Do not retry the save. Finish from the conversation, or tell the user saving needs to be on.",
            false,
        ),
        BrokerError::Malformed | BrokerError::InvalidArguments { .. } => (
            "The arguments did not match the tool schema. Correct them before calling again.",
            false,
        ),
        BrokerError::FrameTooLarge => (
            "The result was too large. Request a smaller page or file instead of repeating this call.",
            false,
        ),
        BrokerError::BudgetExceeded => (
            "This turn has used its limit. Do not retry. Finish with what is already available.",
            false,
        ),
        BrokerError::Authentication => ("The tool request was rejected. Do not retry it.", false),
        BrokerError::Io(_) => (
            "The tool service had a temporary problem. One changed attempt is reasonable; do not loop on the same call.",
            true,
        ),
        BrokerError::Execution(_) => (
            "Change the request or the approach. Do not repeat the identical call.",
            false,
        ),
        BrokerError::Offline(_) => (
            "Do not repeat this read this turn. Other sources may still be reachable; if none is, answer from evidence already available and say that this source could not be reached.",
            false,
        ),
    };
    json!({
        "code": error_code(error),
        "message": error.to_string(),
        "retryable": retryable,
        "next": next,
    })
}

/// As-of refusals keep their own codes so the answer's note can list them
/// (finance-point-in-time.md §3.3).
fn as_of_code(message: &str) -> Option<&'static str> {
    [
        "as_of_unsupported",
        "as_of_empty",
        "as_of_later",
        "as_of_before_check",
    ]
    .into_iter()
    .find(|code| {
        message
            .strip_prefix(code)
            .is_some_and(|rest| rest.starts_with(':'))
    })
}

fn error_code(error: &BrokerError) -> &'static str {
    match error {
        BrokerError::OutboundPaused => "outbound_paused",
        BrokerError::UntrustedTextPaused => "untrusted_text_paused",
        BrokerError::CustomApprovalRequired { .. } => "needs_approval",
        BrokerError::ProviderApprovalRequired { .. } => "needs_approval",
        BrokerError::MethodDenied => "method_denied",
        BrokerError::MemoryDisabled => "memory_disabled",
        BrokerError::ProposalDisabled => "proposal_disabled",
        BrokerError::Authentication => "authentication_failed",
        BrokerError::FrameTooLarge => "frame_too_large",
        BrokerError::InvalidArguments { expected, .. } => {
            as_of_code(expected).unwrap_or("malformed_request")
        }
        BrokerError::Malformed => "malformed_request",
        BrokerError::WithNext { source, .. } => error_code(source),
        BrokerError::Execution(message) => as_of_code(message).unwrap_or("execution_failed"),
        BrokerError::Offline(_) => "offline",
        BrokerError::BudgetExceeded => "transaction_limit",
        BrokerError::Io(_) => "io_failed",
    }
}

/// Bounded, turn-local diagnostics shared by broker calls and observed Pi tools.
/// This is independent of staged delivery state and never participates in ACKs.
pub struct ToolTraceCapture {
    started: std::time::Instant,
    state: StdMutex<ToolTraceState>,
}

#[derive(Default)]
struct ToolTraceState {
    trace: crate::domain::ChatToolTrace,
    observed:
        std::collections::BTreeMap<String, (crate::domain::ChatToolTraceEntry, std::time::Instant)>,
}

impl Default for ToolTraceCapture {
    fn default() -> Self {
        Self {
            started: std::time::Instant::now(),
            state: StdMutex::default(),
        }
    }
}

fn trace_millis(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

// Count serialized bytes without allocating or retaining a provider payload.
fn trace_result_size(value: &Value) -> Option<u64> {
    struct Counter(u64);
    impl io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len() as u64);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value).ok()?;
    Some(counter.0)
}

fn trace_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut end = limit.saturating_sub(14).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[truncated]", &text[..end])
}

fn secret_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "secret",
        "token",
        "key",
        "password",
        "passwd",
        "authorization",
        "credential",
        "cookie",
    ]
    .iter()
    .any(|part| key.contains(part))
}

fn trace_redact(value: &Value, depth: usize) -> Value {
    if depth > 16 {
        return json!({"_truncated": true});
    }
    match value {
        Value::Object(object) if object.len() > 256 => json!({"_truncated": true}),
        Value::Array(array) if array.len() > 256 => json!({"_truncated": true}),
        Value::Object(object) => Value::Object(
            object
                .iter()
                .map(|(key, value)| {
                    (
                        trace_text(key, 256),
                        if secret_key(key) {
                            json!("[REDACTED]")
                        } else {
                            trace_redact(value, depth + 1)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(array) => Value::Array(
            array
                .iter()
                .map(|value| trace_redact(value, depth + 1))
                .collect(),
        ),
        Value::String(text) => {
            // No result body is ever passed to this function.
            if crate::util::credential_text::contains_credential_text(text) {
                json!("[REDACTED]")
            } else {
                json!(trace_text(text, 2048))
            }
        }
        value => value.clone(),
    }
}

fn trace_error_text(value: Option<&Value>) -> Value {
    // Provider error text can echo a credential in prose, so any credential
    // word suppresses the whole message.
    let conservative = value.and_then(Value::as_str).is_some_and(|text| {
        let lower = text.to_ascii_lowercase();
        [
            "bearer ",
            "basic ",
            "sk-",
            "-----begin",
            "password",
            "passwd",
            "api_key",
            "apikey",
            "api-key",
            "access_token",
            "authorization",
            "secret",
            "token=",
            "key=",
        ]
        .iter()
        .any(|needle| lower.contains(needle))
            || crate::util::credential_text::contains_credential_text(text)
    });
    if conservative {
        json!("[REDACTED]")
    } else {
        trace_redact(value.unwrap_or(&Value::Null), 0)
    }
}

fn trace_value(value: &Value) -> Value {
    let redacted = trace_redact(value, 0);
    if serde_json::to_vec(&redacted).map_or(true, |bytes| {
        bytes.len() > crate::domain::MAX_TOOL_TRACE_VALUE_BYTES
    }) {
        json!({"_truncated": true})
    } else {
        redacted
    }
}

fn trace_error(value: &Value) -> Value {
    // Retain the error contract even when the message exceeds the byte budget.
    let mut error = json!({
        "code": value.get("code").and_then(Value::as_str).map(|code| trace_text(code, 128)),
        "message": trace_error_text(value.get("message")),
        "retryable": value.get("retryable").and_then(Value::as_bool),
        "next": trace_error_text(value.get("next")),
    });
    if value.get("code").and_then(Value::as_str) == Some("needs_approval") {
        for field in ["connection_id", "tool_name", "reason"] {
            error[field] = value
                .get(field)
                .and_then(Value::as_str)
                .map(|v| trace_text(v, 128))
                .into();
        }
    }
    for limit in [512, 256, 128, 64] {
        if serde_json::to_vec(&error)
            .is_ok_and(|bytes| bytes.len() <= crate::domain::MAX_TOOL_TRACE_VALUE_BYTES)
        {
            break;
        }
        for key in ["message", "next"] {
            if let Some(text) = error[key].as_str() {
                error[key] = json!(trace_text(text, limit));
            }
        }
    }
    error
}

impl ToolTraceCapture {
    fn entry(
        &self,
        call_id: Option<&str>,
        name: &str,
        component: Option<&str>,
        args: Option<&Value>,
    ) -> crate::domain::ChatToolTraceEntry {
        crate::domain::ChatToolTraceEntry {
            call_id: call_id.map(|id| trace_text(id, 256)),
            name: trace_text(name, 256),
            component: component.map(|id| trace_text(id, 256)),
            args: args.map(trace_value),
            start_offset_ms: trace_millis(self.started.elapsed()),
            duration_ms: 0,
            outcome: crate::domain::ToolTraceOutcome::Error,
            error: None,
            result_bytes: None,
        }
    }

    fn begin(
        self: Arc<Self>,
        call_id: Option<&str>,
        name: &str,
        component: Option<&str>,
        args: Option<&Value>,
    ) -> ToolTraceGuard {
        let entry = self.entry(call_id, name, component, args);
        ToolTraceGuard {
            capture: self,
            entry: Some(entry),
            started: std::time::Instant::now(),
        }
    }

    pub(crate) fn observe_start(&self, call_id: &str, name: &str, args: &Value) {
        if !matches!(
            name,
            "capability_load" | "read" | "write" | "edit" | "bash" | "ls" | "find" | "grep"
        ) {
            return;
        }
        let capability = name == "capability_load";
        let entry = self.entry(
            Some(call_id),
            name,
            if capability {
                args.get("id").and_then(Value::as_str)
            } else {
                None
            },
            if capability { Some(args) } else { None },
        );
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.observed.len() + state.trace.entries.len() >= crate::domain::MAX_TOOL_TRACE_ENTRIES
        {
            state.trace.truncated = true;
            return;
        }
        state
            .observed
            .insert(trace_text(call_id, 256), (entry, std::time::Instant::now()));
    }

    pub(crate) fn observe_end(&self, call_id: &str, failed: bool, result: &Value) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((mut entry, started)) = state.observed.remove(&trace_text(call_id, 256)) {
            entry.duration_ms = trace_millis(started.elapsed());
            entry.outcome = if failed {
                crate::domain::ToolTraceOutcome::Error
            } else {
                crate::domain::ToolTraceOutcome::Ok
            };
            if entry.name == "capability_load" {
                entry.result_bytes = trace_result_size(result);
                if failed {
                    entry.error = Some(trace_error(&json!({
                        "code": "capability_load_failed",
                        "message": result.get("content").and_then(Value::as_array)
                            .and_then(|content| content.iter().find_map(|item| item.get("text").and_then(Value::as_str))),
                        "retryable": false,
                        "next": null,
                    })));
                }
            }
            state.trace.push(entry);
        }
    }

    pub(crate) fn mark_untrusted_text(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .trace
            .untrusted_text = true;
    }

    pub fn snapshot(&self) -> crate::domain::ChatToolTrace {
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let mut trace = state.trace.clone();
        for (entry, started) in state.observed.values() {
            let mut entry = entry.clone();
            entry.duration_ms = trace_millis(started.elapsed());
            entry.error = Some(
                json!({"code": "interrupted", "message": "Tool did not finish before the turn ended", "retryable": false, "next": null}),
            );
            trace.push(entry);
        }
        trace.entries.sort_by_key(|entry| entry.start_offset_ms);
        trace
    }

    /// Whether the host refused a call of this model tool call with `code`
    /// (a native tool's authorization or the broker method itself). The broker
    /// records the refusal before it answers, so it is here by the time Pi
    /// reports the tool as finished.
    pub(crate) fn refused(&self, call_id: &str, code: &str) -> bool {
        let call_id = trace_text(call_id, 256);
        let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.trace.entries.iter().any(|entry| {
            entry.call_id.as_deref() == Some(call_id.as_str())
                && entry
                    .error
                    .as_ref()
                    .and_then(|error| error.get("code"))
                    .and_then(Value::as_str)
                    == Some(code)
        })
    }
}

struct ToolTraceGuard {
    capture: Arc<ToolTraceCapture>,
    entry: Option<crate::domain::ChatToolTraceEntry>,
    started: std::time::Instant,
}

impl ToolTraceGuard {
    fn finish(mut self, error: Option<&Value>, result_bytes: Option<u64>) {
        if let Some(mut entry) = self.entry.take() {
            entry.duration_ms = trace_millis(self.started.elapsed());
            entry.outcome = if error.is_some() {
                crate::domain::ToolTraceOutcome::Error
            } else {
                crate::domain::ToolTraceOutcome::Ok
            };
            entry.error = error.map(trace_error);
            entry.result_bytes = result_bytes;
            self.capture
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .trace
                .push(entry);
        }
    }
}

impl Drop for ToolTraceGuard {
    fn drop(&mut self) {
        if let Some(mut entry) = self.entry.take() {
            entry.duration_ms = trace_millis(self.started.elapsed());
            entry.error = Some(
                json!({"code": "interrupted", "message": "Tool execution was interrupted", "retryable": false, "next": null}),
            );
            self.capture
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .trace
                .push(entry);
        }
    }
}

#[cfg(test)]
mod tests;
