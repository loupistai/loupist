use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
};

use serde::Serialize;
use tokio::sync::watch;

use crate::{
    app::CommandError,
    chat::control::{AcceptedChatControl, ChatControlHandle},
    runs::maintenance::{ActivityLease, MaintenanceBlocker, MaintenanceCoordinator},
};

pub const MAX_MODEL_WORKERS: usize = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunKind {
    Chat,
    MemoryWrite,
    ChatMutation,
}

impl RunKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::MemoryWrite => "memory_write",
            Self::ChatMutation => "chat_mutation",
        }
    }

    const fn activity_label(self) -> &'static str {
        match self {
            Self::Chat => "Chat session",
            Self::MemoryWrite => "Memory write",
            Self::ChatMutation => "Chat change",
        }
    }

    fn consumes_model_slot(self) -> bool {
        matches!(self, Self::Chat)
    }

    fn loupist_access(self) -> LoupistAccess {
        match self {
            Self::MemoryWrite => LoupistAccess::ExclusiveMemory,
            Self::Chat | Self::ChatMutation => LoupistAccess::SharedMemory,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LoupistAccess {
    SharedMemory,
    ExclusiveMemory,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum RunTarget {
    ChatThread(String),
    MemoryWriteSession(String),
}

impl RunTarget {
    fn value(&self) -> &str {
        match self {
            Self::ChatThread(value) | Self::MemoryWriteSession(value) => value,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunActivity {
    pub run_id: String,
    pub loupist_id: String,
    pub kind: String,
    pub target: String,
    pub started_at_ms: i64,
}

#[derive(Clone, Debug)]
pub struct RunSpec {
    pub run_id: String,
    pub loupist_id: String,
    pub kind: RunKind,
    pub target: RunTarget,
}

#[derive(Debug)]
struct ActiveRun {
    spec: RunSpec,
    started_at_ms: i64,
    cancel: watch::Sender<bool>,
    chat_control: Option<ChatControlHandle>,
    terminal: TerminalState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TerminalState {
    Running,
    CancelRequested,
    CompletionClaimed,
}

#[derive(Debug, Default)]
struct RunTable {
    runs: HashMap<String, ActiveRun>,
    writer_queues: HashMap<String, VecDeque<String>>,
    shutting_down: bool,
}

#[derive(Clone, Debug)]
pub struct RunCoordinator {
    inner: Arc<Mutex<RunTable>>,
    maintenance: MaintenanceCoordinator,
    changes: watch::Sender<u64>,
}

#[derive(Debug)]
pub struct RunRegistration {
    pub cancel: watch::Receiver<bool>,
    pub lease: RunLease,
}

/// A Memory writer that already owns its FIFO queue position but has not yet
/// waited for existing readers to drain. Reserving is synchronous so a Chat
/// can establish the writer barrier before releasing its shared lease.
#[derive(Debug)]
pub struct PendingMemoryWrite {
    #[cfg(test)]
    pub(crate) wait_error: Option<CommandError>,
    coordinator: RunCoordinator,
    registration: RunRegistration,
    loupist_id: String,
    run_id: String,
}

impl PendingMemoryWrite {
    #[cfg(test)]
    pub async fn wait(self) -> Result<RunRegistration, CommandError> {
        let (registration, ready) = self.wait_retaining_registration().await;
        ready?;
        Ok(registration)
    }

    /// Failure still owns admission until the caller has persisted its terminal result.
    pub(crate) async fn wait_retaining_registration(
        self,
    ) -> (RunRegistration, Result<(), CommandError>) {
        #[cfg(test)]
        if let Some(error) = self.wait_error {
            return (self.registration, Err(error));
        }
        let ready = self
            .coordinator
            .wait_for_memory_writer(&self.loupist_id, &self.run_id)
            .await;
        (self.registration, ready)
    }
}

#[derive(Debug)]
pub struct RunLease {
    run_id: String,
    coordinator: Weak<Mutex<RunTable>>,
    changes: watch::Sender<u64>,
    _activity: ActivityLease,
}

impl Drop for RunLease {
    fn drop(&mut self) {
        let Some(coordinator) = self.coordinator.upgrade() else {
            return;
        };
        if let Ok(mut table) = coordinator.lock() {
            if let Some(run) = table.runs.remove(&self.run_id) {
                if run.spec.kind == RunKind::MemoryWrite {
                    let loupist_id = run.spec.loupist_id;
                    let remove_queue = if let Some(queue) = table.writer_queues.get_mut(&loupist_id)
                    {
                        queue.retain(|queued| queued != &self.run_id);
                        queue.is_empty()
                    } else {
                        false
                    };
                    if remove_queue {
                        table.writer_queues.remove(&loupist_id);
                    }
                }
            }
        };
        let next = self.changes.borrow().wrapping_add(1);
        self.changes.send_replace(next);
    }
}

impl Default for RunCoordinator {
    fn default() -> Self {
        Self::new(MaintenanceCoordinator::default())
    }
}

impl RunCoordinator {
    pub fn new(maintenance: MaintenanceCoordinator) -> Self {
        let (changes, _) = watch::channel(0);
        Self {
            inner: Arc::new(Mutex::new(RunTable::default())),
            maintenance,
            changes,
        }
    }

    pub fn register(
        &self,
        spec: RunSpec,
        availability_check: impl FnOnce() -> Result<(), CommandError>,
    ) -> Result<RunRegistration, CommandError> {
        self.register_with_controls(spec, None, availability_check)
    }

    pub fn register_chat(
        &self,
        spec: RunSpec,
        chat_control: ChatControlHandle,
        availability_check: impl FnOnce() -> Result<(), CommandError>,
    ) -> Result<RunRegistration, CommandError> {
        if spec.kind != RunKind::Chat {
            return Err(CommandError::internal(
                "Chat controls may only be attached to Chat runs",
            ));
        }
        self.register_with_controls(spec, Some(chat_control), availability_check)
    }

    fn register_with_controls(
        &self,
        spec: RunSpec,
        chat_control: Option<ChatControlHandle>,
        availability_check: impl FnOnce() -> Result<(), CommandError>,
    ) -> Result<RunRegistration, CommandError> {
        let activity = self.maintenance.admit_activity(MaintenanceBlocker {
            id: spec.run_id.clone(),
            kind: spec.kind.as_str().into(),
            label: format!("Active {}", spec.kind.activity_label()),
        })?;
        let mut table = self
            .inner
            .lock()
            .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
        // The availability check executes under the same short registry lock as
        // admission. Loupist deletion quarantines under this lock too, closing the
        // check/register race without holding a lock across async work.
        availability_check()?;
        Self::admit(&table, &spec)?;

        let (cancel, receiver) = watch::channel(false);
        let run_id = spec.run_id.clone();
        table.runs.insert(
            run_id.clone(),
            ActiveRun {
                spec,
                started_at_ms: chrono::Utc::now().timestamp_millis(),
                cancel,
                chat_control,
                terminal: TerminalState::Running,
            },
        );
        Ok(RunRegistration {
            cancel: receiver,
            lease: RunLease {
                run_id,
                coordinator: Arc::downgrade(&self.inner),
                changes: self.changes.clone(),
                _activity: activity,
            },
        })
    }

    /// Atomically reserves the Loupist's writer queue while the caller may still
    /// hold a Chat reader lease. New readers are rejected as soon as this
    /// returns, closing the Chat-completion-to-Memory-finalization admission
    /// gap without awaiting (and deadlocking on) the caller's own reader.
    pub fn reserve_memory_write(
        &self,
        spec: RunSpec,
        availability_check: impl FnOnce() -> Result<(), CommandError>,
    ) -> Result<PendingMemoryWrite, CommandError> {
        if spec.kind != RunKind::MemoryWrite {
            return Err(CommandError::internal(
                "only Memory writes may reserve the writer queue",
            ));
        }
        let activity = self.maintenance.admit_activity(MaintenanceBlocker {
            id: spec.run_id.clone(),
            kind: spec.kind.as_str().into(),
            label: format!("Pending {}", spec.kind.activity_label()),
        })?;
        let run_id = spec.run_id.clone();
        let loupist_id = spec.loupist_id.clone();
        let registration = {
            let mut table = self
                .inner
                .lock()
                .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
            availability_check()?;
            Self::admit_waiting_memory_writer(&table, &spec)?;
            let (cancel, receiver) = watch::channel(false);
            table.runs.insert(
                run_id.clone(),
                ActiveRun {
                    spec,
                    started_at_ms: chrono::Utc::now().timestamp_millis(),
                    cancel,
                    chat_control: None,
                    terminal: TerminalState::Running,
                },
            );
            table
                .writer_queues
                .entry(loupist_id.clone())
                .or_default()
                .push_back(run_id.clone());
            RunRegistration {
                cancel: receiver,
                lease: RunLease {
                    run_id: run_id.clone(),
                    coordinator: Arc::downgrade(&self.inner),
                    changes: self.changes.clone(),
                    _activity: activity,
                },
            }
        };
        Ok(PendingMemoryWrite {
            #[cfg(test)]
            wait_error: None,
            coordinator: self.clone(),
            registration,
            loupist_id,
            run_id,
        })
    }

    async fn wait_for_memory_writer(
        &self,
        loupist_id: &str,
        run_id: &str,
    ) -> Result<(), CommandError> {
        let mut changes = self.changes.subscribe();
        loop {
            let ready = {
                let table = self
                    .inner
                    .lock()
                    .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
                let is_queue_head = table
                    .writer_queues
                    .get(loupist_id)
                    .and_then(|queue| queue.front())
                    .is_some_and(|queued| queued == run_id);
                let readers_drained = table.runs.values().all(|run| {
                    run.spec.loupist_id != loupist_id || run.spec.kind == RunKind::MemoryWrite
                });
                is_queue_head && readers_drained
            };
            if ready {
                return Ok(());
            }
            changes
                .changed()
                .await
                .map_err(|_| CommandError::internal("Memory writer queue stopped"))?;
        }
    }

    fn admit_waiting_memory_writer(
        table: &RunTable,
        incoming: &RunSpec,
    ) -> Result<(), CommandError> {
        Self::admit_during_shutdown(table, incoming)?;
        if table.runs.contains_key(&incoming.run_id) {
            return Err(CommandError::conflict("run id is already active"));
        }
        for active in table.runs.values() {
            if active.spec.loupist_id == incoming.loupist_id
                && active.spec.target == incoming.target
            {
                return Err(CommandError::conflict(
                    "this Loupist target already has an active run",
                ));
            }
        }
        Ok(())
    }

    pub async fn submit_chat_control(
        &self,
        loupist_id: &str,
        thread_id: &str,
        prompt: String,
    ) -> Result<AcceptedChatControl, CommandError> {
        let control = {
            let table = self
                .inner
                .lock()
                .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
            let run = table
                .runs
                .values()
                .find(|run| {
                    run.spec.kind == RunKind::Chat
                        && run.spec.loupist_id == loupist_id
                        && matches!(
                            &run.spec.target,
                            RunTarget::ChatThread(value) if value == thread_id
                        )
                })
                .ok_or_else(|| CommandError::not_found("active Chat run"))?;
            if run.terminal != TerminalState::Running {
                return Err(CommandError::conflict(
                    "the Chat run is already stopping or completing",
                ));
            }
            run.chat_control
                .clone()
                .ok_or_else(|| CommandError::internal("Chat control is missing"))?
        };
        control
            .submit(prompt)
            .await
            .map_err(|error| CommandError::conflict(error.to_string()))
    }

    fn admit(table: &RunTable, incoming: &RunSpec) -> Result<(), CommandError> {
        Self::admit_during_shutdown(table, incoming)?;
        if table.runs.contains_key(&incoming.run_id) {
            return Err(CommandError::conflict("run id is already active"));
        }
        if incoming.kind.consumes_model_slot()
            && table
                .runs
                .values()
                .filter(|run| run.spec.kind.consumes_model_slot())
                .count()
                >= MAX_MODEL_WORKERS
        {
            return Err(CommandError::conflict(
                "the four-worker model execution pool is full",
            ));
        }
        for active in table.runs.values() {
            if active.spec.loupist_id != incoming.loupist_id {
                continue;
            }
            if active.spec.target == incoming.target {
                return Err(CommandError::conflict(
                    "this Loupist target already has an active run",
                ));
            }
            if active.spec.kind.loupist_access() == LoupistAccess::ExclusiveMemory
                || incoming.kind.loupist_access() == LoupistAccess::ExclusiveMemory
            {
                return Err(CommandError::conflict(
                    "this Loupist has an active Memory reader or writer",
                ));
            }
        }
        Ok(())
    }

    pub async fn cancel(&self, run_id: &str, expected_kind: RunKind) -> Result<(), CommandError> {
        {
            let mut table = self
                .inner
                .lock()
                .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
            let run = table
                .runs
                .get_mut(run_id)
                .ok_or_else(|| CommandError::not_found("active run"))?;
            if run.spec.kind != expected_kind {
                return Err(CommandError::conflict(
                    "run kind does not match the command",
                ));
            }
            match run.terminal {
                TerminalState::Running => run.terminal = TerminalState::CancelRequested,
                TerminalState::CancelRequested => {}
                TerminalState::CompletionClaimed => {
                    return Err(CommandError::conflict(
                        "run completion already owns the terminal boundary",
                    ));
                }
            }
            let _ = run.cancel.send(true);
        }
        Ok(())
    }

    fn admit_during_shutdown(table: &RunTable, incoming: &RunSpec) -> Result<(), CommandError> {
        let completing_chat = incoming.kind == RunKind::MemoryWrite
            && table.runs.values().any(|run| {
                run.spec.loupist_id == incoming.loupist_id
                    && run.spec.kind == RunKind::Chat
                    && run.terminal == TerminalState::CompletionClaimed
            });
        if table.shutting_down && !completing_chat {
            return Err(CommandError::conflict("Loupist is closing"));
        }
        Ok(())
    }

    pub(crate) fn request_shutdown(&self) -> Result<(), CommandError> {
        let mut table = self
            .inner
            .lock()
            .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
        table.shutting_down = true;
        for run in table.runs.values_mut() {
            if run.spec.kind == RunKind::Chat && run.terminal == TerminalState::Running {
                run.terminal = TerminalState::CancelRequested;
                let _ = run.cancel.send(true);
            }
        }
        Ok(())
    }

    pub(crate) async fn drain(&self) -> Result<(), CommandError> {
        let mut changes = self.changes.subscribe();
        loop {
            let empty = self
                .inner
                .lock()
                .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?
                .runs
                .is_empty();
            if empty {
                return Ok(());
            }
            changes
                .changed()
                .await
                .map_err(|_| CommandError::internal("run shutdown observation stopped"))?;
        }
    }

    /// Linearizes durable completion against cancellation. Once completion
    /// wins, a later Stop cannot acknowledge cancellation for work that is
    /// already being committed. If Stop won first, the caller must not commit.
    pub fn claim_completion(
        &self,
        run_id: &str,
        expected_kind: RunKind,
    ) -> Result<bool, CommandError> {
        let mut table = self
            .inner
            .lock()
            .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
        let run = table
            .runs
            .get_mut(run_id)
            .ok_or_else(|| CommandError::not_found("active run"))?;
        if run.spec.kind != expected_kind {
            return Err(CommandError::conflict(
                "run kind does not match the command",
            ));
        }
        match run.terminal {
            TerminalState::Running => {
                run.terminal = TerminalState::CompletionClaimed;
                Ok(true)
            }
            TerminalState::CancelRequested => Ok(false),
            TerminalState::CompletionClaimed => {
                Err(CommandError::conflict("run completion was already claimed"))
            }
        }
    }

    pub fn begin_loupist_mutation(
        &self,
        loupist_id: &str,
        mutation: impl FnOnce(),
    ) -> Result<(), CommandError> {
        let table = self
            .inner
            .lock()
            .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
        if table
            .runs
            .values()
            .any(|run| run.spec.loupist_id == loupist_id)
        {
            return Err(CommandError::conflict(
                "stop this Loupist's active runs before deleting it",
            ));
        }
        mutation();
        Ok(())
    }

    #[cfg(any(test, feature = "eval-harness"))]
    pub fn active_count(&self) -> usize {
        self.inner
            .lock()
            .map(|table| table.runs.len())
            .unwrap_or_default()
    }

    pub fn activities(&self) -> Result<Vec<RunActivity>, CommandError> {
        let table = self
            .inner
            .lock()
            .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))?;
        let mut activities = table
            .runs
            .values()
            .map(|run| RunActivity {
                run_id: run.spec.run_id.clone(),
                loupist_id: run.spec.loupist_id.clone(),
                kind: run.spec.kind.as_str().into(),
                target: run.spec.target.value().into(),
                started_at_ms: run.started_at_ms,
            })
            .collect::<Vec<_>>();
        activities.sort_by(|left, right| {
            left.started_at_ms
                .cmp(&right.started_at_ms)
                .then_with(|| left.run_id.cmp(&right.run_id))
        });
        Ok(activities)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(run_id: &str, kind: RunKind, target: &str) -> RunSpec {
        RunSpec {
            run_id: run_id.into(),
            loupist_id: "loupist-a".into(),
            kind,
            target: match kind {
                RunKind::Chat | RunKind::ChatMutation => RunTarget::ChatThread(target.into()),
                RunKind::MemoryWrite => RunTarget::MemoryWriteSession(target.into()),
            },
        }
    }

    #[tokio::test]
    async fn concurrent_chats_enforce_capacity_target_uniqueness_and_individual_stop() {
        let coordinator = RunCoordinator::default();
        let mut registrations = Vec::new();
        for i in 0..MAX_MODEL_WORKERS {
            registrations.push(
                coordinator
                    .register(
                        spec(&format!("run-{i}"), RunKind::Chat, &format!("chat-{i}")),
                        || Ok(()),
                    )
                    .unwrap(),
            );
        }
        assert_eq!(coordinator.active_count(), 4);
        assert!(coordinator
            .register(spec("fifth", RunKind::Chat, "chat-5"), || Ok(()))
            .is_err());
        coordinator.cancel("run-1", RunKind::Chat).await.unwrap();
        for (i, registration) in registrations.iter().enumerate() {
            assert_eq!(*registration.cancel.borrow(), i == 1);
        }
        drop(registrations.pop());
        assert!(coordinator
            .register(spec("same-target", RunKind::Chat, "chat-0"), || Ok(()))
            .is_err());
        let replacement = coordinator
            .register(spec("replacement", RunKind::Chat, "chat-3"), || Ok(()))
            .unwrap();
        assert_eq!(coordinator.active_count(), 4);
        drop(registrations);
        drop(replacement);
        assert_eq!(coordinator.active_count(), 0);
    }

    #[tokio::test]
    async fn shutdown_stops_chat_admission_and_waits_for_owned_cleanup() {
        let coordinator = RunCoordinator::default();
        let registration = coordinator
            .register(spec("chat-a", RunKind::Chat, "thread-a"), || Ok(()))
            .unwrap();
        coordinator.request_shutdown().unwrap();
        assert!(*registration.cancel.borrow());
        assert!(!coordinator
            .claim_completion("chat-a", RunKind::Chat)
            .unwrap());
        assert!(coordinator
            .register(spec("chat-b", RunKind::Chat, "thread-b"), || Ok(()))
            .is_err());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), coordinator.drain())
                .await
                .is_err()
        );
        drop(registration);
        tokio::time::timeout(std::time::Duration::from_secs(1), coordinator.drain())
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn shutdown_preserves_claimed_completion_and_its_memory_handoff() {
        let coordinator = RunCoordinator::default();
        let chat = coordinator
            .register(spec("chat-a", RunKind::Chat, "thread-a"), || Ok(()))
            .unwrap();
        assert!(coordinator
            .claim_completion("chat-a", RunKind::Chat)
            .unwrap());
        coordinator.request_shutdown().unwrap();
        assert!(!*chat.cancel.borrow());
        let pending = coordinator
            .reserve_memory_write(
                spec("writer-a", RunKind::MemoryWrite, "memory-a"),
                || Ok(()),
            )
            .unwrap();
        let mut unrelated = spec("writer-b", RunKind::MemoryWrite, "memory-b");
        unrelated.loupist_id = "loupist-b".into();
        assert!(coordinator
            .reserve_memory_write(unrelated, || Ok(()))
            .is_err());
        drop(chat);
        let writer = pending.wait().await.unwrap();
        assert!(!*writer.cancel.borrow());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), coordinator.drain())
                .await
                .is_err()
        );
        drop(writer);
        tokio::time::timeout(std::time::Duration::from_secs(1), coordinator.drain())
            .await
            .unwrap()
            .unwrap();
        assert!(coordinator
            .reserve_memory_write(
                spec("late-writer", RunKind::MemoryWrite, "memory-c"),
                || Ok(())
            )
            .is_err());
    }

    #[tokio::test]
    async fn chat_can_reserve_writer_barrier_before_releasing_its_reader() {
        let coordinator = RunCoordinator::default();
        let reader = coordinator
            .register(spec("chat-a", RunKind::Chat, "thread-a"), || Ok(()))
            .unwrap();
        let pending = coordinator
            .reserve_memory_write(
                spec("writer-a", RunKind::MemoryWrite, "memory-a"),
                || Ok(()),
            )
            .unwrap();

        // The synchronous reservation closes both the same-thread ordering
        // gap and reader barging from another thread before the Chat lease is
        // released.
        assert!(coordinator
            .register(spec("chat-b", RunKind::Chat, "thread-a"), || Ok(()))
            .is_err());
        assert!(coordinator
            .register(spec("chat-c", RunKind::Chat, "thread-c"), || Ok(()))
            .is_err());

        let writer = pending.wait();
        tokio::pin!(writer);
        tokio::select! {
            result = &mut writer => panic!("writer entered before its Chat reader left: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {}
        }
        drop(reader);
        let writer = tokio::time::timeout(std::time::Duration::from_secs(1), writer)
            .await
            .unwrap()
            .unwrap();
        drop(writer);
        assert_eq!(coordinator.active_count(), 0);
    }

    #[test]
    fn dropping_unawaited_writer_reservation_reopens_reader_admission() {
        let coordinator = RunCoordinator::default();
        let pending = coordinator
            .reserve_memory_write(
                spec("writer-a", RunKind::MemoryWrite, "memory-a"),
                || Ok(()),
            )
            .unwrap();
        assert!(coordinator
            .register(spec("chat-a", RunKind::Chat, "thread-a"), || Ok(()))
            .is_err());
        drop(pending);
        let reader = coordinator
            .register(spec("chat-a", RunKind::Chat, "thread-a"), || Ok(()))
            .unwrap();
        drop(reader);
        assert_eq!(coordinator.active_count(), 0);
    }

    #[tokio::test]
    async fn memory_writers_queue_fifo_behind_multiple_readers_without_reader_barging() {
        let coordinator = RunCoordinator::default();
        let reader_a = coordinator
            .register(spec("chat-a", RunKind::Chat, "thread-a"), || Ok(()))
            .unwrap();
        let reader_b = coordinator
            .register(spec("chat-b", RunKind::Chat, "thread-b"), || Ok(()))
            .unwrap();

        let writer_a = coordinator
            .reserve_memory_write(
                spec("writer-a", RunKind::MemoryWrite, "memory-a"),
                || Ok(()),
            )
            .unwrap()
            .wait();
        tokio::pin!(writer_a);
        tokio::select! {
            result = &mut writer_a => panic!("first writer entered while readers were active: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {}
        }
        let writer_b = coordinator
            .reserve_memory_write(
                spec("writer-b", RunKind::MemoryWrite, "memory-b"),
                || Ok(()),
            )
            .unwrap()
            .wait();
        tokio::pin!(writer_b);
        tokio::select! {
            result = &mut writer_b => panic!("second writer bypassed the queue: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {}
        }
        assert!(coordinator
            .register(spec("chat-c", RunKind::Chat, "thread-c"), || Ok(()))
            .is_err());

        drop(reader_a);
        drop(reader_b);
        let writer_a = tokio::time::timeout(std::time::Duration::from_secs(1), writer_a)
            .await
            .unwrap()
            .unwrap();
        tokio::select! {
            result = &mut writer_b => panic!("second writer entered before the queue head left: {result:?}"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(25)) => {}
        }
        drop(writer_a);
        let writer_b = tokio::time::timeout(std::time::Duration::from_secs(1), writer_b)
            .await
            .unwrap()
            .unwrap();
        drop(writer_b);
        assert_eq!(coordinator.active_count(), 0);
    }
}
