//! Frozen unattended budgets share the Chat lease and the usage ledger's round counter.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, LazyLock, Mutex, Weak},
    time::Duration,
};

use tokio::{sync::Notify, time::Instant};

use crate::scheduled_tasks::types::{BudgetOutcome, RunBudget};

pub(crate) const GRACE_ROUNDS: u32 = 8;
const GRACE_TIME: Duration = Duration::from_secs(5 * 60);
const WALL_TIME: Duration = Duration::from_secs(30 * 60);
type ChatKey = (String, String);
static TURNS: LazyLock<Mutex<BTreeMap<ChatKey, Weak<TurnBudget>>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[derive(Debug)]
pub(crate) struct BudgetLease {
    key: ChatKey,
    budget: Arc<TurnBudget>,
}

impl Drop for BudgetLease {
    fn drop(&mut self) {
        let mut turns = TURNS.lock().unwrap_or_else(|error| error.into_inner());
        if turns
            .get(&self.key)
            .is_some_and(|saved| saved.ptr_eq(&Arc::downgrade(&self.budget)))
        {
            turns.remove(&self.key);
        }
    }
}

#[derive(Debug)]
pub(crate) struct TurnBudget {
    frozen: RunBudget,
    wall_deadline: Instant,
    learning: Option<crate::scheduled_tasks::learning::TurnContext>,
    progress: Mutex<Progress>,
    observed: Notify,
}

#[derive(Debug, Default)]
struct Progress {
    rounds: u32,
    hard_at: Option<Instant>,
    refused: bool,
    cancelled: bool,
    calls: BTreeSet<String>,
}

impl BudgetLease {
    pub(crate) fn bind(
        loupist_id: &str,
        thread_id: &str,
        frozen: RunBudget,
        elapsed: Duration,
        learning: Option<crate::scheduled_tasks::learning::TurnContext>,
    ) -> Self {
        let key = (loupist_id.to_owned(), thread_id.to_owned());
        let budget = Arc::new(TurnBudget {
            frozen,
            wall_deadline: Instant::now() + WALL_TIME.saturating_sub(elapsed),
            learning,
            progress: Mutex::default(),
            observed: Notify::new(),
        });
        TURNS
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(key.clone(), Arc::downgrade(&budget));
        Self { key, budget }
    }
}

pub(crate) fn for_chat(loupist_id: &str, thread_id: &str) -> Option<Arc<TurnBudget>> {
    TURNS
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .get(&(loupist_id.to_owned(), thread_id.to_owned()))
        .and_then(Weak::upgrade)
}

impl TurnBudget {
    pub(crate) fn observe<'a>(
        &self,
        rounds: u32,
        now: Instant,
        calls: impl Iterator<Item = &'a str>,
    ) {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        progress.rounds = progress.rounds.max(rounds);
        if progress.rounds >= self.frozen.hard {
            progress.hard_at.get_or_insert(now);
        }
        progress.calls.extend(calls.map(str::to_owned));
        drop(progress);
        self.observed.notify_waiters();
    }

    /// The Pi event stream and broker socket are separate transports. A
    /// tagged request waits until Rust has counted the assistant round that
    /// emitted it, even if the broker's socket became readable first.
    pub(crate) async fn wait_for_call(&self, call_id: &str) -> bool {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let notified = self.observed.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self
                    .progress
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .calls
                    .remove(call_id)
                {
                    break;
                }
                notified.await;
            }
        })
        .await
        .is_ok()
    }

    pub(crate) fn deadline(&self) -> Instant {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        progress.hard_at.map_or(self.wall_deadline, |at| {
            (at + GRACE_TIME).min(self.wall_deadline)
        })
    }

    pub(crate) fn exhausted(&self, now: Instant) -> bool {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        progress.rounds >= self.frozen.requests
            || now >= self.wall_deadline
            || progress.hard_at.is_some_and(|at| now >= at + GRACE_TIME)
    }

    pub(crate) fn refusal(&self) -> bool {
        let mut progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.hard_at.is_some() {
            progress.refused = true;
        }
        progress.refused
    }

    pub(crate) fn learning(&self) -> Option<&crate::scheduled_tasks::learning::TurnContext> {
        self.learning.as_ref()
    }

    pub(crate) fn mark_cancelled(&self) {
        self.progress
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .cancelled = true;
    }

    pub(crate) fn outcome(&self) -> BudgetOutcome {
        let progress = self
            .progress
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if progress.cancelled {
            BudgetOutcome::Cancelled
        } else if progress.refused {
            BudgetOutcome::Refused
        } else if progress.rounds > 0 && progress.rounds >= self.frozen.soft {
            BudgetOutcome::Soft
        } else {
            BudgetOutcome::None
        }
    }
}
