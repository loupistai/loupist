use std::cell::OnceCell;

use objc2::{
    class, define_class, msg_send, rc::Retained, runtime::AnyObject, sel, DefinedClass,
    MainThreadMarker, MainThreadOnly,
};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSString, NSUserDefaults};
use tauri::{AppHandle, WebviewWindow};

/// AppKit adds items of its own to an app's menus: Enter Full Screen to the
/// View menu, and Start Dictation… and Emoji & Symbols to the Edit menu.
/// They are titled in the bundle's development language whatever the
/// interface language, and the menu table already has its own Full Screen.
/// Called before the event loop starts, so AppKit reads them at launch.
pub(crate) fn disable_automatic_menu_items() {
    let defaults = NSUserDefaults::standardUserDefaults();
    defaults.setBool_forKey(false, &NSString::from_str("NSFullScreenMenuItemEverywhere"));
    for key in [
        "NSDisabledDictationMenuItem",
        "NSDisabledCharacterPaletteMenuItem",
    ] {
        defaults.setBool_forKey(true, &NSString::from_str(key));
    }
}

/// AppKit appends AutoFill (Contact…, Passwords…) to the Edit menu each time
/// a main menu is set, with no setting to turn it off and in English under a
/// Korean interface. It comes after the items Loupist put there, so the Edit
/// submenu at `position` keeps its first `own` items. Runs on the main thread
/// after the menu is set; a submenu not titled `title` is left alone.
pub(super) fn keep_own_edit_items(position: usize, title: &str, own: usize) {
    let (Ok(position), Ok(own)) = (isize::try_from(position), isize::try_from(own)) else {
        return;
    };
    // The app menu is AppKit's, read and changed on the main thread.
    unsafe {
        let main: Option<Retained<AnyObject>> = msg_send![&*application(), mainMenu];
        let Some(main) = main else {
            return;
        };
        let top: isize = msg_send![&*main, numberOfItems];
        if position >= top {
            return;
        }
        let item: Option<Retained<AnyObject>> = msg_send![&*main, itemAtIndex: position];
        let submenu: Option<Retained<AnyObject>> = item.and_then(|item| msg_send![&*item, submenu]);
        let Some(submenu) = submenu else {
            return;
        };
        let shown: Retained<NSString> = msg_send![&*submenu, title];
        if shown.to_string() != title {
            return;
        }
        loop {
            let count: isize = msg_send![&*submenu, numberOfItems];
            if count <= own {
                break;
            }
            let _: () = msg_send![&*submenu, removeItemAtIndex: count - 1];
        }
    }
}

fn application() -> Retained<AnyObject> {
    // AppKit is initialized and this is called only on the event loop's main thread.
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}

pub(super) fn native_modal(window: &WebviewWindow) -> bool {
    let Ok(window) = window.ns_window() else {
        return false;
    };
    // Tauri owns this NSWindow. We read its attached sheet on the main thread.
    let window = unsafe { &*window.cast::<AnyObject>() };
    unsafe {
        let modal: Option<Retained<AnyObject>> = msg_send![&*application(), modalWindow];
        let sheet: Option<Retained<AnyObject>> = msg_send![window, attachedSheet];
        modal.is_some() || sheet.is_some()
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = AppHandle]
    struct MenuWindowObserver;

    impl MenuWindowObserver {
        #[unsafe(method(menuWindowChanged:))]
        fn window_changed(&self, _: &NSNotification) {
            // Read AppKit on the next event-loop turn, after it updates the sheet
            // or modal window that posted the notification.
            let app = self.ivars().clone();
            tauri::async_runtime::spawn(async move {
                let handle = app.clone();
                if let Err(error) = app.run_on_main_thread(move || {
                    super::update_window(&handle, false, |_| {});
                }) {
                    eprintln!("Loupist menu could not refresh the native window: {error}");
                }
            });
        }
    }
);

thread_local! {
    // NotificationCenter keeps the selector observer weakly. It lives with NSApplication.
    static OBSERVER: OnceCell<Retained<MenuWindowObserver>> = const { OnceCell::new() };
}

pub(super) fn observe_window_changes(app: &AppHandle) {
    let mtm = MainThreadMarker::new().expect("native menu setup on main thread");
    OBSERVER.with(|slot| {
        slot.get_or_init(|| {
            let observer = mtm.alloc::<MenuWindowObserver>().set_ivars(app.clone());
            let observer: Retained<MenuWindowObserver> =
                unsafe { msg_send![super(observer), init] };
            let center = NSNotificationCenter::defaultCenter();
            for name in [
                "NSApplicationDidBecomeActiveNotification",
                "NSWindowDidBecomeKeyNotification",
                "NSWindowDidResignKeyNotification",
                "NSWindowWillBeginSheetNotification",
                "NSWindowDidEndSheetNotification",
                "NSWindowDidMiniaturizeNotification",
                "NSWindowDidDeminiaturizeNotification",
                "NSWindowDidEnterFullScreenNotification",
                "NSWindowDidExitFullScreenNotification",
            ] {
                // AppKit delivers these notifications to this main-thread observer.
                unsafe {
                    center.addObserver_selector_name_object(
                        &observer,
                        sel!(menuWindowChanged:),
                        Some(&NSString::from_str(name)),
                        None,
                    );
                }
            }
            observer
        });
    });
}

#[cfg(feature = "e2e")]
#[tauri::command]
pub(crate) async fn e2e_native_menu_snapshot(
    app: AppHandle,
    webview: tauri::Webview,
) -> Result<serde_json::Value, super::CommandError> {
    use super::CommandError;

    if webview.label() != "main" {
        return Err(CommandError::invalid(
            "Native menu snapshot belongs to the main webview",
        ));
    }
    let (send, receive) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let _ = send.send(menu_snapshot());
    })
    .map_err(|error| CommandError::internal(error.to_string()))?;
    receive
        .await
        .map_err(|error| CommandError::internal(error.to_string()))
}

#[cfg(feature = "e2e")]
fn menu_snapshot() -> serde_json::Value {
    use objc2::runtime::{Bool, Sel};
    use objc2_foundation::NSArray;

    // The actual AppKit tree also includes any items AppKit inserted itself.
    fn items(menu: &AnyObject) -> Vec<serde_json::Value> {
        unsafe {
            let entries: Retained<NSArray<AnyObject>> = msg_send![menu, itemArray];
            entries
                .to_vec()
                .into_iter()
                .filter_map(|item| {
                    let separator: Bool = msg_send![&*item, isSeparatorItem];
                    if separator.as_bool() {
                        return None;
                    }
                    let title: Retained<NSString> = msg_send![&*item, title];
                    let enabled: Bool = msg_send![&*item, isEnabled];
                    let action: Option<Sel> = msg_send![&*item, action];
                    let submenu: Option<Retained<AnyObject>> = msg_send![&*item, submenu];
                    Some(serde_json::json!({
                        "title": title.to_string(),
                        "enabled": enabled.as_bool(),
                        "action": action.map(|selector| selector.name().to_string_lossy()),
                        "items": submenu.as_deref().map(items),
                    }))
                })
                .collect()
        }
    }

    let menu: Option<Retained<AnyObject>> = unsafe { msg_send![&*application(), mainMenu] };
    serde_json::json!(menu.as_deref().map(items).unwrap_or_default())
}
