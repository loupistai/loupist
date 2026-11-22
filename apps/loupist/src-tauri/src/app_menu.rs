use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{LazyLock, Mutex};

use serde::Deserialize;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    AppHandle, Emitter, Manager, WebviewWindow, WindowEvent,
};

use crate::app::{AppState, CommandError};

#[cfg(target_os = "macos")]
pub(crate) mod macos;

const ZOOM_STEPS: &[u16] = &[50, 67, 80, 90, 100, 110, 125, 150, 175, 200];
const ACTUAL_SIZE: u16 = 100;
const ZOOM_FILE: &str = "webview-zoom.json";

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
enum MenuLanguage {
    #[default]
    En,
    Ko,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AppMenuContext {
    language: MenuLanguage,
    shell: bool,
    loupist: bool,
    print: bool,
    sidebar: bool,
    modal: bool,
}

impl Default for AppMenuContext {
    fn default() -> Self {
        // Until this document reports its state, the renderer guards its own actions.
        Self {
            language: MenuLanguage::default(),
            shell: true,
            loupist: true,
            print: true,
            sidebar: true,
            modal: false,
        }
    }
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum NativeRole {
    Separator,
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Fullscreen,
    Minimize,
    Maximize,
    BringAllToFront,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum AvailableWhen {
    #[default]
    Always,
    Shell,
    Sidebar,
    Loupist,
    Print,
    Window,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MenuRow {
    id: Option<String>,
    role: Option<NativeRole>,
    labels: Option<[String; 2]>,
    mac_labels: Option<[String; 2]>,
    alternate: Option<[String; 2]>,
    translation: Option<String>,
    #[serde(default)]
    ellipsis: bool,
    #[serde(default)]
    mac_only: bool,
    accelerator: Option<String>,
    mac_accelerator: Option<String>,
    #[serde(default)]
    when: AvailableWhen,
}

#[derive(Deserialize)]
struct MenuGroup {
    id: String,
    labels: [String; 2],
    items: Vec<MenuRow>,
}

static MENU: LazyLock<Vec<MenuGroup>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../src/app/nativeMenu.json")).expect("native menu table")
});
static APP_TRANSLATIONS: LazyLock<[serde_json::Value; 2]> = LazyLock::new(|| {
    [
        serde_json::from_str(include_str!("../../src/i18n/locales/en/app.json"))
            .expect("English menu translations"),
        serde_json::from_str(include_str!("../../src/i18n/locales/ko/app.json"))
            .expect("Korean menu translations"),
    ]
});

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MainWindow {
    available: bool,
    native_modal: bool,
    fullscreen: bool,
    maximized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MenuState {
    context: AppMenuContext,
    window: MainWindow,
    zoom: u16,
}

struct InstalledMenu(Mutex<MenuState>);

impl MenuState {
    fn reset_renderer_context(&mut self) {
        self.context = AppMenuContext {
            language: self.context.language,
            ..AppMenuContext::default()
        };
    }
}

impl MenuLanguage {
    fn index(self) -> usize {
        usize::from(self == Self::Ko)
    }
}

impl MenuRow {
    fn label(&self, state: MenuState, app_name: &str, mac: bool) -> String {
        let index = state.context.language.index();
        if let Some(key) = self.translation.as_ref() {
            let translated = key
                .split('.')
                .fold(&APP_TRANSLATIONS[index], |entry, name| &entry[name]);
            let value = translated.as_str().expect("native menu translation key");
            return if self.ellipsis {
                format!("{value}…")
            } else {
                value.to_owned()
            };
        }
        let alternate = match self.role {
            Some(NativeRole::Fullscreen) => state.window.fullscreen,
            Some(NativeRole::Maximize) => state.window.maximized,
            _ => false,
        };
        let labels = if alternate {
            self.alternate.as_ref()
        } else if mac {
            self.mac_labels.as_ref().or(self.labels.as_ref())
        } else {
            self.labels.as_ref()
        };
        labels.expect("native menu label")[index].replace("{app}", app_name)
    }

    fn enabled(&self, state: MenuState) -> bool {
        let window = state.window.available && !state.window.native_modal && !state.context.modal;
        let shell = window && state.context.shell;
        match self.when {
            AvailableWhen::Always => true,
            AvailableWhen::Shell => shell,
            AvailableWhen::Sidebar => shell && state.context.sidebar,
            AvailableWhen::Loupist => shell && state.context.loupist,
            AvailableWhen::Print => shell && state.context.print,
            AvailableWhen::Window => window,
            AvailableWhen::ZoomIn => window && zoom_step(state.zoom, 1).is_some(),
            AvailableWhen::ZoomOut => window && zoom_step(state.zoom, -1).is_some(),
            AvailableWhen::ZoomReset => window && state.zoom != ACTUAL_SIZE,
        }
    }
}

impl MenuGroup {
    fn label(&self, language: MenuLanguage, app_name: &str) -> String {
        self.labels[language.index()].replace("{app}", app_name)
    }
}

fn app_display_name(app: &AppHandle) -> String {
    #[cfg(target_os = "macos")]
    {
        use objc2_foundation::{NSBundle, NSString};

        let bundle = NSBundle::mainBundle();
        if let Some(name) = ["CFBundleDisplayName", "CFBundleName"]
            .iter()
            .filter_map(|key| bundle.objectForInfoDictionaryKey(&NSString::from_str(key)))
            .filter_map(|value| value.downcast::<NSString>().ok())
            .map(|name| name.to_string())
            .find(|name| !name.trim().is_empty())
        {
            return name;
        }
    }
    app.config()
        .product_name
        .clone()
        .unwrap_or_else(|| "Loupist".to_owned())
}

fn native_item(
    app: &AppHandle,
    role: NativeRole,
    label: &str,
) -> tauri::Result<Option<PredefinedMenuItem<tauri::Wry>>> {
    let label = Some(label);
    let item = match role {
        NativeRole::About => PredefinedMenuItem::about(app, label, None),
        NativeRole::Services => PredefinedMenuItem::services(app, label),
        NativeRole::Hide => PredefinedMenuItem::hide(app, label),
        NativeRole::HideOthers => PredefinedMenuItem::hide_others(app, label),
        NativeRole::ShowAll => PredefinedMenuItem::show_all(app, label),
        NativeRole::Undo => PredefinedMenuItem::undo(app, label),
        NativeRole::Redo => PredefinedMenuItem::redo(app, label),
        NativeRole::Cut => PredefinedMenuItem::cut(app, label),
        NativeRole::Copy => PredefinedMenuItem::copy(app, label),
        NativeRole::Paste => PredefinedMenuItem::paste(app, label),
        NativeRole::SelectAll => PredefinedMenuItem::select_all(app, label),
        NativeRole::BringAllToFront => PredefinedMenuItem::bring_all_to_front(app, label),
        NativeRole::Quit if cfg!(target_os = "macos") => PredefinedMenuItem::quit(app, label),
        NativeRole::Fullscreen if cfg!(target_os = "macos") => {
            PredefinedMenuItem::fullscreen(app, label)
        }
        NativeRole::Minimize if !cfg!(target_os = "linux") => {
            PredefinedMenuItem::minimize(app, label)
        }
        NativeRole::Maximize if !cfg!(target_os = "linux") => {
            PredefinedMenuItem::maximize(app, label)
        }
        _ => return Ok(None),
    };
    item.map(Some)
}

fn build_menu(app: &AppHandle, state: MenuState) -> tauri::Result<()> {
    let mac = cfg!(target_os = "macos");
    // Muda treats unescaped ampersands as mnemonics in menu titles.
    let app_name = app_display_name(app).replace('&', "&&");
    let menu = Menu::new(app)?;
    for group in MENU.iter() {
        let submenu = Submenu::with_id(
            app,
            &group.id,
            group.label(state.context.language, &app_name),
            group.id != "edit" || state.window.available,
        )?;
        for row in group.items.iter().filter(|row| !row.mac_only || mac) {
            if matches!(row.role, Some(NativeRole::Separator)) {
                submenu.append(&PredefinedMenuItem::separator(app)?)?;
                continue;
            }
            let label = row.label(state, &app_name, mac);
            if let Some(role) = row.role {
                if let Some(native) = native_item(app, role, &label)? {
                    if row.enabled(state) {
                        submenu.append(&native)?;
                    } else if let Some(id) = row.id.as_deref() {
                        // Tauri cannot disable an individual predefined item. Keep its
                        // label while a focused window cannot take the action.
                        let accelerator = if mac {
                            row.mac_accelerator
                                .as_deref()
                                .or(row.accelerator.as_deref())
                        } else {
                            row.accelerator.as_deref()
                        };
                        submenu.append(&MenuItem::with_id(app, id, label, false, accelerator)?)?;
                    }
                    continue;
                }
            }
            let id = row.id.as_deref().expect("custom menu id");
            let item = MenuItem::with_id(
                app,
                id,
                label,
                row.enabled(state),
                row.accelerator.as_deref(),
            )?;
            submenu.append(&item)?;
        }
        menu.append(&submenu)?;
        #[cfg(target_os = "macos")]
        if group.id == "window" {
            submenu.set_as_windows_menu_for_nsapp()?;
        }
    }
    app.set_menu(menu)?;
    Ok(())
}

fn window_state(window: &WebviewWindow) -> MainWindow {
    #[cfg(target_os = "macos")]
    let (available, native_modal) = (
        // The app menu is global. A visible document can have no key window
        // for part of a panel or fullscreen transition.
        window.is_visible().unwrap_or(false) && !window.is_minimized().unwrap_or(false),
        macos::native_modal(window),
    );
    #[cfg(not(target_os = "macos"))]
    let (available, native_modal) = (window.is_focused().unwrap_or(false), false);
    MainWindow {
        available,
        native_modal,
        fullscreen: window.is_fullscreen().unwrap_or(false),
        maximized: cfg!(target_os = "linux") && window.is_maximized().unwrap_or(false),
    }
}

fn main_window_state(app: &AppHandle) -> MainWindow {
    app.get_webview_window("main")
        .as_ref()
        .map(window_state)
        .unwrap_or_default()
}

impl InstalledMenu {
    fn update(
        &self,
        update: impl FnOnce(&mut MenuState),
        render: impl FnOnce(MenuState) -> tauri::Result<()>,
    ) -> tauri::Result<()> {
        let (previous, next) = {
            let mut current = self.0.lock().expect("native menu state");
            let previous = *current;
            update(&mut current);
            (previous, *current)
        };
        if previous != next {
            if let Err(error) = render(next) {
                let mut current = self.0.lock().expect("native menu state");
                if *current == next {
                    *current = previous;
                }
                return Err(error);
            }
        }
        Ok(())
    }
}

pub(crate) fn install(app: &AppHandle) -> tauri::Result<()> {
    let main = app.get_webview_window("main");
    let root = &app.state::<AppState>().artifacts.app_data_dir;
    let mut zoom = read_zoom(root);
    if let Some(window) = main.as_ref().filter(|_| zoom != ACTUAL_SIZE) {
        if let Err(error) = window.set_zoom(f64::from(zoom) / 100.0) {
            eprintln!("Loupist could not restore webview zoom: {error}");
            zoom = ACTUAL_SIZE;
        }
    }
    let state = MenuState {
        context: AppMenuContext::default(),
        window: main.as_ref().map(window_state).unwrap_or_default(),
        zoom,
    };
    build_menu(app, state)?;
    app.manage(InstalledMenu(Mutex::new(state)));
    #[cfg(target_os = "macos")]
    macos::observe_window_changes(app);
    app.on_menu_event(|app, event| {
        if let Err(error) = dispatch(app, event.id().as_ref()) {
            eprintln!("Loupist menu action failed: {error}");
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn app_menu_update(
    request: AppMenuContext,
    app: AppHandle,
    webview: tauri::Webview,
) -> Result<(), CommandError> {
    if webview.label() != "main" {
        return Err(CommandError::invalid(
            "Menu context belongs to the main webview",
        ));
    }
    let app_handle = app.clone();
    app.run_on_main_thread(move || {
        let Some(menu) = app_handle.try_state::<InstalledMenu>() else {
            return;
        };
        let window = main_window_state(&app_handle);
        if let Err(error) = menu.update(
            |state| {
                state.context = request;
                state.window = window;
            },
            |state| build_menu(&app_handle, state),
        ) {
            eprintln!("Loupist menu could not be updated: {error}");
        }
    })
    .map_err(|error| CommandError::internal(error.to_string()))
}

pub(crate) fn renderer_reset(webview: &tauri::Webview) {
    if webview.label() != "main" {
        return;
    }
    let app = webview.app_handle().clone();
    let handle = app.clone();
    if let Err(error) = app.run_on_main_thread(move || {
        update_window(&handle, false, MenuState::reset_renderer_context);
    }) {
        eprintln!("Loupist menu could not reset renderer state: {error}");
    }
}

pub(crate) fn window_event(window: &tauri::Window, event: &WindowEvent) {
    let changing_window = matches!(
        event,
        WindowEvent::Focused(_) | WindowEvent::Resized(_) | WindowEvent::Destroyed
    );
    if window.label() != "main" || !changing_window {
        return;
    }
    update_window(
        window.app_handle(),
        matches!(event, WindowEvent::Destroyed),
        |_| {},
    );
}

fn update_window(app: &AppHandle, destroyed: bool, update: impl FnOnce(&mut MenuState)) {
    let Some(menu) = app.try_state::<InstalledMenu>() else {
        return;
    };
    let window = if destroyed {
        MainWindow::default()
    } else {
        main_window_state(app)
    };
    if let Err(error) = menu.update(
        |next| {
            next.window = window;
            update(next);
        },
        |next| build_menu(app, next),
    ) {
        eprintln!("Loupist menu could not update window actions: {error}");
    }
}

fn action(id: &str, state: MenuState) -> Option<&'static str> {
    let row = MENU
        .iter()
        .flat_map(|group| &group.items)
        .find(|row| row.id.as_deref() == Some(id))?;
    row.id.as_deref().filter(|_| row.enabled(state))
}

fn dispatch(app: &AppHandle, id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let menu = app.state::<InstalledMenu>();
    let mut current = *menu.0.lock().expect("native menu state");
    current.window = main_window_state(app);
    let Some(id) = action(id, current) else {
        return Ok(());
    };
    if id == "quit" {
        app.exit(0);
        return Ok(());
    }
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    match id {
        "close-window" => window.close()?,
        "fullscreen" => window.set_fullscreen(!window.is_fullscreen()?)?,
        "minimize" => window.minimize()?,
        "maximize" => {
            if window.is_maximized()? {
                window.unmaximize()?
            } else {
                window.maximize()?
            }
        }
        "zoom-in" | "zoom-out" | "actual-size" => {
            let target = match id {
                "zoom-in" => zoom_step(current.zoom, 1),
                "zoom-out" => zoom_step(current.zoom, -1),
                _ => Some(ACTUAL_SIZE),
            };
            if let Some(zoom) = target {
                window.set_zoom(f64::from(zoom) / 100.0)?;
                menu.update(|state| state.zoom = zoom, |state| build_menu(app, state))?;
                write_zoom(&app.state::<AppState>().artifacts.app_data_dir, zoom)?;
            }
        }
        _ => app.emit_to("main", crate::events::MENU_ACTION, id)?,
    }
    Ok(())
}

pub(crate) fn zoom_factor(app: &AppHandle) -> f64 {
    app.try_state::<InstalledMenu>()
        .map(|menu| f64::from(menu.0.lock().expect("native menu state").zoom) / 100.0)
        .unwrap_or(1.0)
}

fn zoom_step(zoom: u16, delta: isize) -> Option<u16> {
    let index = ZOOM_STEPS.iter().position(|&step| step == zoom)?;
    ZOOM_STEPS.get(index.checked_add_signed(delta)?).copied()
}

fn read_zoom(root: &Path) -> u16 {
    let Ok(file) = File::open(root.join(ZOOM_FILE)) else {
        return ACTUAL_SIZE;
    };
    let mut bytes = Vec::new();
    file.take(32)
        .read_to_end(&mut bytes)
        .ok()
        .and_then(|_| serde_json::from_slice::<u16>(&bytes).ok())
        .filter(|zoom| ZOOM_STEPS.contains(zoom))
        .unwrap_or(ACTUAL_SIZE)
}

fn write_zoom(root: &Path, zoom: u16) -> std::io::Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    writeln!(file, "{zoom}")?;
    file.as_file().sync_all()?;
    file.persist(root.join(ZOOM_FILE))?;
    Ok(())
}

#[cfg(test)]
mod tests;
