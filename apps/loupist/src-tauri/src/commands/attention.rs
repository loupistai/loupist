//! What waits for the user across Loupists: refused tools that still need approval,
//! finished answers that asked the user a question, unread scheduled results and
//! Journal checks coming due. The sidebar's "Needs attention" group renders this
//! list; Rust decides what belongs in it.
use std::{
    collections::HashMap,
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

/// Refusals and questions older than this stop asking for attention.
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
    /// A chat's finished last answer asked the user something (`asks_user`); shown until they reply.
    Question,
    /// A line the user set on an asset page was crossed; shown until the page opens.
    LineCrossed,
    BriefingConflict,
    UnreadResults,
    /// Watcher alerts delivered but not on screen yet.
    WatchAlerts,
    CheckDue,
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
        AttentionKind::Approval | AttentionKind::Question => 0,
        AttentionKind::LineCrossed => 1,
        AttentionKind::BriefingConflict => 2,
        AttentionKind::UnreadResults => 3,
        AttentionKind::WatchAlerts => 4,
        AttentionKind::CheckDue => 5,
    }
}

/// Approvals and questions first, then conflicting briefings, unread results and checks;
/// newest first within a kind, checks by due date.
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

/// Closing offers of more work, lowercased: they end with a question mark but
/// ask nothing the user must answer ("Want me to chart it?").
const OFFER_PHRASES: &[&str] = &[
    "want me to",
    "like me to",
    "would you like",
    "do you want",
    "shall i ",
    "should i ",
    "can i ",
    "may i ",
    "anything else",
    "let me know",
    "if you'd like",
    "if you want",
    "원하시면",
    "필요하시면",
];

/// Whether a finished answer ends by asking the user something only they can
/// answer. Deliberately narrow: only the answer's last line counts, outside code,
/// headings, quotes and tables; its last sentence must end with a question mark;
/// and a closing offer of more work ("Want me to…?", a Korean "…할까요?") is not
/// a question. A missed question costs less than a Needs attention list that
/// fills with polite closings.
pub(crate) fn asks_user(answer: &str) -> bool {
    let mut fenced = false;
    let mut last = None;
    for line in answer.lines().map(str::trim) {
        if line.starts_with("```") || line.starts_with("~~~") {
            fenced = !fenced;
            // A code block after the text is the answer's end.
            last = None;
        } else if !fenced && !line.is_empty() {
            last = Some(line);
        }
    }
    let Some(line) = last.filter(|_| !fenced) else {
        return false;
    };
    if line.starts_with(['#', '>', '|']) {
        return false;
    }
    let line = strip_trailing_marks(strip_list_marker(line));
    if !line.ends_with(['?', '？']) {
        return false;
    }
    let sentence = last_sentence(line).to_lowercase();
    let offer = OFFER_PHRASES.iter().any(|phrase| sentence.contains(phrase))
        // Korean "…할까요?" / "…드릴까요?" proposes the Loupist's next step.
        || sentence.trim_end_matches(['?', '？']).trim_end().ends_with("까요");
    !offer
}

/// "- ", "* ", "+ ", "1. " or "1) " before a list item's text.
fn strip_list_marker(line: &str) -> &str {
    if let Some(rest) = line
        .strip_prefix(['-', '*', '+'])
        .and_then(|rest| rest.strip_prefix(' '))
    {
        return rest.trim_start();
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if digits > 0 {
        if let Some(rest) = line[digits..]
            .strip_prefix(['.', ')'])
            .and_then(|rest| rest.strip_prefix(' '))
        {
            return rest.trim_start();
        }
    }
    line
}

/// The sentence without what may follow its last mark: emphasis, closing quotes
/// and a citation or link (`…?[1](#evidence-…)`, `…? [^2]`).
fn strip_trailing_marks(mut text: &str) -> &str {
    loop {
        let trimmed = text.trim_end_matches(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '*' | '_' | '`' | '"' | '\'' | '”' | '’' | '»' | '」' | '』'
                )
        });
        let next = if trimmed.ends_with(')') {
            match trimmed
                .rfind("](")
                .and_then(|close| trimmed[..close].rfind('['))
            {
                Some(open) => &trimmed[..open],
                None => trimmed.strip_suffix(')').unwrap_or(trimmed),
            }
        } else if trimmed.ends_with(']') {
            trimmed.rfind('[').map_or(trimmed, |open| &trimmed[..open])
        } else {
            trimmed
        };
        if next.len() == text.len() {
            return next;
        }
        text = next;
    }
}

/// The text after the line's last sentence break (a mark followed by a space).
fn last_sentence(line: &str) -> &str {
    let mut start = 0;
    let mut previous: Option<char> = None;
    for (at, c) in line.char_indices() {
        if c.is_whitespace()
            && previous.is_some_and(|mark| matches!(mark, '.' | '!' | '?' | '。' | '！' | '？'))
        {
            start = at;
        }
        previous = Some(c);
    }
    line[start..].trim()
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
            if let Ok(items) = collect(&state).await {
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
    let loupists = state
        .store
        .list_loupists()
        .map_err(|error| CommandError::internal(error.to_string()))?;
    let mut items = Vec::new();
    let mut refused: Vec<(String, String, String, i64, Vec<ChatToolRefusal>)> = Vec::new();
    for loupist in &loupists {
        if state.ensure_loupist_available(&loupist.id).is_err() {
            continue;
        }
        if let Ok(chats) = state
            .store
            .chats_ending_in_refusals(&loupist.id, now - WAITING_WINDOW_MS)
        {
            for (chat_id, _title, at_ms, json) in chats {
                let refusals: Vec<ChatToolRefusal> =
                    serde_json::from_str(&json).unwrap_or_default();
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
        }
        // A reply from the user ends the wait: only a chat's last answer is read.
        if let Ok(chats) = state
            .store
            .chats_ending_in_answer(&loupist.id, now - WAITING_WINDOW_MS)
        {
            for (chat_id, title, at_ms, answer) in chats {
                if !asks_user(&answer) {
                    continue;
                }
                items.push(AttentionItem {
                    kind: AttentionKind::Question,
                    loupist_id: loupist.id.clone(),
                    loupist_name: loupist.name.clone(),
                    chat_id: Some(chat_id),
                    record_id: None,
                    title,
                    count: None,
                    due_date: None,
                    at_ms,
                });
            }
        }
        if let Ok((count, conflict, latest)) = state.store.unread_runs_summary(&loupist.id) {
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
        }
        if let Ok((count, latest)) =
            crate::commands::assets::with_assets(state, &loupist.id, false, |tx| {
                crate::knowledge::watch::unseen(tx, &loupist.id)
            })
        {
            if count > 0 {
                items.push(AttentionItem {
                    kind: AttentionKind::WatchAlerts,
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
        }
        if let Ok(crossed) = crate::commands::assets::with_assets(state, &loupist.id, false, |tx| {
            crate::knowledge::assets::unseen_crossings(tx, &loupist.id)
        }) {
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
        if let Ok(records) = crate::commands::knowledge::journal::open_checks(state, &loupist.id) {
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
    }
    if !refused.is_empty() {
        // Only refusals the user has not approved since still need them.
        if let Ok(manager) = service::for_state(state) {
            let connections = manager.lock().await;
            for (loupist_id, loupist_name, chat_id, at_ms, needs) in refused {
                let Some(pending) = needs.iter().find(|item| {
                    connections.approval_pending(&loupist_id, &item.connection_id, &item.tool_name)
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
    let mut chars = line.chars();
    let head: String = chars.by_ref().take(PREVIEW_CHARS).collect();
    if chars.next().is_some() {
        format!("{}…", head.trim_end())
    } else {
        head
    }
}

/// An OS notification for a chat that finished or needs the user while it is not
/// visible. The body comes from the stored chat, never from the renderer; nothing is
/// sent unless the user granted notification permission.
#[tauri::command]
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
    let title: String = request.title.trim().chars().take(120).collect();
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
        .body(preview(&answer.content))
        .show()
        .is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn an_answer_asks_only_when_its_last_line_is_a_question_for_the_user() {
        for asks in [
            "Revenue grew 11%.\n\nWhich market do you mean, KOSPI or KOSDAQ?",
            "Before I go on:\n\n- Is this a taxable account?\n- What is your horizon?",
            "I found two filings. Should the 2.5% guidance or the 3% one count?",
            "Is this the 2025 filing? [1](#evidence-q3)",
            "**Do you hold it in a pension account?**",
            "보유 기간은 어느 정도인가요?",
        ] {
            assert!(asks_user(asks), "{asks}");
        }
        for not in [
            "Revenue grew 11% while margin fell to 14.8%.",
            "Is the margin decline lasting? The evidence says no.",
            "Margin fell.\n\nWant me to chart the next quarter too?",
            "Would you like me to compare it with Morrow Components?",
            "Shall I keep watching inventory days?",
            "다음 분기도 차트로 그려 드릴까요?",
            "원하시면 공급망도 살펴볼게요?",
            "## What changes the view?",
            "> Is this the right question?",
            "| Is it? | Yes |",
            "Run this:\n\n```python\nprint('ready?')\n```",
            "What next?\n\n```\ncode\n```",
            "",
        ] {
            assert!(!asks_user(not), "{not}");
        }
    }

    #[test]
    fn only_chats_whose_last_message_is_a_finished_answer_are_read_for_questions() {
        use crate::commands::tests::{chat, profile, seed_profile};
        use crate::test_fixtures::chat_message;
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        let question = chat_message("a1", ChatRole::Assistant, "Which market do you mean?");
        let mut asked = chat("chat-asked", "loupist-a", 10);
        asked.title = "Hanul margin".into();
        asked.messages = vec![
            chat_message("u1", ChatRole::User, "Check Hanul"),
            question.clone(),
        ];
        state.store.create_chat(&asked).unwrap();
        // The user already answered: nothing waits there.
        let mut answered = chat("chat-answered", "loupist-a", 10);
        answered.messages = vec![
            question.clone(),
            chat_message("u2", ChatRole::User, "KOSPI"),
        ];
        state.store.create_chat(&answered).unwrap();
        // A stopped answer and a host-posted one are not the model asking.
        let mut stopped = chat("chat-stopped", "loupist-a", 10);
        let mut aborted = question.clone();
        aborted.status = crate::domain::ChatMessageStatus::Aborted;
        stopped.messages = vec![aborted];
        state.store.create_chat(&stopped).unwrap();
        let mut hosted = chat("chat-hosted", "loupist-a", 10);
        let mut receipt = question;
        receipt.host_composed = true;
        hosted.messages = vec![receipt];
        state.store.create_chat(&hosted).unwrap();
        let rows = state.store.chats_ending_in_answer("loupist-a", 0).unwrap();
        assert_eq!(
            rows,
            vec![(
                "chat-asked".to_owned(),
                "Hanul margin".to_owned(),
                10,
                "Which market do you mean?".to_owned()
            )]
        );
        assert!(state
            .store
            .chats_ending_in_answer("loupist-a", 11)
            .unwrap()
            .is_empty());
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
        assert_eq!(preview(&long).chars().count(), PREVIEW_CHARS + 1);
    }
}
