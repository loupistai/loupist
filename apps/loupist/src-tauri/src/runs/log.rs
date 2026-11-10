//! Content-free, local run diagnostics. Never pass payloads or error bodies here.
use crate::{
    app::{AppState, CommandError},
    fs::secure_delete::{PrivateDirectoryGuard, SecureDeletionRoot},
    store::LoupistStore,
};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const DAILY_BYTES: usize = 20 * 1024 * 1024;
const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RunLog {
    pub kind: String,
    pub loupist_id: Option<String>,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,
    pub started_at_ms: i64,
    pub duration_ms: u64,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub usage: Option<Usage>,
    pub tools: BTreeMap<String, u64>,
    pub refused_tools: Vec<RefusedTool>,
    pub memory_changes: BTreeMap<String, u64>,
    pub notes: Vec<String>,
    /// World jobs: `llm` and `embedding` calls the host made for the job.
    /// Requests the host refused are not counted.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inference_calls: BTreeMap<String, u64>,
    /// World jobs: milliseconds paid model calls took, summed per extraction
    /// stage. Calls sent together overlap, so the sum can exceed the duration.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stage_ms: BTreeMap<String, u64>,
    /// World jobs: how long the job waited in the queue before this attempt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_wait_ms: Option<u64>,
    /// Chat turns: duration of the turn's first OpenBB call. It includes the
    /// worker's cold start when no pooled OpenBB process was ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openbb_first_call_ms: Option<u64>,
    #[serde(skip)]
    observed_calls: BTreeSet<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Usage {
    pub input: u64,
    pub cached_input: u64,
    pub output: u64,
}
#[derive(Default)]
pub(crate) struct InferenceUsage {
    started: u64,
    finished: u64,
    unknown: bool,
    input: u64,
    cached_input: u64,
    output: u64,
}
impl InferenceUsage {
    pub fn begin(&mut self) {
        self.started += 1;
    }
    pub fn observe(&mut self, usage: &serde_json::Value, not_sent: bool) {
        self.finished += 1;
        if not_sent {
            return;
        }
        match (
            usage["input"].as_u64(),
            usage["cacheRead"].as_u64(),
            usage["cacheWrite"].as_u64(),
            usage["output_tokens"].as_u64(),
        ) {
            (Some(input), Some(cached), Some(write), Some(output)) => {
                self.input = self.input.saturating_add(input).saturating_add(write);
                self.cached_input = self.cached_input.saturating_add(cached);
                self.output = self.output.saturating_add(output);
            }
            _ => self.unknown = true,
        }
    }
    pub fn snapshot(&self) -> Option<Usage> {
        (!self.unknown && self.started == self.finished).then_some(Usage {
            input: self.input,
            cached_input: self.cached_input,
            output: self.output,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RefusedTool {
    pub name: String,
    pub reason: String,
}

impl RunLog {
    pub fn new(kind: &str, loupist: &str, run: &str, started: i64) -> Self {
        Self {
            kind: kind.into(),
            loupist_id: Some(loupist.into()),
            run_id: run.into(),
            chat_id: None,
            started_at_ms: started,
            duration_ms: 0,
            status: "ok".into(),
            failure_kind: None,
            model: None,
            effort: None,
            usage: None,
            tools: BTreeMap::new(),
            refused_tools: vec![],
            memory_changes: BTreeMap::new(),
            notes: vec![],
            inference_calls: BTreeMap::new(),
            stage_ms: BTreeMap::new(),
            queue_wait_ms: None,
            openbb_first_call_ms: None,
            observed_calls: BTreeSet::new(),
        }
    }
    pub fn finish(&mut self, status: &str, failure: Option<&str>) {
        self.duration_ms = crate::commands::now_ms()
            .saturating_sub(self.started_at_ms)
            .max(0) as u64;
        self.status = status.into();
        self.failure_kind = failure.map(str::to_owned);
    }
    pub fn model(&mut self, model: &crate::providers::catalog::ExecutionModelLock) {
        self.model = Some(format!("{}/{}", model.provider, model.model));
        self.effort = Some(model.thinking_level.clone());
    }
    pub fn message(&mut self, message: &crate::domain::ChatMessage) {
        if let Some(model) = &message.execution_model {
            self.model(model);
        }
        if let Some(p) = &message.performance {
            self.usage = Some(Usage {
                input: p.input_tokens.saturating_add(p.cache_write_tokens),
                cached_input: p.cache_read_tokens,
                output: p.output_tokens,
            });
        }
        self.refusals(&message.tool_refusals);
        if let Some(trace) = &message.tool_trace {
            self.trace(trace);
        }
        if let Some(update) = &message.memory_update {
            self.memory(update);
        }
    }
    pub fn observe_pi_tool(&mut self, event: &serde_json::Value) {
        if event.get("type").and_then(serde_json::Value::as_str) != Some("tool_execution_start") {
            return;
        }
        if let (Some(id), Some(name)) = (
            event.get("toolCallId").and_then(serde_json::Value::as_str),
            event.get("toolName").and_then(serde_json::Value::as_str),
        ) {
            if self.observed_calls.insert(id.to_owned()) {
                *self.tools.entry(name.to_owned()).or_default() += 1;
            }
        }
    }
    pub fn trace(&mut self, trace: &crate::domain::ChatToolTrace) {
        for entry in &trace.entries {
            if !entry
                .call_id
                .as_ref()
                .is_some_and(|id| self.observed_calls.contains(id))
            {
                *self.tools.entry(entry.name.clone()).or_default() += 1;
            }
            if let Some(code) = entry
                .error
                .as_ref()
                .and_then(|e| e.get("code"))
                .and_then(serde_json::Value::as_str)
            {
                if matches!(
                    code,
                    "needs_approval"
                        | "outbound_paused"
                        | "untrusted_text_paused"
                        | "method_denied"
                        | "memory_disabled"
                        | "proposal_disabled"
                        | "authentication_failed"
                        | "budget_exceeded"
                ) && !self
                    .refused_tools
                    .iter()
                    .any(|r| r.name == entry.name && r.reason == code)
                {
                    self.refused_tools.push(RefusedTool {
                        name: entry.name.clone(),
                        reason: code.into(),
                    });
                }
            }
        }
        if trace.truncated && self.observed_calls.is_empty() {
            self.notes.push("Tool counts are partial.".into());
        }
        // The broker traces MCP calls under their server id. A capability
        // load names the loaded capability, so it is not an OpenBB call.
        self.openbb_first_call_ms = trace
            .entries
            .iter()
            .filter(|entry| {
                entry.component.as_deref() == Some("openbb") && entry.name != "capability_load"
            })
            .min_by_key(|entry| entry.start_offset_ms)
            .map(|entry| entry.duration_ms);
    }
    pub fn refusals(&mut self, refusals: &[crate::domain::ChatToolRefusal]) {
        self.refused_tools = refusals
            .iter()
            .map(|r| RefusedTool {
                name: r.tool_name.clone(),
                reason: enum_name(&r.reason),
            })
            .collect();
    }
    pub fn memory(&mut self, update: &crate::domain::MemoryUpdateResult) {
        self.memory_changes.clear();
        for change in &update.changes {
            *self.memory_changes.entry(change.kind.clone()).or_default() += 1;
        }
    }
    pub fn record_app(&self, app_data: &Path) {
        if append(app_data, self, &[], crate::commands::now_ms(), DAILY_BYTES).is_err() {
            eprintln!("Run diagnostics could not be written");
        }
    }
    pub fn record(&self, store: &dyn LoupistStore) {
        let result = store
            .get_loupist(self.loupist_id.as_deref().unwrap_or(""))
            .ok()
            .flatten()
            .ok_or_else(|| CommandError::not_found("Loupist"))
            .and_then(|loupist| {
                append(
                    &Path::new(&loupist.workspace_root).join(".loupist"),
                    self,
                    &[],
                    crate::commands::now_ms(),
                    DAILY_BYTES,
                )
            });
        if result.is_err() {
            eprintln!("Run diagnostics could not be written");
        }
    }
}
pub(crate) fn enum_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "other".into())
}

// Apply both existing credential-shape detection and exact protected-value checks
// to every string, including map keys. Only typed metadata reaches this boundary.
fn scrub(value: &mut serde_json::Value, protected: &[String]) {
    match value {
        serde_json::Value::String(text) => {
            if crate::util::credential_text::contains_credential_text(text)
                || crate::mcp::contains_protected_value(
                    &serde_json::Value::String(text.clone()),
                    protected,
                )
            {
                *text = "[redacted]".into();
            } else {
                *text = text.chars().take(256).collect();
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(|v| scrub(v, protected)),
        serde_json::Value::Object(map) => {
            let old = std::mem::take(map);
            for (key, mut v) in old {
                let mut safe_key = serde_json::Value::String(key);
                scrub(&mut safe_key, protected);
                scrub(&mut v, protected);
                map.insert(safe_key.as_str().unwrap().into(), v);
            }
        }
        _ => {}
    }
}
fn io_error(_: impl std::fmt::Display) -> CommandError {
    CommandError::internal("Run diagnostics are unavailable")
}
fn directory(root: &Path) -> Result<(PathBuf, PrivateDirectoryGuard), CommandError> {
    let pinned = SecureDeletionRoot::open(root)?;
    let guard = pinned.ensure_private_subdirectory(Path::new("logs"))?;
    Ok((root.join("logs"), guard))
}
fn file(
    guard: &PrivateDirectoryGuard,
    path: &Path,
    name: &str,
    create: bool,
) -> Result<File, CommandError> {
    #[cfg(unix)]
    {
        use rustix::fs::{openat, Mode, OFlags};
        let flags = if create {
            OFlags::RDWR | OFlags::CREATE
        } else {
            OFlags::RDONLY
        };
        let fd = openat(
            guard.file(),
            name,
            flags | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(io_error)?;
        let result = File::from(fd);
        let metadata = result.metadata().map_err(io_error)?;
        use std::os::unix::fs::MetadataExt;
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(io_error("not a private file"));
        }
        if create {
            rustix::fs::fchmod(&result, Mode::RUSR | Mode::WUSR).map_err(io_error)?;
        }
        let _ = path;
        Ok(result)
    }
    #[cfg(not(unix))]
    {
        let target = path.join(name);
        if std::fs::symlink_metadata(&target)
            .is_ok_and(|m| m.file_type().is_symlink() || !m.is_file())
        {
            return Err(io_error("not a file"));
        }
        let _ = guard;
        std::fs::OpenOptions::new()
            .read(true)
            .write(create)
            .create(create)
            .open(target)
            .map_err(io_error)
    }
}
fn names(guard: &PrivateDirectoryGuard, path: &Path) -> Result<Vec<String>, CommandError> {
    #[cfg(unix)]
    let entries = rustix::fs::Dir::read_from(guard.file())
        .map_err(io_error)?
        .map(|e| {
            e.map(|e| e.file_name().to_string_lossy().into_owned())
                .map_err(io_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    #[cfg(not(unix))]
    let entries = std::fs::read_dir(path)
        .map_err(io_error)?
        .map(|e| {
            e.map(|e| e.file_name().to_string_lossy().into_owned())
                .map_err(io_error)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let _ = path;
    Ok(entries
        .into_iter()
        .filter(|name| log_date(name).is_some())
        .collect())
}
fn log_date(name: &str) -> Option<chrono::NaiveDate> {
    if name.len() != 19 {
        return None;
    }
    chrono::NaiveDate::parse_from_str(
        name.strip_prefix("runs-")?.strip_suffix(".jsonl")?,
        "%Y%m%d",
    )
    .ok()
}
fn append(
    root: &Path,
    record: &RunLog,
    protected: &[String],
    now: i64,
    cap: usize,
) -> Result<(), CommandError> {
    let (path, guard) = directory(root)?;
    let lock = file(&guard, &path, ".lock", true)?;
    fs2::FileExt::lock_exclusive(&lock).map_err(io_error)?;
    let today = Utc
        .timestamp_millis_opt(now)
        .single()
        .ok_or_else(|| io_error("date"))?
        .date_naive();
    let oldest = Utc
        .timestamp_millis_opt(now.saturating_sub(13 * DAY_MS))
        .single()
        .ok_or_else(|| io_error("date"))?
        .date_naive();
    for name in names(&guard, &path)? {
        if log_date(&name).is_some_and(|date| date < oldest) {
            #[cfg(unix)]
            rustix::fs::unlinkat(guard.file(), name.as_str(), rustix::fs::AtFlags::empty())
                .map_err(io_error)?;
            #[cfg(not(unix))]
            std::fs::remove_file(path.join(name)).map_err(io_error)?;
        }
    }
    let mut value = serde_json::to_value(record).map_err(io_error)?;
    scrub(&mut value, protected);
    let mut line = serde_json::to_vec(&value).map_err(io_error)?;
    line.push(b'\n');
    if line.len() > cap {
        return Err(io_error("entry too large"));
    }
    let name = format!("runs-{}.jsonl", today.format("%Y%m%d"));
    let mut output = file(&guard, &path, &name, true)?;
    use std::io::{Seek, SeekFrom};
    let mut length = output.metadata().map_err(io_error)?.len();
    if length > 0 {
        output.seek(SeekFrom::End(-1)).map_err(io_error)?;
        let mut last = [0];
        output.read_exact(&mut last).map_err(io_error)?;
        if last[0] != b'\n' {
            let offset = length.saturating_sub(cap as u64);
            output.seek(SeekFrom::Start(offset)).map_err(io_error)?;
            let mut tail = Vec::new();
            output.read_to_end(&mut tail).map_err(io_error)?;
            length = tail
                .iter()
                .rposition(|b| *b == b'\n')
                .map(|i| offset + i as u64 + 1)
                .unwrap_or(0);
            output.set_len(length).map_err(io_error)?;
        }
    }
    if length + line.len() as u64 > cap as u64 {
        // Keep only complete recent records within the daily cap.
        let keep = cap - line.len();
        let offset = length.saturating_sub(keep as u64);
        output.seek(SeekFrom::Start(offset)).map_err(io_error)?;
        let mut tail = Vec::new();
        output.read_to_end(&mut tail).map_err(io_error)?;
        if offset > 0 {
            tail = tail
                .iter()
                .position(|b| *b == b'\n')
                .map(|i| tail[i + 1..].to_vec())
                .unwrap_or_default();
        }
        tail.extend(line);
        guard.write_atomic(&path, &name, &tail, false)?;
    } else {
        output.seek(SeekFrom::End(0)).map_err(io_error)?;
        output.write_all(&line).map_err(io_error)?;
    }
    Ok(())
}

pub(crate) fn record_scheduled(store: &dyn LoupistStore, run: &crate::scheduled_tasks::types::Run) {
    use crate::scheduled_tasks::types::{RunStatus, TaskKind};
    let mut record = RunLog::new(
        if run.kind == TaskKind::Briefing {
            "briefing"
        } else {
            "scheduled"
        },
        &run.loupist_id,
        &run.run_id,
        run.started_at_ms,
    );
    record.chat_id = run.thread_id.clone();
    if let Some(chat) = run
        .thread_id
        .as_ref()
        .and_then(|id| store.get_chat(id).ok().flatten())
    {
        if let Some(message) = chat.messages.iter().rev().find(|m| {
            m.role == crate::domain::ChatRole::Assistant && m.created_at_ms >= run.started_at_ms
        }) {
            record.message(message);
            record.failure_kind = message.failure_kind.as_ref().map(enum_name);
        }
    }
    let failure = record.failure_kind.clone();
    match run.status {
        RunStatus::Succeeded => record.finish("ok", None),
        RunStatus::Interrupted => record.finish("cancelled", None),
        RunStatus::Failed => record.finish(
            "failed",
            Some(failure.as_deref().unwrap_or("scheduled_run_failed")),
        ),
        RunStatus::Running => return,
    }
    record.record(store);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticsRequest {
    pub loupist_id: Option<String>,
}
fn diagnostics_root(state: &AppState, loupist: Option<&str>) -> Result<PathBuf, CommandError> {
    let Some(loupist) = loupist else {
        return Ok(state.artifacts.app_data_dir.clone());
    };
    state.ensure_loupist_available(loupist)?;
    Ok(PathBuf::from(
        state
            .store
            .get_loupist(loupist)
            .map_err(crate::commands::map_store)?
            .ok_or_else(|| CommandError::not_found("Loupist"))?
            .workspace_root,
    )
    .join(".loupist"))
}
#[tauri::command(rename_all = "snake_case")]
pub fn run_logs_reveal(
    request: DiagnosticsRequest,
    state: tauri::State<'_, AppState>,
) -> Result<(), CommandError> {
    let (path, _guard) = directory(&diagnostics_root(&state, request.loupist_id.as_deref())?)?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&path)
        .spawn()
        .map_err(io_error)?;
    #[cfg(windows)]
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,{}", path.display()))
        .spawn()
        .map_err(io_error)?;
    #[cfg(not(any(target_os = "macos", windows)))]
    open::that_detached(path).map_err(io_error)?;
    Ok(())
}
#[tauri::command(rename_all = "snake_case")]
pub fn run_logs_last_error(
    request: DiagnosticsRequest,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    last_error(&diagnostics_root(&state, request.loupist_id.as_deref())?)
}
fn last_error(root: &Path) -> Result<Option<String>, CommandError> {
    let (path, guard) = directory(root)?;
    let lock = file(&guard, &path, ".lock", true)?;
    fs2::FileExt::lock_shared(&lock).map_err(io_error)?;
    let mut logs = names(&guard, &path)?;
    logs.sort();
    for name in logs.into_iter().rev() {
        let mut text = String::new();
        file(&guard, &path, &name, false)?
            .take(DAILY_BYTES as u64)
            .read_to_string(&mut text)
            .map_err(io_error)?;
        for line in text.lines().rev() {
            if let Ok(record) = serde_json::from_str::<RunLog>(line) {
                if record.status == "failed" {
                    let mut value = serde_json::to_value(record).map_err(io_error)?;
                    scrub(&mut value, &[]);
                    return serde_json::to_string(&value).map(Some).map_err(io_error);
                }
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daily_rotation_retention_and_size_keep_complete_recent_lines() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path().canonicalize().unwrap();
        let start = 1_800_000_000_000;
        for day in 0..16 {
            append(
                dir.as_path(),
                &RunLog::new("chat", "loupist", "run", start),
                &[],
                start + day * DAY_MS,
                DAILY_BYTES,
            )
            .unwrap();
        }
        let (path, guard) = directory(dir.as_path()).unwrap();
        assert_eq!(names(&guard, &path).unwrap().len(), 14);
        for i in 0..20 {
            append(
                dir.as_path(),
                &RunLog::new("chat", "loupist", &i.to_string(), start),
                &[],
                start + 15 * DAY_MS,
                1024,
            )
            .unwrap();
        }
        let name = names(&guard, &path).unwrap().into_iter().max().unwrap();
        let text = std::fs::read_to_string(path.join(name)).unwrap();
        assert!(text.len() <= 1024);
        for line in text.lines() {
            serde_json::from_str::<RunLog>(line).unwrap();
        }
        assert_eq!(
            serde_json::from_str::<RunLog>(text.lines().last().unwrap())
                .unwrap()
                .run_id,
            "19"
        );
    }
    #[test]
    fn seeded_protected_secret_never_appears_even_in_map_keys() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path().canonicalize().unwrap();
        let secret = "seeded-private-value".to_string();
        let mut record = RunLog::new("chat", "loupist", "run", 1_800_000_000_000);
        record.model = Some(secret.clone());
        record.tools.insert(secret.clone(), 1);
        record.notes.push("api_key=another-private-value".into());
        append(
            dir.as_path(),
            &record,
            std::slice::from_ref(&secret),
            record.started_at_ms,
            DAILY_BYTES,
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.as_path().join("logs/runs-20270115.jsonl")).unwrap();
        assert!(!text.contains(&secret));
        assert!(!text.contains("another-private-value"));
        assert!(text.contains("[redacted]"));
    }
    #[test]
    fn inference_usage_distinguishes_zero_unknown_and_cache_reads() {
        let mut usage = InferenceUsage::default();
        assert_eq!(usage.snapshot().unwrap().input, 0);
        usage.begin();
        assert!(usage.snapshot().is_none());
        usage.observe(
            &serde_json::json!({"input": 10, "cacheRead": 30, "cacheWrite": 5, "output_tokens": 4}),
            false,
        );
        let measured = usage.snapshot().unwrap();
        assert_eq!(
            (measured.input, measured.cached_input, measured.output),
            (15, 30, 4)
        );
        usage.begin();
        usage.observe(&serde_json::Value::Null, true);
        assert_eq!(usage.snapshot().unwrap().input, 15);
        usage.begin();
        usage.observe(&serde_json::Value::Null, false);
        assert!(usage.snapshot().is_none());
    }

    #[test]
    fn torn_append_is_repaired_before_the_next_complete_record() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let now = 1_800_000_000_000;
        let mut record = RunLog::new("chat", "loupist", "run", now);
        record.finish("failed", Some("provider_error"));
        append(&root, &record, &[], now, DAILY_BYTES).unwrap();
        let target = root.join("logs/runs-20270115.jsonl");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .unwrap()
            .write_all(b"{partial")
            .unwrap();
        append(&root, &record, &[], now, DAILY_BYTES).unwrap();
        let text = std::fs::read_to_string(target).unwrap();
        assert_eq!(text.lines().count(), 2);
        for line in text.lines() {
            serde_json::from_str::<RunLog>(line).unwrap();
        }
        assert!(last_error(&root).unwrap().is_some());
    }

    #[cfg(unix)]
    #[test]
    fn log_files_cannot_follow_symlinks_or_mutate_hard_link_targets() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let target = root.join("private-target");
        std::fs::write(&target, b"untouched").unwrap();
        std::fs::create_dir(root.join("logs")).unwrap();
        let log = root.join("logs/runs-20270115.jsonl");
        std::os::unix::fs::symlink(&target, &log).unwrap();
        let record = RunLog::new("chat", "loupist", "run", 1_800_000_000_000);
        assert!(append(&root, &record, &[], record.started_at_ms, DAILY_BYTES).is_err());
        std::fs::remove_file(&log).unwrap();
        std::fs::hard_link(&target, &log).unwrap();
        assert!(append(&root, &record, &[], record.started_at_ms, DAILY_BYTES).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "untouched");
    }
    #[test]
    fn pi_native_tools_are_counted_once_without_arguments_or_results() {
        let mut log = RunLog::new("chat", "loupist", "run", 1);
        let event = serde_json::json!({"type":"tool_execution_start","toolCallId":"call-1","toolName":"read","args":{"path":"private-content"}});
        log.observe_pi_tool(&event);
        log.observe_pi_tool(&event);
        log.trace(&crate::domain::ChatToolTrace {
            entries: vec![crate::domain::ChatToolTraceEntry {
                call_id: Some("call-1".into()),
                name: "read".into(),
                component: None,
                args: None,
                start_offset_ms: 0,
                duration_ms: 1,
                outcome: crate::domain::ToolTraceOutcome::Ok,
                error: None,
                result_bytes: None,
            }],
            truncated: false,
            untrusted_text: false,
        });
        assert_eq!(log.tools.get("read"), Some(&1));
        let encoded = serde_json::to_string(&log).unwrap();
        assert!(!encoded.contains("private-content"));
        assert!(!encoded.contains("call-1"));
        assert!(!encoded.contains("openbbFirstCallMs"));
    }
    #[test]
    fn a_turns_first_openbb_call_latency_is_logged_by_start_order() {
        let entry = |name: &str, component: Option<&str>, start: u64, duration: u64| {
            crate::domain::ChatToolTraceEntry {
                call_id: None,
                name: name.into(),
                component: component.map(str::to_owned),
                args: None,
                start_offset_ms: start,
                duration_ms: duration,
                outcome: crate::domain::ToolTraceOutcome::Ok,
                error: None,
                result_bytes: None,
            }
        };
        let mut log = RunLog::new("chat", "loupist", "run", 1);
        // Entries are recorded as calls finish; a later quick call can finish first.
        log.trace(&crate::domain::ChatToolTrace {
            entries: vec![
                entry("capability_load", Some("openbb"), 0, 30),
                entry("web_search", Some("web-research"), 0, 900),
                entry("equity_price_historical", Some("openbb"), 9_000, 250),
                entry("mcp.connect", Some("openbb"), 1_000, 7_400),
            ],
            truncated: false,
            untrusted_text: false,
        });
        assert_eq!(log.openbb_first_call_ms, Some(7_400));
        let encoded: serde_json::Value = serde_json::to_value(&log).unwrap();
        assert_eq!(encoded["openbbFirstCallMs"], 7_400);
        let decoded: RunLog = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded.openbb_first_call_ms, Some(7_400));
    }
}
