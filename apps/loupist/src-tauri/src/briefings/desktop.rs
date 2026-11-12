//! Native effects are isolated from schedule persistence and testable with fakes.
use crate::{
    app::{AppState, CommandError},
    scheduled_tasks::types::{Run, RunStatus, TaskKind},
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::Path,
    sync::{Mutex, OnceLock},
};
use tauri::{Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

static APP: OnceLock<tauri::AppHandle> = OnceLock::new();
static SETTINGS_LOCK: Mutex<()> = Mutex::new(());
const TRAY_ID: &str = "briefings-tray";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResidentSettings {
    pub keep_running: bool,
    pub notification_permission_asked: bool,
    /// None follows the automatic default; Some preserves the user's choice.
    #[serde(default)]
    pub keep_running_choice: Option<bool>,
}
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct InboxChanged {
    pub loupist_id: String,
}
#[derive(Clone, Debug, Serialize)]
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

pub(crate) trait DesktopEffects {
    fn notify(&self, title: &str, body: &str, target: &InboxOpen);
    fn changed(&self, loupist_id: &str);
}

pub(crate) fn deliver(effects: &dyn DesktopEffects, loupist_name: &str, run: &Run) {
    let task = if run.kind == TaskKind::Briefing {
        "briefing"
    } else {
        &run.task_title
    };
    let suffix = match run.status {
        RunStatus::Succeeded => "ready",
        RunStatus::Failed => "couldn't finish",
        RunStatus::Interrupted
            if run.error.as_deref() == Some(crate::scheduled_tasks::USER_STOPPED) =>
        {
            "stopped"
        }
        RunStatus::Interrupted => "couldn't finish",
        RunStatus::Running => "running",
    };
    effects.notify(
        &format!("{loupist_name}: {task} {suffix}"),
        run.summary.lines().next().unwrap_or(""),
        &InboxOpen {
            loupist_id: run.loupist_id.clone(),
            run_id: Some(run.run_id.clone()),
        },
    );
    effects.changed(&run.loupist_id);
}
struct NativeEffects(tauri::AppHandle);
impl DesktopEffects for NativeEffects {
    fn notify(&self, title: &str, body: &str, _target: &InboxOpen) {
        let notifications = self.0.notification();
        if matches!(
            notifications.permission_state(),
            Ok(tauri_plugin_notification::PermissionState::Granted)
        ) && notifications
            .builder()
            .title(title)
            .body(body)
            .show()
            .is_err()
        {
            eprintln!("Briefing desktop notification could not be delivered");
        }
    }
    fn changed(&self, loupist_id: &str) {
        let _ = self.0.emit(
            crate::events::INBOX_CHANGED,
            InboxChanged {
                loupist_id: loupist_id.into(),
            },
        );
        let _ = refresh_tray(&self.0);
    }
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
fn write_settings(root: &Path, settings: &ResidentSettings) -> Result<(), CommandError> {
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

pub(crate) fn enable_for_watching(state: &AppState) -> Result<(), CommandError> {
    enable_automatic(state, false)
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
        if let Some(app) = APP.get() {
            let _ = app.notification().request_permission();
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn briefing_resident_settings_get(
    state: tauri::State<'_, AppState>,
) -> Result<ResidentSettings, CommandError> {
    read_settings(&state.artifacts.app_data_dir)
}
#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ResidentRequest {
    keep_running: bool,
}
#[tauri::command]
pub(crate) fn briefing_resident_settings_set(
    request: ResidentRequest,
    state: tauri::State<'_, AppState>,
) -> Result<ResidentSettings, CommandError> {
    set_keep_running(&state, request.keep_running)
}

pub(super) fn set_keep_running(
    state: &AppState,
    keep_running: bool,
) -> Result<ResidentSettings, CommandError> {
    let _lock = SETTINGS_LOCK
        .lock()
        .map_err(|_| CommandError::internal("Briefing settings lock failed"))?;
    let mut settings = read_settings(&state.artifacts.app_data_dir)?;
    settings.keep_running = keep_running;
    settings.keep_running_choice = Some(keep_running);
    write_settings(&state.artifacts.app_data_dir, &settings)?;
    Ok(settings)
}

pub(crate) fn completed(state: &AppState, run: &Run) {
    // Results are read in the home Thread; a busy Thread receives them later.
    if let Err(error) = crate::chat::home::deliver_finished_runs(state, &run.loupist_id) {
        eprintln!("Scheduled result could not be posted to the Thread ({error})");
    }
    if let Some(app) = APP.get() {
        let name = state
            .store
            .get_loupist(&run.loupist_id)
            .ok()
            .flatten()
            .map(|loupist| loupist.name)
            .unwrap_or_else(|| "Loupist".into());
        deliver(&NativeEffects(app.clone()), &name, run);
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

fn tray_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
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
    if let Some(state) = app.try_state::<AppState>() {
        if let Ok(loupists) = state.store.list_loupists() {
            for loupist in loupists {
                let count = state.store.inbox_unread_count(&loupist.id).unwrap_or(0);
                let title = if count == 0 {
                    loupist.name
                } else {
                    format!("{} ({count})", loupist.name)
                };
                briefings.append(&MenuItem::with_id(
                    app,
                    format!("briefing-loupist:{}", loupist.id),
                    title,
                    true,
                    None::<&str>,
                )?)?;
            }
        }
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
fn refresh_tray(app: &tauri::AppHandle) -> tauri::Result<()> {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(tray_menu(app)?))?;
    }
    Ok(())
}

pub(crate) fn install(app: &tauri::AppHandle) -> tauri::Result<()> {
    let _ = APP.set(app.clone());
    let mut tray = tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .menu(&tray_menu(app)?)
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

pub(crate) fn close_requested(window: &tauri::Window, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
        if window.label() == "main" {
            if let Some(state) = window.try_state::<AppState>() {
                if close_window(
                    &NativeEffects(window.app_handle().clone()),
                    read_settings(&state.artifacts.app_data_dir).is_ok_and(|s| s.keep_running),
                ) {
                    api.prevent_close();
                }
            }
        }
    }
}
