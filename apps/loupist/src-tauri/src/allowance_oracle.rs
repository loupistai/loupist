//! Allowance oracle (master plan 3.8 as amended by D2-4, and 6.1).
//!
//! A seeded sequence of events runs against a real Loupist folder store and
//! against a reference that keeps plain counters per local day and class:
//! every model request that was dispatched, whatever the ledger managed to
//! store, and what each running row may still spend. After every event:
//!
//! - what the allowance reports as left in each bucket (background, its World
//!   share, chat World) is the reference's limit minus used minus reserved,
//!   under the process's local day and under other time zones;
//! - no call is dispatched without admission, and each admission decision is
//!   the reference's;
//! - a reservation ends only when its usage is recorded or its row has ended,
//!   and a crash neither frees nor forgets what was dispatched;
//! - a job's origin is the origin of the turn that saved its Source;
//! - a queued job is held exactly when the reference says it cannot start,
//!   with the reference's cause;
//! - the claim pass takes the reference's next job, and a seed that stops
//!   making progress (a lock held across a wait) fails instead of hanging.
//!
//! The reference was written from the design, not from the code. The adapter
//! calls the host's own functions: the World worker's per-call ledger row
//! ([`crate::knowledge::graph_service::WorldAccount`]) and a Chat turn's usage
//! row ([`crate::runs::log::RunLog::end_turn`] then `record_usage`).
//!
//! The default run uses 200 seeds; `LOUPIST_ORACLE_SEEDS=2000` runs the long
//! form. A failure lists every failing seed with the events up to the failure.
//!
//! This module is a child of `graph_jobs::tests`, so it uses the World job
//! fixtures and the claim, reservation and recovery functions directly.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::domain::ChatFailureKind;
use crate::knowledge::graph_inference::{InferenceDisposition, InferenceReply};
use crate::knowledge::{Authorship, RetainNewSource};
use crate::runs::log::UsageSurface;
use crate::scheduled_tasks::allowance::{
    admit_run, admit_world_attempt, local_date_in_zone, Admission, AllowanceState, HoldReason,
    Limits, WorldOrigin,
};
use crate::store::model_usage::{fixture_row, ModelUsageFailure, ModelUsageKind, ModelUsageStatus};

const LOUPIST: &str = "loupist-oracle";
const PROVIDER: &str = "openai";

fn seed_count() -> u64 {
    std::env::var("LOUPIST_ORACLE_SEEDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200)
}

/// splitmix64: small, seeded and in-tree.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0xA076_1D64_78BD_642F)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound.max(1) as u64) as usize
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

// --- The alphabet -------------------------------------------------------

/// How the Source behind a World job was saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Saved {
    /// The user saved it directly.
    ByUser,
    /// An interactive turn in a Chat of the user's.
    Interactive,
    /// An interactive turn in a Chat a scheduled run is also running in.
    InteractiveBesideRun,
    /// A scheduled run in its own Chat.
    ScheduledChat,
    /// A scheduled run in an existing Chat, enqueued while the run is in
    /// this phase.
    ScheduledInChat(Phase),
    /// An unattended capture with no Chat.
    Unattended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    PlanLimit,
    OutOfCredit,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zone {
    Utc,
    NewYork,
    Berlin,
    Tokyo,
    LordHowe,
    Kathmandu,
}

impl Zone {
    fn tz(self) -> chrono_tz::Tz {
        match self {
            Zone::Utc => chrono_tz::UTC,
            Zone::NewYork => chrono_tz::America::New_York,
            Zone::Berlin => chrono_tz::Europe::Berlin,
            Zone::Tokyo => chrono_tz::Asia::Tokyo,
            Zone::LordHowe => chrono_tz::Australia::Lord_Howe,
            Zone::Kathmandu => chrono_tz::Asia::Kathmandu,
        }
    }
}

/// Every event the state machine knows. A new event is a compile error in
/// [`Sim::apply`] and in [`generate`].
#[derive(Clone, Copy, Debug)]
enum Event {
    Enqueue(Saved),
    StartRun { budget: u32 },
    RunCall,
    ClaimWorld,
    WorldCall,
    LedgerFails,
    FinishRun,
    FailRun(Failure),
    FinishWorld,
    FailWorld(Failure),
    ReviewClaim,
    ReviewFinish,
    InteractiveFailure(Failure),
    Crash,
    Settings { background: u32, world_chat: u32 },
    Midnight,
    Hours(u32),
    MoveZone(Zone),
    TryAgain,
    Continue,
    ReadNow,
}

fn generate(rng: &mut Rng, calls: u32) -> Event {
    // A saved allowance is 0 or at least 32, the largest per-Source limit.
    let limits = [0, 32, calls + 32, 40, 2 * calls + 32];
    match rng.below(21) {
        0 | 1 => Event::Enqueue(match rng.below(8) {
            0 => Saved::ByUser,
            1 => Saved::Interactive,
            2 => Saved::InteractiveBesideRun,
            3 => Saved::ScheduledChat,
            4 => Saved::ScheduledInChat(Phase::Running),
            5 => Saved::ScheduledInChat(Phase::Succeeded),
            6 => Saved::ScheduledInChat(Phase::Failed),
            _ => Saved::Unattended,
        }),
        2 => Event::StartRun {
            budget: rng.pick(&[4, 5, 8, 12]),
        },
        3 => Event::RunCall,
        4 | 5 => Event::ClaimWorld,
        6 | 7 => Event::WorldCall,
        8 => Event::LedgerFails,
        9 => Event::FinishRun,
        10 => Event::FailRun(rng.pick(&[Failure::PlanLimit, Failure::OutOfCredit, Failure::Other])),
        11 => Event::FinishWorld,
        12 => {
            Event::FailWorld(rng.pick(&[Failure::PlanLimit, Failure::OutOfCredit, Failure::Other]))
        }
        13 => match rng.below(2) {
            0 => Event::ReviewClaim,
            _ => Event::ReviewFinish,
        },
        14 => Event::InteractiveFailure(rng.pick(&[Failure::PlanLimit, Failure::OutOfCredit])),
        15 => Event::Crash,
        16 => Event::Settings {
            background: rng.pick(&limits),
            world_chat: rng.pick(&limits),
        },
        17 => match rng.below(3) {
            0 => Event::Midnight,
            _ => Event::Hours(rng.pick(&[1, 5, 11, 23])),
        },
        18 => Event::MoveZone(rng.pick(&[
            Zone::Utc,
            Zone::NewYork,
            Zone::Berlin,
            Zone::Tokyo,
            Zone::LordHowe,
            Zone::Kathmandu,
        ])),
        19 => match rng.below(2) {
            0 => Event::TryAgain,
            _ => Event::Continue,
        },
        _ => Event::ReadNow,
    }
}

// --- The reference ------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// Scheduled runs.
    Run,
    /// Background World jobs and reviews: the background bucket and its share.
    BackgroundWorld,
    /// Chat-origin World jobs and user attempts.
    ChatWorld,
}

#[derive(Clone, Debug)]
struct Spent {
    at_ms: i64,
    class: Class,
    requests: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JobState {
    Queued,
    Running,
    Stopped,
    Done,
}

#[derive(Clone, Debug)]
struct RefJob {
    id: String,
    source: String,
    origin: JobOrigin,
    user: bool,
    remaining: u32,
    state: JobState,
    created: usize,
}

impl RefJob {
    fn class(&self) -> Class {
        if self.user || self.origin == JobOrigin::Chat {
            Class::ChatWorld
        } else {
            Class::BackgroundWorld
        }
    }

    fn claim_rank(&self) -> u8 {
        if self.user {
            0
        } else if self.origin == JobOrigin::Chat {
            1
        } else {
            2
        }
    }
}

#[derive(Clone, Debug)]
struct RefRun {
    id: String,
    budget: u32,
    spent: u32,
}

#[derive(Clone, Debug)]
struct RefHold {
    at_ms: i64,
    reason: HoldReason,
    /// Kept only in memory: a restart forgets it.
    until_restart: bool,
}

#[derive(Clone, Debug, Default)]
struct Reference {
    limits: (u32, u32),
    spent: Vec<Spent>,
    /// Provider holds, each for the local day it happened on.
    holds: Vec<RefHold>,
    jobs: Vec<RefJob>,
    runs: Vec<RefRun>,
    reviews: Vec<String>,
    streak: u8,
}

/// What a bucket has left: limit minus used today minus reserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Left {
    background: u32,
    /// What a background World attempt may use: the smaller of the
    /// background bucket and its World share.
    world: u32,
    chat: u32,
}

impl Reference {
    fn used(&self, day: &dyn Fn(i64) -> Option<chrono::NaiveDate>, now: i64, class: Class) -> u64 {
        let today = day(now);
        self.spent
            .iter()
            .filter(|spent| spent.class == class && day(spent.at_ms) == today)
            .map(|spent| spent.requests)
            .sum()
    }

    fn left(&self, day: &dyn Fn(i64) -> Option<chrono::NaiveDate>, now: i64) -> Left {
        let run_used = self.used(day, now, Class::Run);
        let world_used = self.used(day, now, Class::BackgroundWorld);
        let chat_used = self.used(day, now, Class::ChatWorld);
        // A running run reserves its frozen budget: its requests reach the
        // ledger when its turn ends.
        let run_reserved: u64 = self.runs.iter().map(|run| u64::from(run.budget)).sum();
        let world_reserved: u64 = self
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Running && job.class() == Class::BackgroundWorld)
            .map(|job| u64::from(job.remaining))
            .sum::<u64>()
            + self.reviews.len() as u64;
        let chat_reserved: u64 = self
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Running && job.class() == Class::ChatWorld)
            .map(|job| u64::from(job.remaining))
            .sum();
        let (background, world_chat) = self.limits;
        let rest = |limit: u32, used: u64, reserved: u64| {
            u64::from(limit)
                .saturating_sub(used)
                .saturating_sub(reserved) as u32
        };
        let left = rest(
            background,
            run_used + world_used,
            run_reserved + world_reserved,
        );
        Left {
            background: left,
            world: left.min(rest(background / 2, world_used, world_reserved)),
            chat: rest(world_chat, chat_used, chat_reserved),
        }
    }

    fn held_by_provider(&self, now: i64) -> Option<HoldReason> {
        let today = crate::local_time::date_at_ms(now);
        self.holds
            .iter()
            .filter(|hold| crate::local_time::date_at_ms(hold.at_ms) == today)
            .max_by_key(|hold| hold.at_ms)
            .map(|hold| hold.reason)
    }

    /// Why a queued job cannot start now, or None when it can.
    fn job_hold(&self, job: &RefJob, now: i64) -> Option<HoldReason> {
        if job.user || job.state != JobState::Queued {
            return None;
        }
        let left = self.left(&crate::local_time::date_at_ms, now);
        let remaining = match job.class() {
            Class::ChatWorld => left.chat,
            _ => {
                if let Some(reason) = self.held_by_provider(now) {
                    return Some(reason);
                }
                left.world
            }
        };
        (remaining < job.remaining).then_some(HoldReason::Allowance)
    }

    fn next_claim(&self, now: i64) -> Option<String> {
        if self.jobs.iter().any(|job| job.state == JobState::Running) {
            return None;
        }
        let mut ready: Vec<&RefJob> = self
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Queued && self.job_hold(job, now).is_none())
            .collect();
        ready.sort_by_key(|job| (job.claim_rank(), job.created));
        if self.streak >= 3 {
            if let Some(job) = ready.iter().find(|job| job.user) {
                return Some(job.id.clone());
            }
            if let Some(job) = ready.iter().find(|job| job.claim_rank() == 2) {
                return Some(job.id.clone());
            }
        }
        ready.first().map(|job| job.id.clone())
    }
}

// --- The system under test ---------------------------------------------

struct Sim {
    directory: tempfile::TempDir,
    store: KnowledgeStore,
    settings: GraphProcessingSettings,
    reference: Reference,
    now: i64,
    zone: Zone,
    ledger_fails: bool,
    serial: u32,
    /// Per running World attempt: the worker's ledger account.
    attempts: BTreeMap<String, std::sync::Arc<crate::knowledge::graph_service::WorldAccount>>,
    streak: u8,
}

impl Sim {
    fn open(calls: u32, limits: (u32, u32), now: i64) -> Self {
        let (directory, store) = super::open();
        let mut settings = enabled();
        settings.helper_provider = Some(PROVIDER.into());
        settings.max_llm_calls = calls;
        let sim = Self {
            directory,
            store,
            settings,
            reference: Reference {
                limits,
                ..Reference::default()
            },
            now,
            zone: Zone::Utc,
            ledger_fails: false,
            serial: 0,
            attempts: BTreeMap::new(),
            streak: 0,
        };
        sim.write_limits(limits);
        sim.store
            .transact(|write| {
                store_settings(write.tx, LOUPIST, &sim.settings)?;
                write.tx.execute(
                    "INSERT INTO loupist_profiles (id, updated_at_ms, workspace_root, root_device, root_inode, data_json)
                     VALUES (?1, 1, '/research/oracle', NULL, NULL, '{}')",
                    [LOUPIST],
                )?;
                write.tx.execute(
                    "INSERT INTO chat_sessions (id, loupist_id, updated_at_ms, data_json)
                     VALUES ('chat-user', ?1, 1, '{}')",
                    [LOUPIST],
                )?;
                Ok(())
            })
            .unwrap();
        sim
    }

    fn write_limits(&self, (background, world_chat): (u32, u32)) {
        std::fs::write(
            self.directory.path().join("config.toml"),
            format!(
                "background_daily_requests = {background}\nworld_chat_daily_requests = {world_chat}\n"
            ),
        )
        .unwrap();
        let read = allowance_limits(&self.store.connection);
        assert_eq!(
            (read.background, read.world_chat),
            (background, world_chat),
            "the oracle's limits reach the allowance"
        );
    }

    fn limits(&self) -> Limits {
        Limits {
            background: self.reference.limits.0,
            world_chat: self.reference.limits.1,
        }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.serial += 1;
        format!("{prefix}-{}", self.serial)
    }

    fn state_in(&self, day: impl Fn(i64) -> Option<chrono::NaiveDate>) -> AllowanceState {
        let admission = Admission::prepare(&self.store.connection, Some(LOUPIST)).unwrap();
        let connection = self.store.connection.lock_loupist(LOUPIST).unwrap();
        admission
            .state_with_clock(Some(&connection), self.limits(), self.now, day)
            .unwrap()
    }

    fn state(&self) -> AllowanceState {
        self.state_in(crate::local_time::date_at_ms)
    }

    /// The next ledger write fails once: the store refuses the row.
    fn arm_ledger(&mut self) -> bool {
        if !std::mem::take(&mut self.ledger_fails) {
            return false;
        }
        crate::store::model_usage::write_model_usage(
            &self.store.connection.lock().unwrap(),
            "oracle-create",
            &fixture_row(0),
        )
        .unwrap();
        self.store
            .connection
            .lock()
            .unwrap()
            .execute_batch(
                "DELETE FROM model_usage WHERE id = 'oracle-create';
                 CREATE TRIGGER oracle_ledger_fails BEFORE INSERT ON model_usage
                 BEGIN SELECT RAISE(ABORT, 'ledger write failed'); END;",
            )
            .unwrap();
        true
    }

    fn disarm_ledger(&self, armed: bool) {
        if armed {
            self.store
                .connection
                .lock()
                .unwrap()
                .execute_batch("DROP TRIGGER IF EXISTS oracle_ledger_fails;")
                .unwrap();
        }
    }

    fn usage_row(
        &self,
        kind: ModelUsageKind,
        surface: UsageSurface,
        origin: Option<WorldOrigin>,
        run_id: &str,
        requests: u64,
        failure: Option<Failure>,
    ) -> crate::store::model_usage::ModelUsageRow {
        let mut row = fixture_row(self.now);
        row.loupist_id = Some(LOUPIST.into());
        row.data.kind = kind;
        row.data.surface = surface;
        row.data.world_origin = origin;
        row.data.provider = PROVIDER.into();
        row.data.model = "model".into();
        row.data.run_id = run_id.into();
        row.data.status = ModelUsageStatus::Ok;
        row.data.measures.requests = Some(requests);
        row.data.failure_kind =
            failure.map(|failure| ModelUsageFailure::Chat(chat_failure(failure)));
        row
    }

    fn job(&self, id: &str) -> GraphJobView {
        self.store.graph_job(LOUPIST, id).unwrap()
    }

    fn running_job(&self) -> Option<usize> {
        self.reference
            .jobs
            .iter()
            .position(|job| job.state == JobState::Running)
    }

    // --- Saving a Source ---------------------------------------------

    fn save(&mut self, saved: Saved) -> Result<(), String> {
        let serial = self.next_id("source");
        let request = source_request(LOUPIST, "", format!("Notes {serial}.").as_bytes());
        let chat = match saved {
            Saved::ByUser | Saved::Unattended => None,
            Saved::Interactive | Saved::InteractiveBesideRun => Some("chat-user".to_string()),
            Saved::ScheduledChat => Some(format!("scheduled-chat-{serial}")),
            Saved::ScheduledInChat(_) => Some(format!("chat-existing-{serial}")),
        };
        let message = format!("answer-{serial}");
        self.store
            .transact(|write| {
                if let Some(chat) = chat.as_deref().filter(|chat| *chat != "chat-user") {
                    write.tx.execute(
                        "INSERT INTO chat_sessions (id, loupist_id, updated_at_ms, data_json)
                         VALUES (?1, ?2, 1, '{}')",
                        rusqlite::params![chat, LOUPIST],
                    )?;
                }
                let run = match saved {
                    Saved::ScheduledInChat(phase) => Some((
                        chat.clone().unwrap(),
                        phase,
                        (phase == Phase::Succeeded).then(|| message.clone()),
                    )),
                    Saved::InteractiveBesideRun => {
                        Some(("chat-user".to_string(), Phase::Running, None))
                    }
                    _ => None,
                };
                if let Some((thread, phase, kept)) = run {
                    insert_run(
                        write.tx,
                        &format!("beside-{serial}"),
                        &thread,
                        0,
                        phase,
                        kept,
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let settings = self.settings.clone();
        let now = self.now;
        let unattended = match &chat {
            Some(_) => None,
            None => {
                let mut request = request.clone();
                request.record_id = source_module(&self.store, LOUPIST);
                request.origin_locator = format!("{}/{serial}", request.origin_locator);
                request.authorship = if saved == Saved::ByUser {
                    Authorship::User
                } else {
                    Authorship::Publisher
                };
                Some(self.store.retain_source(request).unwrap())
            }
        };
        let job = self
            .store
            .transact(|write| {
                let retained = match &chat {
                    Some(chat) => write.retain_new_source_with_origin(
                        RetainNewSource {
                            origin: request.origin.clone(),
                            loupist_id: request.loupist_id.clone(),
                            at_ms: request.at_ms,
                            title: request.title.clone(),
                            source_class: request.source_class,
                            origin_locator: format!("{}/{serial}", request.origin_locator),
                            provider_identity: request.provider_identity.clone(),
                            authorship: Authorship::Publisher,
                            coverage: request.coverage,
                            origin_chat_id: Some(chat.clone()),
                            request_ref: None,
                            declared_digest: None,
                            material: request.material.clone(),
                            publication_time: None,
                        },
                        &message,
                        // What the tool executor's Source save stores for
                        // this turn: a scheduled run's turn is unattended.
                        crate::commands::source_save_origin(match saved {
                            Saved::ScheduledChat | Saved::ScheduledInChat(_) => {
                                Some("scheduled-run")
                            }
                            _ => None,
                        }),
                        None,
                    )?,
                    None => unattended.clone().unwrap(),
                };
                write.enqueue_source_world(LOUPIST, &retained, &settings, now)
            })
            .unwrap()
            .expect("a queued job");
        // The interactive turn's neighbour run ends; it reserved nothing.
        if saved == Saved::InteractiveBesideRun {
            self.store
                .transact(|write| {
                    write.tx.execute(
                        "UPDATE scheduled_task_runs SET status = 'succeeded', finished_at_ms = started_at_ms
                         WHERE run_id = ?1",
                        [format!("beside-{serial}")],
                    )?;
                    Ok(())
                })
                .unwrap();
        }
        let expected = match saved {
            Saved::ByUser | Saved::Interactive | Saved::InteractiveBesideRun => JobOrigin::Chat,
            Saved::ScheduledChat | Saved::ScheduledInChat(_) | Saved::Unattended => {
                JobOrigin::Background
            }
        };
        let created = self.reference.jobs.len();
        self.reference.jobs.push(RefJob {
            id: job.job_id.clone(),
            source: job
                .document
                .source_binding
                .as_ref()
                .unwrap()
                .source_record_id
                .clone(),
            origin: expected,
            user: false,
            remaining: job.document.extraction.max_llm_calls,
            state: JobState::Queued,
            created,
        });
        if job.document.origin != expected {
            return Err(format!(
                "origin: a Source saved {saved:?} queued as {:?}, the admitted turn was {expected:?}",
                job.document.origin
            ));
        }
        Ok(())
    }

    // --- Scheduled runs ----------------------------------------------

    fn start_run(&mut self, budget: u32) -> Result<(), String> {
        let left = self
            .reference
            .left(&crate::local_time::date_at_ms, self.now);
        let expected = if self.reference.held_by_provider(self.now).is_some() {
            None
        } else {
            Some(budget.min(left.background)).filter(|frozen| *frozen >= 5)
        };
        let id = self.next_id("run");
        let state = self.state();
        let admitted = admit_run(&state, Some(PROVIDER), budget).ok();
        if admitted != expected {
            return Err(format!(
                "run admission: the allowance froze {admitted:?}, the reference {expected:?}"
            ));
        }
        if let Some(frozen) = admitted {
            self.store
                .transact(|write| {
                    write.tx.execute(
                        "INSERT INTO chat_sessions (id, loupist_id, updated_at_ms, data_json)
                         VALUES (?1, ?2, 1, '{}')",
                        rusqlite::params![format!("scheduled-chat-{id}"), LOUPIST],
                    )?;
                    insert_run(
                        write.tx,
                        &id,
                        &format!("scheduled-chat-{id}"),
                        frozen,
                        Phase::Running,
                        None,
                    )?;
                    Ok(())
                })
                .unwrap();
            self.reference.runs.push(RefRun {
                id,
                budget: frozen,
                spent: 0,
            });
        }
        Ok(())
    }

    /// A run's turn ends: the Chat turn closes its record
    /// (`RunLog::end_turn`) and writes its usage row (`RunLog::record`, which
    /// logs a failed write and goes on), then the scheduler finishes the row.
    fn end_run(&mut self, failure: Option<Failure>) {
        let Some(run) = self.reference.runs.pop() else {
            return;
        };
        let armed = self.arm_ledger();
        let recorded = self
            .turn_log(
                &run.id,
                UsageSurface::Scheduled,
                u64::from(run.spent),
                failure,
            )
            .record_usage(&self.store.connection)
            .is_ok();
        self.disarm_ledger(armed);
        self.store
            .transact(|write| {
                // `finish_run` stores the error the scheduler maps the
                // turn's failure to.
                let error = failure.map(|failure| {
                    crate::scheduled_tasks::failed_run_error(chat_failure(failure), "failed")
                });
                write.tx.execute(
                    "UPDATE scheduled_task_runs SET status = ?2, finished_at_ms = ?3,
                        data_json = CASE WHEN ?4 IS NULL THEN data_json
                                         ELSE json_set(data_json, '$.error', ?4) END
                     WHERE run_id = ?1",
                    rusqlite::params![
                        run.id,
                        if failure.is_some() {
                            "failed"
                        } else {
                            "succeeded"
                        },
                        self.now,
                        error
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        // The reference: what the run spent, or, when its row never reached
        // the ledger, its frozen budget: the most it can have spent, and all
        // the host can know without the row.
        self.reference.spent.push(Spent {
            at_ms: self.now,
            class: Class::Run,
            requests: u64::from(if recorded { run.spent } else { run.budget }),
        });
        // A plan limit holds the provider whether or not its row was stored.
        if let Some(Failure::PlanLimit | Failure::OutOfCredit) = failure {
            self.reference.holds.push(RefHold {
                at_ms: self.now,
                reason: hold_reason(failure.unwrap()),
                until_restart: false,
            });
        }
    }

    /// A Chat turn's record as the host closes it.
    fn turn_log(
        &self,
        run_id: &str,
        surface: UsageSurface,
        requests: u64,
        failure: Option<Failure>,
    ) -> crate::runs::log::RunLog {
        use crate::runs::log::{RunLog, TurnEnd};
        let mut log = RunLog::new("chat", LOUPIST, run_id, self.now);
        log.surface = Some(surface);
        log.model = Some(format!("{PROVIDER}/model"));
        log.requests = Some(requests);
        log.end_turn(match failure {
            None => TurnEnd::Completed {
                model: None,
                memory_save_skip: None,
            },
            Some(failure) => TurnEnd::Failed(chat_failure(failure)),
        });
        log.duration_ms = 0;
        log
    }

    // --- World attempts ----------------------------------------------

    /// The graph worker's claim pass (`select_world_job`): runnable jobs,
    /// those the allowance admits, `pick_claim`, then `claim_running`.
    fn claim_world(&mut self) -> Result<(), String> {
        let expected = self.reference.next_claim(self.now);
        let state = self.state();
        let picked = self
            .store
            .transact(|write| {
                let busy = list_jobs_with_status(write.tx, LOUPIST, JOB_RUNNING)?
                    .iter()
                    .any(|job| job.document.episode_kind == EPISODE_SPAN);
                if busy {
                    return Ok(None);
                }
                let mut ready = Vec::new();
                for job in queued_runnable(write.tx)? {
                    let calls = dispatch_calls(write.tx, &job)?;
                    if !allowance_refuses(&state, &job, calls, Some(PROVIDER)) {
                        ready.push(job);
                    }
                }
                Ok(pick_claim(&ready, self.streak).map(|job| job.job_id.clone()))
            })
            .unwrap();
        if picked != expected {
            return Err(format!(
                "claim: the worker picked {picked:?}, the reference {expected:?}"
            ));
        }
        let Some(id) = picked else {
            return Ok(());
        };
        let settings = self.settings.clone();
        let now = self.now;
        let admission = Admission::prepare(&self.store.connection, Some(LOUPIST)).unwrap();
        let claimed = self
            .store
            .transact(|write| {
                let state = admission
                    .state(Some(write.tx), self.limits(), now)
                    .map_err(|_| KnowledgeError::Invalid("allowance"))?;
                claim_running(write.tx, &settings, &id, now, Some(&state))
            })
            .unwrap();
        drop(admission);
        let GraphJobClaim::Acquired(job) = claimed else {
            return Err(format!("claim: {id} was picked but not acquired"));
        };
        let started = self.now;
        let origin = super::allowance_origin(&job.document);
        self.attempts.insert(
            id.clone(),
            std::sync::Arc::new(crate::knowledge::graph_service::WorldAccount::new(
                self.store.connection.clone(),
                format!("world:{id}:{started}"),
                LOUPIST.into(),
                started,
                origin,
                PROVIDER.into(),
                "model".into(),
                String::new(),
            )),
        );
        self.streak = note_claim_streak(self.streak, &job.document);
        let reference = &mut self.reference;
        let index = reference.jobs.iter().position(|job| job.id == id).unwrap();
        reference.jobs[index].state = JobState::Running;
        reference.streak = if reference.jobs[index].claim_rank() == 2 {
            0
        } else {
            reference.streak.saturating_add(1)
        };
        Ok(())
    }

    /// One paid World call, as `drive_job_with_account` makes it: the
    /// worker's account counts it, the outbox reserves it, and the worker's
    /// inference sends it only when the account admits it; a call not sent is
    /// refunded on both.
    fn world_call(&mut self) -> Result<(), String> {
        let Some(index) = self.running_job() else {
            return Ok(());
        };
        if self.reference.jobs[index].remaining == 0 {
            return Ok(());
        }
        let id = self.reference.jobs[index].id.clone();
        let class = self.reference.jobs[index].class();
        let account = self.attempts.get(&id).cloned().unwrap();
        let armed = self.arm_ledger();
        let counted = account.account(1, self.now).is_ok();
        self.disarm_ledger(armed);
        let attempt = self.job(&id).document.attempt_id.unwrap();
        let reserved = self
            .store
            .transact(|write| reserve_model_call(write.tx, &id, &attempt))
            .is_ok();
        let dispatched = reserved && account.admit_call();
        if reserved && !dispatched {
            self.store
                .transact(|write| refund_model_call(write.tx, &id, &attempt))
                .unwrap();
        }
        if !dispatched {
            let _ = account.account(-1, self.now);
        }
        if dispatched {
            self.reference.jobs[index].remaining -= 1;
            self.reference.spent.push(Spent {
                at_ms: self.now,
                class,
                requests: 1,
            });
            if !counted {
                return Err(
                    "a World call was dispatched although its ledger row was not stored".into(),
                );
            }
        }
        Ok(())
    }

    /// The attempt ends: the job's status, then the worker's final usage row
    /// (`RunLog::record`) under the attempt's ledger id.
    fn end_world(&mut self, failure: Option<Failure>) {
        let Some(index) = self.running_job() else {
            return;
        };
        let id = self.reference.jobs[index].id.clone();
        let class = self.reference.jobs[index].class();
        let job = self.job(&id);
        let mut document = job.document.clone();
        clear_lease(&mut document);
        let next = match failure {
            None => JOB_COMPLETED,
            Some(Failure::Other) => {
                document.failure = Some(JobFailure {
                    kind: JobFailureKind::InvalidJson,
                    at_ms: self.now,
                });
                JOB_FAILED_EXTRACTION
            }
            // The provider refused the call: nothing was sent, the job queues again.
            Some(_) => JOB_QUEUED,
        };
        self.store
            .transact(|write| {
                write_status(write.tx, &id, JOB_RUNNING, next, &document)?;
                Ok(())
            })
            .unwrap();
        let account = self.attempts.remove(&id).unwrap();
        // The worker's final row carries what `final_requests` leaves it.
        let (row_id, sent) = (
            account.id.clone(),
            account.final_requests(account.requests(), self.now),
        );
        let row = self.usage_row(
            ModelUsageKind::World,
            UsageSurface::World,
            Some(if class == Class::ChatWorld {
                WorldOrigin::Chat
            } else {
                WorldOrigin::Background
            }),
            &row_id,
            sent,
            failure,
        );
        let armed = self.arm_ledger();
        let recorded = self.store.connection.put_model_usage(&row_id, &row).is_ok();
        self.disarm_ledger(armed);
        let job = &mut self.reference.jobs[index];
        job.state = match next {
            JOB_COMPLETED => JobState::Done,
            JOB_QUEUED => JobState::Queued,
            _ => JobState::Stopped,
        };
        // Nothing stored records a World attempt's provider hold but its final
        // row: one whose row was lost lasts until restart.
        if let Some(failure @ (Failure::PlanLimit | Failure::OutOfCredit)) = failure {
            self.reference.holds.push(RefHold {
                at_ms: self.now,
                reason: hold_reason(failure),
                until_restart: !recorded,
            });
        }
    }

    // --- Reviews -----------------------------------------------------

    /// A schema or method review's claim (`schema_writer::claim`): one
    /// background call admitted and reserved in the claiming transaction.
    fn review_claim(&mut self) -> Result<(), String> {
        let left = self
            .reference
            .left(&crate::local_time::date_at_ms, self.now);
        let expected = self.reference.held_by_provider(self.now).is_none() && left.world >= 1;
        let id = self.next_id("wsr");
        let now = self.now;
        let admission = Admission::prepare(&self.store.connection, Some(LOUPIST)).unwrap();
        let admitted = self
            .store
            .transact(|write| {
                let state = admission
                    .state(Some(write.tx), self.limits(), now)
                    .map_err(|_| KnowledgeError::Invalid("allowance"))?;
                let admitted =
                    admit_world_attempt(&state, WorldOrigin::Background, 1, Some(PROVIDER)).is_ok();
                if admitted {
                    reserve_background_call(write.tx, LOUPIST, &id, 1, now)?;
                }
                Ok(admitted)
            })
            .unwrap();
        drop(admission);
        if admitted != expected {
            return Err(format!(
                "review admission: the allowance said {admitted}, the reference {expected}"
            ));
        }
        if admitted {
            self.reference.reviews.push(id);
        }
        Ok(())
    }

    /// The review's reply: `record_sent_review`, then `finish` releases the
    /// reservation only when the usage row was stored.
    fn review_finish(&mut self) {
        let Some(id) = self.reference.reviews.first().cloned() else {
            return;
        };
        let mut log = crate::runs::log::RunLog::new("schema", LOUPIST, &id, self.now);
        log.model = Some(format!("{PROVIDER}/model"));
        let reply = InferenceReply {
            disposition: InferenceDisposition::Completed,
            body: serde_json::json!({"status": "ok", "json_object": {},
                "usage": {"input_tokens": 1, "output_tokens": 1, "certainty": "accounted"}}),
        };
        let armed = self.arm_ledger();
        let recorded = crate::knowledge::graph_service::record_sent_review(
            &self.store.connection,
            &mut log,
            &reply,
        );
        self.disarm_ledger(armed);
        if recorded.is_ok() {
            self.store
                .transact(|write| release_background_call(write.tx, &id))
                .unwrap();
            self.reference.reviews.retain(|review| *review != id);
            self.reference.spent.push(Spent {
                at_ms: self.now,
                class: Class::BackgroundWorld,
                requests: 1,
            });
        }
    }

    // --- Restart ---------------------------------------------------

    /// The app crashed: startup recovers World attempts and review
    /// reservations (`recover_with_usage`) and marks unfinished runs
    /// interrupted, as `scheduled_tasks_recover_running` does.
    fn crash(&mut self) {
        let armed = self.arm_ledger();
        let _ = self
            .store
            .transact(|write| recover_with_usage(&self.store.connection, write.tx));
        self.disarm_ledger(armed);
        self.store
            .transact(|write| {
                write.tx.execute(
                    "UPDATE scheduled_task_runs SET status = 'interrupted', finished_at_ms = ?1
                     WHERE status = 'running'",
                    [self.now],
                )?;
                Ok(())
            })
            .unwrap();
        self.attempts.clear();
        // The reference: dispatched requests stay counted; a run's frozen
        // budget is all a restart can know it spent; a review reservation the
        // ledger never saw is counted, unless the ledger still fails.
        let now = self.now;
        for run in std::mem::take(&mut self.reference.runs) {
            self.reference.spent.push(Spent {
                at_ms: now,
                class: Class::Run,
                requests: u64::from(run.budget),
            });
        }
        if !armed {
            for _ in std::mem::take(&mut self.reference.reviews) {
                self.reference.spent.push(Spent {
                    at_ms: crate::local_time::now().timestamp_millis(),
                    class: Class::BackgroundWorld,
                    requests: 1,
                });
            }
        }
        for job in &mut self.reference.jobs {
            if job.state == JobState::Running {
                job.state = JobState::Queued;
            }
        }
        self.reference.holds.retain(|hold| !hold.until_restart);
        self.store.connection.forget_lost_holds_on_restart();
        self.streak = 0;
        self.reference.streak = 0;
    }

    // --- The user's actions --------------------------------------------

    /// Try again (or Continue all) on a stopped Source: a fresh allowance,
    /// marked as the user's attempt.
    fn try_again(&mut self, continue_all: bool) -> Result<(), String> {
        let Some(index) = self
            .reference
            .jobs
            .iter()
            .position(|job| job.state == JobState::Stopped)
        else {
            return Ok(());
        };
        let source = self.reference.jobs[index].source.clone();
        let outcome = if continue_all {
            let settings = self.settings.clone();
            self.store
                .transact(|write| {
                    retry_sources_processing(
                        write.tx,
                        &settings,
                        LOUPIST,
                        std::slice::from_ref(&source),
                        settings.max_llm_calls,
                    )
                })
                .map(|retried| retried.continued == 1)
                .unwrap_or(false)
        } else {
            retry_admitted_source(&self.store.connection, LOUPIST, &source, self.now).is_ok()
        };
        if !outcome {
            return Err(format!(
                "Try again on a stopped Source was refused ({continue_all})"
            ));
        }
        let job = &mut self.reference.jobs[index];
        job.user = true;
        job.origin = JobOrigin::Chat;
        job.remaining = self.settings.max_llm_calls;
        job.state = JobState::Queued;
        Ok(())
    }

    /// Read now on a queued Source: a held job becomes the user's attempt in
    /// place and keeps its calls; one the allowance would start is refused.
    fn read_now(&mut self) -> Result<(), String> {
        let Some(index) = self
            .reference
            .jobs
            .iter()
            .position(|job| job.state == JobState::Queued)
        else {
            return Ok(());
        };
        let held = self
            .reference
            .job_hold(&self.reference.jobs[index], self.now)
            .is_some();
        let source = self.reference.jobs[index].source.clone();
        let outcome = retry_admitted_source(&self.store.connection, LOUPIST, &source, self.now);
        if outcome.is_ok() != held {
            return Err(format!(
                "Read now: held {held}, the host answered {:?}",
                outcome.map(|job| job.status)
            ));
        }
        if held {
            let job = &mut self.reference.jobs[index];
            job.user = true;
            job.origin = JobOrigin::Chat;
        }
        Ok(())
    }

    // --- One event and the invariants --------------------------------

    fn apply(&mut self, event: Event) -> Result<(), String> {
        // Each event happens a moment after the one before it.
        self.now += 1;
        match event {
            Event::Enqueue(saved) => self.save(saved)?,
            Event::StartRun { budget } => self.start_run(budget)?,
            Event::RunCall => {
                if let Some(run) = self
                    .reference
                    .runs
                    .iter_mut()
                    .find(|run| run.spent < run.budget)
                {
                    run.spent += 1;
                }
            }
            Event::ClaimWorld => self.claim_world()?,
            Event::WorldCall => self.world_call()?,
            Event::LedgerFails => self.ledger_fails = true,
            Event::FinishRun => self.end_run(None),
            Event::FailRun(failure) => self.end_run(Some(failure)),
            Event::FinishWorld => self.end_world(None),
            Event::FailWorld(failure) => self.end_world(Some(failure)),
            Event::ReviewClaim => self.review_claim()?,
            Event::ReviewFinish => self.review_finish(),
            Event::InteractiveFailure(failure) => {
                let turn = self.next_id("turn");
                assert!(self
                    .turn_log(&turn, UsageSurface::Research, 1, Some(failure))
                    .record_usage(&self.store.connection)
                    .unwrap());
                // Master plan 4.8: an interactive plan-limit or out-of-credit
                // failure on the provider holds background work today.
                self.reference.holds.push(RefHold {
                    at_ms: self.now,
                    reason: hold_reason(failure),
                    until_restart: false,
                });
            }
            Event::Crash => self.crash(),
            Event::Settings {
                background,
                world_chat,
            } => {
                self.reference.limits = (background, world_chat);
                self.write_limits((background, world_chat));
            }
            Event::Midnight => {
                let next = crate::knowledge::graph_service::next_local_midnight_ms(self.now)
                    .expect("next midnight");
                self.now = next + 60_000;
            }
            Event::Hours(hours) => self.now += i64::from(hours) * 3_600_000,
            Event::MoveZone(zone) => self.zone = zone,
            Event::TryAgain => self.try_again(false)?,
            Event::Continue => self.try_again(true)?,
            Event::ReadNow => self.read_now()?,
        }
        self.check()
    }

    fn check(&self) -> Result<(), String> {
        let expected = self
            .reference
            .left(&crate::local_time::date_at_ms, self.now);
        let actual = left_of(&self.state());
        if actual != expected {
            return Err(format!(
                "left: the allowance reports {actual:?}, the reference {expected:?}"
            ));
        }
        let zone = self.zone.tz();
        let day = |at: i64| local_date_in_zone(at, zone);
        let expected = self.reference.left(&day, self.now);
        let actual = left_of(&self.state_in(day));
        if actual != expected {
            return Err(format!(
                "left in {:?}: the allowance reports {actual:?}, the reference {expected:?}",
                self.zone
            ));
        }
        let state = self.state();
        let provider_held = state
            .provider_holds
            .get(PROVIDER)
            .map(|hold| provider(hold.reason));
        if provider_held != self.reference.held_by_provider(self.now).map(provider) {
            return Err(format!(
                "provider hold: the allowance has {provider_held:?}, the reference {:?}",
                self.reference.held_by_provider(self.now)
            ));
        }
        let holds = queued_holds(&self.store.connection, LOUPIST, self.now).unwrap();
        for job in &self.reference.jobs {
            let stored = self.job(&job.id);
            if (stored.document.origin, stored.document.user_attempt) != (job.origin, job.user) {
                return Err(format!(
                    "job {}: stored as {:?}/user {}, the reference {:?}/user {}",
                    job.id,
                    stored.document.origin,
                    stored.document.user_attempt,
                    job.origin,
                    job.user
                ));
            }
            if job.state != JobState::Queued {
                continue;
            }
            let held = holds.get(&job.id).map(|hold| match hold.cause {
                JobHoldCause::Allowance => HoldReason::Allowance,
                JobHoldCause::PlanLimit | JobHoldCause::OutOfCredit => HoldReason::PlanLimit,
            });
            if held != self.reference.job_hold(job, self.now).map(provider) {
                return Err(format!(
                    "hold of {}: the job view says {held:?}, the reference {:?}",
                    job.id,
                    self.reference.job_hold(job, self.now)
                ));
            }
        }
        Ok(())
    }
}

/// Plan limit and out of credit are both a provider hold; which one a day
/// shows when both happened is the later row's.
fn provider(reason: HoldReason) -> HoldReason {
    match reason {
        HoldReason::Allowance => HoldReason::Allowance,
        _ => HoldReason::PlanLimit,
    }
}

fn chat_failure(failure: Failure) -> ChatFailureKind {
    match failure {
        Failure::PlanLimit => ChatFailureKind::PlanLimit,
        Failure::OutOfCredit => ChatFailureKind::OutOfCredit,
        Failure::Other => ChatFailureKind::Other,
    }
}

fn hold_reason(failure: Failure) -> HoldReason {
    match failure {
        Failure::OutOfCredit => HoldReason::OutOfCredit,
        _ => HoldReason::PlanLimit,
    }
}

/// What the state leaves in each bucket, read through admission alone.
fn left_of(state: &AllowanceState) -> Left {
    let largest = |origin: WorldOrigin| -> u32 {
        let (mut low, mut high) = (0u32, 100_000u32);
        while low < high {
            let middle = (low + high).div_ceil(2);
            if admit_world_attempt(state, origin, middle, None).is_ok() {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        low
    };
    Left {
        background: state.background_remaining(),
        world: largest(WorldOrigin::Background),
        chat: largest(WorldOrigin::Chat),
    }
}

/// A scheduled run row in `thread`, with its task. `kept` is the reply
/// message a succeeded run keeps.
fn insert_run(
    tx: &rusqlite::Transaction<'_>,
    id: &str,
    thread: &str,
    budget: u32,
    phase: Phase,
    kept: Option<String>,
) -> KnowledgeResult<()> {
    tx.execute(
        "INSERT INTO scheduled_tasks (
            id, loupist_id, revision, status, next_run_at_ms, target_thread_id, updated_at_ms, data_json
         ) VALUES (?1, ?2, 1, 'paused', NULL, ?3, 1, '{}')",
        rusqlite::params![format!("task-{id}"), LOUPIST, thread],
    )?;
    // As `claim_in` stores it: the frozen budget and the admitted provider.
    let mut data = serde_json::json!({
        "kind": "custom",
        "budget": {"requests": budget},
        "provider": PROVIDER,
    });
    if let Some(message) = kept {
        data["message_id"] = message.into();
    }
    let (status, finished) = match phase {
        Phase::Running => ("running", None),
        Phase::Succeeded => ("succeeded", Some(2)),
        Phase::Failed => ("failed", Some(2)),
    };
    tx.execute(
        "INSERT INTO scheduled_task_runs (
            id, task_id, loupist_id, task_revision, thread_id, run_id, status,
            scheduled_for_ms, started_at_ms, finished_at_ms, data_json, task_snapshot_json
         ) VALUES (?1, ?2, ?3, 1, ?4, ?1, ?5, 1, 1, ?6, ?7, '{}')",
        rusqlite::params![
            id,
            format!("task-{id}"),
            LOUPIST,
            thread,
            status,
            finished,
            data.to_string()
        ],
    )?;
    Ok(())
}

// --- Running seeds -------------------------------------------------------

/// The events of one seed, run on their own thread so a lock held across a
/// wait fails the seed instead of the suite.
fn run_seed(seed: u64) -> Result<(), String> {
    let (sender, receiver) = mpsc::channel();
    let progress = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let trace = progress.clone();
    std::thread::spawn(move || {
        let mut rng = Rng::new(seed);
        let calls = rng.pick(&[8, 16, 32]);
        let limits = (
            rng.pick(&[0, 40, 2 * calls + 32]),
            rng.pick(&[0, 32, calls + 32]),
        );
        // Near the US and European daylight-saving changes, and today.
        let starts = [
            crate::local_time::now().timestamp_millis(),
            chrono::DateTime::parse_from_rfc3339("2027-03-14T04:30:00Z")
                .unwrap()
                .timestamp_millis(),
            chrono::DateTime::parse_from_rfc3339("2027-10-30T22:30:00Z")
                .unwrap()
                .timestamp_millis(),
        ];
        let mut sim = Sim::open(calls, limits, rng.pick(&starts));
        let mut outcome = sim.check();
        for _ in 0..24 {
            if outcome.is_err() {
                break;
            }
            let event = generate(&mut rng, calls);
            trace.lock().unwrap().push(format!("{event:?}"));
            outcome = sim.apply(event);
        }
        let _ = sender.send(outcome);
    });
    let outcome = match receiver.recv_timeout(Duration::from_secs(60)) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            Err("no progress for 60 s: a lock is held across a wait".into())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("panicked: see the output above".into()),
    };
    outcome.map_err(|failure| {
        let events = progress.lock().unwrap();
        format!("{failure}\n    after {}", events.join(", "))
    })
}

#[test]
fn oracle_allowance_state_machine() {
    let mut failures = Vec::new();
    let mut kinds = BTreeSet::new();
    for seed in 0..seed_count() {
        if let Err(case) = run_seed(seed) {
            let first = case.lines().next().unwrap_or("");
            kinds.insert(first.split(':').next().unwrap_or("").to_string());
            failures.push(format!("seed {seed}: {case}"));
        }
    }
    assert!(
        failures.is_empty(),
        "allowance: {} failing seeds; invariants {kinds:?}\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// --- The open findings as named tests -------------------------------------

/// Runs `body` on its own thread: a lock held across a wait fails the test
/// after `seconds` instead of hanging the suite.
fn within(seconds: u64, body: impl FnOnce() + Send + 'static) {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        body();
        let _ = sender.send(());
    });
    match receiver.recv_timeout(Duration::from_secs(seconds)) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("no progress for {seconds} s: a lock is held across a wait")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("the test body panicked"),
    }
}

fn sim(calls: u32, limits: (u32, u32)) -> Sim {
    Sim::open(calls, limits, crate::local_time::now().timestamp_millis())
}

/// (a) World calls for a Source a scheduled run saved count as background,
/// from the origin of the admitted turn: in the run's own Chat, in an
/// existing Chat while the run is running, after it succeeded, and after it
/// failed. A Source an interactive turn saved in a Chat a scheduled run is
/// also running in stays the user's.
#[test]
fn a_source_keeps_the_origin_of_the_turn_that_saved_it() {
    let mut failures = Vec::new();
    for saved in [
        Saved::ScheduledChat,
        Saved::ScheduledInChat(Phase::Running),
        Saved::ScheduledInChat(Phase::Succeeded),
        Saved::ScheduledInChat(Phase::Failed),
        Saved::InteractiveBesideRun,
        Saved::Interactive,
        Saved::ByUser,
        Saved::Unattended,
    ] {
        let mut sim = sim(8, (40, 32));
        if let Err(failure) = sim.save(saved) {
            failures.push(failure);
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A World attempt running across local midnight charges each call to the
/// local day it was made on, read in each of several time zones. One ledger
/// row dated by the attempt's last call put both calls on the second day.
#[test]
fn a_world_attempt_across_midnight_charges_each_call_to_its_own_day() {
    use chrono::TimeZone;
    for zone in [
        Zone::Utc,
        Zone::NewYork,
        Zone::Tokyo,
        Zone::Kathmandu,
        Zone::LordHowe,
    ] {
        let tz = zone.tz();
        let midnight = tz
            .with_ymd_and_hms(2027, 3, 15, 0, 0, 0)
            .single()
            .unwrap()
            .timestamp_millis();
        let minute = 60_000;
        let mut sim = Sim::open(8, (40, 32), midnight - 10 * minute);
        let account = crate::knowledge::graph_service::WorldAccount::new(
            sim.store.connection.clone(),
            "world:midnight:1".into(),
            LOUPIST.into(),
            sim.now,
            WorldOrigin::Background,
            PROVIDER.into(),
            "model".into(),
            String::new(),
        );
        account.account(1, midnight - 5 * minute).unwrap();
        account.account(1, midnight + 5 * minute).unwrap();
        sim.now = midnight + 6 * minute;
        let sent = account.final_requests(2, sim.now);
        let row = sim.usage_row(
            ModelUsageKind::World,
            UsageSurface::World,
            Some(WorldOrigin::Background),
            &account.id,
            sent,
            None,
        );
        sim.store
            .connection
            .put_model_usage(&account.id, &row)
            .unwrap();
        let day = |at: i64| local_date_in_zone(at, tz);
        for (at, label) in [
            (midnight - minute, "before"),
            (midnight + 7 * minute, "after"),
        ] {
            sim.now = at;
            let left = left_of(&sim.state_in(day));
            assert_eq!(
                (left.background, left.world),
                (39, 19),
                "{zone:?}, {label} midnight: one call on each day"
            );
        }
    }
}

/// (b) A review reserves its call at claim time. A ledger write error keeps
/// the reservation; a restart with the ledger still failing keeps it; the
/// next restart counts it and only then releases it.
#[test]
fn a_review_reservation_ends_only_with_its_recorded_usage() {
    let mut sim = sim(8, (40, 32));
    sim.review_claim().unwrap();
    sim.check().unwrap();
    sim.ledger_fails = true;
    sim.review_finish();
    sim.check().unwrap();
    assert_eq!(sim.reference.reviews.len(), 1, "the reservation is kept");
    sim.ledger_fails = true;
    sim.crash();
    sim.check().unwrap();
    sim.crash();
    sim.check().unwrap();
    assert!(sim.reference.reviews.is_empty());
}

/// (b) The same rule for every class: a run's requests and a World call stay
/// counted when the ledger refuses their row or the app crashes.
#[test]
fn a_ledger_error_or_a_crash_never_frees_dispatched_requests() {
    let mut failures = Vec::new();
    // A run's usage row fails to store as its turn ends.
    let mut run = sim(8, (40, 32));
    run.start_run(8).unwrap();
    run.apply(Event::RunCall).unwrap();
    run.apply(Event::RunCall).unwrap();
    run.ledger_fails = true;
    if let Err(failure) = run.apply(Event::FinishRun) {
        failures.push(format!("run, ledger error: {failure}"));
    }
    // The app crashes while a run is running.
    let mut crashed = sim(8, (40, 32));
    crashed.start_run(8).unwrap();
    crashed.apply(Event::RunCall).unwrap();
    if let Err(failure) = crashed.apply(Event::Crash) {
        failures.push(format!("run, crash: {failure}"));
    }
    // A World call's ledger row fails to store.
    let mut world = sim(8, (40, 32));
    world.save(Saved::Unattended).unwrap();
    world.apply(Event::ClaimWorld).unwrap();
    world.ledger_fails = true;
    if let Err(failure) = world.apply(Event::WorldCall) {
        failures.push(format!("World call, ledger error: {failure}"));
    }
    // A failed run's usage row carries the plan limit that holds background work.
    let mut held = sim(8, (40, 32));
    held.start_run(8).unwrap();
    held.ledger_fails = true;
    if let Err(failure) = held.apply(Event::FailRun(Failure::PlanLimit)) {
        failures.push(format!("plan limit, ledger error: {failure}"));
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// (c) A successful single-Source retry returns without taking the
/// process-wide admission lock it holds a second time.
#[test]
fn a_successful_source_retry_returns_and_leaves_admission_free() {
    within(30, || {
        let mut sim = sim(8, (40, 32));
        sim.save(Saved::Interactive).unwrap();
        sim.apply(Event::ClaimWorld).unwrap();
        sim.apply(Event::WorldCall).unwrap();
        sim.apply(Event::FailWorld(Failure::Other)).unwrap();
        sim.apply(Event::TryAgain).unwrap();
        drop(Admission::prepare(&sim.store.connection, Some(LOUPIST)).unwrap());
        sim.apply(Event::ClaimWorld).unwrap();
    });
}

/// (d) Read now on a Source the day's allowance holds promotes the held job
/// in place with the calls it has left, returns, and the worker can claim it.
/// A hold lookup that fails after the commit still returns the saved job.
#[test]
fn read_now_promotes_a_held_job_in_place() {
    within(30, || {
        let mut sim = sim(8, (40, 0));
        sim.save(Saved::Interactive).unwrap();
        let before = sim.job(&sim.reference.jobs[0].id);
        assert!(sim
            .reference
            .job_hold(&sim.reference.jobs[0], sim.now)
            .is_some());
        sim.apply(Event::ReadNow).unwrap();
        let after = sim.job(&before.job_id);
        assert_eq!(after.status, JOB_QUEUED);
        assert!(after.document.user_attempt);
        assert_eq!(
            after.document.extraction.max_llm_calls,
            before.document.extraction.max_llm_calls
        );
        sim.apply(Event::ClaimWorld).unwrap();
        assert_eq!(sim.job(&before.job_id).status, JOB_RUNNING);
        let returned = committed_retry(after.clone(), Err(KnowledgeError::Invalid("lookup")));
        assert_eq!(returned.job_id, after.job_id);
    });
}

/// (e) A Source of several chunks whose first chunk failed on malformed
/// output is one failed Source in the change feed, whatever its later
/// chunk's state.
#[test]
fn a_source_whose_first_chunk_failed_is_one_failed_source() {
    use crate::knowledge::world_changes::{collect, finish, Axis, FeedQuery, Since};
    let mut failures = Vec::new();
    for later in [
        JOB_QUEUED,
        JOB_RUNNING,
        JOB_PAUSED,
        JOB_COMPLETED,
        JOB_CANCELLED,
        JOB_FAILED_BEFORE,
        JOB_RECONCILE,
        JOB_RECONCILED,
    ] {
        let (_directory, store) = super::open();
        let source = source_module(&store, "owner");
        let (span, _) = retain(&store, "owner", &source, None, b"Two chunks of notes.");
        let (world_id, _) = world(&store, "owner");
        let body = store
            .transact(|write| {
                let payload = |chunk: u32| {
                    serde_json::json!({
                        "sourceBinding": {"sourceRecordId": source, "spanId": span},
                        "chunkIndex": chunk,
                        "chunkCount": 2,
                    })
                    .to_string()
                };
                write.tx.execute(
                    "INSERT INTO knowledge_graph_outbox (
                        job_id, loupist_id, world_record_id, world_revision_id, status,
                        payload_json, payload_digest, created_at_ms
                     ) VALUES
                     ('job-first', 'owner', ?1, 'rev', 'failed_extraction', ?2, 'd1', 1),
                     ('job-later', 'owner', ?1, 'rev', ?3, ?4, 'd2', 2)",
                    rusqlite::params![world_id, payload(0), later, payload(1)],
                )?;
                Ok(finish(collect(
                    write.tx,
                    &FeedQuery {
                        loupist_id: "owner".into(),
                        since: Since::Millis {
                            ms: 0,
                            inclusive: true,
                        },
                        axis: Axis::Recorded,
                        axis_explicit: true,
                        about: Vec::new(),
                        kinds: None,
                        limit: 30,
                        first_run: false,
                        cutoff: None,
                    },
                )?)
                .body)
            })
            .unwrap();
        if body["failed"] != 1 || body["pending"] != 0 {
            failures.push(format!(
                "later chunk {later}: failed {}, pending {}",
                body["failed"], body["pending"]
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// (f) Undoing a learning run after an earlier single undo counts each of the
/// run's sections once, kept or restored, under the marker the undo writes and
/// under a marker written before counts were stored, on a repeat and after a
/// restart.
#[test]
fn a_learning_run_undo_counts_each_section_once() {
    use crate::knowledge::lens_page::{
        AnchorInput, Basis, BasisRef, FilingOrigin, LensOp, OpOutcome, PageDelta, PageKind,
        PageTarget, SectionKey,
    };
    use crate::knowledge::MessageUndo;
    let basis = || Basis {
        refs: vec![BasisRef::Claim {
            claim_id: "clm_1".into(),
        }],
        uncertainty: None,
    };
    let put = |key: &str, markdown: &str, expected: Option<String>| LensOp::Put {
        key: SectionKey::new(key).unwrap(),
        heading: key.to_uppercase(),
        markdown: markdown.into(),
        basis: basis(),
        expected,
    };
    let turn = |message: &str| FilingOrigin::Turn {
        chat_id: "chat-1".into(),
        message_id: message.into(),
        user_message_id: Some("user-1".into()),
        unattended: false,
    };
    let digest = |store: &KnowledgeStore, id: &str, key: &str| {
        store
            .read(|read| read.read_page("loupist-a", id))
            .unwrap()
            .body
            .sections()
            .iter()
            .find(|section| section.key.as_str() == key)
            .map(|section| section.digest.clone())
    };
    let mut failures = Vec::new();
    // Which of the run's two sections an earlier single undo restored, whether
    // a later edit keeps the other, and which marker the run's undo leaves.
    for earlier in [None, Some("base"), Some("added")] {
        for edited in [false, true] {
            for legacy in [false, true] {
                let case = format!("earlier {earlier:?}, edited {edited}, legacy {legacy}");
                let (directory, store) = super::open();
                let filed = store
                    .transact(|write| {
                        write.file(
                            "loupist-a",
                            &turn("earlier"),
                            vec![PageDelta {
                                target: PageTarget::Create {
                                    kind: PageKind::Subject,
                                    title: "Northwind Rail".into(),
                                    anchors: vec![AnchorInput::Name {
                                        name: "Northwind Rail".into(),
                                    }],
                                },
                                ops: vec![put("base", "original", None)],
                            }],
                            100,
                        )
                    })
                    .unwrap();
                let id = filed[0].record_id.clone();
                let base = digest(&store, &id, "base");
                let run = store
                    .transact(|write| {
                        write.file(
                            "loupist-a",
                            &turn("run-1"),
                            vec![PageDelta {
                                target: PageTarget::Existing {
                                    record_id: id.clone(),
                                },
                                ops: vec![
                                    put("base", "from-run", base.clone()),
                                    put("added", "from-run", None),
                                ],
                            }],
                            200,
                        )
                    })
                    .unwrap();
                if let Some(section) = earlier {
                    let op = run[0]
                        .outcomes
                        .iter()
                        .find(|op| {
                            matches!(
                                (&op.outcome, section),
                                (OpOutcome::Added { .. }, "added")
                                    | (OpOutcome::Replaced { .. }, "base")
                            )
                        })
                        .map(|op| op.op_id.clone());
                    let Some(op) = op else {
                        failures.push(format!("{case}: no op for {section}"));
                        continue;
                    };
                    store
                        .transact(|write| write.undo_op("loupist-a", &op, 250))
                        .unwrap();
                }
                if edited {
                    let current = digest(&store, &id, "base");
                    if current.is_some() {
                        store
                            .transact(|write| {
                                write.file(
                                    "loupist-a",
                                    &turn("later"),
                                    vec![PageDelta {
                                        target: PageTarget::Existing {
                                            record_id: id.clone(),
                                        },
                                        ops: vec![put("base", "edited", current.clone())],
                                    }],
                                    300,
                                )
                            })
                            .unwrap();
                    }
                }
                let result = store
                    .transact(|write| write.undo_message("loupist-a", "run-1", 400))
                    .unwrap();
                if result.undone + result.kept != 2 {
                    failures.push(format!("{case}: counted {result:?} for two sections"));
                }
                if legacy {
                    let sqlite = store.app_store();
                    let mut connection = sqlite.lock().unwrap();
                    let tx = connection.transaction().unwrap();
                    tx.execute("UPDATE knowledge_erasure SET armed = 1 WHERE id = 1", [])
                        .unwrap();
                    let (record, undoes, at): (String, String, i64) = tx
                        .query_row(
                            "SELECT record_id, undoes_op_id, created_at_ms FROM knowledge_lens_log
                             WHERE outcome = 'message_undo'",
                            [],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        )
                        .unwrap();
                    tx.execute(
                        "DELETE FROM knowledge_lens_log WHERE outcome = 'message_undo'",
                        [],
                    )
                    .unwrap();
                    tx.execute(
                        "INSERT INTO knowledge_lens_log
                            (op_id, loupist_id, record_id, op_json, outcome, origin, created_at_ms, undoes_op_id)
                         VALUES ('lop_legacy', 'loupist-a', ?1, '{\"op\":\"message_undo\"}', 'message_undo', 'user', ?2, ?3)",
                        rusqlite::params![record, at, undoes],
                    )
                    .unwrap();
                    tx.execute("UPDATE knowledge_erasure SET armed = 0 WHERE id = 1", [])
                        .unwrap();
                    tx.commit().unwrap();
                }
                let again = store
                    .transact(|write| write.undo_message("loupist-a", "run-1", 500))
                    .unwrap();
                let path = directory.path().join("knowledge.sqlite");
                drop(store);
                let store = KnowledgeStore::open(&path).unwrap();
                let restarted = store
                    .transact(|write| write.undo_message("loupist-a", "run-1", 600))
                    .unwrap();
                for (when, counted) in [("repeat", again), ("restart", restarted)] {
                    if counted != result {
                        failures.push(format!(
                            "{case}: {when} counted {counted:?}, first {result:?}"
                        ));
                    }
                }
                let _: MessageUndo = result;
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
