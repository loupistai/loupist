use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::store::{StoreError, StoreResult};

pub const MAX_TASKS_PER_LOUPIST: usize = 100;
pub const MAX_RUN_HISTORY: usize = 50;
pub const MAX_PROMPT_BYTES: usize = 128 * 1024;

/// A saved action snapshot, displayed even if the rest of the Chat stops.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskReceipt {
    pub task_id: String,
    pub title: String,
    pub revision: u64,
    pub action: ScheduleAction,
    pub status: TaskStatus,
    pub schedule: Schedule,
    pub target: TaskTarget,
    pub next_run_at_ms: Option<i64>,
    pub use_memory: bool,
    pub update_memory: bool,
    /// Created from a conversation and waiting for the user's one-tap OK in its card.
    #[serde(default)]
    pub awaiting_confirmation: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ScheduleAction {
    Created,
    Updated,
    Paused,
    Resumed,
    Deleted,
    Started,
    Stopped,
    Viewed,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum ScheduleReceipt {
    Task(TaskReceipt),
    Question(QuestionReceipt),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct QuestionReceipt {
    pub task_id: String,
    pub title: String,
    pub question: String,
}

impl ScheduleReceipt {
    pub fn id(&self) -> &str {
        match self {
            Self::Task(task) => &task.task_id,
            Self::Question(item) => &item.task_id,
        }
    }
    pub fn validate(&self) -> StoreResult<()> {
        match self {
            Self::Task(task) => task.validate(),
            Self::Question(item) => {
                identifier(&item.task_id)?;
                bounded_text(&item.title, 200, false)?;
                bounded_text(&item.question, 1000, false)
            }
        }
    }
    pub fn from_task(task: &Task, action: ScheduleAction) -> Self {
        Self::Task(TaskReceipt {
            task_id: task.id.clone(),
            title: task.title.clone(),
            revision: task.revision,
            action,
            status: task.status,
            schedule: task.schedule.clone(),
            target: task.target.clone(),
            next_run_at_ms: task.next_run_at_ms,
            use_memory: task.use_memory,
            update_memory: task.update_memory,
            awaiting_confirmation: task.awaiting_confirmation,
        })
    }
}
impl TaskReceipt {
    pub fn validate(&self) -> StoreResult<()> {
        identifier(&self.task_id)?;
        bounded_text(&self.title, 200, false)?;
        super::schedule::validate_schedule(&self.schedule)?;
        if let TaskTarget::ExistingChat { thread_id } = &self.target {
            identifier(thread_id)?;
        }
        if self.revision == 0 || self.next_run_at_ms.is_some_and(|at| at < 0) {
            return Err(invalid("invalid scheduled action receipt"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    Once {
        at_ms: i64,
    },
    Interval {
        every_minutes: u32,
    },
    Daily {
        hour: u8,
        minute: u8,
        timezone: String,
    },
    Weekly {
        days: Vec<u8>,
        hour: u8,
        minute: u8,
        timezone: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskTarget {
    NewChat,
    /// Host creates and binds a Chat owned by this task on its first run.
    #[serde(rename = "dedicated_chat")]
    Dedicated,
    ExistingChat {
        thread_id: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Active,
    Paused,
    Completed,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Paused => "paused",
            Self::Completed => "completed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Succeeded,
    Failed,
    Interrupted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }
}

/// A kind another Loupist version wrote and this one does not know reads as
/// `Unknown` with its stored name, so every rewrite of its task or run (a
/// finish, a pause after a failure, a deleted target Chat) writes the name back
/// unchanged. Such a task is listed but never run, and its definition and
/// status are not changed here (`TaskKind::is_known`); it can still be deleted.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    test,
    derive(ts_rs::TS),
    ts(type = "\"custom\" | \"briefing\" | \"earnings\" | string")
)]
pub enum TaskKind {
    Custom,
    Briefing,
    /// "Results before and after": automatic, created by Rust, no model call;
    /// Pause/Resume and Run now only.
    Earnings,
    Unknown(String),
}

impl TaskKind {
    /// The kinds this build runs, by their stored names.
    pub const KNOWN: [Self; 3] = [Self::Custom, Self::Briefing, Self::Earnings];

    pub fn as_str(&self) -> &str {
        match self {
            Self::Custom => "custom",
            Self::Briefing => "briefing",
            Self::Earnings => "earnings",
            Self::Unknown(name) => name,
        }
    }

    pub fn is_known(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

impl Serialize for TaskKind {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskKind {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(Self::KNOWN
            .into_iter()
            .find(|kind| kind.as_str() == name)
            .unwrap_or(Self::Unknown(name)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SkippedTool {
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskDraft {
    pub title: String,
    pub prompt: String,
    pub schedule: Schedule,
    pub target: TaskTarget,
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
    pub use_memory: bool,
    pub update_memory: bool,
    pub status: TaskStatus,
}

impl TaskDraft {
    pub fn validate(&self) -> StoreResult<()> {
        if self.status == TaskStatus::Completed {
            return Err(invalid("new or edited task must be active or paused"));
        }
        self.validate_definition()
    }

    fn validate_definition(&self) -> StoreResult<()> {
        bounded_text(&self.title, 200, false)?;
        if self.prompt.trim().is_empty()
            || self.prompt.len() > MAX_PROMPT_BYTES
            || self.prompt.contains('\0')
        {
            return Err(invalid("scheduled prompt is empty or exceeds its limit"));
        }
        super::schedule::validate_schedule(&self.schedule)?;
        if let TaskTarget::ExistingChat { thread_id } = &self.target {
            identifier(thread_id)?;
        }
        // An empty model means Auto: Rust resolves it at each run (W17.5).
        let auto = self.model_profile_id.is_empty();
        if !auto && !crate::providers::catalog::valid_model_profile_id(&self.model_profile_id) {
            return Err(invalid("scheduled model profile is invalid"));
        }
        bounded_text(&self.thinking_level, 64, auto)?;
        if self.run_options.len() > 16 {
            return Err(invalid("too many scheduled model options"));
        }
        for (key, value) in &self.run_options {
            bounded_text(key, 64, false)?;
            bounded_text(value, 256, false)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Task {
    pub kind: TaskKind,
    pub id: String,
    pub loupist_id: String,
    pub revision: u64,
    pub title: String,
    pub prompt: String,
    pub schedule: Schedule,
    pub target: TaskTarget,
    #[serde(default)]
    pub session_thread_id: Option<String>,
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
    pub use_memory: bool,
    pub update_memory: bool,
    pub status: TaskStatus,
    pub next_run_at_ms: Option<i64>,
    pub last_run_at_ms: Option<i64>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub last_error: Option<String>,
    /// A task the agent created from a conversation stays paused until the user confirms it
    /// in the Chat card; any later status decision clears this.
    #[serde(default)]
    pub awaiting_confirmation: bool,
    /// The reply that proposed it; that reply cannot turn it on in the user's place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub proposed_in_message_id: Option<String>,
}

impl Task {
    pub fn draft(&self) -> TaskDraft {
        TaskDraft {
            title: self.title.clone(),
            prompt: self.prompt.clone(),
            schedule: self.schedule.clone(),
            target: self.target.clone(),
            model_profile_id: self.model_profile_id.clone(),
            thinking_level: self.thinking_level.clone(),
            run_options: self.run_options.clone(),
            use_memory: self.use_memory,
            update_memory: self.update_memory,
            status: self.status,
        }
    }

    pub fn validate(&self) -> StoreResult<()> {
        identifier(&self.id)?;
        identifier(&self.loupist_id)?;
        valid_kind(&self.kind)?;
        if let Some(id) = &self.session_thread_id {
            identifier(id)?;
            if self.target != TaskTarget::Dedicated {
                return Err(invalid("only a dedicated task may retain its session"));
            }
        }
        self.draft().validate_definition()?;
        if self.kind == TaskKind::Briefing
            && (self.target != TaskTarget::Dedicated || !self.use_memory || !self.update_memory)
        {
            return Err(invalid("briefings require a dedicated Memory-enabled Chat"));
        }
        if self.revision == 0
            || self.revision > i64::MAX as u64
            || self.created_at_ms < 0
            || self.updated_at_ms < self.created_at_ms
            || self.next_run_at_ms.is_some_and(|at| at < 0)
            || self.last_run_at_ms.is_some_and(|at| at < 0)
            || (self.status == TaskStatus::Active) != self.next_run_at_ms.is_some()
            || (self.status == TaskStatus::Completed
                && !matches!(self.schedule, Schedule::Once { .. }))
        {
            return Err(invalid("stored scheduled task state is invalid"));
        }
        if let Some(error) = &self.last_error {
            bounded_error(error)?;
        }
        Ok(())
    }
}

/// What a host run did, one item per capture, as fields the renderer words in
/// the reader's language; the results task (`commands/earnings.rs`) writes them,
/// so each kind is named for it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind")]
pub enum RunSummaryItem {
    /// Expected values captured before a results date.
    #[serde(rename = "earnings_expected")]
    Expected {
        name: String,
        period: String,
        count: usize,
    },
    /// Reported values recorded after it.
    #[serde(rename = "earnings_reported")]
    Reported {
        name: String,
        period: String,
        count: usize,
    },
    /// A capture that failed; the run's `error` keeps the reason.
    #[serde(rename = "earnings_failed")]
    Failed { name: String, period: String },
    /// Nothing was due.
    #[serde(rename = "earnings_nothing")]
    Nothing,
}

/// Summary items a run keeps: one per capture, at most eight captures a run.
const MAX_SUMMARY_ITEMS: usize = 16;

impl RunSummaryItem {
    fn valid(&self) -> bool {
        match self {
            Self::Expected { name, period, .. }
            | Self::Reported { name, period, .. }
            | Self::Failed { name, period } => {
                !name.trim().is_empty()
                    && name.chars().count() <= 200
                    && !name.contains('\0')
                    && period.chars().count() <= 32
                    && !period.contains('\0')
            }
            Self::Nothing => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Run {
    pub briefing_context: Option<crate::briefings::BriefingContext>,
    pub kind: TaskKind,
    pub read_at_ms: Option<i64>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_items: Vec<RunSummaryItem>,
    pub message_id: Option<String>,
    pub skipped_tools: Vec<SkippedTool>,
    pub id: String,
    pub task_id: String,
    pub loupist_id: String,
    pub task_revision: u64,
    pub task_title: String,
    pub thread_id: Option<String>,
    pub run_id: String,
    pub scheduled_for_ms: i64,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub status: RunStatus,
    pub error: Option<String>,
}

/// Read-only list projection; the latest occurrence is not stored in Task JSON.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskListItem {
    #[serde(flatten)]
    pub task: Task,
    pub latest_run: Option<Run>,
}

impl Run {
    pub fn validate(&self) -> StoreResult<()> {
        for id in [&self.id, &self.task_id, &self.loupist_id, &self.run_id] {
            identifier(id)?;
        }
        valid_kind(&self.kind)?;
        if let Some(thread_id) = &self.thread_id {
            identifier(thread_id)?;
        }
        bounded_text(&self.task_title, 200, false)?;
        if let Some(id) = &self.message_id {
            identifier(id)?;
        }
        if self.summary.chars().count() > 280
            || self.summary.contains('\0')
            || self.read_at_ms.is_some_and(|at| at < 0)
            || (self.status == RunStatus::Running && self.read_at_ms.is_some())
            || self.skipped_tools.len() > 100
            || self.summary_items.len() > MAX_SUMMARY_ITEMS
            || !self.summary_items.iter().all(RunSummaryItem::valid)
        {
            return Err(invalid("stored inbox fields exceed their limits"));
        }
        for skipped in &self.skipped_tools {
            if skipped.name.chars().count() > 128 || skipped.reason.chars().count() > 256 {
                return Err(invalid("stored skipped tool exceeds its limit"));
            }
        }
        if self.task_revision == 0
            || self.task_revision > i64::MAX as u64
            || self.scheduled_for_ms < 0
            || self.started_at_ms < 0
            || self
                .finished_at_ms
                .is_some_and(|at| at < self.started_at_ms)
            || (self.status == RunStatus::Running) == self.finished_at_ms.is_some()
            || (self.status == RunStatus::Running && self.thread_id.is_none())
        {
            return Err(invalid("stored scheduled run state is invalid"));
        }
        if let Some(error) = &self.error {
            bounded_error(error)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Claim {
    pub task: Task,
    pub run: Run,
}

/// An unknown kind is still a short stored name, never free text.
fn valid_kind(kind: &TaskKind) -> StoreResult<()> {
    if let TaskKind::Unknown(name) = kind {
        if name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(invalid("scheduled task kind is invalid"));
        }
    }
    Ok(())
}

pub(crate) fn identifier(value: &str) -> StoreResult<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(invalid("scheduled record identifier is invalid"));
    }
    Ok(())
}

pub(crate) fn bounded_error(value: &str) -> StoreResult<()> {
    if value.len() > 2000 || value.contains('\0') {
        return Err(invalid("scheduled error exceeds its limit"));
    }
    Ok(())
}

fn bounded_text(value: &str, maximum: usize, empty: bool) -> StoreResult<()> {
    if value.len() > maximum
        || (!empty && value.trim().is_empty())
        || value.chars().any(char::is_control)
    {
        return Err(invalid("scheduled text is empty or exceeds its limit"));
    }
    Ok(())
}

pub(crate) fn invalid(message: &str) -> StoreError {
    StoreError::Invalid(message.into())
}
