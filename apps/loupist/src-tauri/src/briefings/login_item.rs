//! Only a user action can change the main application's macOS login registration.
use super::desktop::{read_settings, write_settings, ResidentSettings, SETTINGS_LOCK};
use crate::app::{AppState, CommandError, CommandErrorCode};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) enum LoginItemStatus {
    Off,
    On,
    RequiresApproval,
    Error,
    Unavailable,
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopSettings {
    #[serde(flatten)]
    pub resident: ResidentSettings,
    pub login_item: LoginItemStatus,
    pub login_item_turned_off: bool,
}

#[cfg(any(target_os = "macos", test))]
fn installed_bundle(identifier: &str, path: &Path) -> bool {
    use std::path::Component;
    let path_ok = path.starts_with("/Applications")
        || path.strip_prefix("/Users").is_ok_and(|rest| {
            let mut parts = rest.components();
            matches!(parts.next(), Some(Component::Normal(_)))
                && parts.next() == Some(Component::Normal("Applications".as_ref()))
        });
    identifier == crate::PRODUCTION_APP_IDENTIFIER
        && path_ok
        && path.extension().is_some_and(|ext| ext == "app")
        && !path
            .components()
            .any(|part| part == Component::ParentDir || part.as_os_str() == "AppTranslocation")
        && !path
            .ancestors()
            .skip(1)
            .any(|part| part.extension().is_some_and(|ext| ext == "app"))
}

#[cfg(any(target_os = "macos", test))]
fn system_status(value: isize) -> LoginItemStatus {
    match value {
        0 => LoginItemStatus::Off, // SMAppServiceStatusNotRegistered
        1 => LoginItemStatus::On,
        2 => LoginItemStatus::RequiresApproval,
        _ => LoginItemStatus::Error, // NotFound or an unknown value
    }
}

impl LoginItemStatus {
    fn registered(self) -> bool {
        self == Self::On || self == Self::RequiresApproval
    }
}

trait Service {
    fn status(&self) -> LoginItemStatus;
    fn set(&self, enabled: bool) -> LoginItemStatus;
    fn open_settings(&self);
}

fn unavailable() -> CommandError {
    CommandError::new(
        CommandErrorCode::LoginItemUnavailable,
        "Login items require an installed release bundle",
    )
}

#[cfg(target_os = "macos")]
pub(crate) mod macos;
fn service() -> Option<&'static dyn Service> {
    #[cfg(target_os = "macos")]
    if macos::available() {
        return Some(&macos::MainApp);
    }
    None
}

pub(super) enum Action {
    Read,
    MenuBar(bool),
    Notifications(super::desktop::NotificationSettings),
    Login(bool),
    Dismiss,
}

pub(super) fn apply(state: &AppState, action: Action) -> Result<DesktopSettings, CommandError> {
    update(&state.artifacts.app_data_dir, service(), action)
}

fn update(
    root: &Path,
    service: Option<&dyn Service>,
    action: Action,
) -> Result<DesktopSettings, CommandError> {
    use LoginItemStatus::*;
    let _guard = SETTINGS_LOCK
        .lock()
        .map_err(|_| CommandError::internal("Briefing settings lock failed"))?;
    let mut resident = read_settings(root)?;
    let mut status = service.map_or(Unavailable, Service::status);
    let mut turned_off = false;
    let mut save = status == On && !resident.login_hint_dismissed;
    resident.login_hint_dismissed |= status == On;

    match action {
        Action::Read => {}
        Action::Notifications(settings) => {
            settings.validate()?;
            resident.notifications = settings;
            save = true;
        }
        Action::Dismiss => {
            resident.login_hint_dismissed = true;
            save = true;
        }
        Action::MenuBar(keep) => {
            if !keep && status.registered() {
                let previous = status;
                status = service.ok_or_else(unavailable)?.set(false);
                if status != Off {
                    return Ok(DesktopSettings {
                        resident,
                        login_item: Error,
                        login_item_turned_off: false,
                    });
                }
                turned_off = previous == On;
            }
            resident.keep_running = keep;
            resident.keep_running_choice = Some(keep);
            save = true;
        }
        Action::Login(enabled) => {
            let service = service.ok_or_else(unavailable)?;
            if enabled && matches!(status, Off | On) {
                // Persist residence before enabling an OS-managed launch. A crash between
                // the two can leave the menu bar on but never a login item without it.
                let previous = resident.clone();
                resident.keep_running = true;
                resident.keep_running_choice = Some(true);
                resident.login_hint_dismissed = true;
                write_settings(root, &resident)?;
                if status == Off {
                    status = service.set(true);
                }
                if !status.registered() {
                    resident = previous;
                    save = true;
                }
            } else if !enabled && status.registered() {
                status = service.set(false);
            }
        }
    }
    if save {
        write_settings(root, &resident)?;
    }
    Ok(DesktopSettings {
        resident,
        login_item: status,
        login_item_turned_off: turned_off,
    })
}

#[derive(Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginItemRequest {
    enabled: bool,
}

#[tauri::command]
pub(crate) fn login_item_set(
    request: LoginItemRequest,
    state: tauri::State<'_, AppState>,
) -> Result<DesktopSettings, CommandError> {
    apply(&state, Action::Login(request.enabled))
}

#[tauri::command]
pub(crate) fn login_item_hint_dismiss(
    state: tauri::State<'_, AppState>,
) -> Result<DesktopSettings, CommandError> {
    apply(&state, Action::Dismiss)
}

#[tauri::command]
pub(crate) fn login_item_open_settings() -> Result<(), CommandError> {
    service().ok_or_else(unavailable)?.open_settings();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use LoginItemStatus::*;

    #[test]
    fn only_an_installed_release_bundle_is_admitted() {
        const ID: &str = "com.monarchjuno.loupist";
        for path in [
            "/Applications/Loupist.app",
            "/Users/example/Applications/Loupist.app",
        ] {
            assert!(installed_bundle(ID, Path::new(path)));
            for id in [
                "com.monarchjuno.loupist.development",
                "com.monarchjuno.loupist.e2e",
                "com.monarchjuno.loupist.qa-preview",
                "",
            ] {
                assert!(!installed_bundle(id, Path::new(path)));
            }
        }
        for path in [
            "/Volumes/Loupist/Loupist.app",
            "/private/var/folders/x/AppTranslocation/id/d/Loupist.app",
            "/Applications/AppTranslocation/id/Loupist.app",
            "/Users/example/Downloads/Loupist.app",
            "/Applications/../tmp/Loupist.app",
            "/Applications/Host.app/Contents/Helpers/Loupist.app",
            "/Applications/Loupist",
            "Applications/Loupist.app",
        ] {
            assert!(!installed_bundle(ID, Path::new(path)), "{path}");
        }
    }

    #[test]
    fn reads_all_sm_app_service_statuses() {
        assert_eq!(
            [0, 1, 2, 3, 99].map(system_status),
            [Off, On, RequiresApproval, Error, Error]
        );
    }

    struct Fake {
        status: Cell<LoginItemStatus>,
        calls: RefCell<Vec<bool>>,
        deny: Cell<bool>,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                status: Cell::new(Off),
                calls: RefCell::default(),
                deny: Cell::new(false),
            }
        }
    }
    impl Service for Fake {
        fn status(&self) -> LoginItemStatus {
            self.status.get()
        }
        fn set(&self, enabled: bool) -> LoginItemStatus {
            self.calls.borrow_mut().push(enabled);
            if self.deny.get() {
                return Error;
            }
            self.status.set(if enabled { On } else { Off });
            self.status()
        }
        fn open_settings(&self) {}
    }

    #[test]
    fn changes_are_coupled_and_the_hint_stays_dismissed_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let apply = |action| update(dir.path(), Some(&fake), action).unwrap();
        assert!(!apply(Action::Read).resident.login_hint_dismissed);
        assert_eq!(apply(Action::MenuBar(true)).login_item, Off);
        apply(Action::MenuBar(false));
        assert_eq!(
            apply(Action::Login(true)).resident.keep_running_choice,
            Some(true)
        );
        assert_eq!(apply(Action::Login(true)).login_item, On);
        let off = apply(Action::Login(false));
        assert_eq!(off.login_item, Off);
        assert!(off.resident.keep_running && off.resident.login_hint_dismissed);
        assert_eq!(apply(Action::Login(false)).login_item, Off);
        apply(Action::Login(true));
        let both = apply(Action::MenuBar(false));
        assert_eq!(both.login_item, Off);
        assert!(both.login_item_turned_off);
        assert!(!both.resident.keep_running);
        assert!(!apply(Action::Read).login_item_turned_off);
        assert!(read_settings(dir.path()).unwrap().login_hint_dismissed);
        assert_eq!(*fake.calls.borrow(), vec![true, false, true, false]);
    }

    #[test]
    fn honors_system_changes_and_rolls_back_denials() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let apply = |action| update(dir.path(), Some(&fake), action).unwrap();
        fake.deny.set(true);
        let denied = apply(Action::Login(true));
        assert_eq!(denied.login_item, Error);
        assert!(!denied.resident.keep_running && !denied.resident.login_hint_dismissed);
        fake.status.set(On);
        let kept = apply(Action::MenuBar(true));
        assert!(kept.resident.keep_running);
        let failed = apply(Action::MenuBar(false));
        assert_eq!(failed.login_item, Error);
        assert!(failed.resident.keep_running && !failed.login_item_turned_off);
        fake.status.set(RequiresApproval);
        assert_eq!(apply(Action::Read).login_item, RequiresApproval);
        assert_eq!(apply(Action::Login(true)).login_item, RequiresApproval);
        fake.status.set(Off);
        assert!(apply(Action::Read).resident.login_hint_dismissed);
        assert_eq!(*fake.calls.borrow(), vec![true, false]);
    }

    #[test]
    fn dismissing_only_changes_the_hint_and_unavailable_registration_is_typed() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::new();
        let dismissed = update(dir.path(), Some(&fake), Action::Dismiss).unwrap();
        assert!(dismissed.resident.login_hint_dismissed);
        assert_eq!(dismissed.login_item, Off);
        assert!(!dismissed.resident.keep_running && fake.calls.borrow().is_empty());
        assert!(read_settings(dir.path()).unwrap().login_hint_dismissed);
        assert_eq!(
            update(dir.path(), None, Action::Read).unwrap().login_item,
            Unavailable
        );
        assert_eq!(
            update(dir.path(), None, Action::Login(true))
                .unwrap_err()
                .code,
            CommandErrorCode::LoginItemUnavailable
        );
    }
}
