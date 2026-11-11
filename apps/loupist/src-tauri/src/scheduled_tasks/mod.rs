//! App-owned schedules reuse ordinary Chat admission, execution and persistence.
pub(crate) mod questions;
pub(crate) mod schedule;
pub(crate) mod service;
pub(crate) mod store;
mod stream;
pub(crate) mod types;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use serde::Serialize;
use tauri::ipc::Channel;
use tokio::sync::{watch, Mutex as AsyncMutex};

use crate::{
    app::{AppState, CommandError},
    commands::{
        chat_runtime::{send_chat, ChatAdmission, ChatEventSink},
        map_store, now_ms,
        types::{chat_dto, ChatSendRequest, ChatStreamEvent, ChatThreadDto},
    },
    runs::maintenance::MaintenanceActivityKind,
};
use types::{Run, RunStatus, TaskKind, TaskStatus};

pub(crate) const USER_STOPPED: &str = "Stopped before completion";

struct Lifetime(watch::Sender<bool>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.send_replace(true);
    }
}

struct LiveRun {
    run: Run,
    baseline: Option<ChatThreadDto>,
    replay: stream::Replay,
    subscriber: Option<Channel<ChatStreamEvent>>,
}

#[cfg(feature = "eval-harness")]
type EvalObserver = Arc<dyn Fn(&serde_json::Value) + Send + Sync>;

struct Inner {
    #[cfg(feature = "eval-harness")]
    eval_observer: Mutex<Option<EvalObserver>>,
    stop: watch::Sender<bool>,
    worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
    admission: AsyncMutex<()>,
    live: Mutex<HashMap<String, LiveRun>>,
    /// Local watchers and earnings captures (no model call).
    watch: Arc<crate::commands::watch::Runtime>,
    watch_worker: Mutex<Option<tokio::task::JoinHandle<()>>>,
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
                live: Mutex::new(HashMap::new()),
                watch: Arc::new(crate::commands::watch::Runtime::default()),
                watch_worker: Mutex::new(None),
            }),
            _lifetime: Some(Arc::new(Lifetime(stop))),
        }
    }
}

impl ScheduledSupervisor {
    /// The watcher loop's state, shared with Check now and earnings captures.
    pub(crate) fn watch_runtime(&self) -> Arc<crate::commands::watch::Runtime> {
        self.inner.watch.clone()
    }

    #[cfg(feature = "eval-harness")]
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
        self.inner.watch.mark_started(crate::commands::now_ms());
        {
            // Watchers check every minute for due routes, app resident only.
            let state = state.clone();
            let runtime = self.inner.watch.clone();
            let mut stopped = self.inner.stop.subscribe();
            let handle = tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(60));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        biased;
                        _ = stopped.changed() => { if *stopped.borrow() { break; } }
                        _ = tick.tick() => {
                            if *stopped.borrow() { break; }
                            crate::commands::watch::tick(&state, &runtime).await;
                        }
                    }
                }
            });
            if let Ok(mut watch_worker) = self.inner.watch_worker.lock() {
                *watch_worker = Some(handle);
            }
        }
        let mut stopped = self.inner.stop.subscribe();
        *worker = Some(tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    biased;
                    _ = stopped.changed() => { if *stopped.borrow() { break; } }
                    _ = tick.tick() => {
                        if *stopped.borrow() { break; }
                        if let Err(error) = state.scheduled.poll(&state).await {
                            if error.code != "maintenance_active" {
                                eprintln!("Scheduled task lookup failed: {}", error.code);
                            }
                        }
                    }
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

    pub fn stop(&self) {
        self.inner.stop.send_replace(true);
    }

    /// Holds scheduler admission, as a dispatch or a poll in progress does.
    #[cfg(test)]
    pub(crate) async fn hold_admission(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.inner.admission.lock().await
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
        let watch = self
            .inner
            .watch_worker
            .lock()
            .ok()
            .and_then(|mut worker| worker.take());
        if let Some(watch) = watch {
            // A check in flight finishes its current read; nothing half-writes.
            watch.abort();
            let _ = watch.await;
        }
    }

    /// Claims a run of a host task (no model, no Chat turn) under scheduler
    /// admission and keeps it live until the guard drops, so recovery never
    /// marks it interrupted while it works.
    pub(crate) async fn claim_host_run(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        manual: bool,
    ) -> Result<Option<(types::Claim, HostRunGuard)>, CommandError> {
        let _admission = self.inner.admission.lock().await;
        if *self.inner.stop.borrow() {
            return Err(CommandError::conflict("Scheduled tasks are stopping"));
        }
        let claimed = state
            .store
            .scheduled_task_claim_with(loupist_id, task_id, now_ms(), manual, |_| Ok(()))
            .map_err(map_store)?;
        let Some((claim, ())) = claimed else {
            return Ok(None);
        };
        self.inner
            .live
            .lock()
            .map_err(|_| CommandError::internal("schedule stream lock was poisoned"))?
            .insert(
                claim.run.run_id.clone(),
                LiveRun {
                    run: claim.run.clone(),
                    baseline: None,
                    replay: stream::Replay::default(),
                    subscriber: None,
                },
            );
        let guard = HostRunGuard {
            inner: Arc::downgrade(&self.inner),
            run_id: claim.run.run_id.clone(),
        };
        Ok(Some((claim, guard)))
    }

    pub async fn dispatch(
        &self,
        state: &AppState,
        loupist_id: &str,
        task_id: &str,
        manual: bool,
    ) -> Result<Option<Run>, CommandError> {
        // The earnings task runs on the host: no admission of a Chat turn.
        let task = state
            .store
            .scheduled_task_get(loupist_id, task_id)
            .ok()
            .flatten();
        if task
            .as_ref()
            .is_some_and(|task| task.kind == TaskKind::Earnings)
        {
            if *self.inner.stop.borrow() {
                return Err(CommandError::conflict("Scheduled tasks are stopping"));
            }
            return Box::pin(crate::commands::earnings::dispatch(
                state, loupist_id, task_id, manual, None,
            ))
            .await;
        }
        // A briefing checks followed assets before its prompt is built, without
        // a model call. The refresh runs before admission so Run now, other
        // schedules and maintenance never wait behind its reads, and it stops
        // with the scheduler. What is not reached in time stays "not checked".
        if !*self.inner.stop.borrow() {
            let briefing = task.is_some_and(|task| {
                task.kind == TaskKind::Briefing && (manual || task.status == TaskStatus::Active)
            });
            if briefing {
                // Boxed: a refresh runs tools, and a tool can start a scheduled run.
                let _ = Box::pin(crate::commands::assets::refresh_followed(
                    state,
                    loupist_id,
                    crate::commands::assets::BRIEFING_BUDGET,
                    Some(self.inner.stop.subscribe()),
                ))
                .await;
                // Wallet balances are refreshed for the held flags (≤ 30 s).
                crate::commands::user_context::refresh_wallets(state, None).await;
            }
        }
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
                    "conflict" | "maintenance_active" | "loupist_recovery_required"
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
        let run_id = run.run_id.clone();
        let prompt = if task.kind == TaskKind::Briefing {
            match crate::briefings::build_prompt(
                state,
                loupist_id,
                task.last_run_at_ms,
                run.started_at_ms,
            )
            .and_then(|(prompt, context)| {
                admission.untrusted_text = context.carries_untrusted_text();
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
                    crate::briefings::desktop::completed(state, &finished);
                    return Ok(Some(finished));
                }
            }
        } else {
            task.prompt.clone()
        };
        self.inner
            .live
            .lock()
            .map_err(|_| CommandError::internal("schedule stream lock was poisoned"))?
            .insert(
                run_id.clone(),
                LiveRun {
                    run: run.clone(),
                    baseline: None,
                    replay: stream::Replay::default(),
                    subscriber: None,
                },
            );
        let inner = Arc::downgrade(&self.inner);
        let store = state.store.clone();
        let binding = run.clone();
        let completion_state = state.clone();
        crate::briefings::desktop::changed(loupist_id);
        let baseline_inner = inner.clone();
        let baseline_id = run_id.clone();
        #[allow(unused_mut)]
        let mut sink = ChatEventSink::new(move |event| {
            let terminal = match &event {
                ChatStreamEvent::Completed { .. } => Some((RunStatus::Succeeded, None)),
                ChatStreamEvent::Aborted { .. } => Some((
                    RunStatus::Interrupted,
                    Some(
                        if *completion_state.scheduled.inner.stop.borrow() {
                            "Interrupted before completion."
                        } else {
                            USER_STOPPED
                        }
                        .to_owned(),
                    ),
                )),
                ChatStreamEvent::Error { message, .. } => {
                    Some((RunStatus::Failed, Some(bounded_error(message))))
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
                "This long run can be read from its Chat after completion",
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

/// Keeps a host run in the live set (protected from recovery) while it works.
pub(crate) struct HostRunGuard {
    inner: std::sync::Weak<Inner>,
    run_id: String,
}

impl Drop for HostRunGuard {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.upgrade() {
            if let Ok(mut live) = inner.live.lock() {
                live.remove(&self.run_id);
            }
        }
    }
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
