use super::*;
use crate::domain::ChatFailureKind;
use crate::runs::log::UsageSurface;
use crate::scheduled_tasks::allowance::{admit_world_attempt, Limits, WorldOrigin};
use crate::store::model_usage::{fixture_row, ModelUsageFailure, ModelUsageKind, ModelUsageStatus};

struct Case {
    #[allow(dead_code)]
    directory: tempfile::TempDir,
    store: KnowledgeStore,
    settings: GraphProcessingSettings,
    limits: Limits,
    now: i64,
    later: i64,
}

impl Case {
    fn open() -> Self {
        let now = crate::local_time::now().timestamp_millis();
        let (directory, store) = super::open();
        let mut settings = enabled();
        settings.helper_provider = Some("openai".into());
        let limits = allowance_limits(&store.connection);
        assert!(
            limits.world_chat >= 32,
            "chat allowance {}",
            limits.world_chat
        );
        assert!(limits.background / 2 >= 32, "background share");
        Self {
            directory,
            store,
            settings,
            limits,
            now,
            later: tomorrow_noon(now),
        }
    }

    fn retag(&self, job: &GraphJobView, origin: JobOrigin, user: bool, calls: u32) -> GraphJobView {
        self.store
            .transact(|write| {
                let current = get_job(write.tx, &job.loupist_id, &job.job_id)?;
                let mut document = current.document;
                document.origin = origin;
                document.user_attempt = user;
                document.extraction.max_llm_calls = calls;
                write_status(
                    write.tx,
                    &job.job_id,
                    &current.status,
                    &current.status,
                    &document,
                )?;
                Ok(())
            })
            .unwrap();
        self.reload(job)
    }

    fn reload(&self, job: &GraphJobView) -> GraphJobView {
        self.store.graph_job(&job.loupist_id, &job.job_id).unwrap()
    }

    fn usage(
        &self,
        at: i64,
        origin: Option<WorldOrigin>,
        requests: u64,
        provider: &str,
        failure: Option<ChatFailureKind>,
    ) {
        let mut row = fixture_row(at);
        row.finished_at_ms = at;
        row.data.kind = ModelUsageKind::World;
        row.data.surface = UsageSurface::World;
        row.data.world_origin = origin;
        row.data.provider = provider.into();
        row.data.model = "model".into();
        row.data.status = ModelUsageStatus::Ok;
        row.data.started_at_ms = at;
        row.data.measures.requests = Some(requests);
        row.data.failure_kind = failure.map(ModelUsageFailure::Chat);
        self.store
            .connection
            .put_model_usage(
                &format!(
                    "world-usage-{at}-{provider}-{requests}-{}",
                    origin.is_some()
                ),
                &row,
            )
            .unwrap();
    }

    fn state(&self, at: i64) -> crate::scheduled_tasks::allowance::AllowanceState {
        let admission =
            crate::scheduled_tasks::allowance::Admission::prepare(&self.store.connection, None)
                .unwrap();
        admission.state(None, self.limits, at).unwrap()
    }

    fn payload(&self, job_id: &str) -> serde_json::Value {
        let json: String = self
            .store
            .read(|read| {
                Ok(read.tx.query_row(
                    "SELECT payload_json FROM knowledge_graph_outbox WHERE job_id = ?1",
                    [job_id],
                    |row| row.get(0),
                )?)
            })
            .unwrap();
        serde_json::from_str(&json).unwrap()
    }
}

fn another(case: &Case, loupist: &str, world: &(String, String), bytes: &[u8]) -> GraphJobView {
    let source = source_module(&case.store, loupist);
    let (span, revision) = retain(&case.store, loupist, &source, None, bytes);
    add(
        &case.store,
        &case.settings,
        loupist,
        &world.0,
        &world.1,
        &source,
        &revision,
        &span,
    )
}

fn tomorrow_noon(now_ms: i64) -> i64 {
    let now = crate::local_time::at_ms(now_ms).unwrap();
    let noon = now
        .date_naive()
        .succ_opt()
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    noon.and_local_timezone(*now.offset())
        .single()
        .unwrap()
        .timestamp_millis()
}

/// Daily allowance: a short job waits for midnight with the same call budget,
/// background work does not spend the chat bucket, the fourth claim yields to
/// background, a plan limit holds only background work, and a user attempt
/// runs past the allowance and is counted.
#[test]
fn world_allowance_claim_table() {
    let origin = Case::open();
    let publisher = queued(
        &origin.store,
        &origin.settings,
        "loupist-a",
        b"A publisher filing.",
    );
    assert_eq!(publisher.document.origin, JobOrigin::Background);
    let source = source_module(&origin.store, "loupist-a");
    let mut user_saved = source_request("loupist-a", &source, b"Notes the user kept.");
    user_saved.authorship = crate::knowledge::Authorship::User;
    let retained = origin.store.retain_source(user_saved).unwrap();
    let user_saved_job = origin
        .store
        .transact(|write| write.enqueue_source_world("loupist-a", &retained, &origin.settings, T))
        .unwrap()
        .unwrap();
    assert_eq!(user_saved_job.document.origin, JobOrigin::Chat);
    drop(origin);

    let wait = Case::open();
    let waiting = queued(
        &wait.store,
        &wait.settings,
        "loupist-wait",
        b"Needs more than today.",
    );
    let waiting = wait.retag(&waiting, JobOrigin::Chat, false, 32);
    wait.usage(
        wait.now,
        Some(WorldOrigin::Chat),
        u64::from(wait.limits.world_chat - 20),
        "openai",
        None,
    );
    let parked = claim(&wait.store, &wait.settings, &waiting.job_id, wait.now);
    assert_eq!(parked.status, JOB_QUEUED, "short of the chat allowance");
    assert_eq!(parked.document.extraction.max_llm_calls, 32);
    assert!(
        wait.payload(&waiting.job_id).get("hold").is_none(),
        "a hold is not stored"
    );
    assert!(
        queued_holds(&wait.store.connection, "loupist-wait", wait.now)
            .unwrap()
            .get(&waiting.job_id)
            .is_some_and(|hold| hold.cause == JobHoldCause::Allowance && hold.provider.is_none())
    );
    let started = claim(&wait.store, &wait.settings, &waiting.job_id, wait.later);
    assert_eq!(started.status, JOB_RUNNING, "the next local day admits it");
    assert_eq!(started.document.extraction.max_llm_calls, 32);
    drop(wait);

    let backlog = Case::open();
    let share = backlog.limits.background / 2;
    backlog.usage(
        backlog.now,
        Some(WorldOrigin::Background),
        u64::from(share - 10),
        "openai",
        None,
    );
    let today = backlog.state(backlog.now);
    assert!(
        admit_world_attempt(&today, WorldOrigin::Chat, 32, Some("openai")).is_ok(),
        "a background backlog does not spend the chat allowance"
    );
    assert!(
        admit_world_attempt(&today, WorldOrigin::Background, 32, Some("openai")).is_err(),
        "the backlog spends the background share"
    );
    let background = queued(
        &backlog.store,
        &backlog.settings,
        "loupist-bg",
        b"Background work.",
    );
    let background = backlog.retag(&background, JobOrigin::Background, false, 32);
    let running = claim(
        &backlog.store,
        &backlog.settings,
        &background.job_id,
        backlog.later,
    );
    assert_eq!(running.status, JOB_RUNNING);
    let chat = queued(
        &backlog.store,
        &backlog.settings,
        "loupist-chat",
        b"Chat work.",
    );
    let _chat = backlog.retag(&chat, JobOrigin::Chat, false, backlog.limits.world_chat);
    let beside = backlog.state(backlog.later);
    assert!(
        admit_world_attempt(
            &beside,
            WorldOrigin::Chat,
            backlog.limits.world_chat,
            Some("openai"),
        )
        .is_ok(),
        "a running background reservation stays out of the chat bucket"
    );
    drop(backlog);

    let order = Case::open();
    let order_world = world(&order.store, "loupist-order");
    let mut ranked = vec![another(
        &order,
        "loupist-order",
        &order_world,
        b"background first",
    )];
    for index in 0..4 {
        let job = another(
            &order,
            "loupist-order",
            &order_world,
            format!("chat {index}").as_bytes(),
        );
        ranked.push(order.retag(&job, JobOrigin::Chat, false, 8));
    }
    ranked[0] = order.reload(&ranked[0]);
    assert_eq!(ranked[0].document.origin, JobOrigin::Background);
    let mut streak = 0;
    let mut claimed = Vec::new();
    for _ in 0..4 {
        let picked = pick_claim(&ranked, streak).unwrap();
        claimed.push(if picked.document.origin == JobOrigin::Chat {
            "chat"
        } else {
            "background"
        });
        streak = note_claim_streak(streak, &picked.document);
        let id = picked.job_id.clone();
        ranked.retain(|job| job.job_id != id);
    }
    assert_eq!(claimed, ["chat", "chat", "chat", "background"]);
    drop(order);

    let plan = Case::open();
    plan.store
        .transact(|write| {
            store_settings(write.tx, "loupist-plan", &plan.settings)?;
            Ok(())
        })
        .unwrap();
    let plan_world = world(&plan.store, "loupist-plan");
    let limited = another(&plan, "loupist-plan", &plan_world, b"Background on openai.");
    let limited = plan.retag(&limited, JobOrigin::Background, false, 8);
    let chat_same = another(&plan, "loupist-plan", &plan_world, b"Chat on openai.");
    let chat_same = plan.retag(&chat_same, JobOrigin::Chat, false, 8);
    plan.usage(
        plan.now,
        Some(WorldOrigin::Background),
        1,
        "openai",
        Some(ChatFailureKind::PlanLimit),
    );
    let held = claim(&plan.store, &plan.settings, &limited.job_id, plan.now);
    assert_eq!(held.status, JOB_QUEUED);
    assert_eq!(held.document.extraction.max_llm_calls, 8);
    let hold = queued_holds(&plan.store.connection, "loupist-plan", plan.now)
        .unwrap()
        .remove(&limited.job_id)
        .unwrap();
    assert_eq!(hold.cause, JobHoldCause::PlanLimit);
    assert_eq!(hold.provider.as_deref(), Some("openai"));
    assert!(
        !queued_holds(&plan.store.connection, "loupist-plan", plan.now)
            .unwrap()
            .contains_key(&chat_same.job_id)
    );
    let chat_ran = claim(&plan.store, &plan.settings, &chat_same.job_id, plan.now);
    assert_eq!(
        chat_ran.status, JOB_RUNNING,
        "chat origin ignores the provider hold"
    );
    plan.store
        .transact(|write| {
            let job = get_job(write.tx, "loupist-plan", &chat_ran.job_id)?;
            write_status(
                write.tx,
                &job.job_id,
                JOB_RUNNING,
                JOB_COMPLETED,
                &job.document,
            )?;
            Ok(())
        })
        .unwrap();
    let next_day = claim(&plan.store, &plan.settings, &limited.job_id, plan.later);
    assert_eq!(
        next_day.status, JOB_RUNNING,
        "the provider hold ends at local midnight"
    );
    drop(plan);

    let user = Case::open();
    let user_world = world(&user.store, "loupist-user");
    let attempt = another(&user, "loupist-user", &user_world, b"Read now.");
    let attempt = user.retag(&attempt, JobOrigin::Chat, true, 32);
    assert!(attempt.document.user_attempt);
    let small = another(&user, "loupist-user", &user_world, b"Ordinary chat.");
    let _small = user.retag(&small, JobOrigin::Chat, false, 8);
    let left = 10;
    user.usage(
        user.now,
        Some(WorldOrigin::Chat),
        u64::from(user.limits.world_chat - left),
        "other",
        None,
    );
    let before = user.state(user.now);
    assert!(admit_world_attempt(&before, WorldOrigin::Chat, 8, Some("openai")).is_ok());
    assert!(admit_world_attempt(&before, WorldOrigin::Chat, 32, Some("openai")).is_err());
    let ran = claim(&user.store, &user.settings, &attempt.job_id, user.now);
    assert_eq!(ran.status, JOB_RUNNING, "a user attempt is not refused");
    assert_eq!(ran.document.extraction.max_llm_calls, 32);
    assert_eq!(ran.document.origin, JobOrigin::Chat);
    let reserved = user.state(user.now);
    assert!(
        admit_world_attempt(&reserved, WorldOrigin::Chat, 8, Some("openai")).is_err(),
        "the running user attempt is reserved as chat"
    );
    user.store
        .transact(|write| {
            let job = get_job(write.tx, "loupist-user", &attempt.job_id)?;
            write_status(
                write.tx,
                &job.job_id,
                JOB_RUNNING,
                JOB_COMPLETED,
                &job.document,
            )?;
            Ok(())
        })
        .unwrap();
    let released = user.state(user.now);
    assert!(
        admit_world_attempt(&released, WorldOrigin::Chat, 8, Some("openai")).is_ok(),
        "ending the attempt releases the reservation"
    );
    user.usage(user.now + 1, Some(WorldOrigin::Chat), 32, "other", None);
    let after = user.state(user.now);
    assert!(
        admit_world_attempt(&after, WorldOrigin::Chat, 8, Some("openai")).is_err(),
        "the user attempt's calls were counted in the chat bucket"
    );
    drop(user);
}

#[test]
fn read_now_promotes_a_held_queued_source_without_refilling_it() {
    let case = Case::open();
    let job = queued(&case.store, &case.settings, "loupist-a", b"Held source");
    let source = job
        .document
        .source_binding
        .as_ref()
        .unwrap()
        .source_record_id
        .clone();
    let calls = job.document.extraction.max_llm_calls;
    let admitted = case.state(case.now);
    let refused = case
        .store
        .transact(|write| {
            retry_source_with_allowance(
                write.tx,
                &case.settings,
                "loupist-a",
                &source,
                Some(&admitted),
            )
        })
        .unwrap_err();
    assert!(matches!(refused, KnowledgeError::Invalid(_)));
    let waiting = case.store.graph_job("loupist-a", &job.job_id).unwrap();
    assert!(!waiting.document.user_attempt);
    assert_eq!(waiting.document.extraction.max_llm_calls, calls);

    case.retag(&waiting, JobOrigin::Chat, false, calls);
    case.usage(
        case.now,
        Some(WorldOrigin::Chat),
        u64::from(case.limits.world_chat),
        "openai",
        None,
    );
    let held = case.state(case.now);
    let promoted = case
        .store
        .transact(|write| {
            retry_source_with_allowance(write.tx, &case.settings, "loupist-a", &source, Some(&held))
        })
        .unwrap();
    assert!(promoted.document.user_attempt);
    assert_eq!(promoted.document.origin, JobOrigin::Chat);
    assert_eq!(promoted.status, JOB_QUEUED);
    assert_eq!(promoted.document.extraction.max_llm_calls, calls);
}

#[test]
fn read_now_returns_the_promoted_source_and_frees_admission() {
    let case = Case::open();
    let job = queued(&case.store, &case.settings, "loupist-a", b"Held source");
    let source = job
        .document
        .source_binding
        .as_ref()
        .unwrap()
        .source_record_id
        .clone();
    let calls = job.document.extraction.max_llm_calls;
    case.retag(&job, JobOrigin::Chat, false, calls);
    case.usage(
        case.now,
        Some(WorldOrigin::Chat),
        u64::from(case.limits.world_chat),
        "openai",
        None,
    );
    let promoted = retry_admitted_source(&case.store.connection, "loupist-a", &source, case.now)
        .expect("Read now returns");
    assert!(promoted.document.user_attempt);
    assert_eq!(promoted.document.origin, JobOrigin::Chat);
    assert_eq!(promoted.status, JOB_QUEUED);
    assert_eq!(promoted.document.extraction.max_llm_calls, calls);
    assert!(promoted.hold.is_none());
    drop(
        crate::scheduled_tasks::allowance::Admission::prepare(
            &case.store.connection,
            Some("loupist-a"),
        )
        .expect("admission is free after Read now"),
    );
}

#[test]
fn a_successful_source_retry_frees_admission() {
    let case = Case::open();
    let job = queued(&case.store, &case.settings, "loupist-a", b"Stopped source");
    let source = job
        .document
        .source_binding
        .as_ref()
        .unwrap()
        .source_record_id
        .clone();
    let mut document = job.document.clone();
    document.inference_attempted = true;
    document.failure = Some(JobFailure {
        kind: JobFailureKind::InvalidJson,
        at_ms: case.now,
    });
    case.store
        .transact(|write| {
            store_settings(write.tx, "loupist-a", &case.settings)?;
            write_status(
                write.tx,
                &job.job_id,
                JOB_QUEUED,
                JOB_FAILED_EXTRACTION,
                &document,
            )
        })
        .unwrap();
    let retried = retry_admitted_source(&case.store.connection, "loupist-a", &source, case.now)
        .expect("Try again returns");
    assert_eq!(retried.status, JOB_QUEUED);
    assert!(retried.document.user_attempt);
    assert_eq!(
        retried.document.extraction.max_llm_calls,
        case.settings.max_llm_calls
    );
    drop(
        crate::scheduled_tasks::allowance::Admission::prepare(
            &case.store.connection,
            Some("loupist-a"),
        )
        .expect("admission is free after a successful retry"),
    );
}

/// A Source saved by a Chat turn stores the origin admitted for that turn,
/// and World reads it back whatever the Chat id or its runs say. A capture
/// stored without one reads as background unless the user saved it.
#[test]
fn a_source_save_keeps_the_origin_admitted_for_its_turn() {
    let case = Case::open();
    let enqueue = |retained: &crate::knowledge::RetainedSource| {
        case.store
            .transact(|write| write.enqueue_source_world("loupist-a", retained, &case.settings, T))
            .unwrap()
            .unwrap()
            .document
            .origin
    };
    for (index, (chat, authorship, admitted)) in [
        (
            "scheduled-chat-run-1",
            Authorship::Publisher,
            JobOrigin::Chat,
        ),
        ("chat-existing", Authorship::User, JobOrigin::Background),
        ("chat-existing", Authorship::Publisher, JobOrigin::Chat),
    ]
    .into_iter()
    .enumerate()
    {
        let request = source_request("loupist-a", "", format!("Turn {index}.").as_bytes());
        let retained = case
            .store
            .transact(|write| {
                write.retain_new_source_with_origin(
                    crate::knowledge::RetainNewSource {
                        origin: request.origin.clone(),
                        loupist_id: request.loupist_id.clone(),
                        at_ms: request.at_ms,
                        title: request.title.clone(),
                        source_class: request.source_class,
                        origin_locator: format!("{}-{index}", request.origin_locator),
                        provider_identity: request.provider_identity.clone(),
                        authorship,
                        coverage: request.coverage,
                        origin_chat_id: Some(chat.into()),
                        request_ref: request.request_ref.clone(),
                        declared_digest: request.declared_digest.clone(),
                        material: request.material.clone(),
                        publication_time: request.publication_time.clone(),
                    },
                    &format!("answer-{index}"),
                    admitted,
                    None,
                )
            })
            .unwrap();
        assert_eq!(enqueue(&retained), admitted, "{chat} {authorship:?}");
    }
    // Captures stored without an origin: a Chat id alone no longer makes one chat.
    for (index, authorship, expected) in [
        (0, Authorship::Publisher, JobOrigin::Background),
        (1, Authorship::User, JobOrigin::Chat),
    ] {
        let module = source_module(&case.store, "loupist-a");
        let mut request = source_request(
            "loupist-a",
            &module,
            format!("Unmarked {index}.").as_bytes(),
        );
        request.authorship = authorship;
        request.origin_chat_id = Some("chat-1".into());
        let retained = case.store.retain_source(request).unwrap();
        assert_eq!(enqueue(&retained), expected, "{authorship:?}");
    }
}

#[test]
fn a_failed_hold_lookup_still_returns_the_committed_retry() {
    let case = Case::open();
    let job = queued(&case.store, &case.settings, "loupist-a", b"Saved retry");
    let returned = committed_retry(
        job.clone(),
        Err(KnowledgeError::Invalid("hold lookup failed")),
    );
    assert_eq!(returned.job_id, job.job_id);
    assert_eq!(returned.status, job.status);
    assert!(returned.hold.is_none());
}

#[test]
fn an_allowance_held_world_job_keeps_its_hold_and_budget_across_restart() {
    let now = crate::local_time::now().timestamp_millis();
    let (directory, store) = super::open();
    let path = directory.path().join("knowledge.sqlite");
    let mut settings = enabled();
    settings.helper_provider = Some("openai".into());
    let limits = allowance_limits(&store.connection);
    let waiting = queued(&store, &settings, "loupist-held", b"Needs more than today.");
    let job_id = waiting.job_id.clone();
    let waiting = {
        store
            .transact(|write| {
                let current = get_job(write.tx, "loupist-held", &job_id)?;
                let mut document = current.document;
                document.origin = JobOrigin::Chat;
                document.extraction.max_llm_calls = 32;
                write_status(
                    write.tx,
                    &job_id,
                    &current.status,
                    &current.status,
                    &document,
                )?;
                Ok(())
            })
            .unwrap();
        store.graph_job("loupist-held", &job_id).unwrap()
    };
    let mut row = fixture_row(now);
    row.finished_at_ms = now;
    row.data.kind = ModelUsageKind::World;
    row.data.surface = UsageSurface::World;
    row.data.world_origin = Some(WorldOrigin::Chat);
    row.data.provider = "openai".into();
    row.data.model = "model".into();
    row.data.status = ModelUsageStatus::Ok;
    row.data.started_at_ms = now;
    row.data.measures.requests = Some(u64::from(limits.world_chat - 20));
    store
        .connection
        .put_model_usage("held-world-budget", &row)
        .unwrap();
    let parked = claim(&store, &settings, &waiting.job_id, now);
    assert_eq!(parked.status, JOB_QUEUED);
    assert_eq!(parked.document.extraction.max_llm_calls, 32);
    assert_eq!(
        queued_holds(&store.connection, "loupist-held", now)
            .unwrap()
            .get(&job_id)
            .map(|hold| hold.cause),
        Some(JobHoldCause::Allowance)
    );
    drop(store);

    let store = KnowledgeStore::open(&path).unwrap();
    let reloaded = store.graph_job("loupist-held", &job_id).unwrap();
    assert_eq!(reloaded.status, JOB_QUEUED);
    assert_eq!(reloaded.document.extraction.max_llm_calls, 32);
    assert_eq!(
        queued_holds(&store.connection, "loupist-held", now)
            .unwrap()
            .get(&job_id)
            .map(|hold| hold.cause),
        Some(JobHoldCause::Allowance)
    );
}
