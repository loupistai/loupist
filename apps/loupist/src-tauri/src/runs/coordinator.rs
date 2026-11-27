use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use serde::Serialize;
use tokio::sync::watch;

use crate::{
    app::CommandError,
    chat::control::{AcceptedChatControl, ChatControlHandle},
    domain::MemorySaveSkipReason,
    runs::maintenance::{ActivityLease, MaintenanceBlocker, MaintenanceCoordinator},
};

pub const MAX_MODEL_WORKERS: usize = 4;
const CHAT_SAVE_WAIT: Duration = Duration::from_secs(10 * 60);
const CHAT_READER_RESUME_WAIT: Duration = Duration::from_secs(30);
/// Activity kind of a released save waiting its turn. It is not a Chat run:
/// its conversation accepts a new message.
pub const CHAT_SAVE_ACTIVITY: &str = "chat_save";

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
#[cfg_attr(test, derive(ts_rs::TS))]
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
    chat_access: ChatAccess,
    tools_running: usize,
    tools_parked: usize,
    skip_save: bool,
    /// An interactive reply whose answer is stored and whose save waits on its
    /// own: its conversation is free for the next turn.
    released: Option<SavingMessage>,
    /// The released save's last reported wait.
    save_wait: SaveWait,
    /// The released save's receipt is on its message. Until then the save
    /// counts as pending, also after its skip, deadline or failure settled it.
    receipt_written: bool,
}

/// What a released save last reported while it waits.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct SaveWait {
    pub waiting_for_other_chat: bool,
    pub activity: Option<crate::chat::progress::ChatSaveActivity>,
}

/// The stored reply a released save belongs to, and the Bot review it was offered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SavingMessage {
    pub thread_id: String,
    pub message_id: String,
    pub reviewed: Vec<String>,
    /// Bot memory entries the save resolves or forgets; a newer turn of the
    /// conversation does not see them.
    pub leaving: crate::chat::bot_memory::Leaving,
}

/// A released save that has not resolved yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PendingSave {
    pub run_id: String,
    pub message: SavingMessage,
    pub wait: SaveWait,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ChatAccess {
    #[default]
    Reading,
    PendingSave,
    Saving,
    Settled,
}

impl ActiveRun {
    fn reads_memory(&self) -> bool {
        match self.spec.kind {
            RunKind::Chat => {
                self.chat_access == ChatAccess::Reading
                    && (self.tools_running != 0
                        || (self.tools_parked == 0
                            && self.terminal != TerminalState::CancelRequested))
            }
            RunKind::ChatMutation => true,
            RunKind::MemoryWrite => false,
        }
    }

    fn writes_memory(&self) -> bool {
        self.spec.kind == RunKind::MemoryWrite || self.chat_access == ChatAccess::Saving
    }

    /// Whether this run holds its target. A released save leaves the thread
    /// to the next turn while it keeps its place in the writer queue.
    fn holds_target(&self) -> bool {
        self.released.is_none()
    }
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

#[derive(Debug)]
pub struct RunLease {
    pub(crate) budget: Option<super::budget::BudgetLease>,
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
                table.remove_writer(&run.spec.loupist_id, &self.run_id);
            }
        };
        self.changes
            .send_modify(|next| *next = next.wrapping_add(1));
    }
}

impl RunTable {
    fn remove_writer(&mut self, loupist_id: &str, run_id: &str) {
        if let Some(queue) = self.writer_queues.get_mut(loupist_id) {
            queue.retain(|queued| queued != run_id);
            if queue.is_empty() {
                self.writer_queues.remove(loupist_id);
            }
        }
    }
}

enum SaveAdmission {
    Waiting(Option<String>),
    Acquired,
    Skipped(MemorySaveSkipReason),
}

/// A broker call belongs to its Chat reader while it is doing work. A user
/// card parks just this call, so parallel tools must finish or park as well
/// before the Chat stops excluding a writer.
pub(crate) struct ChatToolAdmission {
    coordinator: RunCoordinator,
    run_id: String,
    parked: bool,
}

impl ChatToolAdmission {
    pub(crate) fn park(&mut self) {
        self.coordinator.change_tool_state(&self.run_id, |run| {
            run.tools_running -= 1;
            run.tools_parked += 1;
        });
        self.parked = true;
    }

    pub(crate) async fn resume(&mut self) -> Result<(), CommandError> {
        self.coordinator.admit_chat_tool(&self.run_id, true).await?;
        self.parked = false;
        Ok(())
    }
}

impl Drop for ChatToolAdmission {
    fn drop(&mut self) {
        self.coordinator.change_tool_state(&self.run_id, |run| {
            if self.parked {
                run.tools_parked -= 1;
            } else {
                run.tools_running -= 1;
            }
        });
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

    fn notify_change(&self) {
        self.changes
            .send_modify(|next| *next = next.wrapping_add(1));
    }

    fn table(&self) -> Result<std::sync::MutexGuard<'_, RunTable>, CommandError> {
        self.inner
            .lock()
            .map_err(|_| CommandError::internal("run coordinator lock was poisoned"))
    }

    fn change_tool_state(&self, run_id: &str, change: impl FnOnce(&mut ActiveRun)) {
        if let Ok(mut table) = self.table() {
            if let Some(run) = table.runs.get_mut(run_id) {
                change(run);
            }
        }
        self.notify_change();
    }

    pub(crate) async fn begin_chat_tool(
        &self,
        run_id: &str,
    ) -> Result<ChatToolAdmission, CommandError> {
        self.admit_chat_tool(run_id, false).await?;
        Ok(ChatToolAdmission {
            coordinator: self.clone(),
            run_id: run_id.to_owned(),
            parked: false,
        })
    }

    async fn admit_chat_tool(&self, run_id: &str, resuming: bool) -> Result<(), CommandError> {
        let mut changes = self.changes.subscribe();
        let admitted = tokio::time::timeout(CHAT_READER_RESUME_WAIT, async {
            loop {
                {
                    let mut table = self.table()?;
                    let run = table
                        .runs
                        .get(run_id)
                        .ok_or_else(|| CommandError::not_found("Chat run"))?;
                    if run.spec.kind != RunKind::Chat
                        || run.chat_access != ChatAccess::Reading
                        || run.terminal != TerminalState::Running
                    {
                        return Err(CommandError::conflict("Chat is no longer reading"));
                    }
                    if !table.runs.values().any(|other| {
                        other.spec.loupist_id == run.spec.loupist_id && other.writes_memory()
                    }) {
                        let run = table.runs.get_mut(run_id).expect("same registry lock");
                        if resuming {
                            run.tools_parked -= 1;
                        }
                        run.tools_running += 1;
                        return Ok(());
                    }
                }
                changes
                    .changed()
                    .await
                    .map_err(|_| CommandError::internal("Chat reader observation stopped"))?;
            }
        })
        .await;
        match admitted {
            Ok(result) => result,
            Err(_) => {
                let _ = self.cancel(run_id, RunKind::Chat).await;
                Err(CommandError::conflict("Chat reader could not resume"))
            }
        }
    }

    /// Queues the save in FIFO order. With `release`, the same step frees the
    /// conversation for a new turn: the save then belongs to its message.
    pub(crate) fn queue_chat_save(
        &self,
        run_id: &str,
        release: Option<SavingMessage>,
    ) -> Result<(), CommandError> {
        let mut table = self.table()?;
        let run = table
            .runs
            .get_mut(run_id)
            .ok_or_else(|| CommandError::not_found("Chat run"))?;
        if run.spec.kind != RunKind::Chat
            || run.terminal != TerminalState::CompletionClaimed
            || run.chat_access != ChatAccess::Reading
            || run.tools_running != 0
            || run.tools_parked != 0
        {
            return Err(CommandError::conflict("Chat is not ready to save"));
        }
        if release.as_ref().is_some_and(|message| {
            !matches!(&run.spec.target, RunTarget::ChatThread(id) if *id == message.thread_id)
        }) {
            return Err(CommandError::internal("released save names another thread"));
        }
        run.chat_access = ChatAccess::PendingSave;
        run.released = release;
        let loupist_id = run.spec.loupist_id.clone();
        table
            .writer_queues
            .entry(loupist_id)
            .or_default()
            .push_back(run_id.to_owned());
        drop(table);
        self.notify_change();
        Ok(())
    }

    /// Only the transition to an active exclusive save closes reader admission.
    /// The deadline and a requested skip are checked under the same registry lock.
    pub(crate) async fn wait_for_chat_save(
        &self,
        run_id: &str,
        mut waiting: impl FnMut(Option<String>),
    ) -> Result<Option<MemorySaveSkipReason>, CommandError> {
        let deadline = tokio::time::Instant::now() + CHAT_SAVE_WAIT;
        let mut changes = self.changes.subscribe();
        let mut reported = None;
        loop {
            match self.poll_chat_save(run_id, deadline)? {
                SaveAdmission::Acquired => return Ok(None),
                SaveAdmission::Skipped(reason) => return Ok(Some(reason)),
                SaveAdmission::Waiting(blocker) => {
                    if Some(&blocker) != reported.as_ref() {
                        waiting(blocker.clone());
                        reported = Some(blocker);
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {}
                updated = changes.changed() => {
                    updated.map_err(|_| CommandError::internal("Chat save observation stopped"))?;
                }
            }
        }
    }

    fn poll_chat_save(
        &self,
        run_id: &str,
        deadline: tokio::time::Instant,
    ) -> Result<SaveAdmission, CommandError> {
        let mut table = self.table()?;
        let run = table
            .runs
            .get(run_id)
            .ok_or_else(|| CommandError::not_found("Chat run"))?;
        if run.chat_access != ChatAccess::PendingSave {
            return Err(CommandError::conflict("Chat save is not pending"));
        }
        let loupist_id = run.spec.loupist_id.clone();
        let skip = if run.skip_save {
            Some(MemorySaveSkipReason::User)
        } else if table.shutting_down {
            Some(MemorySaveSkipReason::Interrupted)
        } else if tokio::time::Instant::now() >= deadline {
            Some(MemorySaveSkipReason::Deadline)
        } else {
            None
        };
        if let Some(reason) = skip {
            table
                .runs
                .get_mut(run_id)
                .expect("same registry lock")
                .chat_access = ChatAccess::Settled;
            table.remove_writer(&loupist_id, run_id);
            drop(table);
            self.notify_change();
            return Ok(SaveAdmission::Skipped(reason));
        }
        let at_front = table
            .writer_queues
            .get(&loupist_id)
            .and_then(|queue| queue.front())
            .is_some_and(|id| id == run_id);
        let mut blockers = table.runs.values().filter(|other| {
            other.spec.loupist_id == loupist_id && (other.reads_memory() || other.writes_memory())
        });
        let first = blockers.next();
        if at_front && first.is_none() {
            table
                .runs
                .get_mut(run_id)
                .expect("same registry lock")
                .chat_access = ChatAccess::Saving;
            table.remove_writer(&loupist_id, run_id);
            return Ok(SaveAdmission::Acquired);
        }
        let blocker = first
            .filter(|_| blockers.next().is_none())
            .and_then(|only| match &only.spec.target {
                RunTarget::ChatThread(id) => Some(id.clone()),
                RunTarget::MemoryWriteSession(_) => None,
            });
        Ok(SaveAdmission::Waiting(blocker))
    }

    /// Released saves of a Loupist (optionally one thread) whose receipt is not
    /// on their message yet: waiting, saving, or settled with the receipt still
    /// to write.
    pub(crate) fn pending_saves(
        &self,
        loupist_id: &str,
        thread_id: Option<&str>,
    ) -> Result<Vec<PendingSave>, CommandError> {
        let table = self.table()?;
        let mut saves = table
            .runs
            .values()
            .filter(|run| {
                run.spec.loupist_id == loupist_id
                    && !run.receipt_written
                    && matches!(
                        run.chat_access,
                        ChatAccess::PendingSave | ChatAccess::Saving | ChatAccess::Settled
                    )
            })
            .filter_map(|run| {
                let message = run.released.clone()?;
                thread_id
                    .is_none_or(|thread| message.thread_id == thread)
                    .then(|| {
                        (
                            run.started_at_ms,
                            PendingSave {
                                run_id: run.spec.run_id.clone(),
                                message,
                                wait: run.save_wait.clone(),
                            },
                        )
                    })
            })
            .collect::<Vec<_>>();
        saves.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.run_id.cmp(&right.1.run_id))
        });
        Ok(saves.into_iter().map(|(_, save)| save).collect())
    }

    /// Records the wait a released save reports, for a renderer that asks later.
    pub(crate) fn note_save_wait(&self, run_id: &str, wait: SaveWait) {
        if let Ok(mut table) = self.table() {
            if let Some(run) = table.runs.get_mut(run_id) {
                run.save_wait = wait;
            }
        }
    }

    /// The released save's receipt is on its message: it is no longer pending.
    pub(crate) fn note_receipt_written(&self, run_id: &str) {
        if let Ok(mut table) = self.table() {
            if let Some(run) = table.runs.get_mut(run_id) {
                run.receipt_written = true;
            }
        }
    }

    pub(crate) fn finish_chat_save(&self, run_id: &str) {
        self.change_tool_state(run_id, |run| {
            run.chat_access = ChatAccess::Settled;
        });
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
                chat_access: ChatAccess::Reading,
                tools_running: 0,
                tools_parked: 0,
                skip_save: false,
                released: None,
                save_wait: SaveWait::default(),
                receipt_written: false,
            },
        );
        drop(table);
        self.notify_change();
        Ok(RunRegistration {
            cancel: receiver,
            lease: RunLease {
                budget: None,
                run_id,
                coordinator: Arc::downgrade(&self.inner),
                changes: self.changes.clone(),
                _activity: activity,
            },
        })
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
                        && run.holds_target()
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
        Self::admit_during_shutdown(table)?;
        if table.runs.contains_key(&incoming.run_id) {
            return Err(CommandError::conflict("run id is already active"));
        }
        if incoming.kind.consumes_model_slot()
            && table
                .runs
                .values()
                .filter(|run| {
                    run.spec.kind.consumes_model_slot() && run.chat_access == ChatAccess::Reading
                })
                .count()
                >= MAX_MODEL_WORKERS
        {
            return Err(CommandError::conflict(
                "the four-worker model execution pool is full",
            ));
        }
        let mut local = table
            .runs
            .values()
            .filter(|active| active.spec.loupist_id == incoming.loupist_id);
        if local
            .clone()
            .any(|active| active.holds_target() && active.spec.target == incoming.target)
        {
            return Err(CommandError::conflict(
                "this Loupist target already has an active run",
            ));
        }
        if local.any(|active| {
            active.writes_memory()
                || (incoming.kind == RunKind::MemoryWrite && active.reads_memory())
        }) {
            if incoming.kind == RunKind::Chat {
                return Err(CommandError::new(
                    crate::app::CommandErrorCode::MemoryBusy,
                    "Waiting for Memory",
                ));
            }
            return Err(CommandError::conflict(
                "this Loupist has an active Memory reader or writer",
            ));
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
                TerminalState::CompletionClaimed if run.chat_access == ChatAccess::PendingSave => {
                    run.skip_save = true;
                    self.notify_change();
                    return Ok(());
                }
                TerminalState::CompletionClaimed => {
                    return Err(CommandError::conflict(
                        "run completion already owns the terminal boundary",
                    ));
                }
            }
            let _ = run.cancel.send(true);
        }
        self.notify_change();
        Ok(())
    }

    fn admit_during_shutdown(table: &RunTable) -> Result<(), CommandError> {
        if table.shutting_down {
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
        drop(table);
        self.notify_change();
        Ok(())
    }

    pub(crate) fn is_shutting_down(&self) -> bool {
        self.inner
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .shutting_down
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
                kind: if run.chat_access == ChatAccess::Saving {
                    RunKind::MemoryWrite.as_str()
                } else if run.released.is_some() {
                    CHAT_SAVE_ACTIVITY
                } else {
                    run.spec.kind.as_str()
                }
                .into(),
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

    #[tokio::test(start_paused = true)]
    async fn pending_saves_keep_fifo_without_excluding_new_readers() {
        let coordinator = RunCoordinator::default();
        let add = |id| coordinator.register(spec(id, RunKind::Chat, id), || Ok(()));
        let older = add("older").unwrap();
        let first = add("first").unwrap();
        let second = add("second").unwrap();
        for id in ["first", "second"] {
            assert!(coordinator.claim_completion(id, RunKind::Chat).unwrap());
            coordinator.queue_chat_save(id, None).unwrap();
        }
        let later = add("later").unwrap();
        assert!(coordinator
            .register(spec("retry", RunKind::Chat, "first"), || Ok(()))
            .is_err());
        assert!(tokio::time::timeout(
            Duration::from_millis(1),
            coordinator.wait_for_chat_save("first", |_| {})
        )
        .await
        .is_err());
        drop((older, later));
        assert!(tokio::time::timeout(
            Duration::from_millis(1),
            coordinator.wait_for_chat_save("second", |_| {})
        )
        .await
        .is_err());
        assert_eq!(
            coordinator
                .wait_for_chat_save("first", |_| {})
                .await
                .unwrap(),
            None
        );
        assert_eq!(add("late").unwrap_err().code, "memory_busy");
        coordinator.finish_chat_save("first");
        assert_eq!(
            coordinator
                .wait_for_chat_save("second", |_| {})
                .await
                .unwrap(),
            None
        );
        coordinator.finish_chat_save("second");
        let admitted = add("late").unwrap();
        drop((first, second, admitted));
        assert_eq!(coordinator.active_count(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn a_human_wait_parks_only_after_parallel_calls_drain_and_rejoins_after_save() {
        let coordinator = RunCoordinator::default();
        let _reader = coordinator
            .register(spec("reader", RunKind::Chat, "question"), || Ok(()))
            .unwrap();
        let _saver = coordinator
            .register(spec("saver", RunKind::Chat, "answer"), || Ok(()))
            .unwrap();
        let mut question = coordinator.begin_chat_tool("reader").await.unwrap();
        let parallel = coordinator.begin_chat_tool("reader").await.unwrap();
        question.park();
        assert!(coordinator
            .claim_completion("saver", RunKind::Chat)
            .unwrap());
        coordinator.queue_chat_save("saver", None).unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(1),
            coordinator.wait_for_chat_save("saver", |_| {})
        )
        .await
        .is_err());
        drop(parallel);
        assert_eq!(
            coordinator
                .wait_for_chat_save("saver", |_| {})
                .await
                .unwrap(),
            None
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(1), question.resume())
                .await
                .is_err()
        );
        coordinator.finish_chat_save("saver");
        question.resume().await.unwrap();
        question.park();
        coordinator.cancel("reader", RunKind::Chat).await.unwrap();
        assert!(question.resume().await.is_err());
        drop(question);
        let writer = coordinator
            .register(spec("writer", RunKind::MemoryWrite, "memory"), || Ok(()))
            .unwrap();
        drop(writer);
    }

    #[tokio::test(start_paused = true)]
    async fn a_released_save_frees_its_thread_and_keeps_its_place_and_reader_wait() {
        let coordinator = RunCoordinator::default();
        let message = |id: &str| SavingMessage {
            thread_id: "bot".into(),
            message_id: format!("reply-{id}"),
            reviewed: vec![format!("ask-{id}")],
            leaving: Default::default(),
        };
        let reader = coordinator
            .register(spec("reader", RunKind::Chat, "other"), || Ok(()))
            .unwrap();
        let (control, _receiver) = crate::chat::control::chat_control_channel();
        let first = coordinator
            .register_chat(spec("first", RunKind::Chat, "bot"), control, || Ok(()))
            .unwrap();
        assert!(coordinator
            .claim_completion("first", RunKind::Chat)
            .unwrap());
        assert!(coordinator
            .queue_chat_save(
                "first",
                Some(SavingMessage {
                    thread_id: "other".into(),
                    ..message("first")
                })
            )
            .is_err());
        coordinator
            .queue_chat_save("first", Some(message("first")))
            .unwrap();
        // The conversation takes its next turn; steering reaches only that turn.
        let (control, _receiver) = crate::chat::control::chat_control_channel();
        let second = coordinator
            .register_chat(spec("second", RunKind::Chat, "bot"), control, || Ok(()))
            .unwrap();
        assert!(coordinator
            .register(spec("third", RunKind::Chat, "bot"), || Ok(()))
            .is_err());
        let kinds = coordinator
            .activities()
            .unwrap()
            .into_iter()
            .map(|activity| (activity.run_id, activity.kind))
            .collect::<HashMap<_, _>>();
        assert_eq!(kinds["first"], CHAT_SAVE_ACTIVITY);
        assert_eq!(kinds["second"], "chat");
        assert_eq!(
            coordinator.pending_saves("loupist-a", Some("bot")).unwrap(),
            [PendingSave {
                run_id: "first".into(),
                message: message("first"),
                wait: SaveWait::default(),
            }]
        );
        // The new turn of its own conversation is a reader the save waits for.
        drop(reader);
        assert!(tokio::time::timeout(
            Duration::from_millis(1),
            coordinator.wait_for_chat_save("first", |_| {})
        )
        .await
        .is_err());
        assert!(coordinator
            .claim_completion("second", RunKind::Chat)
            .unwrap());
        coordinator
            .queue_chat_save("second", Some(message("second")))
            .unwrap();
        assert!(tokio::time::timeout(
            Duration::from_millis(1),
            coordinator.wait_for_chat_save("second", |_| {})
        )
        .await
        .is_err());
        assert_eq!(
            coordinator
                .wait_for_chat_save("first", |_| {})
                .await
                .unwrap(),
            None
        );
        coordinator.finish_chat_save("first");
        // Settled, a save stays pending until its receipt is on its message.
        assert_eq!(
            coordinator.pending_saves("loupist-a", None).unwrap().len(),
            2
        );
        coordinator.note_receipt_written("first");
        assert_eq!(
            coordinator.pending_saves("loupist-a", None).unwrap().len(),
            1
        );
        coordinator.cancel("second", RunKind::Chat).await.unwrap();
        assert_eq!(
            coordinator
                .wait_for_chat_save("second", |_| {})
                .await
                .unwrap(),
            Some(MemorySaveSkipReason::User)
        );
        assert_eq!(
            coordinator.pending_saves("loupist-a", None).unwrap().len(),
            1
        );
        coordinator.note_receipt_written("second");
        assert!(coordinator
            .pending_saves("loupist-a", None)
            .unwrap()
            .is_empty());
        drop((first, second));
        assert_eq!(coordinator.active_count(), 0);
    }

    #[tokio::test]
    async fn stop_or_quit_ends_pending_save_before_it_can_claim_the_writer() {
        for (shutdown, expected) in [
            (false, MemorySaveSkipReason::User),
            (true, MemorySaveSkipReason::Interrupted),
        ] {
            let coordinator = RunCoordinator::default();
            let _older = coordinator
                .register(spec("older", RunKind::Chat, "older"), || Ok(()))
                .unwrap();
            let _saver = coordinator
                .register(spec("saver", RunKind::Chat, "answer"), || Ok(()))
                .unwrap();
            assert!(coordinator
                .claim_completion("saver", RunKind::Chat)
                .unwrap());
            coordinator.queue_chat_save("saver", None).unwrap();
            if shutdown {
                coordinator.request_shutdown().unwrap();
            } else {
                coordinator.cancel("saver", RunKind::Chat).await.unwrap();
            }
            assert_eq!(
                coordinator
                    .wait_for_chat_save("saver", |_| {})
                    .await
                    .unwrap(),
                Some(expected)
            );
        }
    }
}
