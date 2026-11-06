//! The Loupist's home Thread: one continuous, persisted conversation per Loupist
//! where the Loupist speaks first. Scheduled results arrive here as short host
//! messages; topic Chats never receive proactive messages.

use std::collections::HashSet;

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
    store::LoupistStore,
};

/// Stored title; the renderer shows the localized "Thread" label instead.
pub(crate) const HOME_TITLE: &str = "Thread";
/// Recent runs considered for delivery on each pass.
pub(crate) const HANDOFF_PREFIX: &str = "bot-handoff";
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

fn deliverable(run: &Run, home: &ChatSession, delivered: &HashSet<String>) -> bool {
    matches!(run.status, RunStatus::Succeeded | RunStatus::Failed)
        && run
            .finished_at_ms
            .is_some_and(|finished| finished >= home.created_at_ms)
        && run.thread_id.as_deref() != Some(home.id.as_str())
        && !delivered.contains(&run.run_id)
}

/// The Thread message for one finished run: its first paragraph and where it
/// came from. Built only from the run record and its task definition, never
/// from holdings, so quantities and values cannot reach a proactive message.
pub(crate) fn delivery_message(state: &AppState, run: &Run) -> Result<ChatMessage, CommandError> {
    let task = state
        .store
        .scheduled_task_get(&run.loupist_id, &run.task_id)
        .map_err(map_store)?;
    let finished = run.finished_at_ms.unwrap_or(run.started_at_ms);
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
            task_kind: run.kind,
            schedule: task.map(|task| task.schedule),
            run_thread_id: run.thread_id.clone(),
            status: run.status,
            finished_at_ms: finished,
            learned: run
                .message_id
                .as_ref()
                .and_then(|id| super::handoff::card_learned(state, &run.loupist_id, id)),
        }),
        as_of: None,
        number_checks: Vec::new(),
        sensitivity_cards: Vec::new(),
        as_of_report: None,
        alert: None,
        watch: None,
        earnings: None,
        vocabulary: None,
        handoff: None,
        link: None,
    })
}

fn trim_number(value: f64) -> String {
    let text = format!("{value}");
    text.strip_suffix(".0").map(str::to_owned).unwrap_or(text)
}

/// The Thread message for a crossed line. Rust writes the facts; the renderer
/// shows its own localized sentence. Never holdings, never quantities.
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
        sensitivity_cards: Vec::new(),
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
        watch: None,
        earnings: None,
        vocabulary: None,
        handoff: None,
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
        sensitivity_cards: Vec::new(),
        as_of_report: None,
        alert: None,
        watch: None,
        earnings: None,
        vocabulary: None,
        handoff: None,
        link: None,
    })
}

/// One plain sentence per alert for Chat history (the renderer words its own).
fn watch_sentence(event: &crate::knowledge::watch::WatchEventView) -> String {
    use crate::knowledge::watch::WatchKind;
    let facts = &event.facts;
    let text = |key: &str| facts[key].as_str().unwrap_or_default().to_owned();
    match event.kind {
        WatchKind::Filings => format!(
            "{} filed {} ({}, {}).",
            event.name,
            text("title"),
            text("form"),
            event.fact_date
        ),
        WatchKind::Results => format!(
            "{} reports {} results on {}.",
            event.name,
            text("period"),
            text("date")
        ),
        WatchKind::Insiders if facts["buyNotChecked"] == true => format!(
            "{}: {} insider ownership reports between {} and {}.",
            event.name,
            facts["count"],
            text("from"),
            text("to")
        ),
        WatchKind::Insiders => format!(
            "{}: {} insiders bought shares between {} and {}.",
            event.name,
            facts["count"],
            text("from"),
            text("to")
        ),
        WatchKind::Holders => format!(
            "{}: a big holder filed {} ({}).",
            event.name,
            text("title"),
            event.fact_date
        ),
        WatchKind::Flows => format!(
            "{}: foreign and institution buying turned {} over 10 trading days (to {}).",
            event.name,
            if text("direction") == "out" {
                "negative"
            } else {
                "positive"
            },
            text("to")
        ),
        WatchKind::Dividends => format!(
            "{} dividend: record date {}.",
            event.name,
            facts["recordDate"].as_str().unwrap_or("not stated")
        ),
        WatchKind::CryptoMove | WatchKind::FuturesMove => {
            let change = facts["change"].as_f64().unwrap_or_default();
            format!(
                "{} {} {:.1}% {} (to {}).",
                event.name,
                if change < 0.0 { "fell" } else { "rose" },
                change.abs() * 100.0,
                if facts["window"] == "day" {
                    "in a day"
                } else {
                    "over 7 days"
                },
                text("to")
            )
        }
        WatchKind::Drawdown => format!(
            "{} is {:.1}% below its 90-day high of {} ({}).",
            event.name,
            facts["drawdown"].as_f64().unwrap_or_default().abs() * 100.0,
            text("highDate"),
            text("to")
        ),
        WatchKind::Depeg => format!(
            "{} closed at {} on {}, off its $1 peg.",
            event.name, facts["close"], event.fact_date
        ),
        WatchKind::Tvl => format!(
            "{}: value locked on {} fell {:.1}% over 7 days (to {}).",
            event.name,
            text("target"),
            facts["change"].as_f64().unwrap_or_default().abs() * 100.0,
            text("to")
        ),
        WatchKind::Positioning => format!(
            "{}: managed money turned net {} (CFTC, as of {}).",
            event.name,
            text("direction"),
            text("to")
        ),
        WatchKind::OilInventory => match facts["change"].as_f64() {
            Some(change) => format!(
                "{}: U.S. commercial crude stocks {} {:.1} million barrels in the week to {} (EIA).",
                event.name,
                if change < 0.0 { "fell" } else { "rose" },
                change.abs() / 1000.0,
                text("to")
            ),
            None => format!(
                "{}: new EIA weekly petroleum report (week to {}).",
                event.name,
                text("to")
            ),
        },
    }
}

/// The Thread message for one batch of watcher alerts: five, then "+n more".
pub(crate) fn watch_message(
    events: &[crate::knowledge::watch::WatchEventView],
    at_ms: i64,
) -> Result<ChatMessage, CommandError> {
    let shown: Vec<_> = events
        .iter()
        .take(crate::commands::watch::MESSAGE_EVENTS)
        .collect();
    let mut message = host_message(
        "watch",
        shown
            .iter()
            .map(|event| watch_sentence(event))
            .collect::<Vec<_>>()
            .join("\n"),
        at_ms,
    )?;
    message.watch = Some(crate::domain::HomeWatch {
        event_ids: events.iter().map(|event| event.event_id.clone()).collect(),
        events: shown
            .iter()
            .filter_map(|event| serde_json::to_value(event).ok())
            .collect(),
        more: events.len().saturating_sub(shown.len()),
    });
    Ok(message)
}

/// The Thread message for reported results (two measures) or a failed capture.
pub(crate) fn earnings_message(
    notice: &crate::knowledge::earnings::Notice,
    snapshot: &crate::knowledge::earnings::SnapshotView,
) -> Result<ChatMessage, CommandError> {
    let reported = notice.outcome == "reported";
    let content = if reported {
        format!("{} reported {} results.", snapshot.name, snapshot.period)
    } else {
        format!(
            "Couldn't capture expectations for {} before results.",
            snapshot.name
        )
    };
    let mut measures: Vec<&crate::knowledge::earnings::MeasureView> = snapshot
        .measures
        .iter()
        .filter(|measure| !measure.guidance && !measure.reported.is_empty())
        .collect();
    // Rows with something to compare first.
    measures.sort_by_key(|measure| measure.expected.is_empty());
    let mut message = host_message("earnings", content, notice.at_ms)?;
    message.earnings = Some(crate::domain::HomeEarnings {
        asset_id: snapshot.asset_id.clone(),
        event_id: snapshot.event_id.clone(),
        name: snapshot.name.clone(),
        symbol: snapshot.symbol.clone(),
        market: snapshot.market.clone(),
        period: snapshot.period.clone(),
        outcome: notice.outcome.clone(),
        side: notice.side.clone(),
        measures: if reported {
            measures
                .into_iter()
                .take(2)
                .filter_map(|measure| serde_json::to_value(measure).ok())
                .collect()
        } else {
            Vec::new()
        },
    });
    Ok(message)
}

/// Watcher alerts found and not delivered yet, and earnings notices owed.
fn pending_host_news(
    state: &AppState,
    loupist_id: &str,
) -> (
    Vec<crate::knowledge::watch::WatchEventView>,
    Vec<(
        crate::knowledge::earnings::Notice,
        crate::knowledge::earnings::SnapshotView,
    )>,
) {
    let (events, notices) = crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        let notices = crate::knowledge::earnings::pending_notices(tx, loupist_id)?
            .into_iter()
            .filter_map(|notice| {
                crate::knowledge::earnings::get_event(tx, loupist_id, &notice.event_id)
                    .ok()
                    .flatten()
                    .map(|event| (notice, event.asset_id))
            })
            .collect::<Vec<_>>();
        Ok((crate::knowledge::watch::pending(tx, loupist_id)?, notices))
    })
    .unwrap_or_default();
    let notices = notices
        .into_iter()
        .filter_map(|(notice, asset_id)| {
            crate::commands::earnings::snapshot_for(
                state,
                loupist_id,
                &asset_id,
                Some(&notice.event_id),
            )
            .ok()
            .flatten()
            .map(|snapshot| (notice, snapshot))
        })
        .collect();
    (events, notices)
}

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
        format!("Your map now also tracks {}.", labels.join(", ")),
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

fn pending_links(state: &AppState, loupist_id: &str) -> Vec<crate::knowledge::ConnectionView> {
    let at = now_ms();
    crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        crate::knowledge::connections_to_show(tx, loupist_id, at)
    })
    .unwrap_or_default()
}

fn pending_vocabulary(
    state: &AppState,
    loupist_id: &str,
    home: &ChatSession,
) -> Vec<crate::knowledge::SchemaChange> {
    let posted: HashSet<&str> = home
        .messages
        .iter()
        .filter_map(|message| message.vocabulary.as_ref())
        .map(|record| record.change_id.as_str())
        .collect();
    let mut changes = crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        crate::knowledge::recent_changes(tx, loupist_id, 20)
    })
    .unwrap_or_default();
    // A change a chat reply made is already on that reply's receipt.
    changes.retain(|change| {
        matches!(
            change.producer,
            crate::knowledge::Producer::Preset | crate::knowledge::Producer::Review
        ) && change.origin.is_none()
            && !posted.contains(change.change_id.as_str())
            && !crate::knowledge::added_labels(change).is_empty()
    });
    changes.reverse();
    changes
}

/// Crossed lines since the Thread began that it has not announced yet.
fn pending_alerts(
    state: &AppState,
    loupist_id: &str,
    home: &ChatSession,
) -> Vec<crate::knowledge::assets::LineAlert> {
    let posted: HashSet<String> = home
        .messages
        .iter()
        .filter_map(|message| message.alert.as_ref().map(|alert| alert.move_id.clone()))
        .collect();
    crate::commands::assets::with_assets(state, loupist_id, false, |tx| {
        crate::knowledge::assets::line_alerts(tx, loupist_id, home.created_at_ms)
    })
    .unwrap_or_default()
    .into_iter()
    .filter(|alert| !posted.contains(&alert.move_id))
    .collect()
}

/// Posts every finished, undelivered scheduled run into the home Thread, oldest
/// first. A reply running in the Thread owns it, so delivery waits: the next
/// completion or workspace load delivers what is still pending. Returns how
/// many messages were posted.
pub(crate) fn deliver_finished_runs(
    state: &AppState,
    loupist_id: &str,
) -> Result<usize, CommandError> {
    let home = ensure_home_thread(state, loupist_id)?;
    let delivered_in = |chat: &ChatSession| -> HashSet<String> {
        chat.messages
            .iter()
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
    let (watch_events, notices) = pending_host_news(state, loupist_id);
    let research = super::handoff::for_delivery(state, loupist_id)?;
    let research_changes = |chat: &ChatSession| -> Vec<(super::handoff::Handoff, Option<usize>)> {
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
                        if chat.messages[index]
                            .handoff
                            .as_ref()
                            .map(|record| record.status)
                            == Some(handoff.status) =>
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
            .all(|task| home.messages.iter().any(|m| m.id == task.id))
        && questions
            .iter()
            .all(|q| home.messages.iter().any(|m| m.id == q.receipt.task_id))
        && research_changes(&home).is_empty()
        && pending_alerts(state, loupist_id, &home).is_empty()
        && pending_vocabulary(state, loupist_id, &home).is_empty()
        && pending_links(state, loupist_id).is_empty()
        && watch_events.is_empty()
        && notices.is_empty()
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
        if !expected.messages.iter().any(|m| m.id == q.receipt.task_id) {
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
        if expected
            .messages
            .iter()
            .any(|message| message.id == task.id)
        {
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
    let mut chat = expected.clone();
    let mut changed = 0;
    for (handoff, index) in research_changes(&expected) {
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
    for alert in pending_alerts(state, loupist_id, &expected) {
        posted.push(alert_message(&alert)?);
    }
    for change in pending_vocabulary(state, loupist_id, &expected) {
        posted.push(vocabulary_message(&change)?);
    }
    let links = pending_links(state, loupist_id);
    let link_ids: Vec<String> = links
        .iter()
        .map(|link| link.connection_id.clone())
        .collect();
    for link in &links {
        posted.push(link_message(link, now_ms())?);
    }
    let (watch_events, notices) = pending_host_news(state, loupist_id);
    let watch_ids: Vec<String> = watch_events
        .iter()
        .map(|event| event.event_id.clone())
        .collect();
    if !watch_events.is_empty() {
        posted.push(watch_message(&watch_events, now_ms())?);
    }
    for (notice, snapshot) in &notices {
        posted.push(earnings_message(notice, snapshot)?);
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
    state
        .store
        .replace_chat(&expected, &chat)
        .map_err(map_store)?;
    // Delivered once: the alerts and notices now live in the Thread.
    let notice_ids: Vec<String> = notices
        .iter()
        .map(|(notice, _)| notice.notice_id.clone())
        .collect();
    if !watch_ids.is_empty() || !notice_ids.is_empty() || !link_ids.is_empty() {
        let at = now_ms();
        crate::commands::assets::with_assets(state, loupist_id, true, |tx| {
            crate::knowledge::watch::mark_delivered(tx, loupist_id, &watch_ids, "message", at)?;
            crate::knowledge::earnings::mark_notices_posted(tx, loupist_id, &notice_ids, at)?;
            crate::knowledge::connections_mark_shown(tx, loupist_id, &link_ids, at)
        })?;
        crate::commands::watch::changed(loupist_id);
    }
    Ok(count)
}
