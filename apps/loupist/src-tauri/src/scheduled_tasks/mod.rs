//! App-owned schedules reuse ordinary Chat admission, execution and persistence.
pub(crate) mod questions;
pub(crate) mod schedule;
pub(crate) mod service;
pub(crate) mod store;
pub(crate) mod types;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::{watch, Mutex as AsyncMutex, Notify};

use crate::{
    app::{AppState, CommandError},
    chat::replay::Replay,
    commands::{
        chat_runtime::{send_chat, ChatAdmission, ChatEventSink},
        map_store, now_ms,
        types::{chat_dto, ChatSendRequest, ChatStreamEvent, ChatThreadDto},
    },
    runs::maintenance::MaintenanceActivityKind,
};
use types::{Run, RunStatus, TaskKind};

pub(crate) const USER_STOPPED: &str = "Stopped before completion";
pub(crate) const APP_QUIT: &str = "Interrupted when the app quit.";
/// The host's answer for a quiet run that wrote no text; it is no summary.
pub(crate) const QUIET_NO_REPORT: &str = "The scheduled run returned no report.";
/// Startup or poll recovery found the run unfinished: a crash, not a Stop or a quit.
pub(crate) const UNEXPECTED_INTERRUPTION: &str = "Interrupted unexpectedly before completion.";
// Older receipts used this for both graceful quit and crash recovery; do not
// turn an ambiguous historical interruption into a failure.
const LEGACY_INTERRUPTED: &str = "Interrupted before completion.";

// Recheck capacity/offline folders and wall-clock changes at least once a minute.
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
    admission: AsyncMutex<()>,
    changed: Notify,
    live: Mutex<HashMap<String, LiveRun>>,
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
                admission: AsyncMutex::new(()),
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
            loop {
                if *stopped.borrow() {
                    break;
                }
                if let Err(error) = state.scheduled.poll(&state).await {
                    if error.code != "maintenance_active" {
                        eprintln!("Scheduled task lookup failed: {}", error.code);
                    }
                }
                let delay = state
                    .store
                    .scheduled_tasks_next_due()
                    .map(|due| poll_delay(due, now_ms()))
                    .unwrap_or(MAX_POLL_SLEEP);
                tokio::select! {
                    biased;
                    _ = stopped.changed() => { if *stopped.borrow() { break; } }
                    _ = state.scheduled.inner.changed.notified() => {}
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        }));
        Ok(())
    }

    async fn poll(&self, state: &AppState) -> Result<(), CommandError> {
        {
            // A claim cannot commit between the live snapshot and recovery.
            // Finishing a live run concurrently is safe: its receipt is already
            // terminal, or its identity remains protected until the next poll.
            let _admission = self.inner.admission.lock().await;
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
        let tasks = state
            .store
            .scheduled_tasks_due_for_loupists(now_ms(), 100, &loupists)
            .map_err(map_store)?;
        for task in tasks {
            if *self.inner.stop.borrow() {
                break;
            }
            if let Err(error) = self
                .dispatch(state, &task.loupist_id, &task.id, false)
                .await
            {
                if error.code != "maintenance_active" {
                    eprintln!("Scheduled Chat could not start: {}", error.code);
                }
            }
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
            manual,
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
        self.dispatch_inner(state, loupist_id, task_id, manual, Some(observer))
            .await
    }

    async fn dispatch_inner(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        manual: bool,
        #[cfg(feature = "eval-harness")] observer: Option<EvalObserver>,
    ) -> Result<Option<Run>, CommandError> {
        let _admission = self.inner.admission.lock().await;
        if *self.inner.stop.borrow() {
            return Err(CommandError::conflict("Scheduled tasks are stopping"));
        }
        let _activity = state
            .maintenance
            .admit_kind(MaintenanceActivityKind::ChatMutation)?;
        let mut admission_error = None;
        let claimed =
            state
                .store
                .scheduled_task_claim_with(loupist_id, task_id, now_ms(), manual, |claim| {
                    ChatAdmission::new(
                        state,
                        &claim.run.run_id,
                        &claim.run.loupist_id,
                        claim.run.thread_id.as_deref().ok_or_else(|| {
                            crate::store::StoreError::Invalid(
                                "scheduled Chat target is missing".into(),
                            )
                        })?,
                    )
                    .map_err(|error| {
                        admission_error = Some(error.clone());
                        crate::store::StoreError::Conflict("scheduled Chat admission failed")
                    })
                });
        if let Some(error) = admission_error {
            return if !manual
                && matches!(
                    error.code.as_str(),
                    "conflict" | "maintenance_active" | "loupist_storage_unavailable"
                ) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let Some((claim, mut admission)) = claimed.map_err(map_store)? else {
            return Ok(None);
        };
        let mut run = claim.run;
        let task = claim.task;
        let released = claim.released;
        admission.scheduled = Some(crate::domain::ScheduledTurn {
            task_id: task.id.clone(),
            run_id: run.run_id.clone(),
            task_title: task.title.clone(),
            task_kind: task.kind.clone(),
        });
        // A custom run is told how its task's last run ended; the briefing has
        // its own since-the-last-briefing facts. The facts are optional
        // context, so a store read that fails leaves them out.
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
        self.inner
            .live
            .lock()
            .map_err(|_| CommandError::internal("schedule stream lock was poisoned"))?
            .insert(
                run_id.clone(),
                LiveRun {
                    run: run.clone(),
                    baseline: None,
                    replay: Replay::default(),
                    subscriber: None,
                },
            );
        let preparing = PreparingRunGuard {
            inner: Arc::downgrade(&self.inner),
            run_id: run_id.clone(),
        };
        drop(_admission);
        // A run that left the history takes its session with it.
        service::release_sessions(state, loupist_id, Some(&released));
        let prompt = if task.kind == TaskKind::Briefing {
            match crate::briefings::build_prompt(state, loupist_id, task.last_run_at_ms, now_ms())
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
                        .scheduled_task_finish(
                            loupist_id,
                            &run_id,
                            RunStatus::Failed,
                            Some(&bounded_error(&error.message)),
                            now_ms(),
                        )
                        .map_err(map_store)?;
                    if let Ok(mut live) = self.inner.live.lock() {
                        live.remove(&run_id);
                    }
                    crate::briefings::desktop::completed(state, &finished);
                    return Ok(Some(finished));
                }
            }
        } else {
            task.prompt.clone()
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
            model_profile_id: task.model_profile_id,
            thinking_level: task.thinking_level,
            run_options: task.run_options,
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
                .scheduled_task_finish(
                    loupist_id,
                    &run_id,
                    RunStatus::Failed,
                    Some(&bounded_error(&error.message)),
                    now_ms(),
                )
                .map_err(map_store)?;
            crate::briefings::desktop::completed(state, &finished);
            // A claimed run is a durable receipt, including admission failure.
        }
        state
            .store
            .scheduled_task_run_get(loupist_id, &run_id)
            .map_err(map_store)
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
            let terminal = match &event {
                ChatStreamEvent::Completed { .. } => Some((RunStatus::Succeeded, None)),
                ChatStreamEvent::Aborted { .. } => Some((
                    RunStatus::Interrupted,
                    Some(
                        if *completion_state.scheduled.inner.stop.borrow() {
                            APP_QUIT
                        } else {
                            USER_STOPPED
                        }
                        .to_owned(),
                    ),
                )),
                ChatStreamEvent::Error {
                    message,
                    failure_kind,
                    ..
                } => {
                    let error = match failure_kind {
                        crate::domain::ChatFailureKind::NeedsReconnect => {
                            crate::pi::provider_failure::NEEDS_RECONNECT.to_owned()
                        }
                        crate::domain::ChatFailureKind::ModelGone => {
                            crate::pi::provider_failure::MODEL_GONE.to_owned()
                        }
                        _ => bounded_error(message),
                    };
                    Some((RunStatus::Failed, Some(error)))
                }
                _ => None,
            };
            if let ChatStreamEvent::Completed {
                final_text,
                message_id,
                ..
            } = &event
            {
                if store
                    .scheduled_run_result(
                        &binding.loupist_id,
                        &binding.run_id,
                        final_text,
                        message_id,
                    )
                    .is_err()
                {
                    eprintln!("Scheduled Chat summary could not be recorded");
                }
            }
            if let Some((status, error)) = &terminal {
                match store.scheduled_task_finish(
                    &binding.loupist_id,
                    &binding.run_id,
                    *status,
                    error.as_deref(),
                    now_ms(),
                ) {
                    Ok(finished) => {
                        crate::briefings::desktop::completed(&completion_state, &finished)
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
                    if terminal.is_some() {
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
