//! A due occurrence in a disposable eval Loupist, observed through ordinary
//! scheduler admission, Chat completion and the persisted Bot Thread.
use super::{observation, *};
use crate::{
    commands::now_ms,
    domain::{ChatMessageStatus, ChatRole},
    scheduled_tasks::types::{RunStatus, Schedule, TaskDraft, TaskKind, TaskStatus, TaskTarget},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Fixture {
    kind: Kind,
    #[serde(default)]
    run_existing: bool,
    #[serde(default)]
    clock: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Briefing,
    Custom,
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
    if turn.briefing
        || turn.as_of.is_some()
        || !turn.attachments.is_empty()
        || scenario.sessions.len() != 1
        || (!fixture.run_existing && session.turns.len() != 1)
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

pub(super) async fn run(
    state: &AppState,
    home: &ChatSession,
    turn: &Turn,
    fixture: &Fixture,
    model: &str,
    thinking: &str,
    timeout_ms: u64,
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
    let task = if fixture.run_existing {
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
        while !fixture.run_existing && now_ms() < due {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        let claimed = state
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
            .ok_or("due task was not admitted")?;
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
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    };
    tokio::pin!(execution);
    let outcome = tokio::time::timeout(Duration::from_millis(timeout_ms), &mut execution).await;
    let mut record = json!({
        "answer": "", "status": "failed", "tool_calls": null,
        "saved_task": {"id":task.id,"kind":task.kind,"prompt":task.prompt},
        "home_thread_id": home.id,
    });
    let completed = match outcome {
        Ok(Ok(())) => true,
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
                || run.task_revision != task.revision
                || (!fixture.run_existing && run.scheduled_for_ms != due)
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
            let answer = if run.quiet {
                if !deliveries.is_empty() || inbox_ids.iter().any(|id| id == &run.run_id) {
                    return Err("quiet run appeared in Bot or inbox".into());
                }
                source.content.clone()
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
                message.content.clone()
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
            record["answer"] = json!(answer);
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
