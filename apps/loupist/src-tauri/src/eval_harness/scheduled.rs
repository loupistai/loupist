//! A due occurrence in a disposable eval Loupist, observed through ordinary
//! scheduler admission, Chat completion and the persisted Bot Thread.
use super::{observation, *};
use crate::{
    commands::now_ms,
    domain::{ChatMessageStatus, ChatRole},
    scheduled_tasks::types::{
        LearningDepth, LearningKeeps, LearningProgram, LearningScope, ReportPolicy, RunStatus,
        Schedule, TaskDefinition, TaskDelivery, TaskDraft, TaskKind, TaskStatus, TaskTarget,
        TaskWatch,
    },
};

/// A Learning program the scenario starts with, turned on: the arguments of
/// `learning_program_save` and, optionally, a completed earlier occurrence.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProgramSeed {
    id: String,
    title: String,
    prompt: String,
    /// `learning_program_save`'s schedule: `{kind: once, at}` or the stored forms.
    schedule: Value,
    /// `{asset: <followed name>}`, `{world: <World entity name>}`,
    /// `{lens: <page title>}` or `{name}`.
    #[serde(default)]
    subjects: Vec<Value>,
    #[serde(default)]
    preset: Option<String>,
    depth: LearningDepth,
    scope: LearningScope,
    keeps: LearningKeeps,
    report: ReportPolicy,
    delivery: TaskDelivery,
    #[serde(default)]
    previous_run: Option<PreviousRunSeed>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PreviousRunSeed {
    /// When the earlier occurrence started (RFC 3339).
    clock: String,
}

/// A task the fixture seeds or a turn saves is dated at the scenario clock, as
/// scripted publications are, so a first occurrence's `previous_run` (the
/// task's creation) is ordered with them rather than at the host's real time.
pub(super) fn date_at_clock(
    state: &AppState,
    mut task: crate::scheduled_tasks::types::Task,
) -> Result<crate::scheduled_tasks::types::Task> {
    let Some(clock) = super::clock::local_now() else {
        return Ok(task);
    };
    let revision = task.revision;
    task.created_at_ms = clock.timestamp_millis();
    Ok(state.store.scheduled_task_write(task, revision, now_ms())?)
}

/// The saved task of a seeded program.
fn program_task_id(program: &str) -> String {
    format!("learning-eval-{program}")
}

fn schedule(input: &Value) -> Result<Schedule> {
    let mut input = input.clone();
    if input["kind"] == "once" {
        if let Some(at) = input["at"].as_str() {
            let at_ms = chrono::DateTime::parse_from_rfc3339(at)
                .map_err(|_| "a once schedule's at is RFC 3339")?
                .timestamp_millis();
            input = json!({"kind": "once", "at_ms": at_ms});
        }
    }
    Ok(serde_json::from_value(input)?)
}

/// A subject named by what the fixture can know: a followed asset's name, a
/// World entity's name or a Lens page's title.
fn subject(state: &AppState, loupist_id: &str, named: &Value) -> Result<Value> {
    let name = |key: &str| named[key].as_str().map(str::to_owned);
    if let Some(asset) = name("asset") {
        let rows = state.store.assets_list(loupist_id, false)?;
        let matches = crate::store::user_context::matching(&rows, &asset);
        let [row] = matches.as_slice() else {
            return Err(format!("a program subject names one followed asset: {asset}").into());
        };
        return Ok(json!({"asset": row.asset_id}));
    }
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    if let Some(entity) = name("world") {
        let body =
            world::script::head(&store, loupist_id)?.ok_or("a World subject needs a World")?;
        let node = body
            .nodes
            .iter()
            .find(|node| node.name == entity)
            .ok_or_else(|| format!("no World entity is named {entity}"))?;
        return Ok(json!({"world": node.id}));
    }
    if let Some(title) = name("lens") {
        let pages = store.read(|read| read.lens_index(loupist_id))?;
        let page = pages
            .iter()
            .find(|page| page.title == title)
            .ok_or_else(|| format!("no Lens page is titled {title}"))?;
        return Ok(json!({"lens": page.record_id}));
    }
    Ok(named.clone())
}

/// Save each seeded program turned on, with its earlier occurrence.
pub(super) fn seed_programs(
    state: &AppState,
    loupist_id: &str,
    programs: &[ProgramSeed],
    model: &str,
    thinking: &str,
) -> Result<()> {
    for program in programs {
        let subjects = program
            .subjects
            .iter()
            .map(|named| Ok(serde_json::from_value(subject(state, loupist_id, named)?)?))
            .collect::<Result<Vec<_>>>()?;
        let draft = TaskDraft {
            title: program.title.clone(),
            prompt: program.prompt.clone(),
            schedule: schedule(&program.schedule)?,
            target: TaskTarget::NewChat,
            model_profile_id: model.into(),
            thinking_level: thinking.into(),
            run_options: model_run_options(state, model)?,
            use_memory: true,
            update_memory: true,
            status: TaskStatus::Active,
        };
        let definition = TaskDefinition {
            program: Some(LearningProgram {
                depth: program.depth,
                scope: program.scope.clone(),
                keeps: program.keeps,
                // The saved program keeps the seed's preset for display.
                preset: program.preset.clone(),
            }),
            watch: TaskWatch {
                report: Some(program.report),
                delivery: Some(program.delivery),
                subjects: Some(subjects),
                ..TaskWatch::default()
            },
            ..TaskDefinition::default()
        };
        let task = crate::scheduled_tasks::service::create_definition(
            state,
            loupist_id,
            draft,
            &program_task_id(&program.id),
            definition,
        )
        .map_err(|error| error.message)?;
        if task.kind != TaskKind::Learning || task.awaiting_confirmation {
            return Err(format!("program {} was not saved turned on", program.id).into());
        }
        let task = date_at_clock(state, task)?;
        if let Some(previous) = &program.previous_run {
            let started = chrono::DateTime::parse_from_rfc3339(&previous.clock)
                .map_err(|_| "a previous_run clock is RFC 3339")?
                .timestamp_millis();
            state.store.scheduled_task_record_completed_run(
                loupist_id,
                &task.id,
                started,
                started + 60_000,
            )?;
        }
    }
    Ok(())
}

/// Record `requests left` Learning requests for today as the only ones the
/// first occurrence has: the rest of the day's Learning allowance is used.
pub(super) fn spend_learning_allowance(
    state: &AppState,
    loupist_id: &str,
    model: &str,
    left: u32,
) -> Result<()> {
    let now = now_ms();
    let remaining = {
        let admission = crate::scheduled_tasks::allowance::Admission::prepare(&state.store, None)?;
        admission
            .state(
                None,
                crate::scheduled_tasks::allowance::Limits::default(),
                now,
            )?
            .learning_remaining(loupist_id)
    };
    let used = remaining
        .checked_sub(left)
        .ok_or("seed.learning_allowance exceeds the day's Learning allowance")?;
    let model = state.model_catalog()?.resolve(model)?;
    let mut log = crate::runs::log::RunLog::new(
        "chat",
        loupist_id,
        &format!("eval-allowance-{}", uuid::Uuid::new_v4()),
        now,
    );
    log.surface = Some(crate::runs::log::UsageSurface::Learning);
    log.model = Some(format!("{}/{}", model.provider, model.model));
    log.requests = Some(u64::from(used));
    log.record_usage(&state.store)?;
    Ok(())
}

/// A check the scenario starts with, turned on: the arguments of
/// `trigger_save`, saved through its own path and production dry run.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckSeed {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    prompt: String,
    terms: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    when_met_summary: Option<String>,
    template: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    template_params: Option<serde_json::Map<String, Value>>,
}

impl CheckSeed {
    pub(super) fn id(&self) -> &str {
        &self.id
    }
}

/// The proposal a seeded check is saved as, so an occurrence can name it.
fn check_proposal(id: &str) -> String {
    format!("eval-seed-check-{id}")
}

/// Save each seeded check and turn it on from its card, as the user would.
pub(super) async fn seed_checks(
    state: &AppState,
    loupist_id: &str,
    checks: &[CheckSeed],
    model: &str,
    thinking: &str,
) -> Result<()> {
    for check in checks {
        let mut params = json!({
            "prompt": check.prompt, "terms": check.terms, "template": check.template,
            // The harness makes each occurrence due, so the cadence only has
            // to be one a check may have.
            "schedule": {"kind": "interval", "every_minutes": 1440},
        });
        for (key, value) in [
            ("title", check.title.as_ref().map(|title| json!(title))),
            (
                "when_met_summary",
                check.when_met_summary.as_ref().map(|line| json!(line)),
            ),
            (
                "template_params",
                check.template_params.as_ref().map(|params| json!(params)),
            ),
        ] {
            if let Some(value) = value {
                params[key] = value;
            }
        }
        let context = crate::commands::ScheduleContext {
            research_model: None,
            model_profile_id: model.into(),
            thinking_level: thinking.into(),
            run_options: model_run_options(state, model)?,
            use_memory: true,
            update_memory: true,
        };
        let task = Box::pin(crate::commands::eval_save_check(
            state,
            loupist_id,
            &check_proposal(&check.id),
            context,
            params,
        ))
        .await
        .map_err(|error| format!("seed check {}: {}", check.id, error.message))?;
        let trigger = task
            .trigger
            .as_ref()
            .ok_or("a saved check has no trigger")?;
        crate::commands::triggers::approve(
            state,
            loupist_id,
            &task.id,
            task.revision,
            trigger.script.version,
            &trigger.script.sha256,
        )
        .map_err(|error| {
            // The dry run the card shows, which decides whether it can be turned on.
            let test = trigger.proposal_test.as_ref().map(|test| {
                json!({"today": test.today_outcome, "reading": test.today_reading, "error": test.error})
            });
            format!(
                "seed check {} was not turned on: {} (dry run: {})",
                check.id,
                error.message,
                test.unwrap_or_default()
            )
        })?;
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fixture {
    kind: Kind,
    #[serde(default)]
    run_existing: bool,
    #[serde(default)]
    clock: Option<String>,
    /// A `learning` occurrence's seeded program.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    program: Option<String>,
    /// A `check` occurrence's seeded check; absent, the Loupist's one saved check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<String>,
    /// Record the check's dry run with the occurrence.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    dry_run: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Briefing,
    Custom,
    Learning,
    Check,
}

impl Fixture {
    /// The occurrence's clock, when it states one.
    pub(super) fn clock(&self) -> Option<&str> {
        self.clock.as_deref()
    }
}

pub(super) fn present_fixture<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Fixture>, D::Error> {
    Fixture::deserialize(deserializer).map(Some)
}

pub(super) fn validate(scenario: &Scenario, session: &Session, turn: &Turn) -> Result<()> {
    let Some(fixture) = &turn.scheduled else {
        return Ok(());
    };
    if fixture.kind == Kind::Learning {
        match fixture.program.as_deref() {
            Some(program) => {
                if !scenario
                    .seed
                    .programs
                    .iter()
                    .any(|seeded| seeded.id == program)
                {
                    return Err(format!(
                        "a learning occurrence names no seeded program: {program}"
                    )
                    .into());
                }
            }
            // The program an earlier turn saved and accepted on its card.
            None => {
                let index = session
                    .turns
                    .iter()
                    .position(|candidate| std::ptr::eq(candidate, turn))
                    .unwrap_or(0);
                if !session.turns[..index]
                    .iter()
                    .any(|earlier| earlier.approvals == Some(super::Approvals::Accept))
                {
                    return Err("a learning occurrence without a program needs an earlier turn that accepts its card".into());
                }
            }
        }
        if fixture.run_existing {
            return Err("a learning occurrence runs its seeded program; drop run_existing".into());
        }
    } else if fixture.program.is_some() {
        return Err("only a learning occurrence names a program".into());
    }
    if fixture.kind == Kind::Check {
        if fixture.run_existing {
            return Err("a check occurrence runs its saved check; drop run_existing".into());
        }
        if let Some(task) = &fixture.task {
            if !scenario.seed.checks.iter().any(|check| check.id() == task) {
                return Err(format!("a check occurrence names no seeded check: {task}").into());
            }
        }
    } else if fixture.task.is_some() || fixture.dry_run {
        return Err("only a check occurrence names a task or a dry run".into());
    }
    if turn.briefing
        || turn.as_of.is_some()
        || !turn.attachments.is_empty()
        || scenario.sessions.len() != 1
        || (!fixture.run_existing
            && !matches!(fixture.kind, Kind::Learning | Kind::Check)
            && session.turns.len() != 1)
        || session.id != "bot"
        || session.role != "probe"
        || session.memory != "remember"
        || !matches!(scenario.language.as_deref(), Some("en" | "ko"))
    {
        return Err("scheduled requires an isolated bot/probe/remember turn with fixed appearance and no conflicting fixture flags".into());
    }
    if fixture.run_existing && fixture.kind != Kind::Custom {
        return Err("run_existing requires a custom schedule".into());
    }
    if fixture.kind == Kind::Briefing
        && (!scenario.connections.is_empty()
            || !scenario.workspace_files.is_empty()
            || serde_json::to_value(&scenario.seed)? != serde_json::to_value(Seed::default())?)
    {
        return Err("scheduled briefing requires an empty library and no connections".into());
    }
    if fixture.kind == Kind::Custom
        && !fixture.run_existing
        && (turn.user.trim().is_empty() || turn.user.trim() != turn.user)
    {
        return Err(
            "the saved custom request must be nonempty and have no surrounding whitespace".into(),
        );
    }
    Ok(())
}

/// A check occurrence's evaluation, and its dry run when the fixture asks.
fn check_evidence(
    state: &AppState,
    loupist: &str,
    task_id: &str,
    dry_run: Option<&Value>,
    record: &mut Value,
) -> Result<()> {
    let check = state
        .store
        .scheduled_task_get(loupist, task_id)?
        .and_then(|task| task.trigger)
        .ok_or("the saved check disappeared")?;
    record["check"] = json!({"state": check.state, "last": check.last});
    if let Some(dry_run) = dry_run {
        record["trigger_dry_run"] = dry_run.clone();
    }
    Ok(())
}

/// A check occurrence's evaluation and, when the fixture asks, its dry run at
/// the occurrence's clock after the scheduler evaluated it: a check's dry run
/// replays its recorded evaluations, this occurrence's included.
async fn check_occurrence(
    state: &AppState,
    loupist: &str,
    task_id: &str,
    fixture: &Fixture,
    record: &mut Value,
) -> Result<()> {
    let dry_run = if fixture.dry_run {
        let task = state
            .store
            .scheduled_task_get(loupist, task_id)?
            .ok_or("the saved check disappeared")?;
        Some(fresh_dry_run(state, &task).await?)
    } else {
        None
    };
    check_evidence(state, loupist, task_id, dry_run.as_ref(), record)
}

/// A check occurrence's dry run at its clock, run as `trigger_save` runs it.
async fn fresh_dry_run(
    state: &AppState,
    task: &crate::scheduled_tasks::types::Task,
) -> Result<Value> {
    let (test, tools) = Box::pin(crate::commands::eval_dry_run_check(state, task))
        .await
        .map_err(|error| error.message)?;
    let mut record = trace::trigger_dry_run(
        test.fired_on.clone(),
        tools.into_iter().map(|tool| tool.name).collect(),
    );
    record["error"] = json!(test.error);
    Ok(record)
}

/// Whether the scheduler started the occurrence.
enum Admitted {
    Ran,
    /// The allowance held it, with the hold the scheduler recorded.
    Held(Value),
    /// A check the scheduler evaluated and found not met.
    NotMet,
}

pub(super) async fn run(
    state: &AppState,
    home: &ChatSession,
    turn: &Turn,
    fixture: &Fixture,
    model: &str,
    thinking: &str,
    deadline: super::Deadline,
) -> Result<Value> {
    let _turn_clock = fixture
        .clock
        .as_deref()
        .map(|clock| super::clock::install(Some(clock)))
        .transpose()?;
    let started = chrono::Utc::now().to_rfc3339();
    let clock = Instant::now();
    let loupist = &home.loupist_id;
    let now = now_ms();
    let due = now + 1;
    let task = if fixture.kind == Kind::Check {
        // The saved check is due now, so the scheduler evaluates it as it would
        // a due occurrence; only a met check starts a run.
        // A named seeded check, else the Loupist's one saved check.
        let proposal = fixture.task.as_deref().map(check_proposal);
        let checks = state
            .store
            .scheduled_tasks_list(loupist)?
            .into_iter()
            .filter(|task| {
                task.trigger.is_some()
                    && (proposal.is_none() || task.proposed_in_message_id == proposal)
            })
            .collect::<Vec<_>>();
        let [check] = checks.as_slice() else {
            return Err("a check occurrence needs one saved check".into());
        };
        if check.awaiting_confirmation || check.status != TaskStatus::Active {
            // A check the user never turned on has no occurrence to run.
            let mut record = json!({
                "answer": "", "status": "failed", "error": "the saved check is not turned on",
                "saved_task": {"id": check.id, "kind": check.kind, "prompt": check.prompt},
                "runs": [], "scheduled_run": trace::scheduled_run(None, None),
            });
            let dry_run = if fixture.dry_run {
                Some(fresh_dry_run(state, check).await?)
            } else {
                None
            };
            check_evidence(state, loupist, &check.id, dry_run.as_ref(), &mut record)?;
            record["started_at"] = json!(started);
            record["duration_ms"] = json!(clock.elapsed().as_millis() as u64);
            return Ok(record);
        }
        let mut task = check.clone();
        let revision = task.revision;
        task.next_run_at_ms = Some(now);
        state.store.scheduled_task_write(task, revision, now)?
    } else if fixture.kind == Kind::Learning {
        // The program's next occurrence is due now, so the scheduler admits it
        // as it would a due occurrence: allowance, budget levels and one
        // learning run at a time.
        let mut task = match fixture.program.as_deref() {
            Some(program) => state
                .store
                .scheduled_task_get(loupist, &program_task_id(program))?
                .ok_or("the seeded program is missing")?,
            None => {
                let programs = state
                    .store
                    .scheduled_tasks_list(loupist)?
                    .into_iter()
                    .filter(|task| task.kind == TaskKind::Learning)
                    .collect::<Vec<_>>();
                let [program] = programs.as_slice() else {
                    return Err("a learning occurrence needs one saved program".into());
                };
                if program.awaiting_confirmation {
                    return Err("the saved program was not turned on".into());
                }
                program.clone()
            }
        };
        let revision = task.revision;
        task.status = TaskStatus::Active;
        task.next_run_at_ms = Some(now);
        state.store.scheduled_task_write(task, revision, now)?
    } else if fixture.run_existing {
        let tasks = state
            .store
            .scheduled_tasks_list(loupist)?
            .into_iter()
            .filter(|task| task.kind == TaskKind::Custom)
            .collect::<Vec<_>>();
        let [task] = tasks.as_slice() else {
            return Err("run_existing requires one saved custom task".into());
        };
        task.clone()
    } else {
        if !crate::chat::home::is_home(home)
            || !home.messages.is_empty()
            || !home.versions.is_empty()
            || !state.store.scheduled_tasks_list(loupist)?.is_empty()
        {
            return Err(
                "scheduled fixture requires a fresh Bot Thread and no existing tasks".into(),
            );
        }
        let (kind, prompt) = match fixture.kind {
            Kind::Briefing => (TaskKind::Briefing, crate::briefings::PROMPT),
            Kind::Custom => (TaskKind::Custom, turn.user.as_str()),
            Kind::Learning | Kind::Check => {
                return Err("an occurrence runs its saved program or check".into())
            }
        };
        state.store.scheduled_task_create_kind(
            loupist,
            &format!("eval-task-{}", uuid::Uuid::new_v4()),
            TaskDraft {
                title: "Eval scheduled work".into(),
                prompt: prompt.into(),
                schedule: Schedule::Once { at_ms: due },
                target: TaskTarget::Dedicated,
                model_profile_id: model.into(),
                thinking_level: thinking.into(),
                run_options: model_run_options(state, model)?,
                use_memory: true,
                update_memory: true,
                status: TaskStatus::Active,
            },
            kind,
            now,
        )?
    };
    let captured = Arc::new(Mutex::new(observation::TurnObservation::default()));
    let callback = captured.clone();
    let (lifetime, mut settled) = tokio::sync::watch::channel(());
    let launched_run_id = std::sync::OnceLock::new();
    let execution = async {
        while !fixture.run_existing
            && !matches!(fixture.kind, Kind::Learning | Kind::Check)
            && now_ms() < due
        {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let Some(claimed) = state
            .scheduled
            .dispatch_eval(
                state,
                loupist,
                &task.id,
                fixture.run_existing,
                Arc::new(move |event| {
                    let _ = &lifetime;
                    callback.lock().unwrap().observe(event);
                }),
            )
            .await?
        else {
            // A held occurrence leaves no run row; the scheduler keeps the hold
            // on the task.
            let held = state
                .store
                .scheduled_task_get(loupist, &task.id)?
                .and_then(|task| task.hold);
            if fixture.kind == Kind::Learning && held.is_some() {
                return Ok(Admitted::Held(serde_json::to_value(held)?));
            }
            if fixture.kind == Kind::Check {
                // Evaluated and not met: no run, so no model call.
                return Ok(Admitted::NotMet);
            }
            return Err("due task was not admitted".into());
        };
        launched_run_id
            .set(claimed.run_id.clone())
            .expect("one scheduled dispatch");
        // The sink outlives terminal persistence, including scheduler and Bot
        // writes. Its drop also releases us if the runner exits without an event.
        let _ = settled.changed().await;
        let terminal = state
            .store
            .scheduled_task_run_get(loupist, &claimed.run_id)?
            .ok_or("scheduled occurrence disappeared")?;
        if terminal.status == RunStatus::Succeeded && !terminal.quiet {
            // The terminal sink can drop before the post-turn delivery sweep
            // acquires the Bot thread. Read the store until that sweep commits.
            loop {
                let stored = state
                    .store
                    .get_chat(&home.id)?
                    .ok_or("Bot Thread disappeared")?;
                if stored.messages.iter().any(|message| {
                    message
                        .delivery
                        .as_ref()
                        .is_some_and(|delivery| delivery.run_id == terminal.run_id)
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(Admitted::Ran)
    };
    tokio::pin!(execution);
    let outcome = tokio::select! {
        biased;
        done = &mut execution => Ok(done),
        () = deadline => Err(()),
    };
    let mut record = json!({
        "answer": "", "status": "failed", "tool_calls": null,
        "saved_task": {"id":task.id,"kind":task.kind,"prompt":task.prompt},
        "home_thread_id": home.id,
    });
    let completed = match outcome {
        Ok(Ok(Admitted::Ran)) => true,
        Ok(Ok(Admitted::NotMet)) => {
            record["status"] = json!("completed");
            record["scheduled_run"] = trace::scheduled_run(None, None);
            Box::pin(check_occurrence(
                state,
                loupist,
                &task.id,
                fixture,
                &mut record,
            ))
            .await?;
            record["runs"] = json!([]);
            record["started_at"] = json!(started);
            record["duration_ms"] = json!(clock.elapsed().as_millis() as u64);
            record["replay_fallbacks"] = json!(replay::take_fallbacks());
            return Ok(record);
        }
        Ok(Ok(Admitted::Held(hold))) => {
            record["status"] = json!("completed");
            record["scheduled_run"] = trace::scheduled_run(None, Some(hold));
            record["runs"] = json!([]);
            record["started_at"] = json!(started);
            record["duration_ms"] = json!(clock.elapsed().as_millis() as u64);
            record["replay_fallbacks"] = json!(replay::take_fallbacks());
            return Ok(record);
        }
        Ok(Err(error)) => {
            record["error"] = trace::bounded_json(&json!(error.to_string()));
            false
        }
        Err(_) => {
            if let Some(run_id) = launched_run_id.get() {
                if state
                    .store
                    .scheduled_task_run_get(loupist, run_id)?
                    .is_some_and(|run| run.status == RunStatus::Running)
                {
                    let _ = state
                        .cancel_run(run_id, crate::runs::coordinator::RunKind::Chat)
                        .await;
                }
                if tokio::time::timeout(Duration::from_secs(30), &mut execution)
                    .await
                    .is_err()
                {
                    record["cleanup_error"] =
                        json!("timed-out scheduled Chat cleanup did not drain");
                }
            }
            record["error"] = json!("timeout");
            false
        }
    };
    let launched_run_id = launched_run_id.get().map(String::as_str);

    let runs = state
        .store
        .scheduled_task_runs(loupist, &task.id)?
        .into_iter()
        .filter(|run| launched_run_id == Some(run.run_id.as_str()))
        .collect::<Vec<_>>();
    let inbox_ids = state
        .store
        .inbox_runs(loupist, None, 100)?
        .into_iter()
        .map(|run| run.run_id)
        .collect::<Vec<_>>();
    record["inbox_run_ids"] = json!(inbox_ids);
    let stored_home = state
        .store
        .get_chat(&home.id)?
        .ok_or("Bot Thread disappeared")?;
    let deliveries = stored_home
        .messages
        .iter()
        .chain(
            stored_home
                .versions
                .iter()
                .flat_map(|version| &version.messages),
        )
        .filter_map(|message| {
            message
                .delivery
                .as_ref()
                .filter(|delivery| launched_run_id == Some(delivery.run_id.as_str()))
                .map(|delivery| (message, delivery))
        })
        .collect::<Vec<_>>();
    record["runs"] = json!(runs);
    if let Some(run) = runs.first() {
        record["scheduled_run"] = trace::scheduled_run(Some(run), None);
    }
    if fixture.kind == Kind::Check {
        Box::pin(check_occurrence(
            state,
            loupist,
            &task.id,
            fixture,
            &mut record,
        ))
        .await?;
    }
    record["deliveries"] = json!(deliveries
        .iter()
        .map(|(message, delivery)| json!({
            "run_id": delivery.run_id, "task_id": delivery.task_id, "kind": delivery.task_kind,
            "thread_id": stored_home.id, "run_thread_id": delivery.run_thread_id,
            "status": delivery.status, "finished_at_ms": delivery.finished_at_ms,
            "message_id": message.id, "content": message.content,
        }))
        .collect::<Vec<_>>());
    let observed = captured.lock().unwrap();
    observed.write_projection(&mut record);
    if record["admission"].is_object() {
        record["admission"]["task_id"] = json!(task.id);
    }
    if let Some(launched_run_id) = launched_run_id.filter(|_| completed) {
        let evidence = (|| -> Result<()> {
            let [run] = runs.as_slice() else {
                return Err("expected exactly one scheduled run".into());
            };
            if run.run_id != launched_run_id
                || run.task_id != task.id
                // A check's evaluation records itself on the task before its run.
                || (fixture.kind != Kind::Check && run.task_revision != task.revision)
                || (!fixture.run_existing
                    && !matches!(fixture.kind, Kind::Learning | Kind::Check)
                    && run.scheduled_for_ms != due)
            {
                return Err("scheduled run identity does not match".into());
            }
            // A run whose Pi never started has no admission to compare. Report
            // its own failure and kind, as an ordinary Chat turn does.
            if run.status != RunStatus::Succeeded {
                record["failureKind"] = observed
                    .terminal
                    .as_ref()
                    .map_or(Value::Null, |terminal| terminal["failure_kind"].clone());
                observed.pi.record_startup_failure(&mut record);
                return Err(observed
                    .pi
                    .error()
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| run.error.clone())
                    .unwrap_or_else(|| format!("scheduled run {}", run.status.as_str()))
                    .into());
            }
            let thread_id = run
                .thread_id
                .as_deref()
                .ok_or("scheduled Chat is missing")?;
            if record["admission"]["surface"] != "scheduled"
                || record["admission"]["run_id"] != run.run_id
                || record["admission"]["thread_id"] != thread_id
                || thread_id == home.id
            {
                return Err("scheduled run identity or admission does not match".into());
            }
            let thread = state
                .store
                .get_chat(thread_id)?
                .ok_or("scheduled Chat disappeared")?;
            let [.., user, source] = thread.messages.as_slice() else {
                return Err("expected one user and assistant pair in the scheduled Chat".into());
            };
            if fixture.kind == Kind::Briefing {
                let context = user
                    .content
                    .strip_prefix(&task.prompt)
                    .and_then(|suffix| suffix.strip_prefix('\n'))
                    .ok_or("scheduled briefing does not contain its host template")?;
                let data: Value = serde_json::from_str(context)?;
                if data["appLanguage"] != record["resolved_appearance"] {
                    return Err(
                        "scheduled briefing appLanguage differs from resolved appearance".into(),
                    );
                }
            }
            if user.role != ChatRole::User
                || record["admission"]["prompt"] != user.content
                || (fixture.kind == Kind::Custom && user.content != task.prompt)
                || source.role != ChatRole::Assistant
                || source.status != ChatMessageStatus::Complete
                || source.host_composed
                || run.message_id.as_deref() != Some(source.id.as_str())
                || record["terminal_event"]["status"] != "completed"
                || record["terminal_event"]["message_id"] != source.id
            {
                return Err("scheduled Chat prompt or terminal reply does not match".into());
            }
            // The answer is the run's own reply; Bot delivers its summary.
            let delivered = if run.quiet {
                if !deliveries.is_empty() || inbox_ids.iter().any(|id| id == &run.run_id) {
                    return Err("quiet run appeared in Bot or inbox".into());
                }
                None
            } else {
                let [(message, delivery)] = deliveries.as_slice() else {
                    return Err("expected exactly one persisted Bot delivery".into());
                };
                if delivery.run_id != run.run_id
                    || delivery.task_id != task.id
                    || delivery.task_kind != task.kind
                    || delivery.run_thread_id != run.thread_id
                    || delivery.status != run.status
                    || Some(delivery.finished_at_ms) != run.finished_at_ms
                    || message.role != ChatRole::Assistant
                    || message.status != ChatMessageStatus::Complete
                    || run.summary.is_empty()
                    || run.summary != crate::briefings::summary(&source.content)
                    || message.content != run.summary
                {
                    return Err(
                    "persisted Bot message is missing the scheduled reply's summary or attribution"
                        .into(),
                );
                }
                Some(message.content.clone())
            };
            let mut turn_record = trace::persisted(source, &observed.pi);
            trace::enrich_commits(state, &thread, source, &mut turn_record)?;
            turn_record["lens"] = trace::lens_state(state, &thread, Some(&source.id))?;
            if let Value::Object(turn_record) = turn_record {
                record.as_object_mut().unwrap().extend(turn_record);
            }
            record["session_answer"] = json!({
                "thread_id":thread.id, "message_id":source.id, "content":source.content,
                "status":source.status, "host_composed":source.host_composed,
            });
            record["answer"] = json!(source.content);
            if let Some(summary) = delivered {
                record["delivered_summary"] = json!(summary);
            }
            Ok(())
        })();
        if let Err(error) = evidence {
            record["error"] = trace::bounded_json(&json!(error.to_string()));
        }
    }
    // The entire app profile, including the completed task, is disposable. Keep
    // it intact so --keep can be used to audit the evidence against the store.
    observation::label_attempts(&mut record);
    record["started_at"] = json!(started);
    record["duration_ms"] = json!(clock.elapsed().as_millis() as u64);
    record["replay_fallbacks"] = json!(replay::take_fallbacks());
    Ok(record)
}
