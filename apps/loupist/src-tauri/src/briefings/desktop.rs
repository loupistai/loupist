//! Native effects are isolated from schedule persistence and testable with fakes.
use crate::{
    app::{AppState, CommandError},
    scheduled_tasks::types::{DeliveryTarget, NotifyPolicy, Run, RunStatus, TaskKind, WatchPreset},
    store::{SqliteStore, StoreError, StoreResult},
};
use chrono::{Datelike, Timelike};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
};
use tauri::{Emitter, Manager};

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
pub(super) static SETTINGS_LOCK: Mutex<()> = Mutex::new(());
static VISIBLE_BOT: Mutex<Option<String>> = Mutex::new(None);
static MAIN_FOCUSED: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct BotFocusRequest {
    loupist_id: Option<String>,
}

#[tauri::command]
pub(crate) fn notification_bot_focus_set(
    request: BotFocusRequest,
    webview: tauri::Webview,
) -> Result<(), CommandError> {
    if webview.label() != "main" {
        return Err(CommandError::invalid(
            "Bot focus belongs to the main webview",
        ));
    }
    if let Some(id) = request.loupist_id.as_deref() {
        crate::commands::require_text(id, "Loupist", 256)?;
    }
    let window = webview.window();
    MAIN_FOCUSED.store(
        window.is_focused().unwrap_or(false) && window.is_visible().unwrap_or(false),
        Ordering::Relaxed,
    );
    *VISIBLE_BOT
        .lock()
        .map_err(|_| CommandError::internal("Notifications are unavailable"))? = request.loupist_id;
    Ok(())
}

pub(crate) fn renderer_reset(webview: &tauri::Webview) {
    if webview.label() == "main" {
        if let Ok(mut visible) = VISIBLE_BOT.lock() {
            *visible = None;
        }
    }
}

fn same_bot_is_visible(visible: Option<&str>, native_focus: bool, loupist_id: &str) -> bool {
    native_focus && visible == Some(loupist_id)
}

const TRAY_ID: &str = "briefings-tray";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResidentSettings {
    pub keep_running: bool,
    pub notification_permission_asked: bool,
    #[serde(default)]
    #[cfg_attr(test, ts(as = "Option<NotificationSettings>", optional))]
    pub notifications: NotificationSettings,
    /// None follows the automatic default; Some preserves the user's choice.
    #[serde(default)]
    pub keep_running_choice: Option<bool>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[cfg_attr(test, ts(as = "Option<bool>", optional))]
    pub login_hint_dismissed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotificationSettings {
    pub quiet_hours: Option<QuietHours>,
    pub daily_cap: u8,
    pub show_details: bool,
    #[serde(default)]
    pub defaults: NotificationDefaults,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotificationDefaults {
    pub crossed_line: NotifyPolicy,
    pub filings: NotifyPolicy,
    pub briefing: NotifyPolicy,
    pub other: NotifyPolicy,
    pub failures: NotifyPolicy,
}

impl Default for NotificationDefaults {
    fn default() -> Self {
        Self {
            crossed_line: NotifyPolicy::Now,
            filings: NotifyPolicy::Now,
            briefing: NotifyPolicy::Now,
            other: NotifyPolicy::Digest,
            failures: NotifyPolicy::Digest,
        }
    }
}

impl NotificationDefaults {
    pub(crate) fn for_task(
        self,
        kind: &TaskKind,
        preset: Option<WatchPreset>,
        explicit: Option<NotifyPolicy>,
    ) -> NotifyPolicy {
        explicit.unwrap_or(if *kind == TaskKind::Briefing {
            self.briefing
        } else if preset == Some(WatchPreset::FilingsAndResults) {
            self.filings
        } else {
            self.other
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct QuietHours {
    pub start: String,
    pub end: String,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            start: "22:00".into(),
            end: "07:00".into(),
        }
    }
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            quiet_hours: Some(QuietHours::default()),
            daily_cap: 6,
            show_details: true,
            defaults: NotificationDefaults::default(),
        }
    }
}

impl NotificationSettings {
    pub(super) fn validate(&self) -> Result<(), CommandError> {
        if self.daily_cap > 24
            || self.quiet_hours.as_ref().is_some_and(|hours| {
                match (clock_minute(&hours.start), clock_minute(&hours.end)) {
                    (Some(start), Some(end)) => start == end,
                    _ => true,
                }
            })
        {
            return Err(CommandError::invalid(
                "Invalid notification hours or daily cap",
            ));
        }
        Ok(())
    }

    fn is_quiet_at(&self, minute: u32) -> bool {
        self.quiet_hours.as_ref().is_some_and(|hours| {
            let (Some(start), Some(end)) = (clock_minute(&hours.start), clock_minute(&hours.end))
            else {
                return false;
            };
            if start < end {
                (start..end).contains(&minute)
            } else {
                minute >= start || minute < end
            }
        })
    }
}

fn clock_minute(value: &str) -> Option<u32> {
    let time = chrono::NaiveTime::parse_from_str(value, "%H:%M").ok()?;
    Some(time.hour() * 60 + time.minute())
}

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NotificationTemplates {
    result: String,
    private_result: String,
    digest: String,
    line_crossed: String,
}

impl Default for NotificationTemplates {
    fn default() -> Self {
        Self {
            result: "{loupist}: {task}".into(),
            private_result: "{loupist}: a result is ready".into(),
            digest: "{count} results in {loupist}".into(),
            line_crossed: "Line crossed".into(),
        }
    }
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct TemplatesRequest {
    language: String,
    templates: NotificationTemplates,
}

static TEMPLATES: Mutex<Option<NotificationTemplates>> = Mutex::new(None);

#[tauri::command]
pub(crate) fn notification_templates_set(request: TemplatesRequest) -> Result<(), CommandError> {
    if request.language.is_empty()
        || request.language.len() > 32
        || [
            &request.templates.result,
            &request.templates.private_result,
            &request.templates.digest,
            &request.templates.line_crossed,
        ]
        .iter()
        .any(|pattern| {
            pattern.trim().is_empty()
                || pattern.len() > 256
                || pattern.chars().any(char::is_control)
        })
    {
        return Err(CommandError::invalid("Invalid notification templates"));
    }
    *TEMPLATES
        .lock()
        .map_err(|_| CommandError::internal("Notifications are unavailable"))? =
        Some(request.templates);
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct InboxChanged {
    pub loupist_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct InboxOpen {
    pub loupist_id: String,
    pub run_id: Option<String>,
}

pub(crate) trait ResidentEffects {
    fn focus(&self);
    fn select(&self, target: InboxOpen);
    fn quit(&self);
    fn hide(&self);
}

pub(crate) fn tray_action(effects: &dyn ResidentEffects, id: &str) {
    match id {
        "briefing-open" => effects.focus(),
        "briefing-quit" => effects.quit(),
        id => {
            if let Some(loupist_id) = id.strip_prefix("briefing-loupist:") {
                effects.focus();
                effects.select(InboxOpen {
                    loupist_id: loupist_id.into(),
                    run_id: None,
                });
            }
        }
    }
}

pub(crate) fn close_window(effects: &dyn ResidentEffects, keep_running: bool) -> bool {
    if keep_running {
        effects.hide();
    }
    keep_running
}

impl ResidentEffects for NativeEffects {
    fn focus(&self) {
        open(&self.0, None);
    }
    fn select(&self, target: InboxOpen) {
        let _ = self.0.emit(crate::events::INBOX_OPEN, target);
    }
    fn quit(&self) {
        self.0.exit(0);
    }
    fn hide(&self) {
        if let Some(window) = self.0.get_webview_window("main") {
            let _ = window.hide();
        }
    }
}

struct NativeEffects(tauri::AppHandle);

impl NativeEffects {
    fn changed(&self, loupist_id: &str) {
        let _ = self.0.emit(
            crate::events::INBOX_CHANGED,
            InboxChanged {
                loupist_id: loupist_id.into(),
            },
        );
        let entries = tray_entries(&self.0);
        let app = self.0.clone();
        // Tauri menu operations wait for the main thread. During Quit the
        // main thread is draining runs, so finishing a run must not wait here.
        let _ = self.0.run_on_main_thread(move || {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                if let Ok(menu) = tray_menu(&app, entries) {
                    let _ = tray.set_menu(Some(menu));
                }
            }
        });
    }
}

pub(crate) fn notify_for(
    state: &crate::app::AppState,
    task: &crate::scheduled_tasks::types::Task,
) -> NotifyPolicy {
    read_settings(&state.artifacts.app_data_dir)
        .unwrap_or_default()
        .notifications
        .defaults
        .for_task(&task.kind, task.watch.preset, task.delivery().notify)
}

pub(super) fn read_settings(root: &Path) -> Result<ResidentSettings, CommandError> {
    match std::fs::read(root.join("briefings-settings.json")) {
        Ok(bytes) => {
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| CommandError::internal("Briefing resident settings are invalid"))?;
            let legacy = value.get("keepRunningChoice").is_none();
            let mut settings: ResidentSettings = serde_json::from_value(value)
                .map_err(|_| CommandError::internal("Briefing resident settings are invalid"))?;
            // An old file cannot distinguish an automatic value from a user
            // choice. Preserve it as explicit rather than overwrite a prior off.
            if legacy {
                settings.keep_running_choice = Some(settings.keep_running);
            }
            if let Some(choice) = settings.keep_running_choice {
                settings.keep_running = choice;
            }
            Ok(settings)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(ResidentSettings::default())
        }
        Err(_) => Err(CommandError::internal(
            "Briefing resident settings cannot be read",
        )),
    }
}

/// Whether a chat notification may include the stored answer.
/// A missing file keeps the default, which shows details. An unreadable file hides them.
pub(crate) fn notification_shows_details(root: &Path) -> bool {
    read_settings(root)
        .map(|settings| settings.notifications.show_details)
        .unwrap_or(false)
}

pub(super) fn write_settings(root: &Path, settings: &ResidentSettings) -> Result<(), CommandError> {
    let save = || -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        file.write_all(&serde_json::to_vec(settings)?)?;
        file.as_file().sync_all()?;
        file.persist(root.join("briefings-settings.json"))?;
        Ok(())
    };
    save().map_err(|_| CommandError::internal("Briefing resident settings cannot be saved"))
}

pub(crate) fn enable(state: &AppState) -> Result<(), CommandError> {
    enable_automatic(state, true)
}

fn enable_automatic(state: &AppState, notifications: bool) -> Result<(), CommandError> {
    let _lock = SETTINGS_LOCK
        .lock()
        .map_err(|_| CommandError::internal("Briefing settings lock failed"))?;
    let mut settings = read_settings(&state.artifacts.app_data_dir)?;
    if settings.keep_running_choice.is_none() {
        settings.keep_running = true;
    }
    let request_permission =
        notifications && !settings.notification_permission_asked && APP.get().is_some();
    if request_permission {
        settings.notification_permission_asked = true;
    }
    write_settings(&state.artifacts.app_data_dir, &settings)?;
    if request_permission {
        super::notification_os::request_permission();
    }
    Ok(())
}

fn notification_settings_url(mac: bool) -> Option<&'static str> {
    mac.then_some("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
}

#[tauri::command]
pub(crate) fn notification_permission_settings_open() -> Result<(), CommandError> {
    let url = notification_settings_url(cfg!(target_os = "macos"))
        .ok_or_else(|| CommandError::invalid("Notification settings are available on macOS"))?;
    ::open::that(url)
        .map_err(|_| CommandError::internal("Could not open macOS notification settings"))
}

#[tauri::command]
pub(crate) fn briefing_resident_settings_get(
    state: tauri::State<'_, AppState>,
) -> Result<super::login_item::DesktopSettings, CommandError> {
    super::login_item::apply(&state, super::login_item::Action::Read)
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResidentRequest {
    keep_running: Option<bool>,
    notifications: Option<NotificationSettings>,
}
#[tauri::command]
pub(crate) fn briefing_resident_settings_set(
    request: ResidentRequest,
    state: tauri::State<'_, AppState>,
) -> Result<super::login_item::DesktopSettings, CommandError> {
    use super::login_item::{apply, Action};
    match (request.keep_running, request.notifications) {
        (Some(keep_running), None) => apply(&state, Action::MenuBar(keep_running)),
        (None, Some(notifications)) => apply(&state, Action::Notifications(notifications)),
        _ => Err(CommandError::invalid(
            "Choose one desktop setting to update",
        )),
    }
}

pub(crate) fn completed(state: &AppState, run: &Run) {
    if run.quiet && run.status == crate::scheduled_tasks::types::RunStatus::Succeeded {
        changed(&run.loupist_id);
        return;
    }
    // Results are read in the home Thread; a busy Thread receives them later.
    if let Err(error) = crate::chat::home::deliver_finished_runs(state, &run.loupist_id) {
        eprintln!("Scheduled result could not be posted to the Thread ({error})");
    }
    if APP.get().is_some() {
        queue_result(state, run);
    }
    changed(&run.loupist_id);
}

const COALESCE_MS: i64 = 120_000;

#[derive(Deserialize, Serialize)]
struct PendingResult {
    target: InboxOpen,
    task: String,
    lead: String,
    ready_at: i64,
    digest: bool,
}

enum Delivery {
    Sent,
    Retry,
    Discard,
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct NotificationQueue {
    day: Option<i32>,
    sent: u8,
    pending: Vec<PendingResult>,
}

static QUEUE_LOCK: Mutex<()> = Mutex::new(());

fn update_queue(
    store: &SqliteStore,
    update: impl FnOnce(&mut NotificationQueue) -> bool,
) -> StoreResult<()> {
    let _guard = QUEUE_LOCK.lock().map_err(|_| StoreError::LockPoisoned)?;
    let mut queue = store.desktop_notification_queue()?;
    if update(&mut queue) {
        store.save_desktop_notification_queue(&queue)?;
    }
    Ok(())
}

impl NotificationQueue {
    fn enqueue(&mut self, mut result: PendingResult, now: i64) {
        if !result.digest {
            // Anchor a burst to its first result to bound the delay.
            result.ready_at = self
                .pending
                .iter()
                .filter(|pending| {
                    !pending.digest && pending.target.loupist_id == result.target.loupist_id
                })
                .map(|pending| pending.ready_at)
                .find(|ready| *ready >= now)
                .unwrap_or(result.ready_at);
        }
        self.pending.push(result);
    }
}

fn run_policy(run: &Run, notify: NotifyPolicy, failures: NotifyPolicy) -> NotifyPolicy {
    if run.trigger.is_some() {
        // The evaluated occurrence owns delivery, even when the explanation fails.
        return NotifyPolicy::Off;
    }
    if run.status == RunStatus::Succeeded {
        return if run.quiet { NotifyPolicy::Off } else { notify };
    }
    if notify == NotifyPolicy::Off
        || run
            .failure_streak
            .as_ref()
            .is_none_or(|streak| streak.count != 1)
    {
        return NotifyPolicy::Off;
    }
    failures
}

fn queue_result(state: &AppState, run: &Run) {
    let Ok(Some(task)) = state
        .store
        .scheduled_task_get(&run.loupist_id, &run.task_id)
    else {
        return;
    };
    let delivery = state
        .store
        .scheduled_run_delivery(&run.loupist_id, &run.run_id)
        .ok()
        .flatten()
        .unwrap_or_else(|| task.delivery());
    if matches!(delivery.to, DeliveryTarget::None | DeliveryTarget::Briefing) {
        return;
    }
    let defaults = read_settings(&state.artifacts.app_data_dir)
        .unwrap_or_default()
        .notifications
        .defaults;
    let policy = run_policy(
        run,
        defaults.for_task(&task.kind, task.watch.preset, delivery.notify),
        defaults.failures,
    );
    queue_notification(
        state,
        policy,
        InboxOpen {
            loupist_id: run.loupist_id.clone(),
            run_id: Some(run.run_id.clone()),
        },
        task.title,
        run.summary
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(280)
            .collect(),
    );
}

fn queue_notification(
    state: &AppState,
    policy: NotifyPolicy,
    target: InboxOpen,
    task: String,
    lead: String,
) {
    if policy == NotifyPolicy::Off {
        return;
    }
    let focused = MAIN_FOCUSED.load(Ordering::Relaxed);
    let visible = VISIBLE_BOT.lock().ok().and_then(|bot| bot.clone());
    if same_bot_is_visible(visible.as_deref(), focused, &target.loupist_id) {
        return;
    }
    let now = crate::commands::now_ms();
    let digest = policy == NotifyPolicy::Digest;
    let ready_at = if digest {
        let next_briefing = state
            .store
            .scheduled_tasks_list(&target.loupist_id)
            .ok()
            .and_then(|tasks| {
                tasks
                    .into_iter()
                    .filter(|task| {
                        task.kind == TaskKind::Briefing
                            && task.status == crate::scheduled_tasks::types::TaskStatus::Active
                    })
                    .filter_map(|task| task.next_run_at_ms)
                    .min()
            });
        next_briefing.unwrap_or_else(|| {
            let local = chrono::Local::now();
            let zone = crate::scheduled_tasks::presets::local_timezone().to_string();
            let schedule = crate::scheduled_tasks::types::Schedule::Daily {
                hour: 18,
                minute: 0,
                timezone: zone,
            };
            crate::scheduled_tasks::schedule::next_run_after(&schedule, local.timestamp_millis())
                .ok()
                .flatten()
                .unwrap_or(now + COALESCE_MS)
        })
    } else {
        now + COALESCE_MS
    };
    if update_queue(&state.store, |queue| {
        queue.enqueue(
            PendingResult {
                target,
                task,
                lead,
                ready_at,
                digest,
            },
            now,
        );
        true
    })
    .is_err()
    {
        eprintln!("Desktop notification could not be queued");
    }
}

pub(crate) fn queue_crossing(state: &AppState, loupist_id: &str, name: &str, claim: &str) {
    if APP.get().is_none() {
        return;
    }
    let policy = read_settings(&state.artifacts.app_data_dir)
        .unwrap_or_default()
        .notifications
        .defaults
        .crossed_line;
    let task = TEMPLATES
        .lock()
        .ok()
        .and_then(|entry| entry.clone())
        .unwrap_or_default()
        .line_crossed;
    queue_notification(
        state,
        policy,
        InboxOpen {
            loupist_id: loupist_id.into(),
            run_id: None,
        },
        task,
        format!("{name} · {claim}").chars().take(280).collect(),
    );
}

pub(crate) fn resolve_trigger_delivery(
    state: &AppState,
    task: &crate::scheduled_tasks::types::Task,
) -> (DeliveryTarget, NotifyPolicy) {
    let delivery = task.delivery();
    let policy = if delivery.to == DeliveryTarget::Bot {
        notify_for(state, task)
    } else {
        NotifyPolicy::Off
    };
    (delivery.to, policy)
}

/// A met check uses the same queue as a finished run, with its reading as the body.
pub(crate) fn notify_trigger(
    state: &AppState,
    loupist_id: &str,
    title: &str,
    reading: &str,
    policy: NotifyPolicy,
) {
    queue_notification(
        state,
        policy,
        InboxOpen {
            loupist_id: loupist_id.into(),
            run_id: None,
        },
        title.to_owned(),
        reading.chars().take(280).collect(),
    );
}

pub(crate) async fn flush_notifications(state: &AppState) {
    let Some(app) = APP.get() else { return };
    let state = state.clone();
    if tokio::task::spawn_blocking(move || flush_notification_queue(&state, app))
        .await
        .is_err()
    {
        eprintln!("Desktop notification worker stopped");
    }
}

fn flush_queue(
    store: &SqliteStore,
    local: chrono::DateTime<chrono::FixedOffset>,
    settings: &NotificationSettings,
    visible_bot: Option<&str>,
    deliver: impl FnMut(&str, &PendingResult, usize) -> Delivery,
) -> StoreResult<()> {
    if settings.is_quiet_at(local.hour() * 60 + local.minute()) {
        return Ok(());
    }
    let day = Some(local.num_days_from_ce());
    let now = local.timestamp_millis();
    let mut ready = std::collections::BTreeSet::new();
    update_queue(store, |queue| {
        let before = (queue.day, queue.sent, queue.pending.len());
        if queue.day != day {
            queue.day = day;
            queue.sent = 0;
        }
        if settings.daily_cap == 0 {
            queue.pending.clear();
        } else if queue.sent < settings.daily_cap {
            queue.pending.retain(|result| {
                result.ready_at > now || visible_bot != Some(result.target.loupist_id.as_str())
            });
            ready.extend(
                queue
                    .pending
                    .iter()
                    .filter(|result| result.ready_at <= now)
                    .map(|result| result.target.loupist_id.clone()),
            );
        }
        before != (queue.day, queue.sent, queue.pending.len())
    })?;
    let mut deliver = deliver;
    for id in ready {
        let mut reserved = Vec::new();
        // Claim and persist one group before invoking the non-transactional OS.
        update_queue(store, |queue| {
            if queue.day != day || queue.sent >= settings.daily_cap {
                return false;
            }
            let (results, pending) = std::mem::take(&mut queue.pending)
                .into_iter()
                .partition(|result| result.ready_at <= now && result.target.loupist_id == id);
            queue.pending = pending;
            reserved = results;
            if reserved.is_empty() {
                return false;
            }
            queue.sent += 1;
            true
        })?;
        let Some(first) = reserved.first() else {
            continue;
        };
        let delivery = deliver(&id, first, reserved.len());
        if !matches!(delivery, Delivery::Sent) {
            update_queue(store, |queue| {
                if matches!(delivery, Delivery::Retry) {
                    queue.pending.extend(reserved);
                }
                if queue.day == day {
                    queue.sent -= 1;
                }
                true
            })?;
        }
    }
    Ok(())
}

fn flush_notification_queue(state: &AppState, app: &tauri::AppHandle) {
    let Ok(settings) =
        read_settings(&state.artifacts.app_data_dir).map(|settings| settings.notifications)
    else {
        return;
    };
    let focused = MAIN_FOCUSED.load(Ordering::Relaxed);
    let visible = focused
        .then(|| VISIBLE_BOT.lock().ok().and_then(|bot| bot.clone()))
        .flatten();
    let templates = TEMPLATES
        .lock()
        .ok()
        .and_then(|entry| entry.clone())
        .unwrap_or_default();
    if flush_queue(
        &state.store,
        crate::local_time::now(),
        &settings,
        visible.as_deref(),
        |id, first, count| {
            let profile = match state.store.get_loupist_recorded(id) {
                Ok(Some(profile)) => profile,
                Ok(None) => return Delivery::Discard,
                Err(_) => return Delivery::Retry,
            };
            let (title, body) =
                notification_text(&profile.name, first, count, &settings, &templates);
            if super::notification_os::submit(app, &title, body) {
                Delivery::Sent
            } else {
                Delivery::Retry
            }
        },
    )
    .is_err()
    {
        eprintln!("Desktop notification queue could not be updated");
    }
}

fn notification_text<'a>(
    name: &str,
    first: &'a PendingResult,
    count: usize,
    settings: &NotificationSettings,
    templates: &NotificationTemplates,
) -> (String, &'a str) {
    if count > 1 || first.digest {
        (
            templates
                .digest
                .replace("{count}", &count.to_string())
                .replace("{loupist}", name),
            "",
        )
    } else if settings.show_details {
        (
            templates
                .result
                .replace("{task}", &first.task)
                .replace("{loupist}", name),
            first.lead.as_str(),
        )
    } else {
        (templates.private_result.replace("{loupist}", name), "")
    }
}

/// Emits a renderer event when the app is running (tests have no app handle).
pub(crate) fn emit(event: &str, payload: impl Serialize + Clone) {
    if let Some(app) = APP.get() {
        let _ = app.emit(event, payload);
    }
}

#[cfg(test)]
static CHANGED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// How many times the renderer was told this Loupist's chats changed.
#[cfg(test)]
pub(crate) fn changed_count(loupist_id: &str) -> usize {
    CHANGED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter()
        .filter(|id| id.as_str() == loupist_id)
        .count()
}

pub(crate) fn changed(loupist_id: &str) {
    #[cfg(test)]
    CHANGED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .push(loupist_id.to_owned());
    if let Some(app) = APP.get() {
        NativeEffects(app.clone()).changed(loupist_id);
        crate::commands::attention::refresh_indicator(app);
    }
}

pub(crate) fn open(app: &tauri::AppHandle, target: Option<InboxOpen>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    if let Some(target) = target {
        let _ = app.emit(crate::events::INBOX_OPEN, target);
    }
}

fn tray_entries(app: &tauri::AppHandle) -> Vec<(String, String)> {
    let Some(state) = app.try_state::<AppState>() else {
        return Vec::new();
    };
    state
        .store
        .list_loupists()
        .unwrap_or_default()
        .into_iter()
        .map(|loupist| {
            let count = state.store.inbox_unread_count(&loupist.id).unwrap_or(0);
            let title = if count == 0 {
                loupist.name
            } else {
                format!("{} ({count})", loupist.name)
            };
            (format!("briefing-loupist:{}", loupist.id), title)
        })
        .collect()
}

fn tray_menu(
    app: &tauri::AppHandle,
    entries: Vec<(String, String)>,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, Submenu};
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "briefing-open",
        "Open Loupist",
        true,
        None::<&str>,
    )?)?;
    let briefings = Submenu::new(app, "Briefings", true)?;
    for (id, title) in entries {
        briefings.append(&MenuItem::with_id(app, id, title, true, None::<&str>)?)?;
    }
    menu.append(&briefings)?;
    menu.append(&MenuItem::with_id(
        app,
        "briefing-quit",
        "Quit",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}
pub(crate) fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    let _ = APP.set(app.clone());
    let mut tray = tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .menu(&tray_menu(app, tray_entries(app))?)
        .tooltip("Loupist")
        .on_menu_event(|app, event| {
            tray_action(&NativeEffects(app.clone()), event.id.as_ref());
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

pub(crate) fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    if window.label() == "main" {
        if let tauri::WindowEvent::Focused(focused) = event {
            MAIN_FOCUSED.store(*focused, Ordering::Relaxed);
        }
    }
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
            if let Some(state) = window.try_state::<AppState>() {
                if close_window(
                    &NativeEffects(window.app_handle().clone()),
                    read_settings(&state.artifacts.app_data_dir).is_ok_and(|s| s.keep_running),
                ) {
                    MAIN_FOCUSED.store(false, Ordering::Relaxed);
                    api.prevent_close();
                }
            }
        }
    }
}

#[cfg(test)]
mod notification_tests {
    use super::*;

    #[test]
    fn met_check_queues_one_digest_and_honors_inherited_off() {
        use crate::scheduled_tasks::types::{
            RunStatus, RunTrigger, Schedule, TaskDraft, TaskStatus, TaskTarget,
        };

        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_test(root.path().join("app"));
        crate::test_fixtures::create_folder_loupist(&state.store, "loupist-trigger", root.path());
        let id = "loupist-trigger";
        let task = state
            .store
            .scheduled_task_create(
                id,
                TaskDraft {
                    title: "Price checks".into(),
                    prompt: "Explain the move".into(),
                    schedule: Schedule::Interval { every_minutes: 5 },
                    target: TaskTarget::NewChat,
                    model_profile_id: String::new(),
                    thinking_level: String::new(),
                    run_options: Default::default(),
                    use_memory: false,
                    update_memory: false,
                    status: TaskStatus::Active,
                },
                1,
            )
            .unwrap();

        let (_, policy) = resolve_trigger_delivery(&state, &task);
        assert_eq!(policy, NotifyPolicy::Digest);
        notify_trigger(&state, id, &task.title, "Price moved 7%", policy);
        let queue = state.store.desktop_notification_queue().unwrap();
        assert_eq!(queue.pending.len(), 1);
        assert!(queue.pending[0].digest);
        assert_eq!(queue.pending[0].lead, "Price moved 7%");

        let mut settings = ResidentSettings::default();
        settings.notifications.defaults.other = NotifyPolicy::Off;
        write_settings(&state.artifacts.app_data_dir, &settings).unwrap();
        let (_, off) = resolve_trigger_delivery(&state, &task);
        assert_eq!(off, NotifyPolicy::Off);
        notify_trigger(&state, id, &task.title, "Price moved 9%", off);
        assert_eq!(
            state
                .store
                .desktop_notification_queue()
                .unwrap()
                .pending
                .len(),
            1
        );

        // A later settings change cannot make completion announce the reading twice.
        settings.notifications.defaults.other = NotifyPolicy::Now;
        write_settings(&state.artifacts.app_data_dir, &settings).unwrap();
        let now = task.next_run_at_ms.unwrap();
        let (claim, ()) = state
            .store
            .scheduled_task_claim_with(id, &task.id, now, false, |_| Ok(()))
            .unwrap()
            .unwrap();
        let mut run = state
            .store
            .scheduled_task_finish(id, &claim.run.run_id, RunStatus::Succeeded, None, now + 1)
            .unwrap();
        run.trigger = Some(RunTrigger {
            version: 1,
            reading: "Price moved 7%".into(),
            as_of: "2026-11-24".into(),
        });
        queue_result(&state, &run);
        assert_eq!(
            state
                .store
                .desktop_notification_queue()
                .unwrap()
                .pending
                .len(),
            1
        );
    }

    #[test]
    fn quiet_and_digest_results_and_the_local_day_cap_survive_reopen() {
        let root = tempfile::tempdir().unwrap();
        let catalog = root.path().join("catalog.sqlite3");
        let at = |time| chrono::DateTime::parse_from_rfc3339(time).unwrap();
        let settings = NotificationSettings {
            daily_cap: 1,
            ..Default::default()
        };
        let enqueue = |id: &str, ready_at: chrono::DateTime<chrono::FixedOffset>, digest| {
            let store = SqliteStore::open(&catalog).unwrap();
            update_queue(&store, |queue| {
                queue.enqueue(
                    PendingResult {
                        target: InboxOpen {
                            loupist_id: "research".into(),
                            run_id: Some(id.into()),
                        },
                        task: "Filings".into(),
                        lead: String::new(),
                        ready_at: ready_at.timestamp_millis(),
                        digest,
                    },
                    ready_at.timestamp_millis(),
                );
                true
            })
            .unwrap();
        };
        let flush = |local| {
            let store = SqliteStore::open(&catalog).unwrap();
            let mut delivered = Vec::new();
            flush_queue(&store, local, &settings, None, |_, first, count| {
                assert_eq!(count, 1);
                delivered.push(first.target.run_id.clone().unwrap());
                Delivery::Sent
            })
            .unwrap();
            delivered
        };

        let first = at("2026-11-25T17:00:00+09:00");
        enqueue("first", first, false);
        assert_eq!(flush(first), ["first"]);
        let before_quiet = at("2026-11-25T21:00:00+09:00");
        enqueue("quiet", before_quiet, false);
        assert!(flush(before_quiet).is_empty());
        enqueue("digest", at("2026-11-26T18:00:00+09:00"), true);
        assert!(flush(at("2026-11-26T06:00:00+09:00")).is_empty());
        assert_eq!(flush(at("2026-11-26T08:00:00+09:00")), ["quiet"]);
        assert!(flush(at("2026-11-26T18:00:00+09:00")).is_empty());
        assert_eq!(flush(at("2026-11-27T08:00:00+09:00")), ["digest"]);
    }

    #[test]
    fn an_os_submission_is_reserved_on_disk_before_it_can_be_accepted() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("catalog.sqlite3");
        let store = SqliteStore::open(&path).unwrap();
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-25T17:00:00+09:00").unwrap();
        let settings = NotificationSettings {
            daily_cap: 2,
            ..Default::default()
        };
        let mut queue = NotificationQueue::default();
        for id in ["a", "b"] {
            queue.enqueue(
                PendingResult {
                    target: InboxOpen {
                        loupist_id: id.into(),
                        run_id: None,
                    },
                    task: "Check".into(),
                    lead: String::new(),
                    ready_at: now.timestamp_millis(),
                    digest: false,
                },
                now.timestamp_millis(),
            );
        }
        store.save_desktop_notification_queue(&queue).unwrap();
        let mut accepted = false;
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = flush_queue(&store, now, &settings, None, |id, _, _| {
                assert_eq!(id, "a");
                accepted = true;
                panic!("Process exited after the OS accepted the first group");
            });
        }));
        assert!(accepted && interrupted.is_err());
        let mut retried = Vec::new();
        let reopened = SqliteStore::open(&path).unwrap();
        flush_queue(&reopened, now, &settings, None, |id, _, _| {
            retried.push(id.to_owned());
            Delivery::Sent
        })
        .unwrap();
        assert_eq!(retried, ["b"]);
        let saved = reopened.desktop_notification_queue().unwrap();
        assert_eq!(saved.sent, 2);
        assert!(saved.pending.is_empty());
    }

    #[test]
    fn refused_or_failed_deliveries_keep_their_results_and_leave_the_slot_free() {
        let local = chrono::DateTime::parse_from_rfc3339("2026-11-25T17:00:00+09:00").unwrap();
        let settings = NotificationSettings {
            daily_cap: 1,
            ..Default::default()
        };
        let mut queue = NotificationQueue::default();
        for id in ["denied", "failed", "failed", "succeeded"] {
            queue.enqueue(
                PendingResult {
                    target: InboxOpen {
                        loupist_id: id.into(),
                        run_id: None,
                    },
                    task: "Filings".into(),
                    lead: String::new(),
                    ready_at: local.timestamp_millis(),
                    digest: false,
                },
                local.timestamp_millis(),
            );
        }
        let store = SqliteStore::open_in_memory().unwrap();
        store.save_desktop_notification_queue(&queue).unwrap();
        let mut attempts = Vec::new();
        flush_queue(&store, local, &settings, None, |id, _, count| {
            attempts.push((id.to_owned(), count));
            match id {
                "denied" | "failed" => Delivery::Retry,
                "succeeded" => Delivery::Sent,
                _ => unreachable!(),
            }
        })
        .unwrap();
        assert_eq!(
            attempts,
            [
                ("denied".into(), 1),
                ("failed".into(), 2),
                ("succeeded".into(), 1)
            ]
        );
        let queue = store.desktop_notification_queue().unwrap();
        assert_eq!(queue.sent, 1);
        assert_eq!(queue.pending.len(), 3);
        flush_queue(&store, local, &settings, None, |_, _, _| {
            panic!("The daily slot is spent")
        })
        .unwrap();

        let next_day = local + chrono::Days::new(1);
        let settings = NotificationSettings {
            daily_cap: 2,
            ..settings
        };
        attempts.clear();
        flush_queue(&store, next_day, &settings, None, |id, _, count| {
            attempts.push((id.to_owned(), count));
            Delivery::Sent
        })
        .unwrap();
        assert_eq!(attempts, [("denied".into(), 1), ("failed".into(), 2)]);
        let queue = store.desktop_notification_queue().unwrap();
        assert_eq!(queue.sent, 2);
        assert!(queue.pending.is_empty());
    }

    #[test]
    fn notification_settings_open_the_macos_notifications_pane() {
        assert_eq!(
            notification_settings_url(true),
            Some("x-apple.systempreferences:com.apple.Notifications-Settings.extension")
        );
        assert_eq!(notification_settings_url(false), None);
    }

    #[test]
    fn only_the_same_bot_in_a_focused_window_suppresses_notifications() {
        assert!(same_bot_is_visible(Some("loupist-a"), true, "loupist-a"));
        assert!(!same_bot_is_visible(Some("loupist-b"), true, "loupist-a"));
        assert!(!same_bot_is_visible(Some("loupist-a"), false, "loupist-a"));
        assert!(!same_bot_is_visible(None, true, "loupist-a"));
    }

    #[test]
    fn legacy_settings_inherit_quiet_defaults_and_task_overrides_win() {
        let settings: NotificationSettings =
            serde_json::from_str(r#"{"quietHours":null,"dailyCap":6,"showDetails":true}"#).unwrap();
        let policy = settings.defaults;
        assert_eq!(policy.crossed_line, NotifyPolicy::Now);
        assert_eq!(policy.failures, NotifyPolicy::Digest);
        assert_eq!(
            policy.for_task(&TaskKind::Briefing, None, None),
            NotifyPolicy::Now
        );
        assert_eq!(
            policy.for_task(
                &TaskKind::Custom,
                Some(WatchPreset::FilingsAndResults),
                None
            ),
            NotifyPolicy::Now
        );
        for preset in [
            None,
            Some(WatchPreset::ThesisLines),
            Some(WatchPreset::ResultsFollowUp),
        ] {
            assert_eq!(
                policy.for_task(&TaskKind::Custom, preset, None),
                NotifyPolicy::Digest
            );
        }
        let changed = NotificationDefaults {
            briefing: NotifyPolicy::Off,
            ..policy
        };
        assert_eq!(
            changed.for_task(&TaskKind::Briefing, None, None),
            NotifyPolicy::Off
        );
        assert_eq!(
            changed.for_task(&TaskKind::Briefing, None, Some(NotifyPolicy::Now)),
            NotifyPolicy::Now
        );
    }

    #[test]
    fn localized_groups_preserve_privacy_and_use_the_bound_run() {
        let first = PendingResult {
            target: InboxOpen {
                loupist_id: "research".into(),
                run_id: Some("run-a".into()),
            },
            task: "Filings".into(),
            lead: "A private asset name".into(),
            ready_at: 1,
            digest: false,
        };
        let templates = NotificationTemplates::default();
        let settings = NotificationSettings {
            show_details: false,
            ..Default::default()
        };
        assert_eq!(
            notification_text("Research", &first, 1, &settings, &templates),
            ("Research: a result is ready".into(), "")
        );
        assert_eq!(
            notification_text("Research", &first, 3, &settings, &templates),
            ("3 results in Research".into(), "")
        );
        assert_eq!(first.target.run_id.as_deref(), Some("run-a"));
    }

    #[test]
    fn default_quiet_hours_cross_midnight_and_can_be_disabled() {
        let mut settings = NotificationSettings::default();
        assert!(!settings.is_quiet_at(21 * 60 + 59));
        assert!(settings.is_quiet_at(22 * 60));
        assert!(settings.is_quiet_at(0));
        assert!(settings.is_quiet_at(6 * 60 + 59));
        assert!(!settings.is_quiet_at(7 * 60));
        settings.quiet_hours = None;
        assert!(!settings.is_quiet_at(0));
    }
}
