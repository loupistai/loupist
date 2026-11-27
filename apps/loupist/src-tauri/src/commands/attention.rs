//! What waits for the user across Loupists: refused tools that still need approval,
//! live requests for the user to answer, unread scheduled results and
//! Journal checks coming due. The sidebar's "Needs attention" group renders this
//! list; Rust decides what belongs in it.
use std::{
    collections::HashMap,
    path::Path,
    sync::Mutex,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::{
    app::{AppState, CommandError},
    custom_connections::service,
    domain::{ChatRole, ChatToolRefusal, ChatToolRefusalReason},
};

/// Refusals older than this stop asking for attention.
const WAITING_WINDOW_MS: i64 = 14 * 24 * 60 * 60 * 1000;
/// Checks due within this many days (or already late) are listed.
const CHECK_HORIZON_DAYS: i64 = 7;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// One OS notification per chat per minute.
const NOTIFY_INTERVAL: Duration = Duration::from_secs(60);
const PREVIEW_CHARS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) enum AttentionKind {
    Approval,
    /// A live ask_user call is waiting in this chat.
    Question,
    /// A line the user set on an asset page was crossed; shown until the page opens.
    LineCrossed,
    SavedKeyPermission,
    BriefingConflict,
    Proposal,
    TaskPaused,
    UnreadResults,
    /// Watcher alerts delivered but not on screen yet.
    CheckDue,
    /// A check stopped after repeated code errors.
    TriggerFailing,
    /// A newer version of a check is waiting to be turned on.
    TriggerUpdate,
    /// A preset offer waiting on the home Thread.
    Offer,
    /// A question a scheduled run left on the home Thread.
    Ask,
    /// A watch has reported nothing for a long streak.
    WatchQuiet,
    /// A watch has delivered several results in a short window.
    WatchLoud,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttentionItem {
    pub kind: AttentionKind,
    pub loupist_id: String,
    pub loupist_name: String,
    /// The chat to open for an approval or a question.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub chat_id: Option<String>,
    /// The Journal entry for a check.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub record_id: Option<String>,
    /// What the row names: a tool, the chat's title for a question, a check claim, or empty for briefings.
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub due_date: Option<String>,
    pub at_ms: i64,
}

fn rank(kind: AttentionKind) -> u8 {
    match kind {
        // Approvals and questions both wait on the user's reply; newest first between them.
        AttentionKind::Approval
        | AttentionKind::Question
        | AttentionKind::Proposal
        | AttentionKind::TaskPaused
        | AttentionKind::Offer
        | AttentionKind::Ask => 1,
        AttentionKind::WatchLoud | AttentionKind::WatchQuiet => 2,
        AttentionKind::SavedKeyPermission => 0,
        AttentionKind::LineCrossed => 2,
        AttentionKind::BriefingConflict => 3,
        AttentionKind::UnreadResults => 4,
        AttentionKind::CheckDue => 5,
        AttentionKind::TriggerFailing | AttentionKind::TriggerUpdate => 1,
    }
}

/// Saved-key permission first, then waiting questions, approvals, and results.
/// Within a kind, newest first except for checks, which sort by due date.
pub(crate) fn sort_items(items: &mut [AttentionItem]) {
    items.sort_by(|a, b| {
        rank(a.kind).cmp(&rank(b.kind)).then_with(|| {
            if a.kind == AttentionKind::CheckDue {
                a.due_date.cmp(&b.due_date)
            } else {
                b.at_ms.cmp(&a.at_ms)
            }
        })
    });
}

/// Days from the local date `today` (YYYY-MM-DD) to `by_date`, or None if unparseable.
pub(crate) fn days_until(today: chrono::NaiveDate, by_date: &str) -> Option<i64> {
    let due = chrono::NaiveDate::parse_from_str(by_date.get(..10)?, "%Y-%m-%d").ok()?;
    Some((due - today).num_days())
}

#[tauri::command]
pub(crate) async fn attention_list(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<AttentionItem>, CommandError> {
    let items = collect(&state).await?;
    show_indicator(&app, &items);
    Ok(items)
}

/// What the window title and the Dock badge count while the window is not
/// focused: rows of the Needs attention list that arrived since the user last
/// had the window in front. Checks coming due wait for their date in the
/// sidebar and are not counted.
#[derive(Debug, Default)]
pub(crate) struct Indicator {
    /// Each row the user has seen, by key, with its count and time then.
    seen: HashMap<String, (u64, i64)>,
}

impl Indicator {
    fn key(item: &AttentionItem) -> String {
        format!(
            "{:?}\u{1f}{}\u{1f}{}\u{1f}{}",
            item.kind,
            item.loupist_id,
            item.chat_id.as_deref().unwrap_or_default(),
            item.record_id.as_deref().unwrap_or_default()
        )
    }

    /// With focus every row counts as seen and nothing is shown; without it,
    /// the rows and the grouped results or alerts that arrived since.
    pub(crate) fn update(&mut self, items: &[AttentionItem], focused: bool) -> u64 {
        let rows = items
            .iter()
            .filter(|item| item.kind != AttentionKind::CheckDue);
        if focused {
            self.seen = rows
                .map(|item| (Self::key(item), (item.count.unwrap_or(1), item.at_ms)))
                .collect();
            return 0;
        }
        rows.map(|item| {
            let count = item.count.unwrap_or(1);
            match self.seen.get(&Self::key(item)) {
                None => count,
                Some((seen, at_ms)) if item.at_ms > *at_ms => count.saturating_sub(*seen).max(1),
                Some(_) => 0,
            }
        })
        .fold(0, u64::saturating_add)
    }
}

static INDICATOR: std::sync::LazyLock<Mutex<Indicator>> =
    std::sync::LazyLock::new(|| Mutex::new(Indicator::default()));

/// "(3) Loupist" and a Dock badge of 3, or the plain title and no badge.
fn show_indicator(app: &tauri::AppHandle, items: &[AttentionItem]) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let focused = window.is_focused().unwrap_or(false);
    let count = INDICATOR
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .update(items, focused);
    paint_indicator(app, &window, count);
}

fn paint_indicator(app: &tauri::AppHandle, window: &tauri::WebviewWindow, count: u64) {
    let title = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "main")
        .map_or("Loupist", |config| config.title.as_str());
    let _ = window.set_title(&if count == 0 {
        title.to_owned()
    } else {
        format!("({count}) {title}")
    });
    // macOS shows the Dock badge; platforms without one ignore it.
    let _ = window.set_badge_count((count > 0).then(|| i64::try_from(count).unwrap_or(i64::MAX)));
}

/// Recounts from Rust's list, for changes the renderer may not be awake to ask about.
pub(crate) fn refresh_indicator(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(state) = app.try_state::<AppState>() {
            if let Ok(items) = collect(&state).await.inspect_err(|error| {
                eprintln!("Could not refresh attention indicator: {}", error.code);
            }) {
                show_indicator(&app, &items);
            }
        }
    });
}

/// Focus clears the title count and the badge at once; the recount then marks
/// what is on the list as seen.
pub(crate) fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() != "main" {
        return;
    }
    if let tauri::WindowEvent::Focused(focused) = event {
        if *focused {
            if let Some(main) = window.app_handle().get_webview_window("main") {
                paint_indicator(window.app_handle(), &main, 0);
            }
        }
        refresh_indicator(window.app_handle());
    }
}

pub(crate) async fn collect(state: &AppState) -> Result<Vec<AttentionItem>, CommandError> {
    let now = crate::commands::now_ms();
    let today = chrono::Local::now().date_naive();
    let loupists = state.store.list_loupists().map_err(super::map_store)?;
    let mut items = Vec::new();
    let permission_count = crate::app::off_main_thread(state, |state| {
        crate::marketplace::reconcile_credential_presence(state)?;
        Ok(crate::marketplace::key_permission::entries_needing_permission(state)?.len())
    })
    .await?;
    if permission_count > 0 {
        items.push(AttentionItem {
            kind: AttentionKind::SavedKeyPermission,
            loupist_id: String::new(),
            loupist_name: String::new(),
            chat_id: None,
            record_id: None,
            title: String::new(),
            count: Some(permission_count as u64),
            due_date: None,
            at_ms: 0,
        });
    }
    let mut refused: Vec<(String, String, String, i64, Vec<ChatToolRefusal>)> = Vec::new();
    for loupist in &loupists {
        if state.ensure_loupist_available(&loupist.id).is_err() {
            continue;
        }
        let chats = state
            .store
            .chats_ending_in_refusals(&loupist.id, now - WAITING_WINDOW_MS)
            .map_err(super::map_store)?;
        for (chat_id, _title, at_ms, json) in chats {
            let refusals: Vec<ChatToolRefusal> =
                serde_json::from_str(&json).unwrap_or_else(|error| {
                    eprintln!("Could not decode Chat tool refusals: {error}");
                    Vec::new()
                });
            let needs: Vec<_> = refusals
                .into_iter()
                .filter(|item| item.reason == ChatToolRefusalReason::NeedsApproval)
                .collect();
            if !needs.is_empty() {
                refused.push((
                    loupist.id.clone(),
                    loupist.name.clone(),
                    chat_id,
                    at_ms,
                    needs,
                ));
            }
        }
        for (chat_id, at_ms) in crate::chat::questions::waiting_chats(&loupist.id) {
            let Some(chat) = state
                .store
                .get_chat(&chat_id)
                .map_err(super::map_store)?
                .filter(|chat| chat.loupist_id == loupist.id)
            else {
                continue;
            };
            items.push(AttentionItem {
                kind: AttentionKind::Question,
                loupist_id: loupist.id.clone(),
                loupist_name: loupist.name.clone(),
                chat_id: Some(chat_id),
                record_id: None,
                title: chat.title,
                count: None,
                due_date: None,
                at_ms,
            });
        }
        use crate::scheduled_tasks::types::{Schedule, TaskStatus};
        for task in state
            .store
            .scheduled_tasks_list(&loupist.id)
            .map_err(super::map_store)?
        {
            let kind = if task.awaiting_confirmation {
                if task.declined_in_visit.is_some()
                    || matches!(task.schedule, Schedule::Once { at_ms } if at_ms <= now)
                {
                    continue;
                }
                AttentionKind::Proposal
            } else if task.trigger.as_ref().is_some_and(|trigger| {
                trigger.state == crate::scheduled_tasks::types::TriggerPhase::Failing
            }) {
                AttentionKind::TriggerFailing
            } else if task.trigger.as_ref().is_some_and(|trigger| {
                trigger.state == crate::scheduled_tasks::types::TriggerPhase::UpdateProposed
                    && trigger.pending.is_some()
            }) {
                AttentionKind::TriggerUpdate
            } else if task.status == TaskStatus::Paused && task.last_error.is_some() {
                AttentionKind::TaskPaused
            } else {
                continue;
            };
            items.push(AttentionItem {
                kind,
                loupist_id: loupist.id.clone(),
                loupist_name: loupist.name.clone(),
                chat_id: None,
                record_id: Some(task.id),
                title: task.title,
                count: None,
                due_date: None,
                at_ms: task.updated_at_ms,
            });
        }
        let (count, conflict, latest) = state
            .store
            .unread_runs_summary(&loupist.id)
            .map_err(super::map_store)?;
        if count > 0 {
            items.push(AttentionItem {
                kind: if conflict {
                    AttentionKind::BriefingConflict
                } else {
                    AttentionKind::UnreadResults
                },
                loupist_id: loupist.id.clone(),
                loupist_name: loupist.name.clone(),
                chat_id: None,
                record_id: None,
                title: String::new(),
                count: Some(count),
                due_date: None,
                at_ms: latest,
            });
        }
        if let Ok(crossed) = crate::commands::assets::with_assets(state, &loupist.id, false, |tx| {
            crate::knowledge::assets::unseen_crossings(tx, &loupist.id)
        })
        .inspect_err(|error| eprintln!("Could not collect crossed lines: {}", error.code))
        {
            for (asset_id, name, at_ms) in crossed {
                items.push(AttentionItem {
                    kind: AttentionKind::LineCrossed,
                    loupist_id: loupist.id.clone(),
                    loupist_name: loupist.name.clone(),
                    chat_id: None,
                    record_id: Some(asset_id),
                    title: name,
                    count: None,
                    due_date: None,
                    at_ms,
                });
            }
        }
        if let Ok(records) = crate::commands::knowledge::journal::open_checks(state, &loupist.id)
            .inspect_err(|error| eprintln!("Could not collect Journal checks: {}", error.code))
        {
            for record in records {
                let Some(check) = record.check else { continue };
                // An undated line is read on its asset page, not reviewed on a day.
                let Some(by_date) = check.by_date.clone() else {
                    continue;
                };
                let Some(days) = days_until(today, &by_date) else {
                    continue;
                };
                if days > CHECK_HORIZON_DAYS {
                    continue;
                }
                let title = if check.claim.trim().is_empty() {
                    record.judgment.clone()
                } else {
                    check.claim.clone()
                };
                items.push(AttentionItem {
                    kind: AttentionKind::CheckDue,
                    loupist_id: loupist.id.clone(),
                    loupist_name: loupist.name.clone(),
                    chat_id: None,
                    record_id: Some(record.journal_id.clone()),
                    title,
                    count: None,
                    due_date: Some(by_date.get(..10).unwrap_or(&by_date).to_owned()),
                    at_ms: now + days * DAY_MS,
                });
            }
        }
        if let Ok(cards) = crate::chat::cards::waiting(state, &loupist.id) {
            for card in cards {
                items.push(AttentionItem {
                    kind: match card.kind {
                        crate::chat::cards::WaitingKind::Offer => AttentionKind::Offer,
                        crate::chat::cards::WaitingKind::Ask => AttentionKind::Ask,
                    },
                    loupist_id: loupist.id.clone(),
                    loupist_name: loupist.name.clone(),
                    chat_id: Some(card.chat_id),
                    record_id: Some(card.id),
                    title: card.title,
                    count: None,
                    due_date: card.results_date,
                    at_ms: card.at_ms,
                });
            }
        }
        for task in state
            .store
            .scheduled_tasks_list(&loupist.id)
            .map_err(super::map_store)?
        {
            if task.kind != crate::scheduled_tasks::types::TaskKind::Custom
                || task.status != crate::scheduled_tasks::types::TaskStatus::Active
                || task.awaiting_confirmation
            {
                continue;
            }
            let runs = state
                .store
                .scheduled_task_runs(&loupist.id, &task.id)
                .map_err(super::map_store)?;
            let Some(kind) = watch_health(
                task.watch.report,
                crate::briefings::desktop::notify_for(state, &task),
                &runs,
                now,
            ) else {
                continue;
            };
            items.push(AttentionItem {
                kind,
                loupist_id: loupist.id.clone(),
                loupist_name: loupist.name.clone(),
                chat_id: None,
                record_id: Some(task.id),
                title: task.title,
                count: None,
                due_date: None,
                at_ms: task.updated_at_ms,
            });
        }
    }
    if !refused.is_empty() {
        // Only refusals the user has not approved since still need them.
        {
            for (loupist_id, loupist_name, chat_id, at_ms, needs) in refused {
                let Some(pending) = needs.iter().find(|item| {
                    service::approval_pending_for_state(
                        state,
                        &loupist_id,
                        &item.connection_id,
                        &item.tool_name,
                    )
                    .unwrap_or(false)
                }) else {
                    continue;
                };
                items.push(AttentionItem {
                    kind: AttentionKind::Approval,
                    loupist_id,
                    loupist_name,
                    chat_id: Some(chat_id),
                    record_id: None,
                    title: format!("{} · {}", pending.connection_name, pending.tool_name),
                    count: None,
                    due_date: None,
                    at_ms,
                });
            }
        }
    }
    sort_items(&mut items);
    Ok(items)
}

/// A when-met watch that has been quiet for six finished runs in a row.
pub(crate) const QUIET_STREAK: usize = 6;
/// Delivered results inside a week that make a Now watch too loud.
pub(crate) const LOUD_COUNT: usize = 4;
pub(crate) const LOUD_WINDOW_MS: i64 = 7 * DAY_MS;

pub(crate) fn watch_health(
    report: Option<crate::scheduled_tasks::types::ReportPolicy>,
    notify: crate::scheduled_tasks::types::NotifyPolicy,
    runs: &[crate::scheduled_tasks::types::Run],
    now: i64,
) -> Option<AttentionKind> {
    use crate::scheduled_tasks::types::{NotifyPolicy, ReportPolicy, RunStatus};
    let finished: Vec<_> = runs
        .iter()
        .filter(|run| run.status != RunStatus::Running)
        .collect();
    let quiet = report == Some(ReportPolicy::WhenMet)
        && finished.len() >= QUIET_STREAK
        && finished
            .iter()
            .take(QUIET_STREAK)
            .all(|run| run.quiet && run.status == RunStatus::Succeeded);
    if quiet {
        return Some(AttentionKind::WatchQuiet);
    }
    let loud = notify == NotifyPolicy::Now
        && finished
            .iter()
            .filter(|run| {
                run.status == RunStatus::Succeeded
                    && !run.quiet
                    && run
                        .finished_at_ms
                        .is_some_and(|at| now.saturating_sub(at) <= LOUD_WINDOW_MS)
            })
            .count()
            >= LOUD_COUNT;
    loud.then_some(AttentionKind::WatchLoud)
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotifyRequest {
    loupist_id: String,
    chat_id: String,
    /// The localized title the renderer chose ("Quality Compounder finished").
    title: String,
}

static LAST_NOTIFIED: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Whether a chat may notify now; records the time when it may.
pub(crate) fn admit_notification(chat_id: &str, now: Instant) -> bool {
    let Ok(mut guard) = LAST_NOTIFIED.lock() else {
        return false;
    };
    let seen = guard.get_or_insert_with(HashMap::new);
    if seen
        .get(chat_id)
        .is_some_and(|last| now.duration_since(*last) < NOTIFY_INTERVAL)
    {
        return false;
    }
    seen.insert(chat_id.to_owned(), now);
    true
}

/// The first `PREVIEW_CHARS` characters of an answer on one line.
pub(crate) fn preview(text: &str) -> String {
    let line = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(['*', '_', '`', '#'], "");
    crate::util::shorten(&line, PREVIEW_CHARS)
}

/// Body for a chat notification. A missing settings file keeps the default, which
/// shows a preview. Details off, or a file that cannot be read, send an empty body.
fn notification_body(app_data_dir: &Path, answer: &str) -> String {
    if crate::briefings::desktop::notification_shows_details(app_data_dir) {
        preview(answer)
    } else {
        String::new()
    }
}

/// An OS notification for a chat that finished or needs the user while it is not
/// visible. The body comes from the stored chat, never from the renderer, and is
/// empty when desktop notification details are off or that setting cannot be read.
/// Nothing is sent unless the user granted notification permission.
#[tauri::command(async)]
pub(crate) fn attention_notify(
    request: NotifyRequest,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<bool, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    let chat = state
        .store
        .get_chat(&request.chat_id)
        .map_err(|error| CommandError::internal(error.to_string()))?
        .ok_or_else(|| CommandError::not_found("Chat"))?;
    if chat.loupist_id != request.loupist_id {
        return Err(CommandError::not_found("Chat"));
    }
    let title: String = crate::util::shorten(request.title.trim(), 120);
    if title.is_empty() {
        return Err(CommandError::invalid("A notification needs a title"));
    }
    let Some(answer) = chat
        .messages
        .iter()
        .rev()
        .find(|message| message.role == ChatRole::Assistant)
    else {
        return Ok(false);
    };
    let notifications = app.notification();
    if !matches!(
        notifications.permission_state(),
        Ok(tauri_plugin_notification::PermissionState::Granted)
    ) || !admit_notification(&chat.id, Instant::now())
    {
        return Ok(false);
    }
    Ok(notifications
        .builder()
        .title(title)
        .body(notification_body(
            &state.artifacts.app_data_dir,
            &answer.content,
        ))
        .show()
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scheduled_tasks::types::{NotifyPolicy, ReportPolicy, Run, RunStatus, TaskKind};

    fn run(id: &str, finished: i64, quiet: bool) -> Run {
        Run {
            usage: None,
            budget: None,
            failure_streak: None,
            first_run: false,
            memory_save_skip: None,
            discarded_memory_items: None,
            briefing_context: None,
            kind: TaskKind::Custom,
            read_at_ms: None,
            summary: String::new(),
            quiet,
            message_id: None,
            skipped_tools: Vec::new(),
            id: id.into(),
            task_id: "task".into(),
            loupist_id: "loupist-a".into(),
            task_revision: 1,
            task_title: "Filings".into(),
            thread_id: None,
            run_id: id.into(),
            scheduled_for_ms: finished,
            started_at_ms: finished,
            finished_at_ms: Some(finished),
            status: RunStatus::Succeeded,
            error: None,
            trigger: None,
            undo: None,
            provider: None,
        }
    }

    #[test]
    fn a_long_quiet_streak_and_a_loud_week_are_host_cards() {
        let now = 10_000_000_000;
        let quiet: Vec<_> = (0..QUIET_STREAK)
            .map(|index| run(&index.to_string(), now - index as i64, true))
            .collect();
        assert_eq!(
            watch_health(
                Some(ReportPolicy::WhenMet),
                NotifyPolicy::Digest,
                &quiet,
                now
            ),
            Some(AttentionKind::WatchQuiet)
        );
        let loud: Vec<_> = (0..LOUD_COUNT)
            .map(|index| run(&format!("l{index}"), now - index as i64 * 1_000, false))
            .collect();
        assert_eq!(
            watch_health(Some(ReportPolicy::WhenMet), NotifyPolicy::Now, &loud, now),
            Some(AttentionKind::WatchLoud)
        );
        assert_eq!(
            watch_health(
                Some(ReportPolicy::WhenMet),
                NotifyPolicy::Digest,
                &loud,
                now
            ),
            None
        );
    }

    #[tokio::test]
    async fn an_attention_store_failure_is_returned_without_raw_sqlite_details() {
        let temporary = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temporary.path().join("app"));
        crate::commands::tests::seed_profile(
            state.store.as_ref(),
            &crate::commands::tests::profile("loupist-a", &temporary.path().join("workspace"), 1),
        );
        state
            .store
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE chat_sessions")
            .unwrap();
        let error = collect(&state).await.unwrap_err();
        assert_eq!(error.code, "internal");
        assert_eq!(error.message, "Could not access saved data.");
    }

    fn item(kind: AttentionKind, at_ms: i64, due: Option<&str>) -> AttentionItem {
        AttentionItem {
            kind,
            loupist_id: "g".into(),
            loupist_name: "G".into(),
            chat_id: None,
            record_id: None,
            title: String::new(),
            count: None,
            due_date: due.map(Into::into),
            at_ms,
        }
    }

    #[test]
    fn approvals_lead_and_checks_follow_their_due_dates() {
        let mut items = vec![
            item(AttentionKind::CheckDue, 0, Some("2026-11-05")),
            item(AttentionKind::UnreadResults, 5, None),
            item(AttentionKind::CheckDue, 0, Some("2026-11-01")),
            item(AttentionKind::Approval, 1, None),
            item(AttentionKind::BriefingConflict, 2, None),
            item(AttentionKind::Approval, 9, None),
        ];
        sort_items(&mut items);
        let order: Vec<_> = items
            .iter()
            .map(|item| (item.kind, item.at_ms, item.due_date.clone()))
            .collect();
        assert_eq!(order[0], (AttentionKind::Approval, 9, None));
        assert_eq!(order[1].0, AttentionKind::Approval);
        assert_eq!(order[2].0, AttentionKind::BriefingConflict);
        assert_eq!(order[3].0, AttentionKind::UnreadResults);
        assert_eq!(order[4].2.as_deref(), Some("2026-11-01"));
        assert_eq!(order[5].2.as_deref(), Some("2026-11-05"));
    }

    /// The window title and Dock badge count what arrived while the window was
    /// not in front, from Rust's own list, and clear once it is (B80).
    #[test]
    fn the_indicator_counts_what_arrived_since_the_window_was_in_front() {
        let approval = |chat: &str, at_ms| AttentionItem {
            chat_id: Some(chat.into()),
            ..item(AttentionKind::Approval, at_ms, None)
        };
        let results = |count, at_ms| AttentionItem {
            count: Some(count),
            ..item(AttentionKind::UnreadResults, at_ms, None)
        };
        let check = item(AttentionKind::CheckDue, 0, Some("2026-11-03"));
        let mut indicator = Indicator::default();
        // Launched in the background: what waits counts, except checks coming due.
        assert_eq!(
            indicator.update(&[approval("c1", 1), results(2, 5), check.clone()], false),
            3
        );
        // In front: everything is seen and nothing shows.
        assert_eq!(
            indicator.update(&[approval("c1", 1), results(2, 5), check.clone()], true),
            0
        );
        assert_eq!(
            indicator.update(&[approval("c1", 1), results(2, 5), check.clone()], false),
            0
        );
        // Behind other windows: a new approval and one more finished run.
        let later = [
            approval("c1", 1),
            approval("c2", 7),
            results(3, 9),
            check.clone(),
        ];
        assert_eq!(indicator.update(&later, false), 2);
        // The same chat asking again later counts again.
        assert_eq!(
            indicator.update(&[approval("c1", 11), results(3, 9)], false),
            2
        );
        // Seen rows that left the list do not come back as new.
        assert_eq!(indicator.update(&later, true), 0);
        assert_eq!(indicator.update(&[results(3, 9)], false), 0);
    }

    #[test]
    fn due_dates_count_whole_days_and_late_checks_are_negative() {
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        assert_eq!(days_until(today, "2026-11-07"), Some(7));
        assert_eq!(days_until(today, "2026-10-29T00:00:00Z"), Some(-2));
        assert_eq!(days_until(today, "soon"), None);
    }

    #[test]
    fn a_chat_notifies_at_most_once_a_minute() {
        let start = Instant::now();
        assert!(admit_notification("chat-rate", start));
        assert!(!admit_notification(
            "chat-rate",
            start + Duration::from_secs(30)
        ));
        assert!(admit_notification(
            "chat-rate",
            start + Duration::from_secs(61)
        ));
        assert!(admit_notification("chat-other", start));
    }

    #[test]
    fn only_chats_whose_last_answer_was_refused_are_read() {
        use crate::commands::tests::{chat, profile, seed_profile};
        use crate::test_fixtures::chat_message;
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        let refusal = ChatToolRefusal {
            connection_id: "custom-filings".into(),
            connection_name: "Filings".into(),
            tool_name: "read_filing".into(),
            reason: ChatToolRefusalReason::NeedsApproval,
        };
        let mut refused = chat("chat-refused", "loupist-a", 10);
        let mut answer = chat_message("a1", ChatRole::Assistant, "I need your OK for Filings.");
        answer.tool_refusals = vec![refusal.clone()];
        refused.messages = vec![
            chat_message("u1", ChatRole::User, "Read the filing"),
            answer.clone(),
        ];
        state.store.create_chat(&refused).unwrap();
        // The user already replied after the refusal: nothing waits there.
        let mut answered = chat("chat-answered", "loupist-a", 10);
        answered.messages = vec![answer, chat_message("u2", ChatRole::User, "Never mind")];
        state.store.create_chat(&answered).unwrap();
        let rows = state
            .store
            .chats_ending_in_refusals("loupist-a", 0)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "chat-refused");
        let parsed: Vec<ChatToolRefusal> = serde_json::from_str(&rows[0].3).unwrap();
        assert_eq!(parsed, vec![refusal]);
        // Older than the window: not read.
        assert!(state
            .store
            .chats_ending_in_refusals("loupist-a", 11)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn completed_research_questions_do_not_need_attention() {
        use crate::commands::tests::{chat, profile, seed_profile};
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("app"));
        let owner = "briefing-owner";
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(&state.store, &profile(owner, &workspace, 1));
        let mut briefing = chat("finished-briefing", owner, crate::commands::now_ms());
        briefing.messages.push(crate::test_fixtures::chat_message(
            "a1",
            ChatRole::Assistant,
            "Follow-up research\n1. What will inventory show?",
        ));
        state.store.create_chat(&briefing).unwrap();
        assert!(!collect(&state)
            .await
            .unwrap()
            .iter()
            .any(|row| row.kind == AttentionKind::Question));
    }

    #[tokio::test]
    async fn only_live_questions_in_existing_chats_need_an_answer() {
        use crate::chat::questions::{self, QuestionAnswer};
        use crate::commands::tests::{chat, profile, seed_profile};
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::for_test(dir.path().join("app"));
        let owner = "live-question-owner";
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(&state.store, &profile(owner, &workspace, 1));
        let thread = "live-question-chat";
        state.store.create_chat(&chat(thread, owner, 1)).unwrap();
        let register = || questions::register("live-question", owner, Some(thread), None, 2);
        let listed = || async {
            collect(&state).await.unwrap().into_iter().any(|item| {
                item.kind == AttentionKind::Question && item.chat_id.as_deref() == Some(thread)
            })
        };
        let pending = register();
        assert!(listed().await);
        questions::answer(owner, "live-question", QuestionAnswer::Choice(0)).unwrap();
        assert!(!listed().await);
        drop(pending);
        let pending = register();
        drop(pending); // The run stopped or the wait timed out.
        assert!(!listed().await);
        let pending = register();
        crate::commands::records::delete_chat_session(&state, owner, thread, |_| Ok(())).unwrap();
        assert!(!listed().await);
        assert!(pending.await.is_err());
    }

    #[test]
    fn questions_rank_with_approvals() {
        let mut items = vec![
            item(AttentionKind::UnreadResults, 30, None),
            item(AttentionKind::Question, 20, None),
            item(AttentionKind::Approval, 10, None),
        ];
        sort_items(&mut items);
        let kinds: Vec<_> = items.iter().map(|item| item.kind).collect();
        assert_eq!(
            kinds,
            vec![
                AttentionKind::Question,
                AttentionKind::Approval,
                AttentionKind::UnreadResults
            ]
        );
    }

    #[test]
    fn previews_are_one_short_plain_line() {
        assert_eq!(preview("**Hanul**  margin\nfell"), "Hanul margin fell");
        let long = "a".repeat(80);
        assert_eq!(preview(&long).chars().count(), PREVIEW_CHARS);
    }

    /// A stored desktop setting with details off hides the reply. The same answer
    /// is previewed when details are on, and hidden again when the file cannot be read.
    #[test]
    fn hidden_details_omit_the_answer_even_when_the_reply_has_text() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let answer = "Hanul margin fell after the filing";
        assert!(!preview(answer).is_empty());
        assert_eq!(notification_body(root, answer), preview(answer));

        let mut settings = crate::briefings::desktop::ResidentSettings::default();
        settings.notifications.show_details = false;
        std::fs::write(
            root.join("briefings-settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        assert_eq!(notification_body(root, answer), "");

        settings.notifications.show_details = true;
        std::fs::write(
            root.join("briefings-settings.json"),
            serde_json::to_vec(&settings).unwrap(),
        )
        .unwrap();
        assert_eq!(notification_body(root, answer), preview(answer));

        std::fs::write(root.join("briefings-settings.json"), b"{").unwrap();
        assert_eq!(notification_body(root, answer), "");
    }
}
