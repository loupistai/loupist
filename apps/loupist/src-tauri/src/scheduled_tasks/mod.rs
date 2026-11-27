//! App-owned schedules reuse ordinary Chat admission, execution and persistence.
#[path = "../allowance.rs"]
pub(crate) mod allowance;
pub(crate) mod learning;
pub(crate) mod presets;
pub(crate) mod program_edit;
pub(crate) mod questions;
pub(crate) mod schedule;
pub(crate) mod service;
pub(crate) mod store;
pub(crate) mod trigger;
pub(crate) mod types;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::ipc::Channel;
use tokio::{
    sync::{watch, Notify},
    task::JoinSet,
};

use crate::{
    app::{AppState, CommandError},
    chat::replay::Replay,
    commands::{
        chat_runtime::{send_chat, ChatAdmission, ChatEventSink},
        map_store, now_ms,
        triggers::{self, DueCheck, DuePreparation, TriggerOccurrence},
        types::{chat_dto, ChatSendRequest, ChatStreamEvent, ChatThreadDto},
    },
    runs::maintenance::MaintenanceActivityKind,
};
use types::{Run, RunStatus, TaskKind};

#[derive(Clone, Copy)]
enum DispatchMode {
    Due,
    Manual { confirmed: bool },
}

impl DispatchMode {
    fn without_confirmation(manual: bool) -> Self {
        if manual {
            Self::Manual { confirmed: false }
        } else {
            Self::Due
        }
    }

    fn is_manual(self) -> bool {
        matches!(self, Self::Manual { .. })
    }
}

pub(crate) const USER_STOPPED: &str = "Stopped before completion";
pub(crate) const BUDGET_EXCEEDED: &str = "budget_exceeded";
pub(crate) const APP_QUIT: &str = "Interrupted when the app quit.";
/// The host's answer for a quiet run that wrote no text; it is no summary.
pub(crate) const QUIET_NO_REPORT: &str = "The scheduled run returned no report.";
/// Startup or poll recovery found the run unfinished: a crash, not a Stop or a quit.
pub(crate) const UNEXPECTED_INTERRUPTION: &str = "Interrupted unexpectedly before completion.";
// Older receipts used this for both graceful quit and crash recovery; do not
// turn an ambiguous historical interruption into a failure.
const LEGACY_INTERRUPTED: &str = "Interrupted before completion.";

// Recheck capacity/offline folders and wall-clock changes at least once a minute.
pub(crate) const UNREADABLE_TASK: &str = "unreadable_task";
pub(crate) const UNREACHABLE: &str = "unreachable";
const EMPTY_WATCH: &str = "nothing_left_to_watch";

const MAX_POLL_SLEEP: Duration = Duration::from_secs(60);
const RETRY_POLL_SLEEP: Duration = Duration::from_secs(5);

fn poll_delay(next_due: Option<i64>, now: i64) -> Duration {
    match next_due {
        Some(due) if due <= now => RETRY_POLL_SLEEP,
        Some(due) => Duration::from_millis(due.saturating_sub(now) as u64).min(MAX_POLL_SLEEP),
        None => MAX_POLL_SLEEP,
    }
}

struct Lifetime(watch::Sender<bool>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

struct LiveRun {
    run: Run,
    baseline: Option<ChatThreadDto>,
    replay: Replay,
    subscriber: Option<Channel<ChatStreamEvent>>,
}

#[cfg(feature = "eval-harness")]
pub(crate) type EvalObserver = Arc<dyn Fn(&serde_json::Value) + Send + Sync>;

struct Inner {
    #[cfg(feature = "eval-harness")]
    eval_observer: Mutex<Option<EvalObserver>>,
    stop: watch::Sender<bool>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    admission: Mutex<()>,
    checks: Mutex<HashMap<TaskKey, uuid::Uuid>>,
    #[cfg(test)]
    check_evaluator: Mutex<Option<TestCheckEvaluator>>,
    changed: Notify,
    live: Mutex<HashMap<String, LiveRun>>,
}

type TaskKey = (String, String);

#[cfg(test)]
type TestCheckEvaluator = (
    Duration,
    Arc<
        dyn Fn(
                &str,
                &str,
            ) -> futures_util::future::BoxFuture<
                'static,
                Result<trigger::CheckOutput, crate::broker::BrokerError>,
            > + Send
            + Sync,
    >,
);

struct CheckClaim<'a> {
    claims: &'a Mutex<HashMap<TaskKey, uuid::Uuid>>,
    key: TaskKey,
    id: uuid::Uuid,
}

impl CheckClaim<'_> {
    fn is_current(&self) -> bool {
        self.claims.lock().expect("check claims").get(&self.key) == Some(&self.id)
    }
}

impl Drop for CheckClaim<'_> {
    fn drop(&mut self) {
        let mut claims = self.claims.lock().expect("check claims");
        if claims.get(&self.key) == Some(&self.id) {
            claims.remove(&self.key);
        }
    }
}

struct AdmittedRun {
    claim: types::Claim,
    admission: ChatAdmission,
    scope: Option<presets::LiveScope>,
}

struct ChatClaim {
    run: Option<AdmittedRun>,
    blocked: Option<CommandError>,
    held: bool,
}

#[derive(Clone)]
pub(crate) struct ScheduledSupervisor {
    inner: Arc<Inner>,
    // The polling worker holds a non-owning handle so AppState does not keep
    // itself alive through its own task. Dropping the app closes admission.
    _lifetime: Option<Arc<Lifetime>>,
}

#[derive(Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub(crate) struct WatchSnapshot {
    pub run: Run,
    pub thread: ChatThreadDto,
}

impl Default for ScheduledSupervisor {
    fn default() -> Self {
        let (stop, _) = watch::channel(false);
        Self {
            inner: Arc::new(Inner {
                #[cfg(feature = "eval-harness")]
                eval_observer: Mutex::new(None),
                stop: stop.clone(),
                worker: Mutex::new(None),
                admission: Mutex::new(()),
                checks: Mutex::new(HashMap::new()),
                #[cfg(test)]
                check_evaluator: Mutex::new(None),
                changed: Notify::new(),
                live: Mutex::new(HashMap::new()),
            }),
            _lifetime: Some(Arc::new(Lifetime(stop))),
        }
    }
}

impl ScheduledSupervisor {
    // Only the growth eval, a test module, installs an observer.
    #[cfg(all(test, feature = "eval-harness"))]
    pub(crate) fn observe_eval(&self, observer: EvalObserver) {
        *self.inner.eval_observer.lock().expect("eval observer lock") = Some(observer);
    }

    pub fn start(&self, state: &AppState) -> Result<(), CommandError> {
        let mut worker = self
            .inner
            .worker
            .lock()
            .map_err(|_| CommandError::internal("schedule worker lock was poisoned"))?;
        if worker.is_some() {
            return Ok(());
        }
        state
            .store
            .scheduled_tasks_recover_running(now_ms())
            .map_err(map_store)?;
        let mut state = state.clone();
        state.scheduled = Self {
            inner: self.inner.clone(),
            _lifetime: None,
        };
        let mut stopped = self.inner.stop.subscribe();
        *worker = Some(tokio::spawn(async move {
            let mut dispatches = JoinSet::new();
            let mut notifications: Option<tokio::task::JoinHandle<()>> = None;
            while !*stopped.borrow() {
                if notifications.as_ref().is_none_or(|task| task.is_finished()) {
                    let state = state.clone();
                    notifications = Some(tokio::spawn(async move {
                        crate::briefings::desktop::flush_notifications(&state).await;
                    }));
                }
                if let Err(error) = state.scheduled.dispatch_due(&state, &mut dispatches) {
                    if error.code != "maintenance_active" {
                        eprintln!("Scheduled task lookup failed: {}", error.code);
                    }
                }
                let delay = state
                    .store
                    .scheduled_tasks_next_due()
                    .map(|due| poll_delay(due, now_ms()))
                    .unwrap_or(MAX_POLL_SLEEP);
                let sleep = tokio::time::sleep(delay);
                tokio::pin!(sleep);
                loop {
                    tokio::select! {
                        biased;
                        _ = stopped.changed() => break,
                        _ = state.scheduled.inner.changed.notified() => break,
                        _ = &mut sleep => break,
                        Some(result) = dispatches.join_next(), if !dispatches.is_empty() => {
                            if let Err(error) = result {
                                eprintln!("Scheduled dispatch stopped: {error}");
                            }
                        }
                    }
                }
            }
            while dispatches.join_next().await.is_some() {}
            if let Some(notifications) = notifications {
                let _ = notifications.await;
            }
        }));
        Ok(())
    }

    fn dispatch_due(
        &self,
        state: &AppState,
        dispatches: &mut JoinSet<()>,
    ) -> Result<(), CommandError> {
        {
            // A claim cannot commit between the live snapshot and recovery.
            // Finishing a live run concurrently is safe: its receipt is already
            // terminal, or its identity remains protected until the next poll.
            let _admission = self.inner.admission.lock().expect("schedule admission");
            if *self.inner.stop.borrow() {
                return Ok(());
            }
            let _activity = state
                .maintenance
                .admit_kind(MaintenanceActivityKind::ChatMutation)?;
            let live = self
                .inner
                .live
                .lock()
                .map_err(|_| CommandError::internal("schedule stream lock was poisoned"))?
                .keys()
                .cloned()
                .collect();
            state
                .store
                .scheduled_tasks_recover_running_except(now_ms(), &live)
                .map_err(map_store)?;
        }
        // Filter unavailable Loupists before applying the global due limit, so a
        // recovery backlog cannot take every slot on every poll.
        let loupists = state
            .store
            .list_loupists()
            .map_err(map_store)?
            .into_iter()
            .filter(|loupist| state.ensure_loupist_available(&loupist.id).is_ok())
            .map(|loupist| loupist.id)
            .collect::<Vec<_>>();
        let checking = self
            .inner
            .checks
            .lock()
            .expect("check claims")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let tasks = state
            .store
            .scheduled_tasks_due_excluding(now_ms(), 100, &loupists, &checking)
            .map_err(map_store)?;
        let mut learning_dispatched = false;
        for task in tasks {
            if task.kind == TaskKind::Learning && std::mem::replace(&mut learning_dispatched, true)
            {
                continue;
            }
            let state = state.clone();
            dispatches.spawn(async move {
                if let Err(error) = state
                    .scheduled
                    .dispatch(&state, &task.loupist_id, &task.id, false)
                    .await
                {
                    if error.code != "maintenance_active" && !*state.scheduled.inner.stop.borrow() {
                        eprintln!("Scheduled Chat could not start: {}", error.code);
                    }
                }
            });
        }
        Ok(())
    }

    #[cfg(test)]
    async fn poll(&self, state: &AppState) -> Result<(), CommandError> {
        let mut dispatches = JoinSet::new();
        self.dispatch_due(state, &mut dispatches)?;
        while let Some(result) = dispatches.join_next().await {
            result.expect("scheduled dispatch");
        }
        Ok(())
    }

    pub(crate) fn changed(&self) {
        self.inner.changed.notify_one();
    }

    pub fn stop(&self) {
        self.inner.stop.send_replace(true);
    }

    pub async fn drain(&self) {
        let worker = self
            .inner
            .worker
            .lock()
            .ok()
            .and_then(|mut worker| worker.take());
        if let Some(worker) = worker {
            let _ = worker.await;
        }
    }

    pub async fn dispatch(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        manual: bool,
    ) -> Result<Option<Run>, CommandError> {
        self.dispatch_inner(
            state,
            loupist_id,
            task_id,
            DispatchMode::without_confirmation(manual),
            #[cfg(feature = "eval-harness")]
            None,
        )
        .await
    }

    pub(crate) async fn dispatch_manual(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        confirmed: bool,
    ) -> Result<Option<Run>, CommandError> {
        self.dispatch_inner(
            state,
            loupist_id,
            task_id,
            DispatchMode::Manual { confirmed },
            #[cfg(feature = "eval-harness")]
            None,
        )
        .await
    }

    /// Dispatch one due occurrence through the scheduler with a turn-scoped observer.
    #[cfg(feature = "eval-harness")]
    pub(crate) async fn dispatch_eval(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        manual: bool,
        observer: EvalObserver,
    ) -> Result<Option<Run>, CommandError> {
        self.dispatch_inner(
            state,
            loupist_id,
            task_id,
            DispatchMode::without_confirmation(manual),
            Some(observer),
        )
        .await
    }

    async fn dispatch_inner(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        mode: DispatchMode,
        #[cfg(feature = "eval-harness")] observer: Option<EvalObserver>,
    ) -> Result<Option<Run>, CommandError> {
        let key = (loupist_id.to_owned(), task_id.to_owned());
        let mut stopped = self.inner.stop.subscribe();
        let (_activity, now, pending) = {
            let _admission = self.inner.admission.lock().expect("schedule admission");
            if *stopped.borrow() {
                return Err(CommandError::conflict("Scheduled tasks are stopping"));
            }
            let activity = state
                .maintenance
                .admit_kind(MaintenanceActivityKind::ChatMutation)?;
            let claims = &self.inner.checks;
            let other_checks_running = {
                let claims = claims.lock().expect("check claims");
                if claims.contains_key(&key) {
                    return Ok(None);
                }
                claims.keys().any(|(owner, _)| owner == loupist_id)
            };
            let now = now_ms();
            let pending = match triggers::prepare_due(
                state,
                loupist_id,
                task_id,
                now,
                mode.is_manual(),
                other_checks_running,
            )? {
                DuePreparation::Pass => None,
                DuePreparation::Done => return Ok(None),
                DuePreparation::Evaluate(evaluation) => {
                    let id = uuid::Uuid::new_v4();
                    claims.lock().expect("check claims").insert(key.clone(), id);
                    Some((CheckClaim { claims, key, id }, evaluation))
                }
            };
            (activity, now, pending)
        };
        let evaluated = if let Some((claim, evaluation)) = pending {
            let result = tokio::select! {
                biased;
                _ = stopped.changed() => return Ok(None),
                result = self.evaluate_due(state, loupist_id, task_id, now, &evaluation) => result,
            };
            Some((claim, evaluation, result))
        } else {
            None
        };
        let (claimed, occurrence) = {
            let _admission = self.inner.admission.lock().expect("schedule admission");
            if *stopped.borrow() {
                return Ok(None);
            }
            let (check_claim, occurrence) = match evaluated {
                Some((claim, evaluation, result)) if claim.is_current() => {
                    match evaluation.finish(state, now, result)? {
                        DueCheck::Met(occurrence) => (Some(claim), Some(occurrence)),
                        DueCheck::Done => return Ok(None),
                    }
                }
                Some(_) => return Ok(None),
                None => (None, None),
            };
            let claimed =
                self.claim_chat(state, loupist_id, task_id, now, mode, occurrence.as_deref())?;
            drop(check_claim);
            (claimed, occurrence)
        };
        if let Some(occurrence) = &occurrence {
            if occurrence.card.is_some() && !claimed.held {
                crate::briefings::desktop::notify_trigger(
                    state,
                    loupist_id,
                    &occurrence.task.title,
                    &occurrence.context.reading,
                    occurrence.notify,
                );
            }
            crate::briefings::desktop::changed(loupist_id);
        }
        if let Some(error) = claimed.blocked {
            return Err(error);
        }
        let Some(AdmittedRun {
            claim,
            mut admission,
            scope,
        }) = claimed.run
        else {
            return Ok(None);
        };
        let mut run = claim.run;
        let task = claim.task;
        let released = claim.released;
        let effective_prompt = scope.map(|scope| {
            admission.scheduled_followed_ids = scope.followed_ids;
            scope.prompt
        });
        admission.scheduled = Some(crate::domain::ScheduledTurn {
            task_id: task.id.clone(),
            run_id: run.run_id.clone(),
            task_title: task.title.clone(),
            task_kind: task.kind.clone(),
        });
        // A custom run is told how its task's last run ended; the briefing has
        // its own since-the-last-briefing facts. The facts are optional
        // context, so a store read that fails leaves them out.
        admission.trigger = occurrence.map(|occurrence| occurrence.context);
        if task.kind == TaskKind::Custom {
            admission.previous_run = state
                .store
                .scheduled_task_runs_since_prompt(loupist_id, task_id, &run.run_id, &task.prompt)
                .ok()
                .and_then(|runs| {
                    types::PreviousRun::from_runs(&runs, |earlier| {
                        answer_read_third_party_text(state, earlier)
                    })
                });
        }
        let run_id = run.run_id.clone();
        let preparing = PreparingRunGuard {
            inner: Arc::downgrade(&self.inner),
            run_id: run_id.clone(),
        };
        crate::briefings::desktop::emit(
            crate::events::INBOX_CHANGED,
            crate::briefings::desktop::InboxChanged {
                loupist_id: loupist_id.to_owned(),
            },
        );
        // A run that left the history takes its session with it.
        service::release_sessions(state, loupist_id, Some(&released));
        let prompt = if task.kind == TaskKind::Briefing {
            match crate::briefings::build_prompt(
                state,
                loupist_id,
                run.started_at_ms,
                task.edition(),
            )
            .and_then(|(prompt, context)| {
                state
                    .store
                    .scheduled_run_briefing_context(loupist_id, &run_id, context.clone())
                    .map_err(map_store)?;
                run.briefing_context = Some(context);
                Ok(prompt)
            }) {
                Ok(prompt) => prompt,
                Err(error) => {
                    let finished = state
                        .store
                        .scheduled_task_failed_to_start(loupist_id, &run_id, &error, now_ms())
                        .map_err(map_store)?;
                    if let Ok(mut live) = self.inner.live.lock() {
                        live.remove(&run_id);
                    }
                    crate::briefings::desktop::completed(state, &finished);
                    return Ok(Some(finished));
                }
            }
        } else {
            effective_prompt.unwrap_or_else(|| task.prompt.clone())
        };
        if let Ok(mut live) = self.inner.live.lock() {
            if let Some(current) = live.get_mut(&run_id) {
                current.run = run.clone();
            }
        }
        #[allow(unused_mut)]
        let mut sink = self.chat_event_sink(state, &run);
        #[cfg(feature = "eval-harness")]
        if observer.is_some() {
            sink.eval_pi_event = observer;
        }
        let (model_profile_id, thinking_level, run_options) = run_settings(state, &task);
        let request = ChatSendRequest {
            run_id: run_id.clone(),
            loupist_id: loupist_id.to_owned(),
            thread_id: run
                .thread_id
                .clone()
                .ok_or_else(|| CommandError::internal("scheduled Chat target is missing"))?,
            prompt,
            use_memory: task.use_memory,
            update_memory: task.update_memory,
            as_of: None,
            model_profile_id,
            thinking_level,
            run_options,
            mentioned_connection_ids: vec![],
            attachments: vec![],
            pins: vec![],
            selected_artifact: None,
        };
        // Completion delivery owns the live entry once ordinary Chat launch starts.
        preparing.detach();
        if let Err(error) = send_chat(request, sink, state, Some(admission)).await {
            if let Ok(mut live) = self.inner.live.lock() {
                live.remove(&run_id);
            }
            let finished = state
                .store
                .scheduled_task_failed_to_start(loupist_id, &run_id, &error, now_ms())
                .map_err(map_store)?;
            crate::briefings::desktop::completed(state, &finished);
            // A claimed run is a durable receipt, including admission failure.
        }
        state
            .store
            .scheduled_task_run_get(loupist_id, &run_id)
            .map_err(map_store)
    }

    async fn evaluate_due(
        &self,
        state: &AppState,
        _loupist_id: &str,
        _task_id: &str,
        now: i64,
        evaluation: &triggers::DueEvaluation,
    ) -> triggers::EvaluationResult {
        #[cfg(test)]
        {
            let fake = self
                .inner
                .check_evaluator
                .lock()
                .expect("check evaluator")
                .clone();
            if let Some((limit, evaluate)) = fake {
                return triggers::evaluate_with_deadline(limit, evaluate(_loupist_id, _task_id))
                    .await;
            }
        }
        triggers::evaluate_with_deadline(
            triggers::EVALUATION_LIMIT,
            evaluation.evaluate(state, now),
        )
        .await
    }

    /// Claim and publish the live identity before recovery can inspect running receipts.
    fn claim_chat(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        now: i64,
        mode: DispatchMode,
        trigger_occurrence: Option<&TriggerOccurrence>,
    ) -> Result<ChatClaim, CommandError> {
        let manual = mode.is_manual();
        let mut empty_trigger_scope = false;
        let definition = state
            .store
            .scheduled_task_get(loupist_id, task_id)
            .map_err(map_store)?;
        let provider = definition.as_ref().and_then(|task| {
            let (id, effort, options) = run_settings(state, task);
            state
                .pi_execution(&id, &effort, &options)
                .ok()
                .map(|execution| execution.model.provider)
        });
        let scope = if let Some(task) = definition
            .filter(|task| task.watch.preset.is_some() && !task.awaiting_confirmation)
            .filter(|task| {
                manual
                    || (task.status == types::TaskStatus::Active
                        && task.next_run_at_ms.is_some_and(|next| next <= now))
            }) {
            match presets::live_scope(state, &task)? {
                Some(scope) => Some((task.revision, scope)),
                None if trigger_occurrence.is_some() => {
                    empty_trigger_scope = true;
                    None
                }
                None => {
                    if task.status == types::TaskStatus::Active {
                        state
                            .store
                            .scheduled_watch_without_subjects(&task, now)
                            .map_err(map_store)?;
                        crate::briefings::desktop::changed(loupist_id);
                    }
                    if manual {
                        return Err(CommandError::invalid("Nothing left to watch"));
                    }
                    return Ok(ChatClaim {
                        run: None,
                        blocked: None,
                        held: false,
                    });
                }
            }
        } else {
            None
        };
        let mut admission_error = None;
        let admit = |claim: &types::Claim| {
            ChatAdmission::new(
                state,
                &claim.run.run_id,
                &claim.run.loupist_id,
                claim.run.thread_id.as_deref().ok_or_else(|| {
                    crate::store::StoreError::Invalid("scheduled Chat target is missing".into())
                })?,
            )
            .map_err(|error| {
                admission_error = Some(error.clone());
                crate::store::StoreError::Conflict("scheduled Chat admission failed")
            })
        };
        let launch_allowance = store::LaunchAllowance {
            limits: allowance::Limits::load(&state.artifacts.app_data_dir)?,
            admission: allowance::Admission::prepare(&state.store, Some(loupist_id))
                .map_err(map_store)?,
            provider,
            confirmed: matches!(mode, DispatchMode::Manual { confirmed: true }),
        };
        let claimed = if let Some(occurrence) = trigger_occurrence {
            state.store.scheduled_trigger_claim(
                occurrence,
                now,
                empty_trigger_scope,
                Some(&launch_allowance),
                admit,
            )
        } else {
            state.store.scheduled_task_claim(
                loupist_id,
                task_id,
                now,
                manual,
                scope
                    .as_ref()
                    .map_or(store::ClaimScope::Unscoped, |(revision, _)| {
                        store::ClaimScope::Definition(*revision)
                    }),
                Some(&launch_allowance),
                admit,
            )
        };
        let claimed = if trigger_occurrence.is_some() || admission_error.is_none() {
            match claimed.map_err(map_store)? {
                Ok(claimed) => claimed,
                Err(hold) => {
                    crate::briefings::desktop::changed(loupist_id);
                    return Ok(ChatClaim {
                        run: None,
                        blocked: manual.then(|| {
                            CommandError::with_detail(
                                "conflict",
                                "Run now needs confirmation because background work is held.",
                                serde_json::json!({"action": "ask_first", "hold": hold}),
                            )
                        }),
                        held: true,
                    });
                }
            }
        } else {
            None
        };
        let blocked = admission_error.filter(|error| {
            manual
                || !matches!(
                    error.code.as_str(),
                    "conflict"
                        | "memory_busy"
                        | "maintenance_active"
                        | "loupist_storage_unavailable"
                )
        });
        let Some((claim, admission)) = claimed else {
            return Ok(ChatClaim {
                run: None,
                blocked,
                held: false,
            });
        };
        let run_id = claim.run.run_id.clone();
        self.inner
            .live
            .lock()
            .map_err(|_| CommandError::internal("schedule stream lock was poisoned"))?
            .insert(
                run_id.clone(),
                LiveRun {
                    run: claim.run.clone(),
                    baseline: None,
                    replay: Replay::default(),
                    subscriber: None,
                },
            );
        Ok(ChatClaim {
            blocked,
            held: false,
            run: Some(AdmittedRun {
                claim,
                admission,
                scope: scope.map(|(_, scope)| scope),
            }),
        })
    }

    /// The completion path used by ordinary scheduled Chat, including typed provider failures.
    pub(crate) fn chat_event_sink(&self, state: &AppState, run: &Run) -> ChatEventSink {
        let inner = Arc::downgrade(&self.inner);
        let store = state.store.clone();
        let binding = run.clone();
        let completion_state = state.clone();
        crate::briefings::desktop::changed(&run.loupist_id);
        let baseline_inner = inner.clone();
        let baseline_id = run.run_id.clone();
        #[allow(unused_mut)]
        let mut sink = ChatEventSink::new(move |event| {
            let outcome = match &event {
                ChatStreamEvent::Completed {
                    final_text,
                    message_id,
                    memory_save_skip,
                    discarded_memory_items,
                    ..
                } => Some(crate::store::bounded_chat_save(|| {
                    store.complete_scheduled_chat(
                        &binding.loupist_id,
                        &binding.run_id,
                        final_text,
                        message_id,
                        memory_save_skip.map(|reason| {
                            (
                                reason,
                                discarded_memory_items.as_deref().unwrap_or_default(),
                            )
                        }),
                        now_ms(),
                    )
                })),
                ChatStreamEvent::Aborted { .. } => {
                    let budget_cancelled = store
                        .scheduled_task_run_get(&binding.loupist_id, &binding.run_id)
                        .ok()
                        .flatten()
                        .and_then(|run| run.budget)
                        .is_some_and(|budget| budget.outcome == types::BudgetOutcome::Cancelled);
                    let reason = if budget_cancelled {
                        BUDGET_EXCEEDED
                    } else if *completion_state.scheduled.inner.stop.borrow() {
                        APP_QUIT
                    } else {
                        USER_STOPPED
                    };
                    Some(crate::store::bounded_chat_save(|| {
                        store.scheduled_task_finish(
                            &binding.loupist_id,
                            &binding.run_id,
                            RunStatus::Interrupted,
                            Some(reason),
                            now_ms(),
                        )
                    }))
                }
                ChatStreamEvent::Error {
                    message,
                    failure_kind,
                    ..
                } => {
                    let error = failed_run_error(*failure_kind, message);
                    Some(crate::store::bounded_chat_save(|| {
                        store.scheduled_task_finish(
                            &binding.loupist_id,
                            &binding.run_id,
                            RunStatus::Failed,
                            Some(&error),
                            now_ms(),
                        )
                    }))
                }
                _ => None,
            };
            let terminal = outcome.is_some();
            if let Some(outcome) = outcome {
                match outcome {
                    Ok(finished) => {
                        completion_state.graph.wake();
                        crate::store::bounded_chat_save(|| {
                            crate::briefings::desktop::completed(&completion_state, &finished)
                        });
                    }
                    Err(_) => eprintln!("Scheduled Chat completion could not be recorded"),
                }
            }
            if let Some(inner) = inner.upgrade() {
                if let Ok(mut live) = inner.live.lock() {
                    if let Some(current) = live.get_mut(&binding.run_id) {
                        current.replay.observe(&event);
                        if current
                            .subscriber
                            .as_ref()
                            .is_some_and(|channel| channel.send(event).is_err())
                        {
                            current.subscriber = None;
                        }
                    }
                    if terminal {
                        live.remove(&binding.run_id);
                        inner.changed.notify_one();
                    }
                }
            }
            Ok(())
        })
        .with_baseline(move |chat| {
            if let (Some(inner), Ok(thread)) = (baseline_inner.upgrade(), chat_dto(chat)) {
                if let Ok(mut live) = inner.live.lock() {
                    if let Some(current) = live.get_mut(&baseline_id) {
                        current.baseline = Some(thread);
                    }
                }
            }
        });
        #[cfg(feature = "eval-harness")]
        {
            sink.eval_pi_event = self
                .inner
                .eval_observer
                .lock()
                .expect("eval observer lock")
                .clone();
        }
        sink
    }

    pub fn watch(
        &self,
        loupist_id: &str,
        task_id: &str,
        run_id: &str,
        channel: Channel<ChatStreamEvent>,
    ) -> Result<Option<WatchSnapshot>, CommandError> {
        let mut live = self
            .inner
            .live
            .lock()
            .map_err(|_| CommandError::internal("schedule stream lock was poisoned"))?;
        let Some(current) = live.get_mut(run_id) else {
            return Ok(None);
        };
        if current.run.loupist_id != loupist_id || current.run.task_id != task_id {
            return Err(CommandError::not_found("scheduled run"));
        }
        let Some(thread) = current.baseline.clone() else {
            return Ok(None);
        };
        let events = current.replay.events().ok_or_else(|| {
            CommandError::new(
                "scheduled_stream_limit",
                "This long run can be read from its research session after completion",
            )
        })?;
        // Replay and subscription occur under the same mutex as live delivery.
        // A single app renderer owns the subscription; reattachment replaces it.
        for event in events {
            channel
                .send(event)
                .map_err(|_| CommandError::internal("scheduled stream receiver closed"))?;
        }
        current.subscriber = Some(channel);
        Ok(Some(WatchSnapshot {
            run: current.run.clone(),
            thread,
        }))
    }
}

/// Keeps host work or Chat preparation protected from recovery until it ends.
pub(crate) struct PreparingRunGuard {
    inner: std::sync::Weak<Inner>,
    run_id: String,
}

impl PreparingRunGuard {
    fn detach(mut self) {
        self.inner = std::sync::Weak::new();
    }
}

impl Drop for PreparingRunGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            if let Ok(mut live) = inner.live.lock() {
                live.remove(&self.run_id);
                inner.changed.notify_one();
            }
        }
    }
}

/// Whether an earlier run's answer read third-party text, as its Chat recorded
/// it (`ChatMessage::has_untrusted_text`). An answer that can no longer be
/// read counts as one that did.
fn answer_read_third_party_text(state: &AppState, run: &Run) -> bool {
    let (Some(thread_id), Some(message_id)) = (&run.thread_id, &run.message_id) else {
        return true;
    };
    state
        .store
        .get_chat(thread_id)
        .ok()
        .flatten()
        .filter(|chat| chat.loupist_id == run.loupist_id)
        .and_then(|chat| {
            chat.messages
                .iter()
                .find(|message| &message.id == message_id)
                .map(crate::domain::ChatMessage::has_untrusted_text)
        })
        .unwrap_or(true)
}

fn bounded_error(value: &str) -> String {
    let value = value.replace('\0', "");
    let mut end = value.len().min(2000);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests;

/// Change-feed bounds for one scheduled run: an ordinary task, a learning
/// program, a briefing, or a trigger explanation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ScheduledRunContext {
    /// Start of the previous succeeded run, or the task's creation time when
    /// no earlier run has succeeded. A failed or stopped run does not move it.
    pub previous_run_started_at_ms: i64,
    /// The flag stored on the run. A learning program stays true until a run
    /// succeeds; other tasks are true only on the first claim.
    pub first_run: bool,
}

pub(crate) fn is_previous_success(candidate: &Run, current: &Run) -> bool {
    candidate.status == RunStatus::Succeeded
        && candidate.started_at_ms <= current.started_at_ms
        && candidate.run_id != current.run_id
}

/// Bounds for scheduled run `run_id`. `None` when that id is not a scheduled
/// run. The run list is newest first, so the first success is the latest one.
pub(crate) fn scheduled_run_context(
    state: &AppState,
    loupist_id: &str,
    run_id: &str,
) -> Result<Option<ScheduledRunContext>, CommandError> {
    let Some(run) = state
        .store
        .scheduled_task_run_get(loupist_id, run_id)
        .map_err(map_store)?
    else {
        return Ok(None);
    };
    let Some(task) = state
        .store
        .scheduled_run_task_snapshot(loupist_id, &run.run_id)
        .map_err(map_store)?
    else {
        return Ok(None);
    };
    let earlier = state
        .store
        .scheduled_task_runs(loupist_id, &run.task_id)
        .map_err(map_store)?;
    let previous = earlier
        .iter()
        .find(|candidate| is_previous_success(candidate, &run));
    Ok(Some(ScheduledRunContext {
        previous_run_started_at_ms: previous
            .map(|previous| previous.started_at_ms)
            .unwrap_or(task.created_at_ms),
        first_run: run.first_run,
    }))
}

/// The model, thinking level and run options a scheduled run uses. A task's
/// model that cannot run now gives way to Auto for this run, with the controls
/// Auto declares; the task keeps its model, and the run records the one that ran.
pub(crate) fn run_settings(
    state: &AppState,
    task: &types::Task,
) -> (String, String, std::collections::BTreeMap<String, String>) {
    match crate::providers::auto::stored_fallback(
        state,
        &task.model_profile_id,
        &task.thinking_level,
        &task.run_options,
    ) {
        Some(auto) => (auto.model.id, auto.thinking_level, auto.run_options),
        None => (
            task.model_profile_id.clone(),
            task.thinking_level.clone(),
            task.run_options.clone(),
        ),
    }
}

/// The error a failed scheduled run stores for its turn's failure. The
/// allowance reads the plan-limit and out-of-credit ones as a provider hold.
pub(crate) fn failed_run_error(
    failure_kind: crate::domain::ChatFailureKind,
    message: &str,
) -> String {
    match failure_kind {
        crate::domain::ChatFailureKind::NeedsReconnect => {
            crate::pi::provider_failure::NEEDS_RECONNECT.to_owned()
        }
        crate::domain::ChatFailureKind::Offline => UNREACHABLE.to_owned(),
        crate::domain::ChatFailureKind::ModelGone => {
            crate::pi::provider_failure::MODEL_GONE.to_owned()
        }
        crate::domain::ChatFailureKind::PlanLimit => {
            crate::pi::provider_failure::PLAN_LIMIT.to_owned()
        }
        crate::domain::ChatFailureKind::OutOfCredit => {
            crate::pi::provider_failure::OUT_OF_CREDIT.to_owned()
        }
        _ => bounded_error(message),
    }
}
