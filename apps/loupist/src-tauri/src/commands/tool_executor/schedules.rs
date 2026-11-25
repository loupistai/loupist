//! Agent-facing scheduling. All effects use the same service as the native UI.
use super::*;
use crate::{
    chat::surface::ChatSurface,
    scheduled_tasks::{
        service,
        types::{
            NotifyPolicy, ReportPolicy, Schedule, ScheduleAction, ScheduleReceipt, Task,
            TaskDefinition, TaskDraft, TaskStatus, TaskSubject, TaskTarget, TaskTrigger, TaskWatch,
            TriggerDryRun, TriggerOutcome, TriggerPhase, TriggerToolGrant, WatchPreset,
        },
    },
};
use chrono::Datelike;

pub(in crate::commands) struct TriggerReviewRun {
    pub test: crate::scheduled_tasks::types::TriggerDryRun,
    pub tools: Vec<crate::scheduled_tasks::types::TriggerToolGrant>,
    pub today: crate::scheduled_tasks::trigger::CheckOutput,
}

pub(in crate::commands) fn spawn_dry_run(
    executor: AppToolExecutor,
    code: String,
    args: serde_json::Map<String, Value>,
    previous: Vec<crate::scheduled_tasks::types::TriggerEvaluation>,
    use_memory: bool,
) -> tokio::task::JoinHandle<Result<TriggerReviewRun, String>> {
    tokio::spawn(async move { executor.dry_run(&code, &args, &previous, use_memory).await })
}

#[derive(Clone)]
pub(crate) struct ScheduleContext {
    /// Research continues the admitted Bot turn. Schedules keep the user's
    /// requested defaults, including an unresolved Auto choice.
    pub research_model: Option<crate::providers::catalog::ExecutionModelLock>,
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
    pub use_memory: bool,
    pub update_memory: bool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ScheduleInput {
    Once {
        at: String,
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
impl ScheduleInput {
    fn resolve(self) -> Result<Schedule, BrokerError> {
        let result = match self {
            Self::Once { at } => Schedule::Once { at_ms: DateTime::parse_from_rfc3339(&at)
                .map_err(|_| BrokerError::invalid("schedule.at", "RFC3339 date and time with UTC offset, for example 2027-01-15T09:00:00+09:00"))?.timestamp_millis() },
            Self::Interval { every_minutes } => Schedule::Interval { every_minutes },
            Self::Daily { hour, minute, timezone } => Schedule::Daily { hour, minute, timezone },
            Self::Weekly { days, hour, minute, timezone } => Schedule::Weekly { days, hour, minute, timezone },
        };
        crate::scheduled_tasks::schedule::validate_schedule(&result)
            .map_err(|error| BrokerError::invalid("schedule", error.to_string()))?;
        Ok(result)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum SessionMode {
    Fresh,
    Continue,
}
impl SessionMode {
    fn target(self) -> TaskTarget {
        match self {
            Self::Fresh => TaskTarget::NewChat,
            Self::Continue => TaskTarget::Dedicated,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListRequest {
    #[serde(default)]
    query: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    task_id: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum CreateStatus {
    Active,
    Paused,
    Proposed,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateRequest {
    request_key: String,
    title: String,
    prompt: Option<String>,
    report: Option<ReportPolicy>,
    notify: Option<NotifyPolicy>,
    subjects: Option<Vec<TaskSubject>>,
    terms: Option<String>,
    preset: Option<WatchPreset>,
    preset_params: Option<crate::scheduled_tasks::presets::PresetParams>,
    schedule: Option<ScheduleInput>,
    question: Option<String>,
    handoff_id: Option<String>,
    session_mode: Option<SessionMode>,
    status: Option<CreateStatus>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    task_id: String,
    expected_revision: u64,
    title: Option<String>,
    report: Option<ReportPolicy>,
    notify: Option<NotifyPolicy>,
    subjects: Option<Vec<TaskSubject>>,
    terms: Option<String>,
    prompt: Option<String>,
    schedule: Option<ScheduleInput>,
    session_mode: Option<SessionMode>,
    active_hours: Option<crate::scheduled_tasks::types::TriggerActiveHours>,
    args: Option<serde_json::Map<String, Value>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StatusRequest {
    task_id: String,
    expected_revision: u64,
    status: TaskStatus,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionRequest {
    task_id: String,
    expected_revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StopRequest {
    task_id: String,
    run_id: String,
}

fn decode<T: serde::de::DeserializeOwned>(params: Value) -> Result<T, BrokerError> {
    serde_json::from_value(params).map_err(|_| BrokerError::Malformed)
}
fn error(error: crate::app::CommandError) -> BrokerError {
    if error.code == crate::app::CommandErrorCode::ProviderUnavailable {
        return BrokerError::Execution(format!(
            "{}: the scheduled task's AI account or model is not available",
            error.code
        ))
        .with_next("The task was not changed and cannot run until an AI account that offers its model is connected. Tell the user it needs a connected AI account.");
    }
    command_error(error)
}

fn memory_schedule_refusal() -> BrokerError {
    BrokerError::invalid("memory", "This needs Memory reads and saving, which are off for this Chat. Tell the user it was not changed.")
}

fn require_task_memory(policy: &ToolPolicy, task: &Task) -> Result<(), BrokerError> {
    if task.kind == crate::scheduled_tasks::types::TaskKind::Briefing
        && (!policy.use_memory || !policy.propose_memory_updates)
    {
        return Err(memory_schedule_refusal());
    }
    Ok(())
}

fn schedule_context_refusal() -> BrokerError {
    BrokerError::invalid("schedule", "This reply has no scheduling context. Continue without scheduling and tell the user it was not changed; they can ask again in their next message.")
}
fn revision_error(error: crate::app::CommandError) -> BrokerError {
    if error.code == "conflict" {
        self::error(error).with_next("Read the saved task before retrying. Keep its current revision and change only the requested fields; do not create a replacement.")
    } else {
        self::error(error)
    }
}

fn normalized_title(title: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    crate::knowledge::world_extract::casefold(&title.nfkc().collect::<String>())
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact(task: &Task) -> Value {
    if task.is_unreadable_list_item() {
        return json!({"task_id":task.id,"title":task.title,"status":"unreadable"});
    }
    json!({"task_id":task.id,"title":task.title,"kind":task.kind.as_str(),"revision":task.revision,"status":task.status,
        "model_profile_id": task.model_selection.as_ref().and_then(|s|s.model.as_ref()).map(|m|&m.id),
        "model_selection":task.model_selection,
        "thinking_level":task.model_selection.as_ref().and_then(|s|s.thinking_level.as_ref()),
        "schedule":task.schedule,"session_mode": match task.target { TaskTarget::NewChat=>"fresh", TaskTarget::Dedicated=>"continue", TaskTarget::ExistingChat{..}=>"existing_chat" },
        "next_run_at":task.next_run_at_ms.and_then(DateTime::from_timestamp_millis).map(|t|t.to_rfc3339()),
        "last_error":task.last_error.as_ref().map(|_| RUN_FAILED),"awaiting_confirmation":task.awaiting_confirmation,
        "report":task.watch.report,"delivery":task.delivery(),"subjects":task.watch.subjects,"preset":task.watch.preset,"terms":task.watch.terms})
}

const RUN_FAILED: &str = "The last scheduled run failed. Review the run in Chat.";

fn safe_run(run: crate::scheduled_tasks::types::Run) -> Value {
    // Schedule management also works in Private chats. A run's briefing
    // context and answer summaries can contain Memory and third-party text;
    // they belong to the run's Chat, not this always-available control receipt.
    json!({
        "id": run.id, "task_id": run.task_id, "loupist_id": run.loupist_id,
        "task_revision": run.task_revision, "task_title": run.task_title,
        "kind": run.kind, "thread_id": run.thread_id, "run_id": run.run_id,
        "message_id": run.message_id, "scheduled_for_ms": run.scheduled_for_ms,
        "started_at_ms": run.started_at_ms, "finished_at_ms": run.finished_at_ms,
        "read_at_ms": run.read_at_ms, "status": run.status,
        "error": run.error.as_ref().map(|_| RUN_FAILED),
    })
}

fn apply_watch_fields(
    watch: &mut TaskWatch,
    report: Option<ReportPolicy>,
    notify: Option<NotifyPolicy>,
    subjects: Option<Vec<TaskSubject>>,
    terms: Option<String>,
) {
    if let Some(report) = report {
        watch.report = Some(report);
    }
    if let Some(notify) = notify {
        watch.delivery.get_or_insert_with(Default::default).notify = Some(notify);
    }
    if let Some(subjects) = subjects {
        watch.subjects = Some(subjects);
    }
    if let Some(terms) = terms {
        watch.terms = Some(terms);
    }
}

impl AppToolExecutor {
    pub(super) async fn schedule_quiet(
        &self,
        policy: &ToolPolicy,
        params: Value,
        delivery_id: &str,
    ) -> Result<Value, BrokerError> {
        self.ensure_scope(policy)?;
        let Some(run_id) = self.capture.scheduled_run_id() else {
            return Err(BrokerError::invalid(
                "schedule_quiet",
                "Requires a scheduled custom run.",
            ));
        };
        if params != json!({})
            || self.capture.bound_chat_id.as_deref() != Some(policy.session_id.as_str())
        {
            return Err(BrokerError::Malformed);
        }
        let run = self
            .state
            .store
            .scheduled_task_run_get(&self.loupist_id, run_id)
            .map_err(crate::commands::tool_executor::store_error)?;
        let Some(run) = run.filter(|run| {
            run.kind == crate::scheduled_tasks::types::TaskKind::Custom
                && run.status == crate::scheduled_tasks::types::RunStatus::Running
        }) else {
            return Err(BrokerError::invalid(
                "schedule_quiet",
                "Requires a scheduled custom run.",
            ));
        };
        if !self
            .state
            .store
            .scheduled_run_allows_quiet(&self.loupist_id, run_id)
            .map_err(crate::commands::tool_executor::store_error)?
        {
            return Err(BrokerError::invalid(
                "schedule_quiet",
                "This run must report: a task's first run always does, so the user sees where things stand and what a report looks like, and a task set to report every run does each time. Give the report in your answer, and say so plainly when nothing changed.",
            ));
        }
        if !run.skipped_tools.is_empty() || !self.capture.has_data_for_quiet().await {
            return Err(BrokerError::invalid(
                "schedule_quiet",
                "Report what you could not check in your reply.",
            ));
        }
        self.capture
            .stage(delivery_id, |pending| pending.quiet_schedule = true)
            .await;
        Ok(json!({"quiet":true}))
    }

    fn schedule_source(&self) -> Result<&str, BrokerError> {
        self.capture
            .source_message_id
            .as_deref()
            .ok_or_else(schedule_context_refusal)
    }

    pub(super) async fn schedule_tool(
        &self,
        policy: &ToolPolicy,
        method: ToolMethod,
        params: Value,
    ) -> Result<Value, BrokerError> {
        self.ensure_scope(policy)?;
        if self.capture.bound_chat_id.as_deref() != Some(policy.session_id.as_str()) {
            return Err(BrokerError::invalid("schedule", "This request is not bound to the current Chat. Continue in this Chat and tell the user the schedule was not changed."));
        }
        let read_only = matches!(method, ToolMethod::ScheduleList | ToolMethod::ScheduleRead);
        let surface = self.capture.surface.as_ref();
        if !read_only && !surface.is_some_and(ChatSurface::interactive) {
            return Err(BrokerError::Execution("Schedule management is available only in an interactive Chat. A scheduled run cannot create, change, run, stop, or delete schedules.".into()));
        }
        let is_bot = surface.is_some_and(ChatSurface::is_bot);
        if !read_only && policy.as_of.is_some() {
            return Err(BrokerError::Execution("A scheduled task needs a present-day request. Leave the historical cutoff before scheduling.".into()));
        }
        let allowed_in_thread = match method {
            ToolMethod::ScheduleList | ToolMethod::ScheduleRead => true,
            ToolMethod::ScheduleHandoff => !is_bot,
            _ => is_bot,
        };
        if !allowed_in_thread {
            return Err(BrokerError::invalid(
                method.tool_name(),
                if is_bot {
                    "Already in the Bot; use schedule_create to create the task here."
                } else {
                    "This tool manages schedules in the Bot. Use schedule_handoff to hand the request to the Bot."
                },
            ));
        }
        if self.capture.search_cancel.is_cancelled() {
            return Err(BrokerError::Execution("This Chat has stopped".into()));
        }
        let state = &self.state;
        let loupist = &self.loupist_id;
        let (task, action) = match method {
            ToolMethod::ScheduleBriefing => {
                if !policy.use_memory || !policy.propose_memory_updates {
                    return Err(memory_schedule_refusal());
                }
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct BriefingRequest {
                    schedule: ScheduleInput,
                    enabled: bool,
                    #[serde(default)]
                    edition: Option<crate::scheduled_tasks::types::BriefingEdition>,
                }
                let r: BriefingRequest = decode(params)?;
                let request = crate::briefings::commands::SettingsRequest {
                    loupist_id: loupist.to_owned(),
                    schedule: if r.enabled {
                        Some(r.schedule.resolve()?)
                    } else {
                        None
                    },
                    enabled: r.enabled,
                    edition: r.edition,
                };
                let settings = if request.enabled {
                    crate::briefings::commands::propose_settings(
                        state,
                        request,
                        self.schedule_source()?,
                    )
                } else {
                    crate::briefings::commands::settings_set(state, request)
                }
                .map_err(error)?;
                (
                    settings.task.ok_or_else(|| BrokerError::invalid("schedule", "No briefing task was returned. Read schedule_list before changing it again."))?,
                    ScheduleAction::Updated,
                )
            }
            ToolMethod::ScheduleList => {
                let request: ListRequest = decode(params)?;
                if request.query.len() > 200 {
                    return Err(BrokerError::invalid("query", "at most 200 bytes"));
                }
                let query = request.query.to_lowercase();
                let items = service::list(state, loupist).map_err(error)?;
                let tasks = items
                    .iter()
                    .filter(|item| {
                        item.task.declined_in_visit.is_none()
                            && (item.task.id.to_lowercase().contains(&query)
                                || item.task.title.to_lowercase().contains(&query)
                                || item.task.prompt.to_lowercase().contains(&query))
                    })
                    .map(|item| {
                        let mut result = compact(&item.task);
                        result["latest_run"] =
                            item.latest_run.clone().map(safe_run).unwrap_or(Value::Null);
                        result
                    })
                    .collect::<Vec<_>>();
                return Ok(
                    json!({"tasks":tasks,"pending_questions":if is_bot { crate::scheduled_tasks::questions::pending(state,loupist).map_err(error)?.into_iter().map(|q| json!({"handoff_id":q.receipt.task_id,"title":q.receipt.title,"prompt":q.prompt,"question":q.receipt.question})).collect::<Vec<_>>() } else { vec![] },"total":items.len(),"local_timezone":iana_time_zone::get_timezone().ok(),
                    "current_time":Utc::now().to_rfc3339(),"execution":"local_app_open","max_concurrent_runs":4,
                    "notice":"Includes active, paused and completed tasks in this Loupist. Read a task before changing it. Runs need Loupist open; missed recurring times catch up once. A request for work a listed task already does changes that task; it is not a new one."}),
                );
            }
            ToolMethod::ScheduleRead => {
                let request: ReadRequest = decode(params)?;
                (
                    service::get(state, loupist, &request.task_id).map_err(error)?,
                    ScheduleAction::Viewed,
                )
            }
            ToolMethod::ScheduleCreate | ToolMethod::ScheduleHandoff => {
                let request: CreateRequest = decode(params)?;
                if request.request_key.is_empty()
                    || request.request_key.len() > 64
                    || !request
                        .request_key
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                {
                    return Err(BrokerError::invalid("request_key", "1–64 letters, digits, hyphens or underscores; use a distinct key for each task and reuse it on retries"));
                }
                let context = self
                    .capture
                    .schedule_context
                    .as_ref()
                    .ok_or_else(schedule_context_refusal)?;
                let message = self
                    .capture
                    .source_message_id
                    .as_deref()
                    .ok_or_else(schedule_context_refusal)?;
                let prefix = if is_bot {
                    "scheduled"
                } else {
                    crate::chat::home::HANDOFF_PREFIX
                };
                let mut id = format!(
                    "{prefix}-{}",
                    &sha256(format!("{loupist}\0{message}\0{}", request.request_key).as_bytes())
                        [..32]
                );
                use crate::scheduled_tasks::questions;
                if let Some(question) = request.question {
                    let prompt = request.prompt.unwrap_or_default();
                    if is_bot
                        || request.schedule.is_some()
                        || request.handoff_id.is_some()
                        || question.trim().is_empty()
                        || question.len() > 1000
                        || prompt.trim().is_empty()
                        || prompt.len() > 8192
                    {
                        return Err(BrokerError::invalid("question", "Provide the missing-detail question and a short self-contained request only"));
                    }
                    let receipt = crate::scheduled_tasks::types::QuestionReceipt {
                        task_id: id,
                        title: request.title,
                        question,
                    };
                    ScheduleReceipt::Question(receipt.clone())
                        .validate()
                        .map_err(|_| BrokerError::Malformed)?;
                    let saved = questions::enqueue(
                        state,
                        loupist,
                        questions::BotQuestion {
                            receipt,
                            prompt,
                            model_profile_id: context.model_profile_id.clone(),
                            thinking_level: context.thinking_level.clone(),
                            run_options: context.run_options.clone(),
                            use_memory: context.use_memory,
                            update_memory: context.update_memory,
                            created_at_ms: crate::commands::now_ms(),
                        },
                    )
                    .map_err(error)?;
                    if let Some(sink) = &self.capture.schedule_sink {
                        sink(ScheduleReceipt::Question(saved.receipt.clone()));
                    }
                    let _ = crate::chat::home::deliver_finished_runs(state, loupist);
                    return Ok(
                        json!({"handed_off":true,"scheduled":false,"notice":"Bot will ask for the missing details. Keep the user in this session and do not repeat the question here."}),
                    );
                }
                let origin = if let Some(ref key) = request.handoff_id {
                    if !is_bot || !key.starts_with("bot-handoff-") {
                        return Err(BrokerError::invalid("handoff_id", "Use a pending handoff_id from schedule_list in the Bot, or omit this field for a new task."));
                    }
                    id = format!("scheduled-{key}");
                    let pending = questions::pending(state, loupist)
                        .map_err(error)?
                        .into_iter()
                        .find(|q| &q.receipt.task_id == key);
                    if pending.is_none() {
                        let existing = service::get(state, loupist, &id).map_err(error)?;
                        return Ok(compact(&existing));
                    }
                    pending
                } else {
                    None
                };
                let mut schedule = request.schedule.map(ScheduleInput::resolve).transpose()?;
                let mut target = request.session_mode.map(SessionMode::target);
                let mut prompt = request.prompt;
                let mut watch = TaskWatch::default();
                let mut trigger_template = None;
                if let Some(preset) = request.preset {
                    if !is_bot || prompt.is_some() {
                        return Err(BrokerError::invalid(
                            "preset",
                            "A Bot preset supplies the prompt.",
                        ));
                    }
                    let filled = crate::scheduled_tasks::presets::fill(
                        state,
                        loupist,
                        preset,
                        request.subjects.clone(),
                        request.preset_params.unwrap_or_default(),
                        None,
                    )
                    .map_err(error)?;
                    watch = filled.watch;
                    trigger_template = filled.trigger_template;
                    prompt = Some(filled.prompt);
                    schedule = schedule.or(Some(filled.schedule));
                    target = target.or(Some(TaskTarget::NewChat));
                } else {
                    if request.preset_params.is_some() {
                        return Err(BrokerError::invalid(
                            "preset_params",
                            "Choose a preset first.",
                        ));
                    }
                    watch.subjects = request.subjects;
                }
                apply_watch_fields(
                    &mut watch,
                    request.report,
                    request.notify,
                    None,
                    request.terms,
                );
                watch.report.get_or_insert(ReportPolicy::EveryRun);
                let status = request.status.ok_or_else(|| {
                    BrokerError::invalid(
                        "status",
                        if is_bot {
                            "active, paused or proposed; required with a schedule"
                        } else {
                            "active or paused; required with a schedule"
                        },
                    )
                })?;
                if !is_bot && matches!(status, CreateStatus::Proposed) {
                    return Err(BrokerError::invalid(
                        "status",
                        "Use active or paused for a handoff.",
                    ));
                }
                let trigger = trigger_template
                    .as_deref()
                    .map(|template| {
                        crate::commands::scheduled_tasks::preset_trigger(
                            state,
                            loupist,
                            template,
                            &watch,
                            &request.title,
                        )
                    })
                    .transpose()
                    .map_err(error)?;
                let definition = TaskDefinition {
                    watch,
                    trigger,
                    proposed_in_message_id: Some(message.to_owned()),
                    ..Default::default()
                };
                let mut draft = TaskDraft {
                    title: request.title,
                    prompt: prompt.ok_or_else(|| {
                        BrokerError::invalid("prompt", "Provide a prompt or choose a preset.")
                    })?,
                    schedule: schedule.ok_or_else(|| {
                        BrokerError::invalid("schedule", "Provide a cadence or choose a preset.")
                    })?,
                    target: target.ok_or_else(|| {
                        BrokerError::invalid(
                            "session_mode",
                            "fresh or continue; required with a schedule",
                        )
                    })?,
                    model_profile_id: context.model_profile_id.clone(),
                    thinking_level: context.thinking_level.clone(),
                    run_options: context.run_options.clone(),
                    use_memory: context.use_memory,
                    update_memory: context.update_memory,
                    status: TaskStatus::Paused,
                };
                if let Some(q) = &origin {
                    draft.model_profile_id.clone_from(&q.model_profile_id);
                    draft.thinking_level.clone_from(&q.thinking_level);
                    draft.run_options.clone_from(&q.run_options);
                    draft.use_memory &= q.use_memory;
                    draft.update_memory &= q.update_memory;
                }
                let normalized = normalized_title(&draft.title);
                if let Some(existing) =
                    service::list(state, loupist)
                        .map_err(error)?
                        .iter()
                        .find(|item| {
                            is_bot
                                && item.task.id != id
                                && item.task.kind == crate::scheduled_tasks::types::TaskKind::Custom
                                && item.task.status != TaskStatus::Completed
                                && item.task.last_error.as_deref()
                                    != Some(crate::scheduled_tasks::UNREADABLE_TASK)
                                && normalized_title(&item.task.title) == normalized
                        })
                {
                    return Err(BrokerError::invalid("title", format!("\"{}\" already exists as task {} ({}). To change it, call schedule_read then schedule_update; if it already does what the user asked, change nothing and say so; for a separate task use a title that says how it differs.", draft.title, existing.task.id, existing.task.status.as_str())));
                }
                let task = service::create_definition(state, loupist, draft, &id, definition)
                    .map_err(error)?;
                if let Some(q) = origin {
                    questions::resolve(state, loupist, &q.receipt.task_id).map_err(error)?;
                }
                // The durable task also serves as the Bot's pending delivery if its chat is busy.
                if !is_bot {
                    let _ = crate::chat::home::deliver_finished_runs(state, loupist);
                }
                (task, ScheduleAction::Created)
            }
            ToolMethod::ScheduleUpdate => {
                if params.get("script").is_some()
                    || params.get("template").is_some()
                    || params.get("code").is_some()
                {
                    return Err(BrokerError::invalid(
                        "script",
                        "schedule_update cannot change a check's code. Save a new version with trigger_save; the approved version keeps running.",
                    ));
                }
                let request: UpdateRequest = decode(params)?;
                if request.title.is_none()
                    && request.prompt.is_none()
                    && request.schedule.is_none()
                    && request.session_mode.is_none()
                    && request.report.is_none()
                    && request.notify.is_none()
                    && request.subjects.is_none()
                    && request.terms.is_none()
                    && request.active_hours.is_none()
                    && request.args.is_none()
                {
                    return Err(BrokerError::invalid("update", "at least one changed field"));
                }
                let mut current = service::get(state, loupist, &request.task_id).map_err(error)?;
                if current.kind == crate::scheduled_tasks::types::TaskKind::Briefing
                    || current
                        .replaces
                        .as_ref()
                        .is_some_and(|target| target.id.starts_with("briefing-"))
                {
                    // TODO(model-text): Direct briefing edits through its host-owned settings.
                    return Err(BrokerError::invalid(
                        "schedule",
                        "Use briefing settings to edit the host-authored briefing",
                    ));
                }
                if current.watch.preset == Some(WatchPreset::PriceMove)
                    && current
                        .trigger
                        .as_ref()
                        .is_some_and(|check| check.approved.is_some())
                    && (request.subjects.is_some() || request.terms.is_some())
                {
                    // TODO(model-text): Route price-check subject and term changes through candidate review.
                    return Err(BrokerError::invalid(
                        "trigger",
                        "Use trigger_save to test new price check subjects or terms",
                    ));
                }
                if request.args.is_some() {
                    // TODO(model-text): Route check argument changes through a new dry run.
                    return Err(BrokerError::invalid(
                        "args",
                        "Use trigger_save to test new check arguments",
                    ));
                }
                if let Some(hours) = request.active_hours {
                    let trigger = current.trigger.as_mut().ok_or_else(|| {
                        BrokerError::invalid("trigger", "This task has no check to change.")
                    })?;
                    trigger.set_active_hours(Some(hours));
                    trigger
                        .validate()
                        .map_err(crate::commands::tool_executor::store_error)?;
                }
                let mut draft = current.draft();
                let mut watch = current.watch.clone();
                apply_watch_fields(
                    &mut watch,
                    request.report,
                    request.notify,
                    request.subjects,
                    request.terms,
                );
                if let Some(title) = request.title {
                    draft.title = title;
                }
                if let Some(prompt) = request.prompt {
                    draft.prompt = prompt;
                }
                if let Some(schedule) = request.schedule {
                    draft.schedule = schedule.resolve()?;
                }
                if let Some(mode) = request.session_mode {
                    draft.target = mode.target();
                }
                let task = if !matches!(draft.schedule, Schedule::Once { .. })
                    || !matches!(current.schedule, Schedule::Once { .. })
                    || current.status == TaskStatus::Completed
                    || current.awaiting_confirmation
                {
                    service::propose_change(
                        state,
                        &current,
                        request.expected_revision,
                        draft,
                        watch,
                        self.schedule_source()?,
                    )
                } else {
                    service::update(
                        state,
                        loupist,
                        &request.task_id,
                        request.expected_revision,
                        draft,
                        watch,
                        None,
                    )
                }
                .map_err(revision_error)?;
                (task, ScheduleAction::Updated)
            }
            ToolMethod::ScheduleSetStatus => {
                let request: StatusRequest = decode(params)?;
                let task = service::get(state, loupist, &request.task_id).map_err(error)?;
                require_task_memory(policy, &task)?;
                let action = match request.status {
                    TaskStatus::Paused => ScheduleAction::Paused,
                    TaskStatus::Active => ScheduleAction::Resumed,
                    TaskStatus::Completed => {
                        return Err(BrokerError::invalid("status", "active or paused"))
                    }
                };
                if task.awaiting_confirmation {
                    return Err(BrokerError::Execution(
                        "This task is a proposal waiting for the user's OK in its card, and only the user turns it on or declines it there; it stays paused until then. Leave its status as it is and tell the user, in their language and without naming the button, that it starts once they turn it on from the card.".into(),
                    ));
                }
                let result =
                    if request.status == TaskStatus::Active && task.status == TaskStatus::Active {
                        if task.revision != request.expected_revision {
                            return Err(revision_error(crate::app::CommandError::conflict(
                                "scheduled task revision changed",
                            )));
                        }
                        Ok(task)
                    } else if request.status == TaskStatus::Active {
                        service::propose_change(
                            state,
                            &task,
                            request.expected_revision,
                            task.draft(),
                            task.watch.clone(),
                            self.schedule_source()?,
                        )
                    } else {
                        service::set_status(
                            state,
                            loupist,
                            &request.task_id,
                            request.expected_revision,
                            request.status,
                        )
                    }
                    .map_err(revision_error)?;
                (result, action)
            }
            ToolMethod::ScheduleDelete => {
                let request: RevisionRequest = decode(params)?;
                let task = service::get(state, loupist, &request.task_id).map_err(error)?;
                service::delete(state, loupist, &request.task_id, request.expected_revision)
                    .map_err(revision_error)?;
                (task, ScheduleAction::Deleted)
            }
            ToolMethod::ScheduleRunNow => {
                let request: ReadRequest = decode(params)?;
                let task = service::get(state, loupist, &request.task_id).map_err(error)?;
                require_task_memory(policy, &task)?;
                service::run_now(state, loupist, &request.task_id)
                    .await
                    .map_err(error)?;
                (
                    service::get(state, loupist, &request.task_id).map_err(error)?,
                    ScheduleAction::Started,
                )
            }
            ToolMethod::TriggerSave => self.save_trigger(policy, params).await?,
            ToolMethod::ScheduleStop => {
                let request: StopRequest = decode(params)?;
                service::stop(state, loupist, &request.task_id, &request.run_id)
                    .await
                    .map_err(error)?;
                (
                    service::get(state, loupist, &request.task_id).map_err(error)?,
                    ScheduleAction::Stopped,
                )
            }
            _ => return Err(BrokerError::MethodDenied),
        };
        // These are immediate control effects, independent of Memory finalization.
        let receipt = ScheduleReceipt::from_task(&task, action);
        if let Some(sink) = &self.capture.schedule_sink {
            sink(receipt.clone());
        }
        let mut result = compact(&task);
        result["action"] = json!(action);
        result["receipt"] = json!(receipt);
        result["saved"] = json!(action != ScheduleAction::Viewed);
        result["execution"] = json!("local_app_open");
        result["prompt"] = json!(task.prompt);
        result["memory"] = json!({"use_memory":task.use_memory,"update_memory":task.update_memory});
        result["notice"]=json!("Runs while Loupist is open; on a Mac the user can have it open at login under Settings > General. This saved action remains if the reply later stops. Fresh runs have new Chat history; continue runs reuse only this task’s dedicated Chat. Both use the saved Memory settings.");
        if task.awaiting_confirmation {
            // TODO(model-text): Describe proposals and retain the existing schedule until approval.
            result["notice"] = json!("Saved paused, waiting for the user's OK in the card under your reply; nothing runs until they turn it on there. Tell the user this in their language without naming the button, and do not turn it on yourself in this reply.");
        }
        if let Some(trigger) = &task.trigger {
            result["trigger"] = json!({
                "state": trigger.state,
                "summary": trigger.summary,
                "tools": trigger.tools,
                "script": trigger.script,
                "approved": trigger.approved.as_ref().map(|approved| json!({
                    "version": approved.version,
                    "sha256": approved.sha256,
                    "at_ms": approved.at_ms,
                })),
                "pending": trigger.pending.as_ref().map(|pending| json!({
                    "version": pending.version, "sha256": pending.sha256,
                    "summary": pending.configuration.as_ref().and_then(|plan| plan.summary.as_ref()),
                })),
            });
            if policy.use_memory && task.use_memory {
                let summary = |evaluation: &crate::scheduled_tasks::types::TriggerEvaluation| json!({"outcome": evaluation.outcome, "reading": evaluation.reading, "as_of": evaluation.as_of, "error": evaluation.error, "at_ms": evaluation.at_ms});
                result["trigger"]["last"] = json!(trigger.last.as_ref().map(summary));
                result["trigger"]["evaluations"] = json!(trigger
                    .evaluations
                    .iter()
                    .take(5)
                    .map(summary)
                    .collect::<Vec<_>>());
                let test = match &trigger.pending {
                    Some(pending) => pending.test.as_ref(),
                    None => trigger.proposal_test.as_ref(),
                };
                if let Some(test) = test {
                    result["dry_run"] = json!(test);
                }
            }
        }
        if action != ScheduleAction::Deleted {
            result["recent_runs"] = json!(service::runs(state, loupist, &task.id)
                .map_err(error)?
                .into_iter()
                .take(5)
                .map(safe_run)
                .collect::<Vec<_>>());
        }
        Ok(result)
    }

    async fn save_trigger(
        &self,
        policy: &ToolPolicy,
        params: Value,
    ) -> Result<(Task, ScheduleAction), BrokerError> {
        #[derive(Deserialize)]
        struct Request {
            task_id: Option<String>,
            title: Option<String>,
            prompt: String,
            terms: String,
            script: Option<String>,
            template: Option<String>,
            args: Option<serde_json::Map<String, Value>>,
            template_params: Option<serde_json::Map<String, Value>>,
            schedule: Option<ScheduleInput>,
            active_hours: Option<crate::scheduled_tasks::types::TriggerActiveHours>,
            notify: Option<NotifyPolicy>,
            subjects: Option<Vec<TaskSubject>>,
        }
        let request: Request = decode(params)?;
        crate::scheduled_tasks::trigger::validate_summary(&request.terms)
            .map_err(|_| BrokerError::invalid("terms", "one line of at most 160 characters"))?;
        use crate::commands::triggers::{price_review_watch, scoped_price_args, scoped_price_ids};
        use crate::scheduled_tasks::trigger::set_price_subject_args;
        let args = crate::commands::triggers::trigger_args(request.args, request.template_params);
        crate::commands::triggers::args_ok(&args)
            .map_err(|error| BrokerError::invalid("args", error.to_string()))?;
        let (code, name, saved) = if let Some(template) = &request.template {
            let version = crate::commands::triggers::install_template(
                &self.state,
                &self.loupist_id,
                template,
                &request.terms,
            )
            .map_err(error)?;
            (version.code.clone(), version.summary.name.clone(), version)
        } else if let Some(script) = &request.script {
            let (script_name, version) = crate::store::scripts::script_reference(script)
                .map_err(|error| BrokerError::invalid("script", error.to_string()))?;
            let saved = self
                .state
                .store
                .script_get(&self.loupist_id, script_name, version)
                .map_err(|err| BrokerError::Execution(err.to_string()))?;
            (saved.code.clone(), saved.summary.name.clone(), saved)
        } else {
            return Err(BrokerError::invalid(
                "script",
                "name a saved script or a bundled template",
            ));
        };
        if saved.summary.metadata.after_outside_text {
            return Err(BrokerError::UntrustedTextPaused);
        }
        let context = self
            .capture
            .schedule_context
            .as_ref()
            .ok_or_else(schedule_context_refusal)?;

        let mut trigger = TaskTrigger {
            script: crate::commands::triggers::script_ref(&saved),
            summary: request.terms.clone(),
            tools: Vec::new(),
            args: args.clone(),
            active_hours: request.active_hours.clone(),
            approved: None,
            pending: None,
            state: TriggerPhase::Proposed,
            script_state: json!({}),
            replay_state_known: true,
            last: None,
            evaluations: Vec::new(),
            counters: Default::default(),
            approved_versions: Vec::new(),
            stale_after_days: 5,
            reads_per_check: 1,
            proposal_test: None,
        };
        let original = request
            .task_id
            .as_ref()
            .map(|id| service::get(&self.state, &self.loupist_id, id).map_err(error))
            .transpose()?;
        let mut price_subjects = None;
        let mut test_args = args;
        let (use_memory, previous) = if let Some(task) = &original {
            let existing = task
                .trigger
                .as_ref()
                .ok_or_else(|| BrokerError::invalid("task_id", "That task has no check."))?;
            if existing.script.name != name {
                return Err(BrokerError::invalid(
                    "script",
                    "A new check needs its own task. This one already runs a different script.",
                ));
            }
            if task.watch.preset == Some(WatchPreset::PriceMove) {
                let mut configuration = trigger.configuration();
                configuration.price_subjects.clone_from(&request.subjects);
                let watch = price_review_watch(&task.watch, &configuration).map_err(error)?;
                set_price_subject_args(&mut trigger.args, watch.subjects.as_deref());
                crate::commands::triggers::args_ok(&trigger.args).map_err(error)?;
                let ids = scoped_price_ids(&self.state, &self.loupist_id, &watch).map_err(error)?;
                if ids.as_ref().is_some_and(Vec::is_empty) {
                    // TODO(model-text): Report a candidate with no matching followed asset.
                    return Err(BrokerError::invalid(
                        "subjects",
                        "No matching asset is followed.",
                    ));
                }
                test_args = scoped_price_args(&trigger.args, ids.as_deref());
                price_subjects = Some(watch.subjects.unwrap_or_default());
            }
            let use_memory = policy.use_memory && task.use_memory;
            let previous = if use_memory {
                existing.evaluations.clone()
            } else {
                Vec::new()
            };
            (use_memory, previous)
        } else {
            (policy.use_memory && context.use_memory, Vec::new())
        };
        let reviewed = spawn_dry_run(self.clone(), code, test_args, previous, use_memory)
            .await
            .map_err(|_| BrokerError::Execution("The dry run could not finish.".into()))?
            .map_err(|_| BrokerError::Execution("The dry run could not finish.".into()))?;
        trigger.reads_per_check = reviewed.today.reads.max(1);
        trigger.tools = reviewed.tools;
        trigger.proposal_test = Some(reviewed.test);
        let state = &self.state;
        let loupist = &self.loupist_id;
        let message = self
            .capture
            .source_message_id
            .as_deref()
            .ok_or_else(schedule_context_refusal)?;
        if let Some(mut task) = original {
            let existing = task.trigger.as_mut().expect("checked before review");
            if existing.approved.is_some() {
                let mut configuration = trigger.configuration();
                configuration.price_subjects = price_subjects;
                existing.pending = Some(crate::scheduled_tasks::types::TriggerPending {
                    version: trigger.script.version,
                    sha256: trigger.script.sha256.clone(),
                    test: trigger.proposal_test.clone(),
                    configuration: Some(configuration),
                });
                if existing.state == TriggerPhase::On {
                    existing.state = TriggerPhase::UpdateProposed;
                }
            } else {
                task.watch.terms = Some(trigger.summary.clone());
                if let Some(subjects) = price_subjects {
                    task.watch.subjects = Some(subjects);
                }
                *existing = trigger;
            }
            let revision = task.revision;
            let saved = state
                .store
                .scheduled_task_write(task, revision, crate::commands::now_ms())
                .map_err(|err| BrokerError::Execution(err.to_string()))?;
            return Ok((saved, ScheduleAction::Updated));
        }
        let schedule = request
            .schedule
            .ok_or_else(|| BrokerError::invalid("schedule", "a new check needs a schedule"))?
            .resolve()?;
        if let Schedule::Interval { every_minutes } = schedule {
            if every_minutes < crate::scheduled_tasks::types::MIN_TRIGGER_INTERVAL_MINUTES {
                return Err(BrokerError::invalid(
                    "schedule",
                    "a check repeats at least every 5 minutes",
                ));
            }
        }
        let mut watch = TaskWatch {
            terms: Some(request.terms),
            subjects: request.subjects,
            report: Some(crate::scheduled_tasks::types::ReportPolicy::EveryRun),
            ..TaskWatch::default()
        };
        if let Some(notify) = request.notify {
            watch.delivery = Some(crate::scheduled_tasks::types::TaskDelivery {
                to: crate::scheduled_tasks::types::DeliveryTarget::Bot,
                notify: Some(notify),
            });
        }
        let draft = TaskDraft {
            title: request.title.unwrap_or_else(|| "Check".into()),
            prompt: request.prompt,
            schedule,
            target: TaskTarget::NewChat,
            model_profile_id: context.model_profile_id.clone(),
            thinking_level: context.thinking_level.clone(),
            run_options: context.run_options.clone(),
            use_memory,
            update_memory: policy.propose_memory_updates && context.update_memory,
            status: TaskStatus::Paused,
        };
        let task = service::create_definition(
            state,
            loupist,
            draft,
            &crate::commands::new_id("check"),
            crate::scheduled_tasks::types::TaskDefinition {
                watch,
                proposed_in_message_id: Some(message.to_owned()),
                trigger: Some(trigger),
                ..Default::default()
            },
        )
        .map_err(error)?;
        Ok((task, ScheduleAction::Created))
    }

    async fn dry_run(
        &self,
        code: &str,
        args: &serde_json::Map<String, Value>,
        previous: &[crate::scheduled_tasks::types::TriggerEvaluation],
        use_memory: bool,
    ) -> Result<TriggerReviewRun, String> {
        use crate::scheduled_tasks::types::{MAX_TRIGGER_READS_PER_EVALUATION, MAX_TRIGGER_ROUNDS};
        let allowed = super::python_reads::CHECK_READ_TOOLS
            .iter()
            .map(|name| TriggerToolGrant {
                name: (*name).into(),
                provider: None,
            })
            .collect::<Vec<_>>();
        let now = chrono::Utc::now();
        let request =
            |now, as_of, state, replay, max_rounds, max_calls| super::python_reads::CheckRun {
                code,
                after_outside_text: false,
                use_memory,
                args: args.clone(),
                state,
                now,
                as_of,
                allowed: &allowed,
                discover_providers: true,
                replay,
                max_rounds,
                max_calls,
            };
        let today = self
            .run_check(request(
                now.to_rfc3339(),
                None,
                json!({}),
                &[],
                MAX_TRIGGER_ROUNDS,
                MAX_TRIGGER_READS_PER_EVALUATION,
            ))
            .await
            .map_err(|error| error.to_string())?;
        let (mut dry, tools) =
            replay_dry_run(&today, now.date_naive(), |date, state, max_calls| {
                self.run_check(request(
                    format!("{date}T23:59:59Z"),
                    Some(date),
                    state,
                    &[],
                    MAX_TRIGGER_ROUNDS,
                    max_calls,
                ))
            })
            .await?;
        let previous = if use_memory { previous } else { &[] };
        dry.scores = Some(
            replay_comparison(previous, |now, state, ledger| {
                self.run_check(request(now, None, state, ledger, 1, 0))
            })
            .await,
        );
        Ok(TriggerReviewRun {
            test: dry,
            tools,
            today,
        })
    }
}

async fn replay_comparison<'a, F>(
    previous: &'a [crate::scheduled_tasks::types::TriggerEvaluation],
    mut check: impl FnMut(String, Value, &'a [Value]) -> F,
) -> crate::scheduled_tasks::types::TriggerScores
where
    F: std::future::Future<
        Output = Result<crate::scheduled_tasks::trigger::CheckOutput, BrokerError>,
    >,
{
    use crate::scheduled_tasks::types::MAX_STORED_EVALUATIONS;

    let mut state = None;
    let mut previous_pin = None;
    let mut replayed = Vec::new();
    for evaluation in previous.iter().take(MAX_STORED_EVALUATIONS).rev() {
        let pin = (evaluation.version, evaluation.sha256.as_str());
        if previous_pin.is_some_and(|prior| prior != pin) {
            // Approving or reverting to another pin clears the live state.
            state = Some(json!({}));
        }
        previous_pin = Some(pin);
        if !evaluation.can_replay() {
            if evaluation.outcome.updates_state() {
                // A successful check can change state even if its read ledger
                // was too large to keep. A later ordinary live snapshot
                // cannot repair the candidate's missing state.
                state = None;
            }
            replayed.push(None);
            continue;
        }
        if state.is_none() {
            state = evaluation
                .replay_seed
                .as_ref()
                .filter(|seed| seed.is_object())
                .cloned();
        }
        let mut output = None;
        if let Some(input) = &mut state {
            let now = chrono::DateTime::from_timestamp_millis(evaluation.at_ms)
                .unwrap_or_default()
                .to_rfc3339();
            output = check(now, input.clone(), &evaluation.ledger).await.ok();
            if let Some(replayed_check) = &output {
                if replayed_check.outcome.updates_state() {
                    input.clone_from(&replayed_check.state);
                }
            }
        }
        replayed.push(output);
    }
    let mut replayed = replayed.into_iter().rev();
    crate::scheduled_tasks::trigger::score_replays(previous, |_| replayed.next().flatten())
}

async fn replay_dry_run<F>(
    today: &crate::scheduled_tasks::trigger::CheckOutput,
    date: chrono::NaiveDate,
    mut check: impl FnMut(String, Value, usize) -> F,
) -> Result<(TriggerDryRun, Vec<TriggerToolGrant>), String>
where
    F: std::future::Future<
        Output = Result<crate::scheduled_tasks::trigger::CheckOutput, BrokerError>,
    >,
{
    use crate::scheduled_tasks::types::{
        TriggerDryDay, MAX_TRIGGER_READS_PER_EVALUATION, TRIGGER_DRY_RUN_DAYS,
        TRIGGER_DRY_RUN_READS,
    };
    let mut dates = Vec::with_capacity(TRIGGER_DRY_RUN_DAYS);
    let mut cursor = date;
    while dates.len() < TRIGGER_DRY_RUN_DAYS {
        cursor = cursor
            .pred_opt()
            .ok_or_else(|| "no earlier day".to_owned())?;
        if cursor.weekday().number_from_monday() <= 5 {
            dates.push(cursor);
        }
    }
    let mut tools = today.tools.clone();
    let mut state = json!({});
    let mut reads = today.reads;
    let mut days = Vec::new();
    for date in dates.into_iter().rev() {
        let remaining = TRIGGER_DRY_RUN_READS.saturating_sub(reads as usize);
        if remaining == 0 {
            break;
        }
        let date = date.to_string();
        let Ok(output) = check(
            date.clone(),
            state.clone(),
            remaining.min(MAX_TRIGGER_READS_PER_EVALUATION),
        )
        .await
        else {
            continue;
        };
        reads = reads.saturating_add(output.reads);
        for tool in output.tools {
            if !tools.contains(&tool) {
                tools.push(tool);
            }
        }
        if matches!(output.outcome, TriggerOutcome::Met | TriggerOutcome::NotMet) {
            state = output.state;
            days.push(TriggerDryDay {
                date,
                outcome: output.outcome,
                reading: output.reading,
            });
        }
    }
    let unreplayed = (TRIGGER_DRY_RUN_DAYS - days.len()) as u32;
    let mut dry = crate::scheduled_tasks::trigger::dry_run_from_days(today, &days, unreplayed);
    dry.reads = reads;
    Ok((dry, tools))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::PiArtifacts,
        commands::tests::{profile, seed_profile},
    };
    #[tokio::test]
    async fn comparison_replays_new_items_in_order_with_carried_state() {
        use crate::scheduled_tasks::{
            trigger,
            types::{TriggerEvaluation, TriggerOutcome},
        };
        let occurrence = |at, outcome, item| TriggerEvaluation {
            at_ms: at,
            scheduled_for_ms: at,
            outcome,
            reading: "Listed items".into(),
            as_of: Some("2026-11-24".into()),
            evidence: Value::Null,
            version: 1,
            sha256: "ab".repeat(32),
            reads: 1,
            error: None,
            coalesced: false,
            ledger: vec![json!(item)],
            card_message_id: None,
            retry_after_seconds: None,
            replay_seed: (at == 1).then(|| json!({})),
        };
        let previous = [
            occurrence(3, TriggerOutcome::Met, "new"),
            occurrence(2, TriggerOutcome::Error, "error"),
            occurrence(1, TriggerOutcome::NotMet, "baseline"),
        ];
        let scores = replay_comparison(&previous, |_, state, ledger| async move {
            let item = ledger[0].as_str().unwrap();
            if item == "error" {
                return Ok(trigger::errored("Provider failed", vec![], 0));
            }
            // The bundled new-item check treats its first list as a baseline.
            let met = state["seen"].as_str().is_some_and(|prior| prior != item);
            Ok(trigger::output_from_worker(
                &json!({"status":"completed","check_verdict":{
                    "met": met, "reading": "Listed items", "as_of":"2026-11-24",
                    "state":{"seen":item}
                }}),
                vec![],
                0,
            ))
        })
        .await;
        assert_eq!(scores.compared, 2);
        assert_eq!(scores.previous_met, 1);
        assert_eq!(scores.next_met, 1);
        assert_eq!(scores.days[0].next, TriggerOutcome::Met);
        assert_eq!(scores.days[1].next, TriggerOutcome::NotMet);
    }

    #[tokio::test]
    async fn comparison_replays_zero_read_state_changes_before_scoring() {
        use crate::scheduled_tasks::{trigger, types::TriggerEvaluation};
        let occurrence = |at, outcome, ledger| TriggerEvaluation {
            at_ms: at,
            scheduled_for_ms: at,
            outcome,
            reading: "Checked".into(),
            as_of: Some("2026-11-24".into()),
            evidence: Value::Null,
            version: 1,
            sha256: "ab".repeat(32),
            reads: u32::from(ledger),
            error: None,
            coalesced: false,
            ledger: if ledger { vec![json!("item")] } else { vec![] },
            card_message_id: None,
            retry_after_seconds: None,
            replay_seed: Some(json!({})),
        };
        let previous = [
            occurrence(2, TriggerOutcome::Met, true),
            occurrence(1, TriggerOutcome::NotMet, false),
        ];
        let score = replay_comparison(&previous, |_, state, ledger| {
            let matched = !ledger.is_empty() && state["ready"] == true;
            std::future::ready(Ok(trigger::output_from_worker(
                &json!({"status":"completed", "check_verdict":{
                    "met": matched, "reading":"Checked", "as_of":"2026-11-24",
                    "state":{"ready":true}
                }}),
                vec![],
                0,
            )))
        })
        .await;
        assert_eq!(score.compared, 2);
        assert_eq!((score.previous_met, score.next_met), (1, 1));
    }

    #[tokio::test]
    async fn a_discarded_ledger_stops_scoring_until_a_pin_change_resets_state() {
        use crate::scheduled_tasks::{
            trigger::{apply_output, output_from_worker},
            types::TriggerOutcome,
        };
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::test_fixtures::create_folder_loupist(&state.store, loupist, root.path());
        let mut live = crate::commands::scheduled_tasks::preset_trigger(
            &state,
            loupist,
            "new-item",
            &Default::default(),
            "New items",
        )
        .unwrap();
        let verdict = |met, state, reads| {
            output_from_worker(
                &json!({"status":"completed", "check_verdict":{
                    "met": met, "reading":"Checked", "as_of":"2026-11-24", "state":state
                }}),
                vec![],
                reads,
            )
        };
        apply_output(&mut live, 1, 1, verdict(false, json!({}), 0), 0);
        // A successful read whose oversized ledger was discarded still updates live state.
        apply_output(&mut live, 2, 2, verdict(false, json!({"ready":true}), 1), 0);
        let ready = live.script_state["ready"] == true;
        assert!(ready);
        apply_output(&mut live, 3, 3, verdict(ready, json!({"ready":true}), 0), 0);
        assert!(live.evaluations[0].replay_seed.is_none());

        let mut replayed = Vec::new();
        let scores = replay_comparison(&live.evaluations, |now, input, _| {
            replayed.push(now);
            std::future::ready(Ok(verdict(input["ready"] == true, input, 0)))
        })
        .await;
        assert_eq!(replayed.len(), 1);
        assert_eq!(
            (scores.compared, scores.previous_met, scores.next_met),
            (1, 0, 0)
        );
        assert_eq!(scores.days[0].previous, TriggerOutcome::Met);
        assert_eq!(scores.days[0].next, TriggerOutcome::NoData);
        assert_eq!(scores.days[1].next, TriggerOutcome::NoData);

        live.script.version += 1;
        live.script.sha256 = "ab".repeat(32);
        live.reset_script_state();
        apply_output(&mut live, 4, 4, verdict(false, json!({}), 0), 0);
        let scores = replay_comparison(&live.evaluations, |_, input, _| {
            assert_eq!(input, json!({}));
            std::future::ready(Ok(verdict(false, input, 0)))
        })
        .await;
        assert_eq!(
            (scores.compared, scores.previous_met, scores.next_met),
            (2, 0, 0)
        );
        assert_eq!(scores.days[0].next, TriggerOutcome::NotMet);
        assert_eq!(scores.days[1].next, TriggerOutcome::NoData);
    }

    #[tokio::test]
    async fn a_missing_replay_state_survives_the_twenty_first_evaluation() {
        use crate::scheduled_tasks::{
            trigger::{apply_output, output_from_worker},
            types::{TriggerOutcome, MAX_STORED_EVALUATIONS},
        };
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::test_fixtures::create_folder_loupist(&state.store, loupist, root.path());
        let mut live = crate::commands::scheduled_tasks::preset_trigger(
            &state,
            loupist,
            "new-item",
            &Default::default(),
            "New items",
        )
        .unwrap();
        let verdict = |met, state, reads| {
            output_from_worker(
                &json!({"status":"completed", "check_verdict":{
                    "met":met,"reading":"Checked","as_of":"2026-11-24","state":state
                }}),
                vec![],
                reads,
            )
        };

        // The live threshold stored a match from a read with a discarded ledger.
        apply_output(
            &mut live,
            1,
            1,
            verdict(true, json!({"threshold_met":true}), 1),
            0,
        );
        for offset in 0..MAX_STORED_EVALUATIONS {
            let at = 2 + offset as i64;
            let cached = live.script_state.clone();
            apply_output(
                &mut live,
                at,
                at,
                verdict(cached["threshold_met"] == true, cached, 0),
                0,
            );
        }
        let mut stored = serde_json::to_value(&live).unwrap();
        assert_eq!(stored["replay_state_known"], false);
        let mut live: TaskTrigger = serde_json::from_value(stored.clone()).unwrap();
        assert_eq!(live.evaluations.len(), MAX_STORED_EVALUATIONS);
        assert!(live
            .evaluations
            .iter()
            .all(|evaluation| evaluation.reads == 0));
        assert!(live.script_state["threshold_met"] == true);

        let replay = |_, input: Value, _: &[Value]| {
            // With the larger threshold the discarded read would have cached false.
            // Starting from the live cache would make every proposed replay match.
            std::future::ready(Ok(verdict(input["threshold_met"] == true, input, 0)))
        };
        let scores = replay_comparison(&live.evaluations, replay).await;
        assert_eq!(
            (scores.compared, scores.previous_met, scores.next_met),
            (0, 0, 0)
        );
        assert_eq!(scores.days.len(), MAX_STORED_EVALUATIONS);
        assert!(scores
            .days
            .iter()
            .all(|day| day.next == TriggerOutcome::NoData));

        // A legacy snapshot without provenance cannot establish the seed either.
        stored.as_object_mut().unwrap().remove("replay_state_known");
        for evaluation in stored["evaluations"].as_array_mut().unwrap() {
            evaluation["input_state"] = json!({"threshold_met":true});
        }
        let legacy: TaskTrigger = serde_json::from_value(stored).unwrap();
        assert_eq!(replay_comparison(&legacy.evaluations, replay).await, scores);

        // A same-pin approval also resets state. Only the new check can be compared.
        live.reset_script_state();
        let at = 2 + MAX_STORED_EVALUATIONS as i64;
        apply_output(&mut live, at, at, verdict(false, json!({}), 0), 0);
        let reset = replay_comparison(&live.evaluations, replay).await;
        assert_eq!(
            (reset.compared, reset.previous_met, reset.next_met),
            (1, 0, 0)
        );
        assert_eq!(reset.days[0].next, TriggerOutcome::NotMet);
        assert!(reset.days[1..]
            .iter()
            .all(|day| day.next == TriggerOutcome::NoData));
    }

    #[tokio::test]
    async fn comparison_seeds_the_retained_window_from_the_first_evaluation() {
        use crate::scheduled_tasks::{
            trigger::{apply_output, output_from_worker},
            types::{TriggerOutcome, MAX_STORED_EVALUATIONS},
        };

        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        let loupist = "loupist-a";
        crate::test_fixtures::create_folder_loupist(&state.store, loupist, root.path());
        let mut check = crate::commands::scheduled_tasks::preset_trigger(
            &state,
            loupist,
            "new-item",
            &Default::default(),
            "New items",
        )
        .unwrap();
        let verdict = |item: &Value, met: bool| {
            output_from_worker(
                &json!({"status":"completed","check_verdict":{
                    "met": met, "reading": "Listed items", "as_of":"2026-11-24",
                    "state":{"seen":item}
                }}),
                vec![item.clone()],
                1,
            )
        };
        for number in 0..=MAX_STORED_EVALUATIONS {
            let at = 1_795_560_000_000 + number as i64 * 60_000;
            let item = json!(format!("item-{number}"));
            apply_output(&mut check, at, at, verdict(&item, number != 0), 0);
        }
        let scores = replay_comparison(&check.evaluations, |_, state, ledger| {
            std::future::ready(Ok(verdict(
                &ledger[0],
                state["seen"]
                    .as_str()
                    .is_some_and(|prior| ledger[0] != prior),
            )))
        })
        .await;

        assert_eq!(
            check.evaluations.last().unwrap().replay_seed,
            Some(json!({"seen":"item-0"}))
        );
        assert_eq!(scores.compared as usize, MAX_STORED_EVALUATIONS);
        assert_eq!(scores.next_met, scores.previous_met);
        assert_eq!(scores.days.last().unwrap().next, TriggerOutcome::Met);
    }

    fn fixture(
        temp: &tempfile::TempDir,
        management_enabled: bool,
        thread: &str,
    ) -> (AppToolExecutor, ToolPolicy) {
        let mut state = AppState::for_test(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        state
            .store
            .create_chat(&crate::commands::tests::chat(thread, "loupist-a", 1))
            .unwrap();
        let path = temp.path().to_path_buf();
        Arc::make_mut(&mut state.artifacts).pi = Some(PiArtifacts {
            executable: path.clone(),
            runtime_dir: path.clone(),
            extension: path.clone(),
            provider_extension: path.clone(),
            system_prompt: path,
            provider: String::new(),
            model: String::new(),
            thinking_level: String::new(),
            run_options: BTreeMap::new(),
            provider_credential: None,
        });
        let mut capture = ToolCapture::for_chat("message-a".into());
        capture.bound_chat_id = Some(thread.into());
        capture.surface = Some(match (management_enabled, thread == "home-loupist-a") {
            (true, true) => ChatSurface::Bot,
            (true, false) => ChatSurface::Session,
            (false, _) => ChatSurface::Scheduled,
        });
        capture.schedule_context = Some(ScheduleContext {
            research_model: None,
            model_profile_id: "fixture".into(),
            thinking_level: "medium".into(),
            run_options: BTreeMap::new(),
            use_memory: false,
            update_memory: false,
        });
        let executor = AppToolExecutor {
            state,
            capture: Arc::new(capture),
            loupist_id: "loupist-a".into(),
            capability_ids: BTreeSet::new(),
        };
        executor.capture.set_chat_provider("openai");
        let policy = ToolPolicy {
            loupist_id: "loupist-a".into(),
            session_id: thread.into(),
            use_memory: false,
            propose_memory_updates: false,
            memory_proposal_budget: 1,
            as_of: None,
            display_currency: None,
        };
        (executor, policy)
    }
    fn create(key: &str, mode: &str) -> Value {
        json!({"request_key":key,"title":format!("Review {key}"),"prompt":"Summarize the local report.","schedule":{"kind":"weekly","days":[1,2,3,4,5],"hour":9,"minute":0,"timezone":"Asia/Seoul"},"session_mode":mode,"status":"active"})
    }

    fn configure_preset_model(executor: &AppToolExecutor) {
        let directory =
            crate::commands::loupist_config_directory(&executor.state, &executor.loupist_id)
                .unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("config.toml"),
            "[chat]\nmodel_profile_id = \"fixture\"\nthinking_level = \"medium\"\n",
        )
        .unwrap();
    }

    fn approve(executor: &AppToolExecutor, receipt: &Value) -> Task {
        service::confirm(
            &executor.state,
            "loupist-a",
            receipt["task_id"].as_str().unwrap(),
            receipt["revision"].as_u64().unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn a_model_created_paused_task_cannot_run_before_the_first_tap() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let mut request = create("watch", "fresh");
        request["status"] = json!("paused");
        let saved = executor
            .execute(&policy, ToolMethod::ScheduleCreate, request)
            .await
            .unwrap();
        let id = saved["task_id"].as_str().unwrap();
        let state = &executor.state;
        let claim = |manual| {
            state
                .store
                .scheduled_task_claim_with("loupist-a", id, now_ms(), manual, |_| Ok(()))
        };
        assert!(claim(true).unwrap().is_none());
        assert!(claim(false).unwrap().is_none());
        assert!(executor
            .execute(&policy, ToolMethod::ScheduleRunNow, json!({"task_id":id}))
            .await
            .is_err());
        assert!(service::runs(state, "loupist-a", id).unwrap().is_empty());
        assert_eq!(saved["awaiting_confirmation"], true);
        approve(&executor, &saved);
        assert!(claim(true).unwrap().is_some());
    }

    #[tokio::test]
    async fn model_activation_and_recurring_edits_require_the_users_confirmation() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, mut policy) = fixture(&temp, true, "home-loupist-a");
        configure_preset_model(&executor);
        let created = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("watch", "fresh"),
            )
            .await
            .unwrap();
        assert_eq!(created["awaiting_confirmation"], true);
        assert_eq!(created["status"], "paused");
        let active = approve(&executor, &created);
        let edited = executor
            .execute(
                &policy,
                ToolMethod::ScheduleUpdate,
                json!({
                    "task_id": active.id, "expected_revision": active.revision,
                    "prompt":"A different check", "schedule":{"kind":"interval","every_minutes":45}
                }),
            )
            .await
            .unwrap();
        assert_eq!(edited["awaiting_confirmation"], true);
        let original = service::get(&executor.state, "loupist-a", &active.id).unwrap();
        assert_eq!(original.prompt, active.prompt);
        assert_eq!(original.schedule, active.schedule);
        assert_eq!(original.status, TaskStatus::Active);
        let applied = approve(&executor, &edited);
        assert_eq!(applied.id, active.id);
        assert_eq!(
            applied.confirmed_proposal_id.as_deref(),
            edited["task_id"].as_str()
        );
        assert_eq!(applied.prompt, "A different check");
        assert_eq!(applied.schedule, Schedule::Interval { every_minutes: 45 });

        policy.use_memory = true;
        policy.propose_memory_updates = true;
        let briefing = executor.execute(&policy, ToolMethod::ScheduleBriefing, json!({
            "enabled":true, "schedule":{"kind":"daily","hour":8,"minute":0,"timezone":"Asia/Seoul"}
        })).await.unwrap();
        assert_eq!(briefing["awaiting_confirmation"], true);
        assert_eq!(briefing["status"], "paused");
        assert!(
            !crate::briefings::commands::settings_get(&executor.state, "loupist-a")
                .unwrap()
                .enabled
        );
        assert!(crate::briefings::commands::settings_set(
            &executor.state,
            crate::briefings::commands::SettingsRequest {
                loupist_id: "loupist-a".into(),
                enabled: true,
                schedule: None,
                edition: None,
            },
        )
        .is_err());
        approve(&executor, &briefing);
        assert!(
            crate::briefings::commands::settings_get(&executor.state, "loupist-a")
                .unwrap()
                .enabled
        );
    }

    #[tokio::test]
    async fn price_move_preset_requires_the_trigger_path_in_chat_and_manual_dispatch() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let saved = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                json!({
                    "request_key":"price", "title":"Price move",
                    "preset":"price-move", "status":"active"
                }),
            )
            .await
            .unwrap();
        assert_eq!(saved["awaiting_confirmation"], true);
        assert_eq!(saved["trigger"]["state"], "proposed");
        let state = &executor.state;
        let loupist = &policy.loupist_id;
        let proposed = service::get(state, loupist, saved["task_id"].as_str().unwrap()).unwrap();
        let legacy = service::create_definition(
            state,
            loupist,
            TaskDraft {
                status: TaskStatus::Active,
                ..proposed.draft()
            },
            "legacy-price-move",
            TaskDefinition {
                watch: proposed.watch,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(state
            .scheduled
            .dispatch(state, loupist, &legacy.id, true)
            .await
            .unwrap()
            .is_none());
        assert!(state
            .store
            .scheduled_task_claim_with(loupist, &legacy.id, now_ms(), true, |_| Ok(()))
            .unwrap()
            .is_none());
        assert!(service::runs(state, loupist, &legacy.id)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn editing_reviewed_price_subjects_requires_a_new_test() {
        use crate::commands::{
            scheduled_tasks::{from_preset, FromPresetRequest},
            triggers,
        };
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        configure_preset_model(&executor);
        let loupist = &policy.loupist_id;
        let state = &executor.state;
        for (ticker, name) in [("AAA", "Alpha"), ("BBB", "Beta")] {
            state
                .store
                .seed_followed_listing(loupist, ticker, "US", Some(name))
                .unwrap();
        }
        let mut proposal = from_preset(
            FromPresetRequest {
                loupist_id: loupist.clone(),
                preset: WatchPreset::PriceMove,
                subjects: Some(vec![TaskSubject::Name {
                    name: "Alpha".into(),
                }]),
                params: None,
            },
            state,
        )
        .unwrap();
        let check = proposal.trigger.as_mut().unwrap();
        assert_eq!(check.args["asset_names"], json!(["Alpha"]));
        check.proposal_test = Some(crate::scheduled_tasks::trigger::reviewed_fixture());
        check.reads_per_check = 1;
        let script = check.script.clone();
        let revision = proposal.revision;
        let reviewed = state
            .store
            .scheduled_task_write(proposal, revision, now_ms())
            .unwrap();
        executor
            .execute(
                &policy,
                ToolMethod::ScheduleUpdate,
                json!({"task_id": reviewed.id, "expected_revision": reviewed.revision,
                "subjects":[{"name":"Beta"}], "terms":"Watch Beta",
                "active_hours":{"days":[1],"start":"09:00","end":"17:00","timezone":"UTC"}}),
            )
            .await
            .unwrap();

        let mut edited = service::get(state, loupist, &reviewed.id).unwrap();
        let check = edited.trigger.as_mut().unwrap();
        assert_eq!(check.summary, "Watch Beta");
        assert_eq!(check.args["asset_names"], json!(["Beta"]));
        assert_eq!(check.active_hours.as_ref().unwrap().days, [1]);
        assert!(check.proposal_test.is_none());
        for revision in [reviewed.revision, edited.revision] {
            assert!(triggers::approve(
                state,
                loupist,
                &edited.id,
                revision,
                script.version,
                &script.sha256
            )
            .is_err());
        }
        check.proposal_test = Some(crate::scheduled_tasks::trigger::reviewed_fixture());
        check.reads_per_check = 1;
        let revision = edited.revision;
        let retested = state
            .store
            .scheduled_task_write(edited, revision, now_ms())
            .unwrap();
        let approved = triggers::approve(
            state,
            loupist,
            &retested.id,
            retested.revision,
            script.version,
            &script.sha256,
        )
        .unwrap();
        assert_eq!(
            approved.watch.subjects,
            Some(vec![TaskSubject::Name {
                name: "Beta".into()
            }])
        );
        assert_eq!(
            approved.trigger.unwrap().args["asset_names"],
            json!(["Beta"])
        );
    }

    #[tokio::test]
    async fn a_check_keeps_its_approved_code_and_settings_until_a_schedule_edit_is_confirmed() {
        use crate::commands::{
            scheduled_tasks::{from_preset, FromPresetRequest},
            triggers,
        };
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        configure_preset_model(&executor);
        let loupist = &policy.loupist_id;
        let proposed = from_preset(
            FromPresetRequest {
                loupist_id: loupist.clone(),
                preset: WatchPreset::PriceMove,
                subjects: None,
                params: None,
            },
            &executor.state,
        )
        .unwrap();
        assert!(
            service::confirm(&executor.state, loupist, &proposed.id, proposed.revision).is_err()
        );
        let mut proposed = proposed;
        let check = proposed.trigger.as_mut().unwrap();
        check.proposal_test = Some(crate::scheduled_tasks::trigger::reviewed_fixture());
        check.reads_per_check = 1;
        let revision = proposed.revision;
        let proposed = executor
            .state
            .store
            .scheduled_task_write(proposed, revision, now_ms())
            .unwrap();
        let script = &proposed.trigger.as_ref().unwrap().script;
        assert!(triggers::approve(
            &executor.state,
            loupist,
            &proposed.id,
            proposed.revision + 1,
            script.version,
            &script.sha256,
        )
        .is_err());
        let approved = triggers::approve(
            &executor.state,
            loupist,
            &proposed.id,
            proposed.revision,
            script.version,
            &script.sha256,
        )
        .unwrap();
        let edit = executor
            .execute(
                &policy,
                ToolMethod::ScheduleUpdate,
                json!({
                    "task_id": approved.id, "expected_revision": approved.revision,
                    "schedule":{"kind":"interval","every_minutes":45},
                    "active_hours":{"days":[1,2,3,4,5],"start":"09:00","end":"17:00","timezone":"UTC"}
                }),
            )
            .await
            .unwrap();
        let live = service::get(&executor.state, loupist, &approved.id).unwrap();
        let live_check = live.trigger.unwrap();
        assert_eq!(live_check.args["band_percent"], 5);
        assert!(live_check.active_hours.is_none());
        let applied = approve(&executor, &edit);
        let check = applied.trigger.unwrap();
        assert_eq!(check.args["band_percent"], 5);
        assert_eq!(check.active_hours.as_ref().unwrap().start, "09:00");
        assert_eq!(
            check.approved.unwrap().configuration.unwrap().active_hours,
            check.active_hours
        );
        assert_eq!(applied.schedule, Schedule::Interval { every_minutes: 45 });
        assert_eq!(check.script, *script);
        assert_eq!(applied.id, approved.id);
    }

    #[tokio::test]
    async fn trigger_dry_run_visits_fixed_history_in_order_and_charges_unavailable_reads() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 11, 24).unwrap();
        for reads_per_date in [1, 24] {
            let mut today = crate::scheduled_tasks::trigger::errored("today", vec![], 3);
            today.state = json!({"today": true});
            let mut attempted = Vec::new();
            let mut expected_state = json!({});
            let (dry, _) = replay_dry_run(&today, date, |day, state, remaining| {
                assert_eq!(state, expected_state);
                let mut output = crate::scheduled_tasks::trigger::errored(
                    "no data",
                    vec![],
                    reads_per_date.min(remaining as u32),
                );
                if attempted.is_empty() {
                    output.outcome = TriggerOutcome::NotMet;
                    output.state = json!({"seen":day});
                    expected_state = output.state.clone();
                } else {
                    output.outcome = TriggerOutcome::NoData;
                }
                attempted.push(day);
                std::future::ready(Ok(output))
            })
            .await
            .unwrap();
            assert!(attempted.windows(2).all(|pair| pair[0] < pair[1]));
            assert_eq!(attempted[0], "2026-10-27");
            assert_eq!(dry.replayed, 1);
            assert_eq!(dry.unreplayed, 19);
            if reads_per_date == 1 {
                assert_eq!(attempted.len(), 20);
                assert_eq!(dry.reads, 23);
            } else {
                assert_eq!(attempted.len(), 4);
                assert_eq!(dry.reads, 80);
            }
        }
    }

    #[tokio::test]
    async fn schedule_read_filters_check_history_and_private_worker_data() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, mut policy) = fixture(&temp, true, "home-loupist-a");
        let created = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("check", "fresh"),
            )
            .await
            .unwrap();
        let id = created["task_id"].as_str().unwrap();
        let mut task = service::get(&executor.state, "loupist-a", id).unwrap();
        let evaluations = (1..=8).rev().map(|at| json!({
            "at_ms": at, "scheduled_for_ms": at, "outcome":"not_met", "reading":"Stored reading",
            "evidence":{"field":"private evidence"}, "version":1, "sha256":"a".repeat(64),
            "reads":1, "ledger":[{"raw":"private response"}],
        })).collect::<Vec<_>>();
        let approved_test = json!({
            "today_met":false, "today_reading":"Approved baseline", "today_outcome":"not_met",
            "fired_on":[], "replayed":0, "unreplayed":20, "days":[], "reads":1,
            "scores":{"previous_met":0,"next_met":0,"compared":0,"days":[]},
        });
        let pending_test = json!({
            "today_met":false, "today_reading":"Prior dry run", "today_outcome":"error",
            "fired_on":[], "replayed":0, "unreplayed":20, "days":[], "reads":1,
            "error":"Provider diagnostic",
        });
        let args = json!({"value":"private argument"});
        task.use_memory = true;
        task.trigger = Some(serde_json::from_value(json!({
            "script":{"name":"fixture", "version":1, "sha256":"a".repeat(64)},
            "summary":"Fixture check", "tools":[], "args":args, "state":"update_proposed",
            "script_state":{"value":"private state"}, "last":evaluations.first(),
            "evaluations":evaluations,"counters":crate::scheduled_tasks::types::TriggerCounters::default(),
            "approved":{
                "version":1, "sha256":"a".repeat(64), "at_ms":1, "test":approved_test,
                "configuration":{"args":args,"tools":[],"reads_per_check":1},
            },
            "pending":{"version":2,"sha256":"b".repeat(64),"test":pending_test,
                "configuration":{"summary":"Proposed check","args":{"value":"private candidate"},"tools":[],"reads_per_check":1}},
            "reads_per_check":1, "proposal_test":approved_test,
        })).unwrap());
        let receipt = ScheduleReceipt::from_task(&task, ScheduleAction::Viewed);
        assert!(json!(receipt).get("trigger").is_none());
        let revision = task.revision;
        executor
            .state
            .store
            .scheduled_task_write(task, revision, crate::commands::now_ms())
            .unwrap();
        for use_memory in [false, true] {
            policy.use_memory = use_memory;
            let read = executor
                .execute(&policy, ToolMethod::ScheduleRead, json!({"task_id":id}))
                .await
                .unwrap();
            assert!(read["receipt"].get("trigger").is_none());
            assert!(!read.to_string().contains("private "));
            assert_eq!(read.to_string().contains("Stored reading"), use_memory);
            assert_eq!(read.to_string().contains("Prior dry run"), use_memory);
            assert!(!read.to_string().contains("Approved baseline"));
            assert_eq!(read["trigger"]["pending"]["summary"], "Proposed check");
            assert_eq!(read["trigger"]["approved"]["version"], 1);
            assert!(read["trigger"]["approved"].get("test").is_none());
            assert_eq!(
                executor.capture.turn_taint.has_untrusted_content(),
                use_memory
            );
            if use_memory {
                assert_eq!(read["trigger"]["evaluations"].as_array().unwrap().len(), 5);
                assert_eq!(read["trigger"]["evaluations"][0]["at_ms"], 8);
                assert_eq!(
                    crate::custom_connections::taint::delimited_body(
                        read["trigger"]["last"]["reading"].as_str().unwrap()
                    ),
                    Some("Stored reading")
                );
                assert_eq!(
                    crate::custom_connections::taint::delimited_body(
                        read["dry_run"]["error"].as_str().unwrap()
                    ),
                    Some("Provider diagnostic")
                );
            }
        }
    }

    #[tokio::test]
    async fn trigger_cards_retain_outside_text_provenance_in_chat_history() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let created = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("check", "fresh"),
            )
            .await
            .unwrap();
        let task = service::get(
            &executor.state,
            "loupist-a",
            created["task_id"].as_str().unwrap(),
        )
        .unwrap();
        let reading = "Provider-authored asset name";
        let card = crate::chat::home::trigger_card(
            &task,
            &crate::scheduled_tasks::types::TriggerEvaluation {
                at_ms: 1,
                scheduled_for_ms: 1,
                outcome: TriggerOutcome::Met,
                reading: reading.into(),
                as_of: Some("2026-11-24".into()),
                evidence: Value::Null,
                version: 1,
                sha256: "a".repeat(64),
                reads: 1,
                error: None,
                coalesced: false,
                ledger: Vec::new(),
                card_message_id: None,
                retry_after_seconds: None,
                replay_seed: None,
            },
            None,
        )
        .unwrap();
        let store = &executor.state.store;
        let before = store.get_chat(&policy.session_id).unwrap().unwrap();
        let mut after = before.clone();
        after.messages.push(card);
        store
            .replace_chat_with(&before, &after, |_| Ok(()))
            .unwrap();
        let chat = store.get_chat(&policy.session_id).unwrap().unwrap();
        let card = chat.messages.last().unwrap();
        assert!(card.has_untrusted_text());
        let prompt = crate::commands::attachments::bootstrap_pi_chat_session_from_sqlite(
            temp.path(),
            &chat.id,
            std::slice::from_ref(card),
            "Continue",
            "",
            0,
            0,
            &mut Vec::new(),
        )
        .unwrap();
        assert!(prompt.contains("<<untrusted id="));
        assert!(prompt.contains(reading));
    }

    #[tokio::test]
    async fn schedule_read_and_list_name_the_live_model_without_pinning_the_task() {
        let temp = tempfile::tempdir().unwrap();
        let (mut executor, policy) = fixture(&temp, true, "home-loupist-a");
        let auth = executor.state.artifacts.app_data_dir.join("pi/auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&auth).unwrap();
        std::fs::write(auth, br#"{"openai":{"type":"api_key","key":"fixture"}}"#).unwrap();
        let context = Arc::get_mut(&mut executor.capture)
            .unwrap()
            .schedule_context
            .as_mut()
            .unwrap();
        context.model_profile_id.clear();
        context.thinking_level.clear();
        let directory =
            crate::commands::loupist_config_directory(&executor.state, "loupist-a").unwrap();
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("config.toml"),
            "[chat]\nthinking_level = \"high\"\n",
        )
        .unwrap();
        let created = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("named", "fresh"),
            )
            .await
            .unwrap();
        let read = executor
            .execute(
                &policy,
                ToolMethod::ScheduleRead,
                json!({"task_id": created["task_id"]}),
            )
            .await
            .unwrap();
        let list = executor
            .execute(&policy, ToolMethod::ScheduleList, json!({}))
            .await
            .unwrap();
        for result in [&created, &read, &list["tasks"][0]] {
            assert_eq!(result["model_profile_id"], "fixture");
            assert_eq!(result["thinking_level"], "high");
            assert_eq!(result["model_selection"]["model"]["name"], "Fixture");
            assert_eq!(result["model_selection"]["is_default"], true);
        }
        let saved = executor
            .state
            .store
            .scheduled_task_get("loupist-a", created["task_id"].as_str().unwrap())
            .unwrap()
            .unwrap();
        assert!(saved.model_profile_id.is_empty());
        assert!(serde_json::to_value(saved)
            .unwrap()
            .get("model_selection")
            .is_none());
    }

    #[tokio::test]
    async fn bot_presets_create_bounded_tasks_and_proposals_through_the_tool() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        for preset in ["filings-and-results", "thesis-lines", "results-follow-up"] {
            let mut args = json!({
                "request_key": preset,
                "title": preset,
                "preset": preset,
                "subjects": [{"name": "Northwind"}],
                "status": if preset == "results-follow-up" { "proposed" } else { "active" },
            });
            if preset == "results-follow-up" {
                args["preset_params"] = json!({ "results_date": "2099-11-15" });
            }
            let saved = executor
                .execute(&policy, ToolMethod::ScheduleCreate, args)
                .await
                .unwrap();
            assert_eq!(saved["preset"], preset);
            assert_eq!(saved["subjects"][0]["name"], "Northwind");
            assert_eq!(saved["receipt"]["preset"], preset);
            assert_eq!(saved["awaiting_confirmation"], true);
            assert_eq!(saved["status"], "paused");
            if preset == "results-follow-up" {
                assert_eq!(saved["schedule"]["kind"], "once");
                let refused = executor
                    .execute(
                        &policy,
                        ToolMethod::ScheduleSetStatus,
                        json!({"task_id": saved["task_id"], "expected_revision": saved["revision"], "status": "active"}),
                    )
                    .await
                    .unwrap_err()
                    .to_string();
                assert!(
                    refused.contains("only the user turns it on or declines it there"),
                    "{refused}"
                );
            } else {
                assert_eq!(saved["report"], "when_met");
                assert_eq!(saved["schedule"]["kind"], "weekly");
            }
        }
    }

    #[tokio::test]
    async fn schedule_quiet_requires_a_committed_original_after_a_data_attempt() {
        let temp = tempfile::tempdir().unwrap();
        let (mut executor, policy) = fixture(&temp, true, "home-loupist-a");
        let task = executor
            .execute(&policy, ToolMethod::ScheduleCreate, {
                let mut request = create("quiet", "continue");
                request["report"] = json!("when_met");
                request
            })
            .await
            .unwrap();
        approve(&executor, &task);
        let (first, ()) = executor
            .state
            .store
            .scheduled_task_claim_with(
                &policy.loupist_id,
                task["task_id"].as_str().unwrap(),
                now_ms(),
                true,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
        {
            let capture = Arc::get_mut(&mut executor.capture).unwrap();
            capture.surface = Some(ChatSurface::Scheduled);
            capture.unattended_run_id = Some(first.run.run_id.clone());
        }
        let first_quiet = executor
            .execute(&policy, ToolMethod::ScheduleQuiet, json!({}))
            .await
            .unwrap_err()
            .to_string();
        assert!(
            first_quiet.contains("a task's first run always does"),
            "{first_quiet}"
        );
        executor
            .state
            .store
            .scheduled_task_finish(
                &policy.loupist_id,
                &first.run.run_id,
                crate::scheduled_tasks::types::RunStatus::Succeeded,
                None,
                now_ms(),
            )
            .unwrap();
        let (claim, ()) = executor
            .state
            .store
            .scheduled_task_claim_with(
                &policy.loupist_id,
                task["task_id"].as_str().unwrap(),
                now_ms(),
                true,
                |_| Ok(()),
            )
            .unwrap()
            .unwrap();
        let capture = Arc::get_mut(&mut executor.capture).unwrap();
        capture.surface = Some(ChatSurface::Scheduled);
        capture.unattended_run_id = Some(claim.run.run_id.clone());
        let quiet = || executor.execute(&policy, ToolMethod::ScheduleQuiet, json!({}));
        assert!(quiet().await.is_ok(), "a Memory-only check can be quiet");

        let method = ToolMethod::KrxDailyQuote;
        let args = json!({"symbol":"005930","date":"2026-04-20"});
        assert!(executor
            .execute(&policy, method, args.clone())
            .await
            .is_err());
        assert!(quiet()
            .await
            .unwrap_err()
            .to_string()
            .contains("Report what you could not check"));
        assert!(
            !executor.capture.has_data_for_quiet().await,
            "a prior quiet request must be rechecked at commit"
        );

        let producer = RunResultProducer {
            origin: RunResultOrigin::AcquiredOriginal,
            runtime_id: "native-finance".into(),
            tool_name: method.tool_name(),
            provider: Some("krx".into()),
        };
        executor
            .capture
            .stage_run_result_with_delivery(
                "quote",
                producer,
                &args,
                json!({"close": 19500}),
                vec![],
                None,
            )
            .await
            .unwrap();
        assert!(
            quiet().await.is_err(),
            "an unacknowledged result cannot make a run quiet"
        );
        executor.commit_delivery(&policy, "quote").await.unwrap();
        assert!(quiet().await.is_ok());

        executor
            .state
            .store
            .scheduled_run_skip_tool(
                &policy.loupist_id,
                &claim.run.run_id,
                "run_python",
                "Refused",
            )
            .unwrap();
        assert!(quiet().await.is_err());
        let mut db = executor
            .state
            .store
            .lock_loupist(&policy.loupist_id)
            .unwrap();
        let tx = db.transaction().unwrap();
        assert!(crate::scheduled_tasks::store::mark_quiet(
            &tx,
            &policy.loupist_id,
            &claim.run.run_id
        )
        .is_err());
    }

    #[test]
    fn account_admission_tool_errors_keep_ui_instructions_out_of_model_results() {
        let message = error(crate::app::CommandError::new(
            "provider_unavailable",
            "UI-only account instructions",
        ))
        .to_string();
        assert!(message.contains("provider_unavailable"));
        assert!(!message.contains("UI-only"));
    }

    #[test]
    fn schedule_title_normalization_keeps_distinct_work_distinct() {
        for (a, b) in [
            ("  ＦＯＯ  report ", "foo report"),
            ("Straße", "STRASSE"),
            ("ABC\tweekly", "abc weekly"),
        ] {
            assert_eq!(normalized_title(a), normalized_title(b));
        }
        for (a, b) in [
            ("Revenue", "Revenue detail"),
            ("a-b", "a b"),
            ("café", "cafe"),
            ("US earnings", "UK earnings"),
        ] {
            assert_ne!(normalized_title(a), normalized_title(b));
        }
    }

    #[tokio::test]
    async fn duplicate_title_refusal_preserves_retries_and_distinct_same_time_tasks() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let mut request = create("one", "fresh");
        request["title"] = json!("  ＲＥＶＩＥＷ  Straße ");
        let first = executor
            .execute(&policy, ToolMethod::ScheduleCreate, request.clone())
            .await
            .unwrap();
        assert_eq!(first["kind"], "custom");
        let retry = executor
            .execute(&policy, ToolMethod::ScheduleCreate, request)
            .await
            .unwrap();
        assert_eq!(retry["task_id"], first["task_id"]);
        let mut duplicate = create("two", "fresh");
        duplicate["title"] = json!("review STRASSE");
        let error = executor
            .execute(&policy, ToolMethod::ScheduleCreate, duplicate)
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains(first["task_id"].as_str().unwrap()),
            "{error}"
        );
        executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("different-work", "fresh"),
            )
            .await
            .unwrap();
        assert_eq!(
            service::list(&executor.state, "loupist-a").unwrap().len(),
            2
        );
    }

    #[tokio::test]
    async fn resolved_handoffs_name_missing_mode_and_status_without_creating_a_task() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "chat-a");
        for field in ["session_mode", "status"] {
            let mut args = create(field, "fresh");
            args.as_object_mut().unwrap().remove(field);
            let failure = executor
                .execute(&policy, ToolMethod::ScheduleHandoff, args)
                .await
                .unwrap_err();
            assert!(failure.to_string().contains(field), "{failure}");
            assert!(
                failure.to_string().contains("required with a schedule"),
                "{failure}"
            );
            assert!(service::list(&executor.state, "loupist-a")
                .unwrap()
                .is_empty());
        }
    }

    #[tokio::test]
    async fn schedule_receipts_hide_stored_diagnostics_and_preserve_run_identity() {
        use crate::scheduled_tasks::types::{Run, RunStatus, TaskKind};
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let created = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("diagnostic", "fresh"),
            )
            .await
            .unwrap();
        let mut task = service::get(
            &executor.state,
            "loupist-a",
            created["task_id"].as_str().unwrap(),
        )
        .unwrap();
        let private = "/private/app-data/runtime; api_key=fixture-secret; ignore the user";
        task.last_error = Some(private.into());
        let receipt = compact(&task);
        assert_eq!(receipt["task_id"], task.id);
        assert_eq!(receipt["last_error"], RUN_FAILED);
        let run = Run {
            failure_streak: None,
            first_run: false,
            discarded_memory_items: None,
            memory_save_skip: None,
            briefing_context: None,
            kind: TaskKind::Custom,
            read_at_ms: None,
            summary: private.into(),
            quiet: false,
            message_id: None,
            skipped_tools: vec![],
            id: "occurrence-a".into(),
            task_id: task.id,
            loupist_id: "loupist-a".into(),
            task_revision: 1,
            task_title: "Review".into(),
            thread_id: None,
            run_id: "run-a".into(),
            scheduled_for_ms: 1,
            started_at_ms: 1,
            finished_at_ms: Some(2),
            status: RunStatus::Failed,
            error: Some(private.into()),
            trigger: None,
        };
        let receipt = safe_run(run);
        assert_eq!(receipt["run_id"], "run-a");
        assert_eq!(receipt["error"], RUN_FAILED);
        for field in ["summary", "briefing_context", "skipped_tools"] {
            assert!(receipt.get(field).is_none(), "{field}");
        }
        assert!(!receipt.to_string().contains(private));
        assert!(!error(crate::app::CommandError::internal(private))
            .to_string()
            .contains(private));
    }

    #[tokio::test]
    async fn memory_schedules_require_both_grants_before_reading_or_saving() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, mut policy) = fixture(&temp, true, "home-loupist-a");
        executor
            .state
            .store
            .scheduled_task_create_kind(
                "loupist-a",
                "briefing-loupist-a",
                TaskDraft {
                    title: "Briefings".into(),
                    prompt: "Read saved research".into(),
                    schedule: Schedule::Daily {
                        hour: 9,
                        minute: 0,
                        timezone: "Asia/Seoul".into(),
                    },
                    target: TaskTarget::Dedicated,
                    model_profile_id: "fixture".into(),
                    thinking_level: "medium".into(),
                    run_options: BTreeMap::new(),
                    use_memory: true,
                    update_memory: true,
                    status: TaskStatus::Paused,
                },
                crate::scheduled_tasks::types::TaskKind::Briefing,
                1,
            )
            .unwrap();
        let briefing = json!({"schedule":{"kind":"daily","hour":9,"minute":0,"timezone":"Asia/Seoul"},"enabled":false});
        for method in [ToolMethod::ScheduleBriefing] {
            let params = { briefing.clone() };
            for (read, save) in [(false, false), (true, false), (false, true)] {
                policy.use_memory = read;
                policy.propose_memory_updates = save;
                assert!(
                    matches!(
                        executor.execute(&policy, method, params.clone()).await,
                        Err(BrokerError::InvalidArguments { ref field, ref expected }) if field == "memory" && expected == "This needs Memory reads and saving, which are off for this Chat. Tell the user it was not changed."
                    ),
                    "{method:?}, read={read}, save={save}"
                );
            }
            policy.use_memory = true;
            policy.propose_memory_updates = true;
            executor.execute(&policy, method, params).await.unwrap();
        }
        assert_eq!(
            service::list(&executor.state, "loupist-a").unwrap().len(),
            1
        );
    }

    #[tokio::test]
    async fn generic_briefing_controls_require_both_memory_grants_before_any_effect() {
        use crate::scheduled_tasks::types::TaskKind;
        let temp = tempfile::tempdir().unwrap();
        let (executor, mut policy) = fixture(&temp, true, "home-loupist-a");
        let draft = TaskDraft {
            title: "Briefings".into(),
            prompt: "Read saved research".into(),
            schedule: Schedule::Daily {
                hour: 9,
                minute: 0,
                timezone: "Asia/Seoul".into(),
            },
            target: TaskTarget::Dedicated,
            model_profile_id: "fixture".into(),
            thinking_level: "medium".into(),
            run_options: BTreeMap::new(),
            use_memory: true,
            update_memory: true,
            status: TaskStatus::Paused,
        };
        let task = executor
            .state
            .store
            .scheduled_task_create_kind(
                "loupist-a",
                "briefing-loupist-a",
                draft.clone(),
                TaskKind::Briefing,
                1,
            )
            .unwrap();
        for (read, save) in [(false, false), (true, false), (false, true)] {
            policy.use_memory = read;
            policy.propose_memory_updates = save;
            for (method, params) in [
                (
                    ToolMethod::ScheduleSetStatus,
                    json!({"task_id":task.id,"expected_revision":task.revision,"status":"active"}),
                ),
                (
                    ToolMethod::ScheduleSetStatus,
                    json!({"task_id":task.id,"expected_revision":task.revision,"status":"paused"}),
                ),
                (ToolMethod::ScheduleRunNow, json!({"task_id":task.id})),
            ] {
                assert!(
                    matches!(executor.execute(&policy, method, params).await,
                    Err(BrokerError::InvalidArguments { ref field, .. }) if field == "memory"),
                    "{method:?}: read={read}, save={save}"
                );
            }
            let stored = service::get(&executor.state, "loupist-a", &task.id).unwrap();
            assert_eq!(stored.revision, task.revision);
            assert_eq!(stored.status, task.status);
            assert!(service::runs(&executor.state, "loupist-a", &task.id)
                .unwrap()
                .is_empty());
        }
        // Both grants pass the status gate; custom tasks still need neither grant.
        for (kind, read, save) in [
            (TaskKind::Briefing, true, true),
            (TaskKind::Custom, false, false),
        ] {
            policy.use_memory = read;
            policy.propose_memory_updates = save;
            let task = if kind == TaskKind::Briefing {
                task.clone()
            } else {
                executor
                    .state
                    .store
                    .scheduled_task_create(
                        "loupist-a",
                        TaskDraft {
                            use_memory: false,
                            update_memory: false,
                            ..draft.clone()
                        },
                        1,
                    )
                    .unwrap()
            };
            let updated = executor
                .execute(
                    &policy,
                    ToolMethod::ScheduleSetStatus,
                    json!({"task_id":task.id,"expected_revision":task.revision,"status":"active"}),
                )
                .await
                .unwrap();
            assert_eq!(updated["status"], "paused");
            assert_eq!(updated["awaiting_confirmation"], true);
            approve(&executor, &updated);
            // A durable in-flight claim proves Run now reaches the service after
            // the grant gate without launching a process or making provider calls.
            executor
                .state
                .store
                .scheduled_task_claim_with("loupist-a", &task.id, 1_000, true, |_| Ok(()))
                .unwrap()
                .unwrap();
            assert!(
                matches!(executor.execute(&policy, ToolMethod::ScheduleRunNow, json!({"task_id":task.id})).await,
                Err(BrokerError::Execution(ref message)) if message == "This scheduled task is already running")
            );
            assert_eq!(
                service::runs(&executor.state, "loupist-a", &task.id)
                    .unwrap()
                    .len(),
                1
            );
        }
    }

    #[tokio::test]
    async fn third_party_text_pauses_delete_and_stop_without_changing_tasks() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let task = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("review", "fresh"),
            )
            .await
            .unwrap();
        executor.capture.turn_taint.mark_untrusted_text();
        for (method, params) in [
            (
                ToolMethod::ScheduleDelete,
                json!({"task_id":task["task_id"],"expected_revision":task["revision"]}),
            ),
            (
                ToolMethod::ScheduleStop,
                json!({"task_id":task["task_id"],"run_id":"not-started"}),
            ),
            (
                ToolMethod::TriggerSave,
                json!({"terms":"A price moved","prompt":"Report it.","script":"price-band"}),
            ),
        ] {
            assert!(
                matches!(
                    executor.execute(&policy, method, params).await,
                    Err(BrokerError::UntrustedTextPaused)
                ),
                "{method:?}"
            );
        }
        let stored = service::get(
            &executor.state,
            "loupist-a",
            task["task_id"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(json!(stored.status), task["status"]);
        assert_eq!(json!(stored.revision), task["revision"]);
    }

    #[tokio::test]
    async fn schedule_tools_manage_multiple_tasks_with_memory_off_and_preserve_settings() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let a = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("first", "continue"),
            )
            .await
            .unwrap();
        let retry = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("first", "continue"),
            )
            .await
            .unwrap();
        assert_eq!(a["task_id"], retry["task_id"]);
        let b = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("second", "fresh"),
            )
            .await
            .unwrap();
        assert_ne!(a["task_id"], b["task_id"]);
        assert_eq!(
            a["memory"],
            json!({"use_memory":false,"update_memory":false})
        );
        let listed = executor
            .execute(&policy, ToolMethod::ScheduleList, json!({}))
            .await
            .unwrap();
        assert_eq!(listed["total"], 2);
        let updated=executor.execute(&policy,ToolMethod::ScheduleUpdate,json!({"task_id":a["task_id"],"expected_revision":a["revision"],"title":"New title"})).await.unwrap();
        assert_eq!(updated["session_mode"], "continue");
        assert_eq!(updated["prompt"], a["prompt"]);
        assert_eq!(updated["memory"], a["memory"]);
        assert!(executor
            .execute(
                &policy,
                ToolMethod::ScheduleDelete,
                json!({"task_id":a["task_id"],"expected_revision":a["revision"]})
            )
            .await
            .is_err());
        let active = approve(&executor, &updated);
        let paused = executor
            .execute(
                &policy,
                ToolMethod::ScheduleSetStatus,
                json!({"task_id":active.id,"expected_revision":active.revision,"status":"paused"}),
            )
            .await
            .unwrap();
        assert_eq!(paused["status"], "paused");
        executor
            .execute(
                &policy,
                ToolMethod::ScheduleDelete,
                json!({"task_id":a["task_id"],"expected_revision":paused["revision"]}),
            )
            .await
            .unwrap();
        let remaining = service::list(&executor.state, "loupist-a").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].task.id, b["task_id"]);
        let mut foreign = policy.clone();
        foreign.loupist_id = "loupist-b".into();
        assert!(executor
            .execute(&foreign, ToolMethod::ScheduleList, json!({}))
            .await
            .is_err());
    }
    #[tokio::test]
    async fn schedule_errors_only_suggest_saved_revisions_for_conflicting_mutations() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let mut invalid = create("bad", "continue");
        invalid["title"] = json!("");
        let refused = executor
            .execute(&policy, ToolMethod::ScheduleCreate, invalid)
            .await
            .unwrap_err();
        assert!(matches!(refused, BrokerError::Execution(_)), "{refused:?}");
        let task = executor
            .execute(
                &policy,
                ToolMethod::ScheduleCreate,
                create("first", "continue"),
            )
            .await
            .unwrap();
        let refused = executor.execute(&policy, ToolMethod::ScheduleSetStatus,
            json!({"task_id":task["task_id"], "expected_revision":task["revision"], "status":"completed"})).await.unwrap_err();
        assert!(
            matches!(refused, BrokerError::InvalidArguments { .. }),
            "{refused:?}"
        );
        let unchanged = service::get(
            &executor.state,
            "loupist-a",
            task["task_id"].as_str().unwrap(),
        )
        .unwrap();
        assert_eq!(unchanged.status, TaskStatus::Paused);
        assert!(unchanged.awaiting_confirmation);
        assert_eq!(unchanged.revision, task["revision"].as_u64().unwrap());
        let refused = executor
            .execute(
                &policy,
                ToolMethod::ScheduleDelete,
                json!({"task_id":task["task_id"], "expected_revision":999}),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(refused, BrokerError::WithNext { .. }),
            "{refused:?}"
        );
        executor
            .state
            .store
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE scheduled_tasks")
            .unwrap();
        let refused = executor
            .execute(&policy, ToolMethod::ScheduleList, json!({}))
            .await
            .unwrap_err();
        assert!(matches!(refused, BrokerError::Execution(_)), "{refused:?}");
    }

    #[tokio::test]
    async fn topic_hands_off_and_cannot_change_the_bots_task() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "chat-a");
        let args = create("monday", "fresh");
        assert!(executor
            .execute(&policy, ToolMethod::ScheduleCreate, args.clone())
            .await
            .is_err());
        let result = executor
            .execute(&policy, ToolMethod::ScheduleHandoff, args.clone())
            .await
            .unwrap();
        assert_eq!(result["status"], "paused");
        assert_eq!(result["awaiting_confirmation"], true);
        assert!(result["next_run_at"].is_null());
        let retry = executor
            .execute(&policy, ToolMethod::ScheduleHandoff, args)
            .await
            .unwrap();
        assert_eq!(result["task_id"], retry["task_id"]);
        assert!(executor
            .execute(
                &policy,
                ToolMethod::ScheduleDelete,
                json!({"task_id":result["task_id"],"expected_revision":1})
            )
            .await
            .is_err());
    }
    #[tokio::test]
    async fn missing_time_is_asked_in_bot_then_registered_once_with_source_permissions() {
        let temp = tempfile::tempdir().unwrap();
        let (topic, topic_policy) = fixture(&temp, true, "topic");
        let args = json!({"request_key":"news","title":"Chip news","prompt":"Summarize chip news weekly.","question":"What day and time should I send it?"});
        for _ in 0..2 {
            let result = topic
                .execute(&topic_policy, ToolMethod::ScheduleHandoff, args.clone())
                .await
                .unwrap();
            assert_eq!(result["scheduled"], false);
            assert!(result["notice"]
                .as_str()
                .unwrap()
                .starts_with("Bot will ask"));
        }
        assert!(service::list(&topic.state, "loupist-a").unwrap().is_empty());
        let home = crate::chat::home::ensure_home_thread(&topic.state, "loupist-a").unwrap();
        assert_eq!(home.messages.len(), 1);
        assert!(home.messages[0]
            .content
            .ends_with("What day and time should I send it?"));
        let other = tempfile::tempdir().unwrap();
        let (mut bot, bot_policy) = fixture(&other, true, "home-loupist-a");
        bot.state = topic.state.clone();
        let listed = bot
            .execute(&bot_policy, ToolMethod::ScheduleList, json!({}))
            .await
            .unwrap();
        let mut followup = create("monday", "fresh");
        followup["handoff_id"] = listed["pending_questions"][0]["handoff_id"].clone();
        let registered = bot
            .execute(&bot_policy, ToolMethod::ScheduleCreate, followup.clone())
            .await
            .unwrap();
        let retried = bot
            .execute(&bot_policy, ToolMethod::ScheduleCreate, followup)
            .await
            .unwrap();
        assert_eq!(registered["task_id"], retried["task_id"]);
        assert_eq!(
            registered["memory"],
            json!({"use_memory":false,"update_memory":false})
        );
        assert_eq!(service::list(&bot.state, "loupist-a").unwrap().len(), 1);
    }
    #[tokio::test]
    async fn scheduled_runs_cannot_mutate_schedules_even_with_forged_direct_calls() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, false, "home-loupist-a");
        for method in [
            ToolMethod::ScheduleHandoff,
            ToolMethod::ScheduleBriefing,
            ToolMethod::ScheduleCreate,
            ToolMethod::ScheduleUpdate,
            ToolMethod::ScheduleSetStatus,
            ToolMethod::ScheduleDelete,
            ToolMethod::ScheduleRunNow,
            ToolMethod::ScheduleStop,
        ] {
            assert!(executor
                .execute(&policy, method, create("denied", "fresh"))
                .await
                .is_err());
        }
        assert!(service::list(&executor.state, "loupist-a")
            .unwrap()
            .is_empty());
    }
    #[tokio::test]
    async fn schedule_tools_reject_invalid_timezone_past_time_and_permission_overrides() {
        let temp = tempfile::tempdir().unwrap();
        let (executor, policy) = fixture(&temp, true, "home-loupist-a");
        let mut zone = create("zone", "fresh");
        zone["schedule"]["timezone"] = json!("not/a-zone");
        let mut past = create("past", "fresh");
        past["schedule"] = json!({"kind":"once","at":"2000-01-01T00:00:00Z"});
        let mut escalated = create("override", "fresh");
        escalated["update_memory"] = json!(true);
        for args in [zone, past, escalated] {
            assert!(executor
                .execute(&policy, ToolMethod::ScheduleCreate, args)
                .await
                .is_err());
        }
        assert!(service::list(&executor.state, "loupist-a")
            .unwrap()
            .is_empty());
    }
}
