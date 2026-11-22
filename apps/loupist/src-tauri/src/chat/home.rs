//! The Loupist's home Thread: one continuous, persisted conversation per Loupist
//! where the Loupist speaks first. Scheduled results arrive here as short host
//! messages; topic Chats never receive proactive messages.

use std::collections::HashSet;

use rusqlite::params;
use uuid::Uuid;

use crate::{
    app::{AppState, CommandError},
    commands::{map_internal, map_store, new_id, now_ms},
    domain::{
        memory_refs_digest, ChatMessage, ChatMessageStatus, ChatRole, ChatSession, HomeAlert,
        HomeDelivery, HomeLink, MemoryPolicy,
    },
    runs::coordinator::{RunKind, RunTarget},
    scheduled_tasks::types::{Run, RunStatus},
    store::StoreResult,
};

/// Stored title; the renderer shows the localized "Thread" label instead.
pub(crate) const HOME_TITLE: &str = "Thread";
pub(crate) const HANDOFF_PREFIX: &str = "bot-handoff";
/// Recent runs considered for delivery on each pass.
const DELIVERY_SCAN: usize = 50;

/// One home Thread per Loupist, by construction: the id is derived from the Loupist.
pub(crate) fn home_thread_id(loupist_id: &str) -> String {
    format!("home-{loupist_id}")
}

pub(crate) fn is_home(chat: &ChatSession) -> bool {
    chat.id == home_thread_id(&chat.loupist_id)
}

/// Returns the Loupist's home Thread, creating it with the Loupist's Chat defaults.
pub(crate) fn ensure_home_thread(
    state: &AppState,
    loupist_id: &str,
) -> Result<ChatSession, CommandError> {
    let id = home_thread_id(loupist_id);
    if let Some(chat) = state.store.get_chat(&id).map_err(map_store)? {
        return if chat.loupist_id == loupist_id {
            Ok(chat)
        } else {
            Err(CommandError::conflict(
                "home Thread belongs to another Loupist",
            ))
        };
    }
    let defaults = crate::commands::loupist_configuration(state, loupist_id)?.chat;
    let timestamp = now_ms();
    let chat = ChatSession {
        id,
        loupist_id: loupist_id.to_owned(),
        pi_session_id: Uuid::new_v4().to_string(),
        pi_session_cache: None,
        title: HOME_TITLE.into(),
        memory_policy: MemoryPolicy {
            use_memory: defaults.use_memory,
            update_memory: defaults.update_memory,
        },
        messages: Vec::new(),
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
        bot_memory: Default::default(),
    };
    match state.store.create_chat(&chat) {
        Ok(()) => Ok(chat),
        // A concurrent first open created it; read the winner.
        Err(error) => state
            .store
            .get_chat(&chat.id)
            .map_err(map_store)?
            .ok_or_else(|| map_store(error)),
    }
}

/// A finished run is announced unless it was quiet or the user ended it
/// (Stop, Quit). A run recovery found unfinished, after a crash, is announced
/// as one that could not finish.
fn deliverable(run: &Run, home: &ChatSession, delivered: &HashSet<String>) -> bool {
    (matches!(run.status, RunStatus::Succeeded | RunStatus::Failed)
        || (run.status == RunStatus::Interrupted
            && run.error.as_deref() == Some(crate::scheduled_tasks::UNEXPECTED_INTERRUPTION)))
        && !(run.quiet && run.status == RunStatus::Succeeded)
        && run
            .finished_at_ms
            .is_some_and(|finished| finished >= home.created_at_ms)
        && run.thread_id.as_deref() != Some(home.id.as_str())
        && !delivered.contains(&run.run_id)
}

/// The Thread message for one finished run: its first paragraph and where it
/// came from. Built from the run record and its task definition.
pub(crate) fn delivery_message(state: &AppState, run: &Run) -> Result<ChatMessage, CommandError> {
    let task = state
        .store
        .scheduled_task_get(&run.loupist_id, &run.task_id)
        .map_err(map_store)?;
    let finished = run.finished_at_ms.unwrap_or(run.started_at_ms);
    let skipped_saved_keys = run
        .skipped_tools
        .iter()
        .filter(|tool| tool.reason == crate::scheduled_tasks::types::SAVED_KEY_PERMISSION_SKIP)
        .map(|tool| tool.name.clone())
        .collect::<Vec<_>>();
    Ok(ChatMessage {
        tool_refusals: Vec::new(),
        feedback: None,
        id: new_id("delivery"),
        role: ChatRole::Assistant,
        status: ChatMessageStatus::Complete,
        // Model-readable through Chat history; the renderer shows its own
        // localized line for a run that did not finish.
        content: match run.status {
            RunStatus::Succeeded if !run.summary.trim().is_empty() => run.summary.clone(),
            RunStatus::Succeeded => format!("{} finished.", run.task_title),
            _ => format!("{} could not finish.", run.task_title),
        },
        created_at_ms: finished,
        lens_receipts: Vec::new(),
        memory_refs: Vec::new(),
        observed_exact_count: 0,
        refs_truncated: false,
        refs_digest: memory_refs_digest(&[]).map_err(map_internal)?,
        memory_update: None,
        memory_revision: None,
        execution_model: None,
        agent_harness: None,
        attachments: Vec::new(),
        artifact_refs: Vec::new(),
        evidence_refs: Vec::new(),
        schedule_receipts: Vec::new(),
        progress: None,
        performance: None,
        tool_trace: None,
        failure_kind: None,
        host_composed: true,
        steer: false,
        retry_after_seconds: None,
        replaced_model: None,
        delivery: Some(HomeDelivery {
            run_id: run.run_id.clone(),
            task_id: run.task_id.clone(),
            task_title: run.task_title.clone(),
            task_kind: run.kind.clone(),
            schedule: task.map(|task| task.schedule),
            run_thread_id: run.thread_id.clone(),
            status: run.status,
            memory_save_skip: run.memory_save_skip,
            discarded_memory_items: run.discarded_memory_items.clone(),
            finished_at_ms: finished,
            skipped_saved_keys: (!skipped_saved_keys.is_empty()).then_some(skipped_saved_keys),
            learned: run
                .message_id
                .as_ref()
                .filter(|_| run.memory_save_skip.is_none())
                .and_then(|id| super::handoff::card_learned(state, &run.loupist_id, id)),
        }),
        as_of: None,
        number_checks: Vec::new(),
        as_of_report: None,
        alert: None,

        vocabulary: None,
        handoff: None,
        research_session_id: None,
        handoff_note: None,
        handoff_action: None,
        scheduled: None,
        link: None,
    })
}

fn trim_number(value: f64) -> String {
    let text = format!("{value}");
    text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
}

/// The Thread message for a crossed line. Rust writes the facts; the renderer
/// shows its own localized sentence from the asset page's dated observation.
pub(crate) fn alert_message(
    alert: &crate::knowledge::assets::LineAlert,
) -> Result<ChatMessage, CommandError> {
    let unit = alert.line.unit.as_deref().unwrap_or_default();
    let value = if unit.is_empty() || unit == "%" {
        format!("{}{unit}", trim_number(alert.line.observed))
    } else {
        format!("{} {unit}", trim_number(alert.line.observed))
    };
    let set_on = chrono::DateTime::from_timestamp_millis(alert.line.set_at_ms)
        .map(|time| time.date_naive().to_string())
        .unwrap_or_default();
    let source = alert.evidence.first();
    Ok(ChatMessage {
        tool_refusals: Vec::new(),
        feedback: None,
        id: new_id("alert"),
        role: ChatRole::Assistant,
        status: ChatMessageStatus::Complete,
        // Model-readable through Chat history.
        content: format!(
            "{} crossed the line you set on {set_on}: {} — {value} ({}). Thesis marked Broken.",
            alert.name, alert.line.claim, alert.fact_date
        ),
        created_at_ms: alert.at_ms,
        lens_receipts: Vec::new(),
        memory_refs: Vec::new(),
        observed_exact_count: 0,
        refs_truncated: false,
        refs_digest: memory_refs_digest(&[]).map_err(map_internal)?,
        memory_update: None,
        memory_revision: None,
        execution_model: None,
        agent_harness: None,
        attachments: Vec::new(),
        artifact_refs: Vec::new(),
        evidence_refs: Vec::new(),
        schedule_receipts: Vec::new(),
        progress: None,
        performance: None,
        tool_trace: None,
        failure_kind: None,
        replaced_model: None,
        host_composed: true,
        steer: false,
        retry_after_seconds: None,
        delivery: None,
        as_of: None,
        number_checks: Vec::new(),
        as_of_report: None,
        alert: Some(HomeAlert {
            asset_id: alert.asset_id.clone(),
            move_id: alert.move_id.clone(),
            name: alert.name.clone(),
            symbol: alert.symbol.clone(),
            market: alert.market.clone(),
            claim: alert.line.claim.clone(),
            value,
            set_at_ms: alert.line.set_at_ms,
            fact_date: alert.fact_date.clone(),
            source_url: source.and_then(|evidence| evidence.url.clone()),
            source_title: source
                .map(|evidence| evidence.title.clone())
                .unwrap_or_default(),
        }),

        vocabulary: None,
        handoff: None,
        research_session_id: None,
        handoff_note: None,
        handoff_action: None,
        scheduled: None,
        link: None,
    })
}

/// A host message with nothing but its text; the caller sets its record.
pub(crate) fn host_message(
    prefix: &str,
    content: String,
    created_at_ms: i64,
) -> Result<ChatMessage, CommandError> {
    Ok(ChatMessage {
        tool_refusals: Vec::new(),
        feedback: None,
        id: new_id(prefix),
        role: ChatRole::Assistant,
        status: ChatMessageStatus::Complete,
        content,
        created_at_ms,
        lens_receipts: Vec::new(),
        memory_refs: Vec::new(),
        observed_exact_count: 0,
        refs_truncated: false,
        refs_digest: memory_refs_digest(&[]).map_err(map_internal)?,
        memory_update: None,
        memory_revision: None,
        execution_model: None,
        agent_harness: None,
        attachments: Vec::new(),
        artifact_refs: Vec::new(),
        evidence_refs: Vec::new(),
        schedule_receipts: Vec::new(),
        progress: None,
        performance: None,
        tool_trace: None,
        failure_kind: None,
        replaced_model: None,
        host_composed: true,
        steer: false,
        retry_after_seconds: None,
        delivery: None,
        as_of: None,
        number_checks: Vec::new(),
        as_of_report: None,
        alert: None,

        vocabulary: None,
        handoff: None,
        research_session_id: None,
        handoff_note: None,
        handoff_action: None,
        scheduled: None,
        link: None,
    })
}

/// One plain sentence per alert for Chat history (the renderer words its own).
pub(crate) fn vocabulary_message(
    change: &crate::knowledge::SchemaChange,
) -> Result<ChatMessage, CommandError> {
    let kinds = crate::knowledge::added_labels(change);
    let labels: Vec<String> = kinds.iter().map(|kind| kind.labels.en.clone()).collect();
    let korean: Vec<String> = kinds
        .iter()
        .filter_map(|kind| kind.labels.ko.clone())
        .collect();
    let mut message = host_message(
        "vocabulary",
        format!("Your World now also tracks {}.", labels.join(", ")),
        change.created_at_ms,
    )?;
    message.vocabulary = Some(crate::domain::HomeVocabulary {
        change_id: change.change_id.clone(),
        producer: change.producer.as_str().into(),
        labels_ko: if korean.len() == labels.len() {
            korean
        } else {
            Vec::new()
        },
        labels,
    });
    Ok(message)
}

pub(crate) fn link_message(
    view: &crate::knowledge::ConnectionView,
    at_ms: i64,
) -> Result<ChatMessage, CommandError> {
    let through: Vec<&str> = view
        .path
        .iter()
        .take(view.path.len().saturating_sub(1))
        .map(|hop| hop.name.as_str())
        .collect();
    let mut message = host_message(
        "link",
        format!(
            "New link: {} ↔ {} through {}.",
            view.seed_name,
            view.far_name,
            through.join(", ")
        ),
        at_ms,
    )?;
    message.link = Some(HomeLink::from(view));
    Ok(message)
}

fn pending_links(
    state: &AppState,
    loupist_id: &str,
) -> Result<Vec<crate::knowledge::ConnectionView>, CommandError> {
    let at = now_ms();
    crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        crate::knowledge::connections_to_show(tx, loupist_id, at)
    })
}

fn pending_vocabulary(
    state: &AppState,
    loupist_id: &str,
    home: &ChatSession,
) -> Result<Vec<crate::knowledge::SchemaChange>, CommandError> {
    let posted: HashSet<&str> = all_messages(home)
        .filter_map(|message| message.vocabulary.as_ref())
        .map(|record| record.change_id.as_str())
        .collect();
    let mut changes = crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        crate::knowledge::recent_changes(tx, loupist_id, 20)
    })?;
    // Creation presets stay in Settings; chat changes stay on their reply's receipt.
    changes.retain(|change| {
        change.producer == crate::knowledge::Producer::Review
            && change.origin.is_none()
            && !posted.contains(change.change_id.as_str())
            && !crate::knowledge::added_labels(change).is_empty()
    });
    changes.reverse();
    Ok(changes)
}

/// Host deliveries belong to the conversation even when an edit hides its tail.
fn all_messages(chat: &ChatSession) -> impl Iterator<Item = &ChatMessage> {
    chat.messages.iter().chain(
        chat.versions
            .iter()
            .flat_map(|version| version.messages.iter()),
    )
}

/// Crossed lines since the Thread began that it has not announced yet.
fn pending_alerts(
    state: &AppState,
    loupist_id: &str,
    home: &ChatSession,
) -> Result<Vec<crate::knowledge::assets::LineAlert>, CommandError> {
    let posted: HashSet<String> = all_messages(home)
        .filter_map(|message| message.alert.as_ref().map(|alert| alert.move_id.clone()))
        .collect();
    Ok(
        crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
            crate::knowledge::assets::line_alerts(tx, loupist_id, home.created_at_ms)
        })?
        .into_iter()
        .filter(|alert| !posted.contains(&alert.move_id))
        .collect(),
    )
}

fn ensure_price_setup_replies(db: &rusqlite::Connection) -> StoreResult<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS bot_price_setup_replies (
            loupist_id TEXT NOT NULL REFERENCES loupist_profiles(id) ON DELETE CASCADE,
            run_id TEXT NOT NULL,
            content TEXT NOT NULL,
            finished_at_ms INTEGER NOT NULL,
            PRIMARY KEY (loupist_id, run_id)
        ) STRICT;",
    )?;
    Ok(())
}

pub(crate) fn queue_price_setup_reply(
    state: &AppState,
    loupist_id: &str,
    run_id: &str,
    content: &str,
) -> Result<(), CommandError> {
    {
        let db = state.store.lock_loupist(loupist_id).map_err(map_store)?;
        ensure_price_setup_replies(&db).map_err(map_store)?;
        db.execute(
            "INSERT OR IGNORE INTO bot_price_setup_replies VALUES (?1,?2,?3,?4)",
            params![loupist_id, run_id, content, now_ms()],
        )
        .map_err(map_internal)?;
    }
    deliver_finished_runs(state, loupist_id)?;
    Ok(())
}

fn pending_price_setup_replies(
    state: &AppState,
    loupist_id: &str,
) -> StoreResult<Vec<(String, String, i64)>> {
    let db = state.store.lock_loupist(loupist_id)?;
    ensure_price_setup_replies(&db)?;
    let mut select = db.prepare(
        "SELECT run_id, content, finished_at_ms FROM bot_price_setup_replies WHERE loupist_id=?1",
    )?;
    let rows = select.query_map([loupist_id], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Posts pending deliveries into the home Thread, oldest first. A reply
/// running in the Thread owns it, so delivery waits for the next completion
/// or workspace load. Every write tells the renderer. Returns how many
/// messages were posted or updated.
pub(crate) fn deliver_finished_runs(
    state: &AppState,
    loupist_id: &str,
) -> Result<usize, CommandError> {
    let home = ensure_home_thread(state, loupist_id)?;
    let delivered_in = |chat: &ChatSession| -> HashSet<String> {
        all_messages(chat)
            .filter_map(|message| message.delivery.as_ref().map(|d| d.run_id.clone()))
            .collect()
    };
    let runs = state
        .store
        .inbox_runs(loupist_id, None, DELIVERY_SCAN)
        .map_err(map_store)?;
    let questions = crate::scheduled_tasks::questions::pending(state, loupist_id)?;
    let handoffs = state
        .store
        .scheduled_tasks_list(loupist_id)
        .map_err(map_store)?
        .into_iter()
        .filter(|task| task.id.starts_with(HANDOFF_PREFIX))
        .collect::<Vec<_>>();
    let delivered = delivered_in(&home);
    let research = super::handoff::for_delivery(state, loupist_id)?;
    let research_changes = |chat: &ChatSession,
                            research: &[super::handoff::Handoff]|
     -> Vec<(super::handoff::Handoff, Option<usize>)> {
        research
            .iter()
            .filter_map(|handoff| {
                let posted = chat.messages.iter().position(|message| {
                    message
                        .handoff
                        .as_ref()
                        .is_some_and(|record| record.handoff_id == handoff.id)
                });
                match posted {
                    Some(index)
                        if chat.messages[index].handoff.as_ref().is_some_and(|record| {
                            record.status == handoff.status
                                && record.connection_id.as_deref() == handoff.link_connection_id()
                                && record.stop_reason == handoff.stop_reason
                                && record.memory_save_skip == handoff.memory_save_skip
                                && record.discarded_memory_items == handoff.discarded_memory_items
                        }) =>
                    {
                        None
                    }
                    None if chat
                        .versions
                        .iter()
                        .flat_map(|version| &version.messages)
                        .any(|message| {
                            message
                                .handoff
                                .as_ref()
                                .is_some_and(|record| record.handoff_id == handoff.id)
                        }) =>
                    {
                        None
                    }
                    posted => Some((handoff.clone(), posted)),
                }
            })
            .collect()
    };
    if !runs.iter().any(|run| deliverable(run, &home, &delivered))
        && handoffs
            .iter()
            .all(|task| all_messages(&home).any(|m| m.id == task.id))
        && questions
            .iter()
            .all(|q| all_messages(&home).any(|m| m.id == q.receipt.task_id))
        && research_changes(&home, &research).is_empty()
        && pending_alerts(state, loupist_id, &home)?.is_empty()
        && pending_vocabulary(state, loupist_id, &home)?.is_empty()
        && pending_links(state, loupist_id)?.is_empty()
        && pending_price_setup_replies(state, loupist_id)
            .map_err(map_store)?
            .is_empty()
    {
        return Ok(0);
    }
    let Ok(_mutation) = state.register_run(
        new_id("thread-delivery"),
        loupist_id.to_owned(),
        RunKind::ChatMutation,
        RunTarget::ChatThread(home.id.clone()),
    ) else {
        return Ok(0);
    };
    let expected = state
        .store
        .get_chat(&home.id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("home Thread"))?;
    let delivered = delivered_in(&expected);
    let mut pending = runs
        .iter()
        .filter(|run| deliverable(run, &expected, &delivered))
        .collect::<Vec<_>>();
    pending.sort_by_key(|run| (run.finished_at_ms, run.run_id.clone()));
    let mut posted = Vec::new();
    for q in &questions {
        if !all_messages(&expected).any(|m| m.id == q.receipt.task_id) {
            let mut m = host_message(
                "bot-question",
                format!("{}\n\n{}", q.prompt, q.receipt.question),
                q.created_at_ms,
            )?;
            m.id = q.receipt.task_id.clone();
            posted.push(m);
        }
    }
    for task in &handoffs {
        if all_messages(&expected).any(|message| message.id == task.id) {
            continue;
        }
        let mut message = host_message("bot-handoff", task.title.clone(), task.created_at_ms)?;
        message.id = task.id.clone();
        message
            .schedule_receipts
            .push(crate::scheduled_tasks::types::ScheduleReceipt::from_task(
                task,
                crate::scheduled_tasks::types::ScheduleAction::Created,
            ));
        posted.push(message);
    }
    for run in &pending {
        posted.push(delivery_message(state, run)?);
    }
    let price_setup_replies = pending_price_setup_replies(state, loupist_id).map_err(map_store)?;
    for (_, content, at) in &price_setup_replies {
        let mut message = host_message("price-setup-reply", content.clone(), *at)?;
        message.host_composed = false;
        posted.push(message);
    }
    let mut chat = expected.clone();
    let mut changed = 0;
    let research = super::handoff::for_delivery(state, loupist_id)?;
    for (handoff, index) in research_changes(&expected, &research) {
        let message =
            super::handoff::message(state, &handoff, index.map(|i| &expected.messages[i]))?;
        match index {
            Some(index) => {
                chat.messages[index] = message;
                changed += 1;
            }
            None => posted.push(message),
        }
    }
    for alert in pending_alerts(state, loupist_id, &expected)? {
        posted.push(alert_message(&alert)?);
    }
    for change in pending_vocabulary(state, loupist_id, &expected)? {
        posted.push(vocabulary_message(&change)?);
    }
    let links = pending_links(state, loupist_id)?;
    let link_ids: Vec<String> = links
        .iter()
        .map(|link| link.connection_id.clone())
        .collect();
    for link in &links {
        posted.push(link_message(link, now_ms())?);
    }
    if posted.is_empty() && changed == 0 {
        return Ok(0);
    }
    posted.sort_by_key(|message| message.created_at_ms);
    let count = posted.len() + changed;
    chat.messages.extend(posted);
    // The Pi transcript no longer matches; the next turn starts cold and can
    // read these messages through Chat history.
    chat.pi_session_cache = None;
    chat.updated_at_ms = now_ms().max(expected.updated_at_ms.saturating_add(1));
    let at = now_ms();
    state
        .store
        .replace_chat_with(&expected, &chat, |tx| {
            for (run_id, _, _) in &price_setup_replies {
                tx.execute(
                    "DELETE FROM bot_price_setup_replies WHERE loupist_id=?1 AND run_id=?2",
                    params![loupist_id, run_id],
                )?;
            }
            if !link_ids.is_empty() {
                crate::knowledge::connections_mark_shown(tx, loupist_id, &link_ids, at)
                    .map_err(|error| crate::store::StoreError::Invalid(error.to_string()))?;
            }
            Ok(())
        })
        .map_err(map_store)?;
    crate::briefings::desktop::changed(loupist_id);
    Ok(count)
}
