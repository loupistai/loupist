use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use tokio::sync::{watch, Mutex};

use crate::{
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

struct McpProcessPoolInner {
    idle: VecDeque<IdleMcpProcess>,
}

struct PoolLifecycle {
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

impl McpProcessPoolInner {
    fn take_expired(&mut self, ttl: Duration) -> Vec<TurnMcpServer> {
        let now = Instant::now();
        let mut kept = VecDeque::new();
        let mut discarded = Vec::new();
        while let Some(entry) = self.idle.pop_front() {
            if now.saturating_duration_since(entry.idle_since) >= ttl {
                discarded.push(entry.server);
            } else {
                kept.push_back(entry);
            }
        }
        self.idle = kept;
        discarded
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
            })),
            lifecycle: Arc::new(PoolLifecycle {
                closing: AtomicBool::new(false),
                releases: AtomicUsize::new(0),
                changes,
            }),
        }
    }

    pub(crate) fn create_scratch(
        &self,
        loupist_id: &str,
        server_id: &str,
    ) -> Result<PathBuf, String> {
        if self.lifecycle.closing.load(Ordering::Acquire) {
            return Err("MCP pool is closing".into());
        }
        if !valid_server_id(server_id) {
            return Err("invalid MCP server id".into());
        }
        let loupist = sha256(loupist_id.as_bytes());
        let token = uuid::Uuid::new_v4().simple();
        let path = self
            .root
            .join(format!("{}-{server_id}-{token}", &loupist[..12]));
        create_private_directory(&path).map_err(|error| error.to_string())?;
        Ok(path)
    }

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
                while inner.idle.len() >= self.max_idle {
                    if let Some(oldest) = inner.idle.pop_front() {
                        discarded.push(oldest.server);
                    }
                }
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

    pub(crate) async fn shutdown(&self) {
        self.close_admission();
        let idle = {
            let mut inner = self.inner.lock().await;
            std::mem::take(&mut inner.idle)
        };
        for entry in idle {
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
        assert!(pool.create_scratch("loupist", "../openbb").is_err());
        assert!(pool.create_scratch("loupist", "openbb/extra").is_err());
        assert!(pool.create_scratch("loupist", "").is_err());
        let scratch = pool.create_scratch("loupist-a", "openbb").unwrap();
        assert!(scratch.starts_with(temporary.path()));
    }
}
