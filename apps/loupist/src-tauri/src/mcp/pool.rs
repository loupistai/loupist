use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    future::Future,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tokio::{
    sync::{oneshot, watch, Mutex},
    task::JoinHandle,
};

use crate::{
    broker::BrokerError,
    fs::artifact_trust::{create_private_directory, ensure_private_directory},
    mcp::{McpError, McpSession, McpTool, ProviderlessToolPolicy},
    util::sha256,
};

const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(8 * 60);
const DEFAULT_MAX_IDLE: usize = 4;
const RESET_TIMEOUT: Duration = Duration::from_secs(15);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(2);
const DEACTIVATE_TOOLS: &str = "deactivate_tools";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct McpPoolKey {
    pub loupist_id: String,
    pub server_id: String,
    pub authority_fingerprint: String,
}

/// What a process received when it started: its key without the Loupist. A
/// process that has served no turn holds no Loupist's state, so the first turn
/// whose authority matches may lease it from any Loupist.
#[derive(Clone, Debug, Eq, PartialEq)]
struct StartKey {
    server_id: String,
    authority_fingerprint: String,
}

impl StartKey {
    fn of(key: &McpPoolKey) -> Self {
        Self {
            server_id: key.server_id.clone(),
            authority_fingerprint: key.authority_fingerprint.clone(),
        }
    }
}

pub(crate) type McpStartResult = Result<TurnMcpServer, BrokerError>;

pub(crate) struct TurnMcpServer {
    pub session: McpSession,
    pub control_tool_names: BTreeSet<String>,
    pub tools: BTreeMap<String, McpTool>,
    pub enabled_provider_ids: BTreeSet<String>,
    pub providerless_tool_policy: ProviderlessToolPolicy,
    pub provider_receipt_pointer: String,
    pub sensitive_values: Vec<String>,
    pub pool_key: Option<McpPoolKey>,
    pub scratch_dir: Option<PathBuf>,
}

struct IdleMcpProcess {
    key: McpPoolKey,
    server: TurnMcpServer,
    idle_since: Instant,
}

/// Started by the pool and not yet leased by any turn.
struct FreshMcpProcess {
    key: StartKey,
    server: TurnMcpServer,
    idle_since: Instant,
}

struct StartingMcpProcess {
    id: u64,
    key: StartKey,
    /// The one turn waiting for this start. A start with no waiter, or whose
    /// waiter stopped waiting, becomes a fresh idle process.
    waiter: Option<oneshot::Sender<McpStartResult>>,
    task: Option<JoinHandle<()>>,
}

impl StartingMcpProcess {
    fn awaited(&self) -> bool {
        self.waiter
            .as_ref()
            .is_some_and(|waiter| !waiter.is_closed())
    }
}

struct McpProcessPoolInner {
    idle: VecDeque<IdleMcpProcess>,
    fresh: VecDeque<FreshMcpProcess>,
    starting: Vec<StartingMcpProcess>,
    next_start: u64,
}

struct PoolLifecycle {
    openbb_connect_failed: AtomicBool,
    closing: AtomicBool,
    releases: AtomicUsize,
    changes: watch::Sender<u64>,
}

struct ReleaseLease(Arc<PoolLifecycle>);

impl Drop for ReleaseLease {
    fn drop(&mut self) {
        self.0.releases.fetch_sub(1, Ordering::AcqRel);
        self.0
            .changes
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}

enum FreshLease {
    Ready(Box<TurnMcpServer>),
    Waiting(oneshot::Receiver<McpStartResult>),
    Closed,
}

impl McpProcessPoolInner {
    fn take_expired(&mut self, ttl: Duration) -> Vec<TurnMcpServer> {
        let now = Instant::now();
        let mut discarded = Vec::new();
        let mut kept = VecDeque::new();
        while let Some(entry) = self.idle.pop_front() {
            if now.saturating_duration_since(entry.idle_since) >= ttl {
                discarded.push(entry.server);
            } else {
                kept.push_back(entry);
            }
        }
        self.idle = kept;
        let mut kept = VecDeque::new();
        while let Some(entry) = self.fresh.pop_front() {
            if now.saturating_duration_since(entry.idle_since) >= ttl {
                discarded.push(entry.server);
            } else {
                kept.push_back(entry);
            }
        }
        self.fresh = kept;
        discarded
    }

    /// Evicts the longest-idle process, leased before or fresh, until one more fits.
    fn make_room(&mut self, max_idle: usize, discarded: &mut Vec<TurnMcpServer>) {
        while self.idle.len() + self.fresh.len() >= max_idle {
            let fresh_is_older = match (self.idle.front(), self.fresh.front()) {
                (Some(idle), Some(fresh)) => fresh.idle_since <= idle.idle_since,
                (None, Some(_)) => true,
                (_, None) => false,
            };
            let oldest = if fresh_is_older {
                self.fresh.pop_front().map(|entry| entry.server)
            } else {
                self.idle.pop_front().map(|entry| entry.server)
            };
            match oldest {
                Some(server) => discarded.push(server),
                None => break,
            }
        }
    }

    fn place_fresh(
        &mut self,
        key: StartKey,
        server: TurnMcpServer,
        max_idle: usize,
        discarded: &mut Vec<TurnMcpServer>,
    ) {
        // One started process per authority already hides the next start.
        if self.fresh.iter().any(|entry| entry.key == key) {
            discarded.push(server);
            return;
        }
        self.make_room(max_idle, discarded);
        self.fresh.push_back(FreshMcpProcess {
            key,
            server,
            idle_since: Instant::now(),
        });
    }
}

#[derive(Clone)]
pub(crate) struct McpProcessPool {
    root: PathBuf,
    idle_ttl: Duration,
    max_idle: usize,
    inner: std::sync::Arc<Mutex<McpProcessPoolInner>>,
    lifecycle: Arc<PoolLifecycle>,
}

impl McpProcessPool {
    pub(crate) fn prepare(root: PathBuf) -> Result<Self, String> {
        ensure_private_directory(&root).map_err(|error| error.to_string())?;
        sweep_pool_root(&root);
        Ok(Self::with_limits(root, DEFAULT_IDLE_TTL, DEFAULT_MAX_IDLE))
    }

    pub(crate) fn with_limits(root: PathBuf, idle_ttl: Duration, max_idle: usize) -> Self {
        let (changes, _) = watch::channel(0);
        Self {
            root,
            idle_ttl,
            max_idle: max_idle.max(1),
            inner: std::sync::Arc::new(Mutex::new(McpProcessPoolInner {
                idle: VecDeque::new(),
                fresh: VecDeque::new(),
                starting: Vec::new(),
                next_start: 0,
            })),
            lifecycle: Arc::new(PoolLifecycle {
                openbb_connect_failed: AtomicBool::new(false),
                closing: AtomicBool::new(false),
                releases: AtomicUsize::new(0),
                changes,
            }),
        }
    }

    pub(crate) fn record_openbb_connect(&self, connected: bool) {
        self.lifecycle
            .openbb_connect_failed
            .store(!connected, Ordering::Relaxed);
    }

    pub(crate) fn openbb_connect_failed(&self) -> bool {
        self.lifecycle.openbb_connect_failed.load(Ordering::Relaxed)
    }

    /// A private working directory for one process. It carries no Loupist,
    /// because a fresh process may go to any Loupist with matching authority.
    pub(crate) fn create_scratch(&self, server_id: &str) -> Result<PathBuf, String> {
        if self.lifecycle.closing.load(Ordering::Acquire) {
            return Err("MCP pool is closing".into());
        }
        if !valid_server_id(server_id) {
            return Err("invalid MCP server id".into());
        }
        let token = uuid::Uuid::new_v4().simple();
        let path = self.root.join(format!("{server_id}-{token}"));
        create_private_directory(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }

    /// A process this Loupist's earlier turn left with the same authority.
    pub(crate) async fn acquire(&self, key: &McpPoolKey) -> Option<TurnMcpServer> {
        let mut discarded = Vec::new();
        let found = {
            let mut inner = self.inner.lock().await;
            discarded.extend(inner.take_expired(self.idle_ttl));
            if self.lifecycle.closing.load(Ordering::Acquire) {
                None
            } else {
                inner
                    .idle
                    .iter()
                    .position(|entry| entry.key == *key)
                    .and_then(|index| inner.idle.remove(index))
                    .map(|entry| entry.server)
            }
        };
        for server in discarded {
            server.discard().await;
        }
        found
    }

    /// A process for `key` that has served no turn: one already started, one
    /// still starting (this turn waits for it rather than starting another),
    /// or a new `start`. The pool owns every start, so a turn that stops
    /// waiting leaves the process to the next turn with the same authority.
    /// `start` runs only when neither exists.
    pub(crate) async fn lease_fresh<F>(&self, key: &McpPoolKey, start: F) -> McpStartResult
    where
        F: Future<Output = McpStartResult> + Send + 'static,
    {
        let start_key = StartKey::of(key);
        let (lease, discarded) = {
            let mut inner = self.inner.lock().await;
            let mut discarded = inner.take_expired(self.idle_ttl);
            let mut ready = None;
            while let Some(entry) = inner
                .fresh
                .iter()
                .position(|entry| entry.key == start_key)
                .and_then(|index| inner.fresh.remove(index))
            {
                // A process that exited while idle is not handed out.
                if matches!(entry.server.session.is_running().await, Ok(true)) {
                    ready = Some(entry.server);
                    break;
                }
                discarded.push(entry.server);
            }
            let lease = if self.lifecycle.closing.load(Ordering::Acquire) {
                discarded.extend(ready);
                FreshLease::Closed
            } else if let Some(server) = ready {
                FreshLease::Ready(Box::new(server))
            } else {
                let (sender, receiver) = oneshot::channel();
                match inner
                    .starting
                    .iter_mut()
                    .find(|starting| starting.key == start_key && !starting.awaited())
                {
                    Some(starting) => starting.waiter = Some(sender),
                    None => self.begin_start(&mut inner, start_key, Some(sender), start),
                }
                FreshLease::Waiting(receiver)
            };
            (lease, discarded)
        };
        for server in discarded {
            server.discard().await;
        }
        match lease {
            FreshLease::Ready(server) => Ok(*server),
            // Shutdown dropped the start before it finished.
            FreshLease::Waiting(receiver) => receiver.await.unwrap_or_else(|_| Err(stopped())),
            FreshLease::Closed => Err(stopped()),
        }
    }

    /// Starts one process for `key` in the background unless one with the
    /// same authority has started or is starting. Nothing waits for it; a turn
    /// leases it through `lease_fresh`, and unleased it expires like any idle
    /// process. Returns whether a start began.
    pub(crate) async fn prewarm<F>(&self, key: &McpPoolKey, start: F) -> bool
    where
        F: Future<Output = McpStartResult> + Send + 'static,
    {
        let start_key = StartKey::of(key);
        let mut inner = self.inner.lock().await;
        if self.lifecycle.closing.load(Ordering::Acquire)
            || inner.fresh.iter().any(|entry| entry.key == start_key)
            || inner
                .starting
                .iter()
                .any(|starting| starting.key == start_key)
        {
            return false;
        }
        self.begin_start(&mut inner, start_key, None, start);
        true
    }

    fn begin_start<F>(
        &self,
        inner: &mut McpProcessPoolInner,
        key: StartKey,
        waiter: Option<oneshot::Sender<McpStartResult>>,
        start: F,
    ) where
        F: Future<Output = McpStartResult> + Send + 'static,
    {
        let id = inner.next_start;
        inner.next_start = inner.next_start.wrapping_add(1);
        let pool = self.clone();
        // The entry is published under the lock this task needs to finish,
        // so it always finds its entry unless shutdown took it. The start runs
        // as its own task so a panic still answers the waiting turn; aborting
        // this task drops the guard, which aborts the start and its process.
        let task = tokio::spawn(async move {
            let mut start = AbortOnDrop(tokio::spawn(start));
            let result = (&mut start.0).await.unwrap_or_else(|_| Err(stopped()));
            pool.finish_start(id, result).await;
        });
        inner.starting.push(StartingMcpProcess {
            id,
            key,
            waiter,
            task: Some(task),
        });
    }

    async fn finish_start(&self, id: u64, result: McpStartResult) {
        let mut discarded = Vec::new();
        {
            let mut inner = self.inner.lock().await;
            let entry = inner
                .starting
                .iter()
                .position(|starting| starting.id == id)
                .map(|index| inner.starting.swap_remove(index));
            match entry {
                Some(StartingMcpProcess {
                    key, mut waiter, ..
                }) => {
                    let unclaimed = match waiter.take() {
                        Some(waiter) => waiter.send(result).err(),
                        None => Some(result),
                    };
                    if let Some(Ok(server)) = unclaimed {
                        if self.lifecycle.closing.load(Ordering::Acquire) {
                            discarded.push(server);
                        } else {
                            inner.place_fresh(key, server, self.max_idle, &mut discarded);
                        }
                    }
                }
                // Shutdown already took this start: nobody may lease its process.
                None => discarded.extend(result.ok()),
            }
        }
        for server in discarded {
            server.discard().await;
        }
    }

    pub(crate) async fn release(&self, mut server: TurnMcpServer) {
        let _release = self.track_release();
        if server.pool_key.is_none() || self.lifecycle.closing.load(Ordering::Acquire) {
            server.discard().await;
            return;
        }
        match server.session.is_running().await {
            Ok(true) => {}
            _ => {
                server.discard().await;
                return;
            }
        }
        let reset = tokio::select! {
            biased;
            _ = self.wait_closing() => false,
            result = server.reset_to_control_surface() => result.is_ok(),
        };
        if !reset {
            server.discard().await;
            return;
        }
        let key = server
            .pool_key
            .clone()
            .expect("pooled MCP server lost its key");
        let mut discarded = Vec::new();
        {
            let mut inner = self.inner.lock().await;
            discarded.extend(inner.take_expired(self.idle_ttl));
            if self.lifecycle.closing.load(Ordering::Acquire) {
                discarded.push(server);
            } else {
                // Two turns of one Loupist can finish with the same authority.
                if let Some(index) = inner.idle.iter().position(|entry| entry.key == key) {
                    if let Some(previous) = inner.idle.remove(index) {
                        discarded.push(previous.server);
                    }
                }
                inner.make_room(self.max_idle, &mut discarded);
                inner.idle.push_back(IdleMcpProcess {
                    key,
                    server,
                    idle_since: Instant::now(),
                });
            }
        }
        for server in discarded {
            server.discard().await;
        }
    }

    /// Quarantine turn-owned servers while they reset, then make only the
    /// successfully reset processes visible to future acquires. The owned
    /// server values stay outside `idle` for the entire reset, so a new turn
    /// cannot lease partially reset authority or tool state.
    pub(crate) fn release_in_background(&self, servers: Vec<TurnMcpServer>) {
        if servers.is_empty() {
            return;
        }
        let pool = self.clone();
        let release = self.track_release();
        let _reset = tokio::spawn(async move {
            let _release = release;
            for server in servers {
                pool.release(server).await;
            }
        });
    }

    #[cfg(test)]
    pub(crate) async fn idle_count(&self) -> usize {
        self.inner.lock().await.idle.len()
    }

    #[cfg(test)]
    pub(crate) async fn fresh_count(&self) -> usize {
        self.inner.lock().await.fresh.len()
    }

    #[cfg(test)]
    pub(crate) async fn starting_count(&self) -> usize {
        self.inner.lock().await.starting.len()
    }

    fn track_release(&self) -> ReleaseLease {
        self.lifecycle.releases.fetch_add(1, Ordering::AcqRel);
        ReleaseLease(self.lifecycle.clone())
    }

    pub(crate) fn close_admission(&self) {
        self.lifecycle.closing.store(true, Ordering::Release);
        self.lifecycle
            .changes
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }

    async fn wait_closing(&self) {
        let mut changes = self.lifecycle.changes.subscribe();
        while !self.lifecycle.closing.load(Ordering::Acquire) {
            if changes.changed().await.is_err() {
                break;
            }
        }
    }

    /// Stops every process the pool owns: idle, fresh and still starting. An
    /// aborted start drops its process, which ends the process group; a turn
    /// waiting for it gets an error.
    pub(crate) async fn shutdown(&self) {
        self.close_admission();
        let (idle, fresh, starting) = {
            let mut inner = self.inner.lock().await;
            (
                std::mem::take(&mut inner.idle),
                std::mem::take(&mut inner.fresh),
                std::mem::take(&mut inner.starting),
            )
        };
        for mut start in starting {
            if let Some(task) = start.task.take() {
                task.abort();
                let _ = task.await;
            }
        }
        for entry in idle {
            entry.server.discard().await;
        }
        for entry in fresh {
            entry.server.discard().await;
        }
        let mut changes = self.lifecycle.changes.subscribe();
        while self.lifecycle.releases.load(Ordering::Acquire) != 0 {
            if changes.changed().await.is_err() {
                break;
            }
        }
    }
}

fn stopped() -> BrokerError {
    BrokerError::Execution("MCP turn stopped".into())
}

struct AbortOnDrop<T>(JoinHandle<T>);

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl TurnMcpServer {
    pub(crate) async fn reset_to_control_surface(&mut self) -> Result<(), McpError> {
        let listed = self.session.list_tools().await?;
        let extras = listed
            .iter()
            .map(|tool| tool.name.clone())
            .filter(|name| !self.control_tool_names.contains(name))
            .collect::<Vec<_>>();
        if !extras.is_empty() {
            if !self.control_tool_names.contains(DEACTIVATE_TOOLS) {
                return Err(McpError::Protocol);
            }
            let result = self
                .session
                .call_tool(
                    DEACTIVATE_TOOLS,
                    json!({ "tool_names": extras }),
                    RESET_TIMEOUT,
                )
                .await?;
            if result.is_error {
                return Err(McpError::Protocol);
            }
        }
        let listed = self.session.list_tools().await?;
        let names = listed
            .iter()
            .map(|tool| tool.name.clone())
            .collect::<BTreeSet<_>>();
        if names != self.control_tool_names {
            return Err(McpError::Protocol);
        }
        self.tools = listed
            .into_iter()
            .map(|tool| (tool.name.clone(), tool))
            .collect();
        let _ = self.session.take_tools_changed();
        Ok(())
    }

    pub(crate) async fn discard(self) {
        let Self {
            session,
            scratch_dir,
            ..
        } = self;
        let _ = session.shutdown(SHUTDOWN_GRACE).await;
        if let Some(path) = scratch_dir {
            let _ = tokio::fs::remove_dir_all(path).await;
        }
    }
}

pub(crate) fn authority_fingerprint(canonical: &Value) -> String {
    sha256(&serde_json::to_vec(canonical).expect("MCP authority fingerprint must encode"))
}

fn valid_server_id(server_id: &str) -> bool {
    let bytes = server_id.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn sweep_pool_root(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        } else {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_scratch_rejects_unsafe_server_ids() {
        let temporary = tempfile::tempdir().unwrap();
        let pool = McpProcessPool::with_limits(temporary.path().to_path_buf(), DEFAULT_IDLE_TTL, 1);
        assert!(pool.create_scratch("../openbb").is_err());
        assert!(pool.create_scratch("openbb/extra").is_err());
        assert!(pool.create_scratch("").is_err());
        let scratch = pool.create_scratch("openbb").unwrap();
        assert!(scratch.starts_with(temporary.path()));
        pool.close_admission();
        assert!(pool.create_scratch("openbb").is_err());
    }
}
