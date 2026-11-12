//! Agent-facing scheduling. All effects use the same service as the native UI.
use super::*;
use crate::{
    chat::surface::ChatSurface,
    scheduled_tasks::{
        service,
        types::{
            Schedule, ScheduleAction, ScheduleReceipt, Task, TaskDraft, TaskStatus, TaskTarget,
        },
    },
};

#[derive(Clone)]
pub(crate) struct ScheduleContext {
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
#[serde(deny_unknown_fields)]
struct CreateRequest {
    request_key: String,
    title: String,
    prompt: String,
    schedule: Option<ScheduleInput>,
    question: Option<String>,
    handoff_id: Option<String>,
    session_mode: Option<SessionMode>,
    status: Option<TaskStatus>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateRequest {
    task_id: String,
    expected_revision: u64,
    title: Option<String>,
    prompt: Option<String>,
    schedule: Option<ScheduleInput>,
    session_mode: Option<SessionMode>,
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
    command_error(error)
}

fn memory_schedule_refusal() -> BrokerError {
    BrokerError::invalid("memory", "This needs Memory reads and saving, which are off for this Chat. Tell the user it was not changed.")
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

fn compact(task: &Task) -> Value {
    json!({"task_id":task.id,"title":task.title,"revision":task.revision,"status":task.status,
        "schedule":task.schedule,"session_mode": match task.target { TaskTarget::NewChat=>"fresh", TaskTarget::Dedicated=>"continue", TaskTarget::ExistingChat{..}=>"existing_chat" },
        "next_run_at":task.next_run_at_ms.and_then(DateTime::from_timestamp_millis).map(|t|t.to_rfc3339()),
        "last_error":task.last_error.as_ref().map(|_| RUN_FAILED),"awaiting_confirmation":task.awaiting_confirmation})
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

impl AppToolExecutor {
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
        if method == ToolMethod::ScheduleWatch {
            if !policy.use_memory || !policy.propose_memory_updates {
                return Err(memory_schedule_refusal());
            }
            #[derive(Deserialize)]
            #[serde(deny_unknown_fields)]
            struct WatchRequest {
                asset_id: String,
                kind: crate::knowledge::watch::WatchKind,
                enabled: bool,
                expected_revision: i64,
            }
            let request: Option<WatchRequest> = if params == json!({}) {
                None
            } else {
                Some(decode(params)?)
            };
            let view = match request {
                None => crate::commands::watch::settings_view(
                    state,
                    &state.scheduled.watch_runtime(),
                    loupist,
                )
                .map_err(error)?,
                Some(r) => crate::commands::watch::update_setting(
                    state,
                    crate::commands::watch::WatchSettingSetRequest {
                        loupist_id: loupist.to_owned(),
                        asset_id: r.asset_id,
                        kind: r.kind,
                        enabled: r.enabled,
                        expected_revision: r.expected_revision,
                    },
                )
                .map_err(error)?,
            };
            return Ok(json!(view));
        }
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
                }
                let r: BriefingRequest = decode(params)?;
                let settings = crate::briefings::commands::settings_set(
                    state,
                    crate::briefings::commands::SettingsRequest {
                        loupist_id: loupist.to_owned(),
                        schedule: Some(r.schedule.resolve()?),
                        enabled: r.enabled,
                    },
                )
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
                        item.task.title.to_lowercase().contains(&query)
                            || item.task.prompt.to_lowercase().contains(&query)
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
                    "notice":"Includes active, paused and completed tasks in this Loupist. Read a task before changing it. Runs need Loupist open; missed recurring times catch up once."}),
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
                    if is_bot
                        || request.schedule.is_some()
                        || request.handoff_id.is_some()
                        || question.trim().is_empty()
                        || question.len() > 1000
                        || request.prompt.trim().is_empty()
                        || request.prompt.len() > 8192
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
                            prompt: request.prompt,
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
                        json!({"handed_off":true,"scheduled":false,"notice":"Home will ask for the missing details. Keep the user in this session and do not repeat the question here."}),
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
                let mut draft = TaskDraft {
                    title: request.title,
                    prompt: request.prompt,
                    schedule: request
                        .schedule
                        .ok_or_else(|| {
                            BrokerError::invalid(
                                "schedule",
                                "Provide a cadence or hand off a question",
                            )
                        })?
                        .resolve()?,
                    target: request
                        .session_mode
                        .ok_or_else(|| {
                            BrokerError::invalid(
                                "session_mode",
                                "fresh or continue; required with a schedule",
                            )
                        })?
                        .target(),
                    model_profile_id: context.model_profile_id.clone(),
                    thinking_level: context.thinking_level.clone(),
                    run_options: context.run_options.clone(),
                    use_memory: context.use_memory,
                    update_memory: context.update_memory,
                    status: request.status.ok_or_else(|| {
                        BrokerError::invalid("status", "active or paused; required with a schedule")
                    })?,
                };
                if let Some(q) = &origin {
                    draft.model_profile_id.clone_from(&q.model_profile_id);
                    draft.thinking_level.clone_from(&q.thinking_level);
                    draft.run_options.clone_from(&q.run_options);
                    draft.use_memory &= q.use_memory;
                    draft.update_memory &= q.update_memory;
                }
                let task = service::create(state, loupist, draft, Some(&id)).map_err(error)?;
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
                let request: UpdateRequest = decode(params)?;
                if request.title.is_none()
                    && request.prompt.is_none()
                    && request.schedule.is_none()
                    && request.session_mode.is_none()
                {
                    return Err(BrokerError::invalid("update", "at least one changed field"));
                }
                let mut draft = service::get(state, loupist, &request.task_id)
                    .map_err(error)?
                    .draft();
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
                // Rescheduling a consumed one-off makes the new occurrence active.
                if draft.status == TaskStatus::Completed {
                    draft.status = TaskStatus::Active;
                }
                (
                    service::update(
                        state,
                        loupist,
                        &request.task_id,
                        request.expected_revision,
                        draft,
                    )
                    .map_err(revision_error)?,
                    ScheduleAction::Updated,
                )
            }
            ToolMethod::ScheduleSetStatus => {
                let request: StatusRequest = decode(params)?;
                let action = match request.status {
                    TaskStatus::Paused => ScheduleAction::Paused,
                    TaskStatus::Active => ScheduleAction::Resumed,
                    TaskStatus::Completed => {
                        return Err(BrokerError::invalid("status", "active or paused"))
                    }
                };
                if request.status == TaskStatus::Active {
                    let task = service::get(state, loupist, &request.task_id).map_err(error)?;
                    if task.awaiting_confirmation
                        && task.proposed_in_message_id.as_deref()
                            == self.capture.source_message_id.as_deref()
                    {
                        return Err(BrokerError::Execution("The user turns this task on with Turn on in the card under your reply. Tell them it is waiting for their OK; it can be turned on for them in a later reply if they ask.".into()));
                    }
                }
                (
                    service::set_status(
                        state,
                        loupist,
                        &request.task_id,
                        request.expected_revision,
                        request.status,
                    )
                    .map_err(revision_error)?,
                    action,
                )
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
                service::run_now(state, loupist, &request.task_id)
                    .await
                    .map_err(error)?;
                (
                    service::get(state, loupist, &request.task_id).map_err(error)?,
                    ScheduleAction::Started,
                )
            }
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
        result["model_profile_id"] = json!(task.model_profile_id);
        result["thinking_level"] = json!(task.thinking_level);
        result["memory"] = json!({"use_memory":task.use_memory,"update_memory":task.update_memory});
        result["notice"]=json!("Runs while Loupist is open. This saved action remains if the reply later stops. Fresh runs have new Chat history; continue runs reuse only this task’s dedicated Chat. Both use the saved Memory settings.");
        if task.awaiting_confirmation {
            result["notice"] = json!("Saved paused, waiting for the user's OK in the card under your reply; nothing runs until they turn it on there. Say this in one short sentence in the user's language without naming the button, and do not turn it on yourself in this reply.");
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::PiArtifacts,
        commands::tests::{profile, seed_profile},
    };
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
        };
        (executor, policy)
    }
    fn create(key: &str, mode: &str) -> Value {
        json!({"request_key":key,"title":format!("Review {key}"),"prompt":"Summarize the local report.","schedule":{"kind":"weekly","days":[1,2,3,4,5],"hour":9,"minute":0,"timezone":"Asia/Seoul"},"session_mode":mode,"status":"active"})
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
            briefing_context: None,
            kind: TaskKind::Custom,
            read_at_ms: None,
            summary: private.into(),
            summary_items: vec![],
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
        };
        let receipt = safe_run(run);
        assert_eq!(receipt["run_id"], "run-a");
        assert_eq!(receipt["error"], RUN_FAILED);
        for field in [
            "summary",
            "summary_items",
            "briefing_context",
            "skipped_tools",
        ] {
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
        for method in [ToolMethod::ScheduleWatch, ToolMethod::ScheduleBriefing] {
            let params = if method == ToolMethod::ScheduleWatch {
                json!({})
            } else {
                briefing.clone()
            };
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
        let paused=executor.execute(&policy,ToolMethod::ScheduleSetStatus,json!({"task_id":a["task_id"],"expected_revision":updated["revision"],"status":"paused"})).await.unwrap();
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
        assert_eq!(unchanged.status, TaskStatus::Active);
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
        assert_eq!(result["status"], "active");
        assert_eq!(result["awaiting_confirmation"], false);
        assert!(!result["next_run_at"].is_null());
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
                .starts_with("Home will ask"));
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
            ToolMethod::ScheduleWatch,
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
