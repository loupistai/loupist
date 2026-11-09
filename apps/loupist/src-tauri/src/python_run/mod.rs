//! Isolated `run_python`.
//!
//! This process snapshots the files one call may see and runs a separate
//! worker. Wasmtime stays in that worker, so the desktop app does not load
//! it. The worker's compiler and the pinned CPython WASI build can use host
//! CPU until this process kills the worker's process group. Guest linear
//! memory is capped at 512 MiB, which is not the process size. Cold compilation
//! also uses host memory and CPU; the private compiled-module cache reduces
//! that cost on later calls. A guest can write only inside the private snapshot.
//! Authorized workspace subfolder writes are copied back on purpose, including
//! changes made before cancellation or a timeout. A concurrent edit that
//! leaves the same bytes is not detected. WASI preview 1 has no filesystem
//! quota beyond these copy bounds and the parent's kill. Live host
//! directories are never writable preopens.

#[cfg(unix)]
mod snapshot;

#[cfg(all(test, unix))]
mod tests;

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

use crate::{agent_harness::native_policy::NativeRoots, broker::BrokerError, web::SearchCancel};

pub(crate) const MAX_SNAPSHOT_FILES: usize = 2_000;
pub(crate) const MAX_SNAPSHOT_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub(crate) const MAX_WRITEBACK_FILES: usize = 256;
pub(crate) const MAX_WRITEBACK_BYTES: u64 = 32 * 1024 * 1024;
pub(crate) const MAX_TREE_ENTRIES: usize = 10_000;
pub(crate) const MAX_WRITABLE_DIRS: usize = 256;

const CODE_BYTES: usize = 64 * 1024;
const OUTPUT_BYTES: usize = 64 * 1024;
const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_TIMEOUT_SECONDS: u64 = 120;
static WORKERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);

#[derive(Debug)]
pub(crate) struct PythonError {
    message: String,
}

impl PythonError {
    fn failed(message: &str) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<PythonError> for BrokerError {
    fn from(error: PythonError) -> Self {
        BrokerError::Execution(error.message)
    }
}

/// `{code, timeout_seconds?}`. Code is raw bytes: it is not trimmed, and a
/// NUL or more than 64 KiB is rejected.
pub(crate) fn parse_params(params: &Value) -> Result<(String, u64), BrokerError> {
    let object = params.as_object().ok_or(BrokerError::Malformed)?;
    if object
        .keys()
        .any(|key| key != "code" && key != "timeout_seconds")
    {
        return Err(BrokerError::Malformed);
    }
    let code = object
        .get("code")
        .and_then(Value::as_str)
        .ok_or_else(|| BrokerError::invalid("code", "a string of 1 to 65536 bytes"))?;
    if code.is_empty() || code.len() > CODE_BYTES || code.contains('\0') {
        return Err(BrokerError::invalid(
            "code",
            "a string of 1 to 65536 bytes without a NUL",
        ));
    }
    let timeout = match object.get("timeout_seconds") {
        None => DEFAULT_TIMEOUT_SECONDS,
        Some(Value::Number(number)) => number
            .as_u64()
            .filter(|seconds| (1..=MAX_TIMEOUT_SECONDS).contains(seconds))
            .ok_or_else(|| BrokerError::invalid("timeout_seconds", "an integer from 1 to 120"))?,
        Some(_) => {
            return Err(BrokerError::invalid(
                "timeout_seconds",
                "an integer from 1 to 120",
            ))
        }
    };
    Ok((code.to_owned(), timeout))
}

pub(crate) async fn execute(
    roots: &NativeRoots,
    cache_root: &Path,
    code: &str,
    timeout_seconds: u64,
    cancel: SearchCancel,
) -> Result<Value, BrokerError> {
    run_owned(roots, cache_root, code, timeout_seconds, cancel, None).await
}

#[cfg(all(test, unix))]
pub(crate) async fn execute_observed(
    roots: &NativeRoots,
    cache_root: &Path,
    code: &str,
    timeout_seconds: u64,
    cancel: SearchCancel,
    started: tokio::sync::oneshot::Sender<PathBuf>,
) -> Result<Value, BrokerError> {
    run_owned(
        roots,
        cache_root,
        code,
        timeout_seconds,
        cancel,
        Some(started),
    )
    .await
}

/// A broker disconnect may drop its request future. Keep the snapshot owned
/// until kill/reap and copy-back finish, and cancel when the caller disappears.
async fn run_owned(
    roots: &NativeRoots,
    cache_root: &Path,
    code: &str,
    timeout_seconds: u64,
    cancel: SearchCancel,
    started: Option<tokio::sync::oneshot::Sender<PathBuf>>,
) -> Result<Value, BrokerError> {
    struct CancelOnDrop(tokio::sync::watch::Sender<bool>);
    impl Drop for CancelOnDrop {
        fn drop(&mut self) {
            self.0.send_replace(true);
        }
    }
    let (sender, receiver) = tokio::sync::watch::channel(false);
    let _caller = CancelOnDrop(sender);
    let (roots, cache_root, code) = (roots.clone(), cache_root.to_path_buf(), code.to_owned());
    tokio::spawn(async move {
        let disconnected = SearchCancel::from_watch(receiver);
        // Bound total compiler/guest RSS for the app, not just each wasm
        // memory. A queued call owns no snapshot and can still be cancelled.
        let _permit = tokio::select! {
            permit = WORKERS.acquire() => permit.map_err(|_| BrokerError::Execution("Python execution is unavailable".into()))?,
            _ = cancel.cancelled() => return Err(BrokerError::Execution("Python execution was cancelled".into())),
            _ = disconnected.cancelled() => return Err(BrokerError::Execution("Python execution was cancelled".into())),
            _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_seconds)) => return Err(BrokerError::Execution("Python execution timed out while waiting for another run".into())),
        };
        drive(
            &roots,
            &cache_root,
            &code,
            timeout_seconds,
            cancel,
            disconnected,
            started,
        )
        .await
    })
    .await
    .map_err(|_| BrokerError::Execution("Python execution failed".into()))?
}

#[cfg(unix)]
async fn drive(
    roots: &NativeRoots,
    cache_root: &Path,
    code: &str,
    timeout_seconds: u64,
    cancel: SearchCancel,
    disconnected: SearchCancel,
    started: Option<tokio::sync::oneshot::Sender<PathBuf>>,
) -> Result<Value, BrokerError> {
    use std::os::unix::fs::PermissionsExt;

    if !(1..=MAX_TIMEOUT_SECONDS).contains(&timeout_seconds)
        || code.is_empty()
        || code.len() > CODE_BYTES
        || code.contains('\0')
    {
        return Err(BrokerError::invalid(
            "code",
            "a string of 1 to 65536 bytes and a timeout from 1 to 120 seconds",
        ));
    }
    let mut secrets = host_secrets(roots);
    let roots = roots.clone();
    let prepared = tokio::task::spawn_blocking(move || snapshot::prepare(&roots))
        .await
        .map_err(|_| BrokerError::Execution("Python execution failed".into()))??;
    let snapshot = prepared.snapshot_dir().to_path_buf();
    if let Some(started) = started {
        let _ = started.send(snapshot.clone());
    }
    let runtime = runtime_dir()
        .ok_or_else(|| BrokerError::Execution("Python runtime is unavailable".into()))?;
    let worker = worker_path()
        .ok_or_else(|| BrokerError::Execution("Python runtime is unavailable".into()))?;
    let cache = ensure_cache(cache_root)?;
    let _ = std::fs::set_permissions(&snapshot, std::fs::Permissions::from_mode(0o700));
    secrets.extend([
        snapshot.clone(),
        cache.clone(),
        runtime.clone(),
        worker.clone(),
    ]);
    let request = WorkerRequest {
        code: code.to_owned(),
        timeout_seconds,
        runtime,
        cache,
        snapshot: snapshot.clone(),
        writable: prepared.writable_names().to_vec(),
    };
    let bytes = serde_json::to_vec(&request)
        .map_err(|_| BrokerError::Execution("Python execution failed".into()))?;
    let omitted = !prepared.omitted.is_empty();
    let untrusted_inputs = prepared.untrusted_inputs;
    let outcome = supervise_worker(
        &worker,
        &snapshot,
        &bytes,
        timeout_seconds,
        cancel,
        disconnected,
    )
    .await;
    let report = tokio::task::spawn_blocking(move || prepared.writeback())
        .await
        .map_err(|_| BrokerError::Execution("Python execution failed".into()))??;
    let mut result = result_value(&outcome?, &report, omitted, &secret_text(&secrets));
    result["untrusted_input"] = json!(untrusted_inputs);
    Ok(result)
}

#[cfg(not(unix))]
async fn drive(
    _roots: &NativeRoots,
    _cache_root: &Path,
    _code: &str,
    _timeout_seconds: u64,
    _cancel: SearchCancel,
    _disconnected: SearchCancel,
    _started: Option<tokio::sync::oneshot::Sender<PathBuf>>,
) -> Result<Value, BrokerError> {
    Err(BrokerError::Execution(
        "Python execution is unavailable".into(),
    ))
}

fn host_secrets(roots: &NativeRoots) -> Vec<PathBuf> {
    let mut secrets = vec![
        roots.workspace.clone(),
        roots.attachments.clone(),
        roots.scratch.clone(),
        roots.app_data.clone(),
        roots.loupist_data.clone(),
        roots.global.clone(),
    ];
    if let Some(home) = &roots.home {
        secrets.push(home.clone());
    }
    secrets
}

fn secret_text(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .filter(|path| path.len() > 3)
        .collect()
}

#[derive(Serialize)]
struct WorkerRequest {
    code: String,
    timeout_seconds: u64,
    runtime: PathBuf,
    cache: PathBuf,
    snapshot: PathBuf,
    writable: Vec<String>,
}

struct WorkerOutcome {
    status: &'static str,
    exit_code: i32,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    stdout_truncated: bool,
    stderr_truncated: bool,
}

#[cfg(unix)]
enum Stop {
    Exit(std::process::ExitStatus),
    Cancel,
    Timeout,
    Output,
}

#[cfg(unix)]
async fn supervise_worker(
    worker: &Path,
    snapshot: &Path,
    request: &[u8],
    timeout_seconds: u64,
    cancel: SearchCancel,
    disconnected: SearchCancel,
) -> Result<WorkerOutcome, BrokerError> {
    use std::time::Duration;
    use tokio::{io::AsyncWriteExt, process::Command};

    let failed = || BrokerError::Execution("Python execution failed".into());
    let mut child = Command::new(worker)
        .process_group(0)
        .env_clear()
        .current_dir(snapshot)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| failed())?;
    let pid = child.id().ok_or_else(failed)?;
    let mut stdin = child.stdin.take().ok_or_else(failed)?;
    let stdout = child.stdout.take().ok_or_else(failed)?;
    let stderr = child.stderr.take().ok_or_else(failed)?;
    let overflow = std::sync::Arc::new(tokio::sync::Notify::new());
    let stdout_task = tokio::spawn(read_capped(stdout, overflow.clone()));
    let stderr_task = tokio::spawn(read_capped(stderr, overflow.clone()));
    stdin.write_all(request).await.map_err(|_| failed())?;
    drop(stdin);

    let stop = tokio::select! {
        biased;
        status = child.wait() => Stop::Exit(status.map_err(|_| failed())?),
        _ = cancel.cancelled() => Stop::Cancel,
        _ = disconnected.cancelled() => Stop::Cancel,
        _ = tokio::time::sleep(Duration::from_secs(timeout_seconds)) => Stop::Timeout,
        _ = overflow.notified() => Stop::Output,
    };
    let status = match &stop {
        Stop::Exit(status) => *status,
        Stop::Cancel | Stop::Timeout | Stop::Output => {
            kill_group(pid);
            match tokio::time::timeout(Duration::from_secs(2), child.wait()).await {
                Ok(Ok(status)) => status,
                _ => {
                    kill_group(pid);
                    child.wait().await.map_err(|_| failed())?
                }
            }
        }
    };
    let (stdout, stdout_truncated) = stdout_task.await.unwrap_or_default();
    let (mut stderr, stderr_truncated) = stderr_task.await.unwrap_or_default();
    let exit_code = exit_code(&status);
    let (name, fallback) = match stop {
        Stop::Cancel => ("cancelled", "Python execution was cancelled"),
        Stop::Timeout => ("timed_out", "Python execution timed out"),
        Stop::Output => ("failed", "Python execution exceeded an output limit"),
        Stop::Exit(_) if exit_code == 0 => ("completed", ""),
        Stop::Exit(_) if exit_code == 124 => ("timed_out", "Python execution timed out"),
        Stop::Exit(_) if exit_code == 126 => {
            ("failed", "Python execution exceeded a resource limit")
        }
        Stop::Exit(_) => ("failed", "Python execution failed"),
    };
    if stderr.is_empty() && !fallback.is_empty() {
        stderr.extend_from_slice(fallback.as_bytes());
    }
    Ok(WorkerOutcome {
        status: name,
        exit_code,
        stdout,
        stderr,
        stdout_truncated,
        stderr_truncated,
    })
}

#[cfg(unix)]
async fn read_capped(
    mut pipe: impl tokio::io::AsyncRead + Unpin,
    overflow: std::sync::Arc<tokio::sync::Notify>,
) -> (Vec<u8>, bool) {
    use tokio::io::AsyncReadExt;
    let mut output = Vec::new();
    let mut buffer = [0u8; 8 * 1024];
    loop {
        let count = match pipe.read(&mut buffer).await {
            Ok(0) | Err(_) => return (output, false),
            Ok(count) => count,
        };
        let room = OUTPUT_BYTES.saturating_sub(output.len());
        if count > room {
            output.extend_from_slice(&buffer[..room]);
            overflow.notify_one();
            return (output, true);
        }
        output.extend_from_slice(&buffer[..count]);
    }
}

#[cfg(unix)]
fn kill_group(pid: u32) {
    let pid = pid as i32;
    if pid > 1 {
        let _ = crate::process_lease::signal_process_group(pid, libc::SIGKILL);
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
}

#[cfg(unix)]
fn exit_code(status: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    if let Some(code) = status.code() {
        return code.clamp(0, 255);
    }
    status
        .signal()
        .map(|signal| (128 + signal).clamp(0, 255))
        .unwrap_or(1)
}

#[cfg(unix)]
fn result_value(
    outcome: &WorkerOutcome,
    report: &snapshot::WriteReport,
    omitted: bool,
    secrets: &[String],
) -> Value {
    let mut stderr = redact(&outcome.stderr, secrets);
    if report.truncated || omitted {
        if !stderr.is_empty() && !stderr.ends_with('\n') {
            stderr.push('\n');
        }
        stderr.push_str(if report.truncated {
            "Some workspace changes were not copied back.\n"
        } else {
            "Some files were left out of the snapshot because they are too large.\n"
        });
    }
    json!({
        "status": outcome.status,
        "exit_code": outcome.exit_code,
        "stdout": redact(&outcome.stdout, secrets),
        "stderr": stderr,
        "stdout_truncated": outcome.stdout_truncated,
        "stderr_truncated": outcome.stderr_truncated,
        "changed_files": report.changed,
        "conflicts": report.conflicts.iter().map(|conflict| json!({
            "path": conflict.path,
            "reason": conflict.reason,
        })).collect::<Vec<_>>(),
        "artifacts": report.artifacts.iter().map(|artifact| json!({
            "path": artifact.path,
            "kind": artifact.kind,
            "media_type": artifact.media_type,
            "bytes": artifact.bytes,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(unix)]
fn redact(bytes: &[u8], secrets: &[String]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    for secret in secrets {
        text = text.replace(secret.as_str(), "[path]");
    }
    text
}

#[cfg(unix)]
fn ensure_cache(root: &Path) -> Result<PathBuf, BrokerError> {
    use std::os::unix::fs::PermissionsExt;
    let cache = root.join("python-cache");
    std::fs::create_dir_all(&cache)
        .map_err(|_| BrokerError::Execution("Python execution failed".into()))?;
    let _ = std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o700));
    Ok(cache)
}

pub(crate) fn runtime_dir() -> Option<PathBuf> {
    if let Some(path) = env_dir("LOUPIST_PYTHON_RUNTIME") {
        return Some(path);
    }
    #[cfg(debug_assertions)]
    {
        let staged = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/python-runtime");
        if runtime_ready(&staged) {
            return Some(staged);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    [
        parent.join("python-runtime"),
        parent.join("../Resources/python-runtime"),
    ]
    .into_iter()
    .find(|path| runtime_ready(path))
}

fn env_dir(name: &str) -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var(name).ok()?);
    runtime_ready(&path).then_some(path)
}

fn runtime_ready(path: &Path) -> bool {
    path.join("python-snapshot.wasm").is_file()
        && path.join("lib").is_dir()
        && path.join("mpl-data").is_dir()
}

pub(crate) fn worker_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("LOUPIST_PYTHON_WORKER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some(path);
        }
    }
    #[cfg(debug_assertions)]
    {
        let built = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../python-worker/target/release/loupist-python-worker");
        if built.is_file() {
            return Some(built);
        }
    }
    runtime_dir().and_then(|dir| {
        let staged = dir.join("loupist-python-worker");
        staged.is_file().then_some(staged)
    })
}
