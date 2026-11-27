//! Content-free, local run diagnostics. Never pass payloads or error bodies here.
use crate::{
    app::{AppState, CommandError},
    domain::{ChatCostEstimate as CostEstimate, ChatPerformance},
    fs::secure_delete::{PrivateDirectoryGuard, SecureDeletionRoot},
    providers::status::CredentialKind,
    store::{
        model_usage::{
            ModelUsageAccount, ModelUsageData, ModelUsageKind, ModelUsageMeasures, ModelUsageRow,
        },
        SqliteStore,
    },
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

pub(crate) fn compaction_usage(event: &serde_json::Value) -> Option<&serde_json::Value> {
    event
        .pointer("/result/usage")
        .filter(|usage| usage.is_object())
}

/// Fixed host commit phases and reasons; never serialize an error body here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommitStage {
    Research,
    Principles,
    Lens,
    Deliveries,
    Citations,
    Assets,
    Vocabulary,
    Setup,
    Handoff,
    BotMemory,
    Chat,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommitReason {
    JournalEntity,
    JournalCheck,
    InvalidInput,
    Unauthorized,
    Conflict,
    DigestMismatch,
    MissingReference,
    Storage,
    Other,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MemoryCommitFailure {
    pub stage: CommitStage,
    pub reason: CommitReason,
}
impl MemoryCommitFailure {
    pub fn from_code(stage: CommitStage, code: &str) -> Self {
        let reason = match code {
            "journal_entity" | "journal_entities" => CommitReason::JournalEntity,
            "journal_check"
            | "journal_check_direction"
            | "journal_check_threshold"
            | "journal_check_confidence"
            | "journal_check_date" => CommitReason::JournalCheck,
            "invalid" | "invalid_argument" | "journal_field" | "journal_date"
            | "journal_thesis" => CommitReason::InvalidInput,
            "unauthorized" | "inspection_receipt_not_citable" => CommitReason::Unauthorized,
            "conflict" | "revision_conflict" | "cursor_stale" => CommitReason::Conflict,
            "digest_mismatch" => CommitReason::DigestMismatch,
            "not_found" | "missing_bytes" => CommitReason::MissingReference,
            "internal" | "sqlite" | "json" | "io" | "private_storage" | "lock_poisoned" => {
                CommitReason::Storage
            }
            _ => CommitReason::Other,
        };
        Self { stage, reason }
    }
}

/// Local startup diagnostics use a fixed checkpoint and a known host code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChatStartPhase {
    CredentialPresence,
    Request,
    RunAdmission,
    Scratch,
    Model,
    Configuration,
    ConnectorAuthority,
    WorkspaceContext,
    Transcript,
    Launch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChatStartFailure {
    pub phase: ChatStartPhase,
    pub reason: String,
}

impl ChatStartFailure {
    pub fn new(phase: ChatStartPhase, code: &crate::app::CommandErrorCode) -> Self {
        use crate::app::CommandErrorCode;
        let reason = match code {
            CommandErrorCode::Unlisted(_) => CommandErrorCode::Internal.as_str(),
            code => code.as_str(),
        };
        Self {
            phase,
            reason: reason.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ChatSavePath {
    Direct,
    Queued,
    Waited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum UsageSurface {
    Bot,
    Research,
    Handoff,
    Briefing,
    Scheduled,
    Learning,
    DeepResearch,
    World,
    WorldSchema,
    MethodLearning,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunLog {
    #[serde(skip)]
    pub world_origin: Option<crate::scheduled_tasks::allowance::WorldOrigin>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_start_failure: Option<ChatStartFailure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_save_skip: Option<crate::domain::MemorySaveSkipReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat_save_path: Option<ChatSavePath>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_commit_failure: Option<MemoryCommitFailure>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub usage: Option<Usage>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub tokens_partial: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requests: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_estimate: Option<CostEstimate>,
    #[serde(skip)]
    pub surface: Option<UsageSurface>,
    #[serde(skip)]
    pub task_id: Option<String>,
    #[serde(skip)]
    pub credential_kind: Option<CredentialKind>,
    #[serde(skip)]
    pub account_usage: Vec<ModelUsageAccount>,
    #[serde(skip)]
    pub reasoning: Option<u64>,
    pub tools: BTreeMap<String, u64>,
    #[serde(default)]
    pub python_reads: u64,
    #[serde(default)]
    pub python_rounds: u64,
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
    /// World jobs that ran extraction: names the model merged into an existing
    /// node (`proposed`), those merges the host refused because the two names
    /// carry different identifying codes (`refused`), and names the host
    /// joined to the stored node with an equal code where the model had not
    /// (`byCode`). Counts only.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub entity_resolution: BTreeMap<String, u64>,
    /// Chat turns: duration of the turn's first OpenBB call. It includes the
    /// worker's cold start when no pooled OpenBB process was ready.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub openbb_first_call_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub connector_keychain_statuses: BTreeSet<crate::finance_credentials::KeychainReadStatus>,
    #[serde(skip)]
    observed_calls: BTreeSet<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Usage {
    pub input: u64,
    pub cached_input: u64,
    pub output: u64,
}

/// Pi reports uncached input and output. Cache counters can be unreported.
pub(crate) struct PiTokenCounts {
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache_read: Option<u64>,
    pub cache_write: Option<u64>,
    valid_cache: bool,
}

impl PiTokenCounts {
    pub fn read(usage: &serde_json::Value, output_key: &str) -> Self {
        let cache_read = &usage["cacheRead"];
        let cache_write = &usage["cacheWrite"];
        Self {
            input: usage["input"].as_u64(),
            output: usage[output_key].as_u64(),
            cache_read: cache_read.as_u64(),
            cache_write: cache_write.as_u64(),
            valid_cache: [cache_read, cache_write]
                .into_iter()
                .all(|field| field.is_null() || field.is_u64()),
        }
    }

    pub fn total_input(&self) -> Option<u64> {
        self.input_with_cache_writes()?
            .checked_add(self.cache_read.unwrap_or_default())
    }

    fn input_with_cache_writes(&self) -> Option<u64> {
        if !self.valid_cache {
            return None;
        }
        self.input?
            .checked_add(self.cache_write.unwrap_or_default())
    }
}

#[derive(Default)]
pub(crate) struct InferenceUsage {
    started: u64,
    finished: u64,
    unknown: bool,
    requests: u64,
    cost: PiCost,
    totals: Usage,
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
        self.requests = self.requests.saturating_add(1);
        self.cost.observe(&usage["cost"]);
        let tokens = PiTokenCounts::read(usage, "output_tokens");
        if let Some(totals) = self
            .add_tokens(&tokens)
            .filter(|_| usage["certainty"] != "uncertain")
        {
            self.totals = totals;
        } else {
            self.unknown = true;
        }
    }

    fn add_tokens(&self, tokens: &PiTokenCounts) -> Option<Usage> {
        let cached = tokens.cache_read.unwrap_or_default();
        let input = self
            .totals
            .input
            .checked_add(tokens.input_with_cache_writes()?)?;
        let cached_input = self.totals.cached_input.checked_add(cached)?;
        input.checked_add(cached_input)?;
        Some(Usage {
            input,
            cached_input,
            output: self.totals.output.checked_add(tokens.output?)?,
        })
    }
    pub fn apply(&self, log: &mut RunLog) {
        log.usage = self.snapshot();
        log.requests = Some(self.requests);
        log.cost_estimate = self.cost.estimate(self.requests);
    }

    fn snapshot(&self) -> Option<Usage> {
        (!self.unknown && self.started == self.finished).then(|| self.totals.clone())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PiCost {
    microusd: u64,
    billed: u64,
}

impl PiCost {
    pub fn observe(&mut self, cost: &serde_json::Value) {
        if let Some(total) = cost["total"]
            .as_f64()
            .filter(|n| n.is_finite() && *n >= 0.0)
        {
            self.microusd = self
                .microusd
                .saturating_add((total * 1_000_000.0).round() as u64);
            if cost["totalFromProvider"].as_bool() == Some(true) {
                self.billed = self.billed.saturating_add(1);
            }
        }
    }

    pub fn estimate(&self, requests: u64) -> Option<CostEstimate> {
        let from_provider = requests > 0 && self.billed == requests;
        (self.microusd > 0 || from_provider).then_some(CostEstimate {
            microusd: self.microusd,
            from_provider,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct RefusedTool {
    pub name: String,
    pub reason: String,
}

impl RunLog {
    pub fn new(kind: &str, loupist: &str, run: &str, started: i64) -> Self {
        Self {
            kind: kind.into(),
            world_origin: None,
            loupist_id: Some(loupist.into()),
            run_id: run.into(),
            chat_id: None,
            started_at_ms: started,
            duration_ms: 0,
            status: "ok".into(),
            failure_kind: None,
            chat_start_failure: None,
            memory_save_skip: None,
            chat_save_path: None,
            memory_commit_failure: None,
            model: None,
            effort: None,
            usage: None,
            tokens_partial: false,
            requests: None,
            cost_estimate: None,
            surface: None,
            task_id: None,
            credential_kind: None,
            account_usage: Vec::new(),
            reasoning: None,
            tools: BTreeMap::new(),
            python_reads: 0,
            python_rounds: 0,
            refused_tools: vec![],
            memory_changes: BTreeMap::new(),
            notes: vec![],
            inference_calls: BTreeMap::new(),
            stage_ms: BTreeMap::new(),
            queue_wait_ms: None,
            entity_resolution: BTreeMap::new(),
            openbb_first_call_ms: None,
            connector_keychain_statuses: BTreeSet::new(),
            observed_calls: BTreeSet::new(),
        }
    }
    /// Closes a Chat turn's record the way every turn ends: completed with
    /// the model that answered, failed with its typed kind (a plan limit or
    /// out of credit there holds background work on the provider today), or
    /// cancelled. The record is then written with [`Self::record`].
    pub(crate) fn end_turn(&mut self, end: TurnEnd<'_>) {
        match end {
            TurnEnd::Completed {
                model,
                memory_save_skip,
            } => {
                if let Some(model) = model {
                    self.model(model);
                }
                self.memory_save_skip = memory_save_skip;
                self.finish("ok", None);
            }
            TurnEnd::Failed(kind) => self.finish("failed", Some(&enum_name(&kind))),
            TurnEnd::Cancelled => self.finish("cancelled", None),
        }
    }
    pub fn finish(&mut self, status: &str, failure: Option<&str>) {
        self.duration_ms = crate::commands::now_ms()
            .saturating_sub(self.started_at_ms)
            .max(0) as u64;
        self.status = status.into();
        self.failure_kind = failure.map(str::to_owned);
        if status != "failed" && self.memory_save_skip.is_none() {
            self.memory_commit_failure = None;
        }
    }
    pub fn model(&mut self, model: &crate::providers::catalog::ExecutionModelLock) {
        self.model = Some(format!("{}/{}", model.provider, model.model));
        self.effort = Some(model.thinking_level.clone());
    }
    pub fn message(&mut self, message: &crate::domain::ChatMessage) {
        if let Some(model) = &message.execution_model {
            self.model(model);
        }
        if let Some(performance) = &message.performance {
            self.performance(performance);
        }
        self.refusals(&message.tool_refusals);
        if let Some(trace) = &message.tool_trace {
            self.trace(trace);
        }
        if let Some(update) = &message.memory_update {
            self.memory(update);
        }
    }
    fn performance(&mut self, performance: &ChatPerformance) {
        let input = performance
            .input_tokens
            .checked_add(performance.cache_write_tokens)
            .filter(|input| input.checked_add(performance.cache_read_tokens).is_some());
        self.tokens_partial = input.is_none();
        self.usage = input.map(|input| Usage {
            input,
            cached_input: performance.cache_read_tokens,
            output: performance.output_tokens,
        });
        self.requests = performance.model_requests.map(u64::from);
        self.cost_estimate = performance.cost_estimate;
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
            if let Some(run) = &entry.python {
                self.python_reads += u64::from(run.reads);
                self.python_rounds += u64::from(run.rounds);
            }
            if entry.origin == Some(crate::domain::ToolTraceOrigin::Python) {
                continue;
            }
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
        if append(app_data, self, crate::commands::now_ms(), DAILY_BYTES).is_err() {
            eprintln!("Run diagnostics could not be written");
        }
    }
    pub fn record(&self, store: &SqliteStore) {
        let result = store
            .get_loupist(self.loupist_id.as_deref().unwrap_or(""))
            .ok()
            .flatten()
            .ok_or_else(|| CommandError::not_found("Loupist"))
            .and_then(|loupist| {
                append(
                    &Path::new(&loupist.workspace_root).join(".loupist"),
                    self,
                    crate::commands::now_ms(),
                    DAILY_BYTES,
                )
            });
        if result.is_err() {
            eprintln!("Run diagnostics could not be written");
        }
        if self.record_usage(store).is_err() {
            // The allowance counts an ended background run whose row is
            // missing at its frozen budget, so this loss never frees requests.
            eprintln!("Model usage could not be recorded");
        }
    }

    /// Persist this run's model-usage row. `Ok(false)` means there is nothing
    /// to persist. An error means the row was not stored.
    pub fn record_usage(&self, store: &SqliteStore) -> crate::store::StoreResult<bool> {
        let Some(row) = self.model_usage_row() else {
            return Ok(false);
        };
        let id = format!("{}:{}:{}", self.kind, self.run_id, self.started_at_ms);
        store.put_model_usage(&id, &row)?;
        Ok(true)
    }

    fn model_usage_row(&self) -> Option<ModelUsageRow> {
        let (kind, surface) = match self.kind.as_str() {
            "chat" => (
                ModelUsageKind::Chat,
                self.surface.unwrap_or(UsageSurface::Research),
            ),
            "world" => (ModelUsageKind::World, UsageSurface::World),
            "schema" => (ModelUsageKind::Schema, UsageSurface::WorldSchema),
            "learning" => (ModelUsageKind::Learning, UsageSurface::MethodLearning),
            _ => return None,
        };
        let (provider, model) = self.model.as_deref()?.split_once('/')?;
        let status = serde_json::from_value(serde_json::Value::String(self.status.clone())).ok()?;
        let failure_kind = self
            .failure_kind
            .as_ref()
            .and_then(|kind| serde_json::from_value(serde_json::Value::String(kind.clone())).ok());
        let usage = self.usage.as_ref();
        Some(ModelUsageRow {
            finished_at_ms: self
                .started_at_ms
                .saturating_add(i64::try_from(self.duration_ms).unwrap_or(i64::MAX)),
            loupist_id: self.loupist_id.clone(),
            task_id: self.task_id.clone(),
            data: ModelUsageData {
                kind,
                surface,
                world_origin: self.world_origin,
                provider: provider.into(),
                credential_kind: self.credential_kind,
                model: model.into(),
                effort: self.effort.clone(),
                chat_id: self.chat_id.clone(),
                run_id: self.run_id.clone(),
                started_at_ms: self.started_at_ms,
                duration_ms: self.duration_ms,
                status,
                failure_kind,
                measures: ModelUsageMeasures {
                    requests: self.requests,
                    input: usage.map(|usage| usage.input),
                    cached_input: usage.map(|usage| usage.cached_input),
                    output: usage.map(|usage| usage.output),
                    tokens_partial: self.tokens_partial,
                    reasoning: self.reasoning,
                    cost_microusd: self.cost_estimate.map(|cost| cost.microusd),
                    cost_from_provider: self.cost_estimate.map(|cost| cost.from_provider),
                },
                accounts: self.account_usage.clone(),
            },
        })
    }
}
/// How a Chat turn ended, for [`RunLog::end_turn`].
pub(crate) enum TurnEnd<'a> {
    Completed {
        model: Option<&'a crate::providers::catalog::ExecutionModelLock>,
        memory_save_skip: Option<crate::domain::MemorySaveSkipReason>,
    },
    Failed(crate::domain::ChatFailureKind),
    Cancelled,
}

pub(crate) fn enum_name(value: &impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_else(|| "other".into())
}

// Apply credential-shape detection to every string, including map keys.
// Only typed metadata reaches this boundary.
fn scrub(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            if crate::util::credential_text::contains_credential_text(text) {
                *text = "[redacted]".into();
            } else {
                *text = text.chars().take(256).collect();
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(scrub),
        serde_json::Value::Object(map) => {
            let old = std::mem::take(map);
            for (key, mut v) in old {
                let mut safe_key = serde_json::Value::String(key);
                scrub(&mut safe_key);
                scrub(&mut v);
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
fn append(root: &Path, record: &RunLog, now: i64, cap: usize) -> Result<(), CommandError> {
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
    scrub(&mut value);
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

pub(crate) fn record_scheduled(store: &SqliteStore, run: &crate::scheduled_tasks::types::Run) {
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
    record.memory_save_skip = run.memory_save_skip;
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
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
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
#[tauri::command(async, rename_all = "snake_case")]
pub fn run_logs_reveal(
    request: DiagnosticsRequest,
    state: tauri::State<'_, AppState>,
) -> Result<(), CommandError> {
    let (path, _guard) = directory(&diagnostics_root(&state, request.loupist_id.as_deref())?)?;
    #[cfg(target_os = "macos")]
    crate::process_lease::macos_commands::spawn(
        std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(&path),
    )
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
#[tauri::command(async, rename_all = "snake_case")]
pub fn run_logs_last_error(
    request: DiagnosticsRequest,
    state: tauri::State<'_, AppState>,
) -> Result<Option<String>, CommandError> {
    last_error(&diagnostics_root(&state, request.loupist_id.as_deref())?)
}
/// The eval trace reads the host's terminal log, never a Pi-authored diagnostic.
/// A run id selects the exact turn even when another Chat finishes beside it.
#[cfg(feature = "eval-harness")]
pub(crate) fn memory_commit_failure(
    root: &Path,
    run_id: &str,
) -> Result<Option<MemoryCommitFailure>, CommandError> {
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
                if record.run_id == run_id {
                    return Ok(record.memory_commit_failure);
                }
            }
        }
    }
    Ok(None)
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
                    scrub(&mut value);
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
    fn memory_commit_categories_never_serialize_error_text_or_unknown_codes() {
        for private in [
            "/private/user/workspace",
            "api_key=fixture-secret",
            "private user judgment",
        ] {
            let cause = MemoryCommitFailure::from_code(CommitStage::Research, private);
            assert_eq!(cause.reason, CommitReason::Other);
            assert_eq!(
                serde_json::to_value(cause).unwrap(),
                serde_json::json!({"stage":"research","reason":"other"})
            );
            assert!(serde_json::from_value::<MemoryCommitFailure>(
                serde_json::json!({"stage":"research","reason":private})
            )
            .is_err());
        }
        assert_eq!(
            MemoryCommitFailure::from_code(CommitStage::Research, "journal_entity").reason,
            CommitReason::JournalEntity
        );
    }

    #[test]
    fn recovered_completion_keeps_no_commit_failure_category() {
        let mut record = RunLog::new("chat", "loupist", "run", crate::commands::now_ms());
        record.memory_commit_failure = Some(MemoryCommitFailure::from_code(
            CommitStage::Chat,
            "internal",
        ));
        record.finish("ok", None);
        assert!(record.memory_commit_failure.is_none());
    }

    #[cfg(feature = "eval-harness")]
    #[test]
    fn commit_diagnostics_select_the_exact_run_and_old_logs_still_read() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let now = crate::commands::now_ms();
        let mut first = RunLog::new("chat", "loupist", "failed-run", now);
        first.memory_commit_failure = Some(MemoryCommitFailure::from_code(
            CommitStage::Research,
            "journal_entity",
        ));
        first.finish("failed", Some("memory_commit_failed"));
        append(&root, &first, now, DAILY_BYTES).unwrap();
        let second = RunLog::new("chat", "loupist", "other-run", now);
        append(&root, &second, now, DAILY_BYTES).unwrap();
        assert_eq!(
            memory_commit_failure(&root, "failed-run").unwrap(),
            first.memory_commit_failure
        );
        assert_eq!(memory_commit_failure(&root, "other-run").unwrap(), None);
        let mut old = serde_json::to_value(first).unwrap();
        old.as_object_mut().unwrap().remove("memoryCommitFailure");
        assert!(serde_json::from_value::<RunLog>(old)
            .unwrap()
            .memory_commit_failure
            .is_none());
    }

    #[test]
    fn daily_rotation_retention_and_size_keep_complete_recent_lines() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path().canonicalize().unwrap();
        let start = 1_800_000_000_000;
        for day in 0..16 {
            append(
                dir.as_path(),
                &RunLog::new("chat", "loupist", "run", start),
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
    fn credential_shapes_are_redacted_when_recorded_and_read() {
        let temporary = tempfile::tempdir().unwrap();
        let dir = temporary.path().canonicalize().unwrap();
        let secret = "api_key=seeded-private-value";
        let mut record = RunLog::new("chat", "loupist", "run", crate::commands::now_ms());
        record.model = Some(secret.into());
        record.tools.insert(secret.into(), 1);
        record.notes.push("api_key=another-private-value".into());
        record.finish("failed", None);
        record.record_app(&dir);
        let (path, guard) = directory(&dir).unwrap();
        let name = names(&guard, &path).unwrap().into_iter().max().unwrap();
        let text = std::fs::read_to_string(path.join(&name)).unwrap();
        assert!(!text.contains("seeded-private-value"));
        assert!(!text.contains("another-private-value"));
        assert!(text.contains("[redacted]"));
        std::fs::write(path.join(name), serde_json::to_string(&record).unwrap()).unwrap();
        let read = last_error(&dir).unwrap().unwrap();
        assert!(!read.contains("private-value"));
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
    fn inference_usage_marks_aggregate_overflow_unknown() {
        let mut usage = InferenceUsage::default();
        for counts in [
            serde_json::json!({"input":u64::MAX,"output_tokens":1}),
            serde_json::json!({"input":0,"cacheRead":1,"output_tokens":1}),
        ] {
            usage.begin();
            usage.observe(&counts, false);
        }
        assert!(usage.snapshot().is_none());
    }

    #[test]
    fn measured_inference_writes_requests_and_cost_even_when_tokens_are_unknown() {
        let mut log = RunLog::new("world", "loupist", "run", 1);
        let mut usage = InferenceUsage::default();
        usage.begin();
        usage.observe(&serde_json::Value::Null, true);
        usage.begin();
        usage.observe(
            &serde_json::json!({"cost":{"total":0.0000016,"totalFromProvider":true}}),
            false,
        );
        usage.apply(&mut log);
        let billed = serde_json::to_value(&log).unwrap();
        assert_eq!(billed["requests"], 1);
        assert_eq!(
            billed["costEstimate"],
            serde_json::json!({"microusd":2,"fromProvider":true})
        );
        usage.begin();
        usage.observe(&serde_json::Value::Null, false);
        usage.apply(&mut log);
        let estimated = serde_json::to_value(&log).unwrap();
        assert_eq!(estimated["requests"], 2);
        assert_eq!(
            estimated["costEstimate"],
            serde_json::json!({"microusd":2,"fromProvider":false})
        );
        assert!(estimated["usage"].is_null());
        for key in ["surface", "taskId", "credentialKind", "reasoning"] {
            assert!(estimated.get(key).is_none());
        }
    }

    #[test]
    fn message_diagnostics_preserve_chat_usage_fields() {
        let mut message = crate::test_fixtures::chat_message(
            "answer",
            crate::domain::ChatRole::Assistant,
            "Done.",
        );
        let mut log = RunLog::new("scheduled", "loupist", "run", 1);
        log.message(&message);
        assert!(serde_json::to_value(&log)
            .unwrap()
            .get("requests")
            .is_none());
        message.performance = Some(ChatPerformance {
            input_tokens: 8,
            cache_read_tokens: 5,
            cache_write_tokens: 2,
            output_tokens: 4,
            model_requests: Some(2),
            cost_estimate: Some(CostEstimate {
                microusd: 250,
                from_provider: false,
            }),
            ..Default::default()
        });
        log.message(&message);
        let mut overflow = log.clone();
        message.performance.as_mut().unwrap().input_tokens = u64::MAX;
        overflow.message(&message);
        assert!(overflow.tokens_partial);
        assert!(overflow.usage.is_none());
        let record = serde_json::to_value(log).unwrap();
        assert_eq!(record["requests"], 2);
        assert_eq!(
            record["costEstimate"],
            serde_json::json!({"microusd":250,"fromProvider":false})
        );
        assert_eq!(
            record["usage"],
            serde_json::json!({"input":10,"cachedInput":5,"output":4})
        );
    }

    #[test]
    fn recording_model_runs_replaces_attempts_and_excludes_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open_catalog(dir.path().join("catalog.sqlite3")).unwrap();
        crate::test_fixtures::create_folder_loupist(&store, "usage", &dir.path().join("workspace"));
        let now = crate::commands::now_ms();
        for kind in [
            "chat",
            "world",
            "schema",
            "learning",
            "scheduled",
            "briefing",
            "unset",
            "app",
        ] {
            let mut record = RunLog::new(
                if kind == "unset" { "chat" } else { kind },
                "usage",
                kind,
                now,
            );
            record.model = (kind != "unset").then(|| "openrouter/vendor/model".into());
            record.requests = Some(1);
            record.usage = Some(Usage {
                input: 2,
                cached_input: 3,
                output: 4,
            });
            if kind == "chat" {
                record.surface = Some(UsageSurface::Bot);
                record.credential_kind = Some(CredentialKind::ApiKey);
            }
            if kind == "app" {
                record.kind = "chat".into();
                record.record_app(dir.path());
                continue;
            }
            record.record(&store);
            record.requests = Some(2);
            record.record(&store);
        }
        let rows = store.model_usage_in_range(now, now + 1).unwrap();
        assert_eq!(
            rows.iter().map(|row| row.data.surface).collect::<Vec<_>>(),
            [
                UsageSurface::Bot,
                UsageSurface::MethodLearning,
                UsageSurface::WorldSchema,
                UsageSurface::World
            ],
        );
        for row in rows {
            assert_eq!(row.data.model, "vendor/model");
            assert_eq!(row.data.measures.requests, Some(2));
            assert_eq!(row.data.measures.input, Some(2));
            assert_eq!(
                row.data.credential_kind,
                (row.data.surface == UsageSurface::Bot).then_some(CredentialKind::ApiKey)
            );
        }
    }

    #[test]
    fn torn_append_is_repaired_before_the_next_complete_record() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let now = 1_800_000_000_000;
        let mut record = RunLog::new("chat", "loupist", "run", now);
        record.finish("failed", Some("provider_error"));
        append(&root, &record, now, DAILY_BYTES).unwrap();
        let target = root.join("logs/runs-20270115.jsonl");
        std::fs::OpenOptions::new()
            .append(true)
            .open(&target)
            .unwrap()
            .write_all(b"{partial")
            .unwrap();
        append(&root, &record, now, DAILY_BYTES).unwrap();
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
        assert!(append(&root, &record, record.started_at_ms, DAILY_BYTES).is_err());
        std::fs::remove_file(&log).unwrap();
        std::fs::hard_link(&target, &log).unwrap();
        assert!(append(&root, &record, record.started_at_ms, DAILY_BYTES).is_err());
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
                origin: None,
                parent_step: None,
                python: None,
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
                origin: None,
                parent_step: None,
                python: None,
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
