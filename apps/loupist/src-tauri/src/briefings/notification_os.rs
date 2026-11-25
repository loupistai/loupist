//! Return false only when the request was definitely rejected or never submitted.
//! An uncertain acknowledgement must not queue a second OS request.

#[cfg(target_os = "macos")]
pub(super) use macos::request_permission;

#[cfg(target_os = "macos")]
pub(super) fn submit(_app: &tauri::AppHandle, title: &str, body: &str) -> bool {
    objc2::rc::autoreleasepool(|_| macos::submit(title, body))
}

#[cfg(target_os = "macos")]
mod macos {
    use std::{sync::mpsc, time::Duration};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };

    fn center() -> Option<Retained<UNUserNotificationCenter>> {
        // The native fixture runs without system dialogs or notifications.
        if cfg!(feature = "e2e") {
            return None;
        }
        // UserNotifications requires a real app bundle. The legacy
        // notification backend can override the reported bundle identifier.
        if !NSBundle::mainBundle()
            .bundlePath()
            .to_string()
            .ends_with(".app")
        {
            return None;
        }
        Some(UNUserNotificationCenter::currentNotificationCenter())
    }

    fn request(center: &UNUserNotificationCenter) {
        let reply = RcBlock::new(|_: objc2::runtime::Bool, _: *mut NSError| {});
        center.requestAuthorizationWithOptions_completionHandler(
            UNAuthorizationOptions::Alert,
            &reply,
        );
    }

    pub(in crate::briefings) fn request_permission() {
        objc2::rc::autoreleasepool(|_| {
            if let Some(center) = center() {
                request(&center);
            }
        });
    }

    pub(super) fn submit(title: &str, body: &str) -> bool {
        let Some(center) = center() else { return false };
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(title));
        content.setBody(&NSString::from_str(body));
        let id = NSString::from_str(&uuid::Uuid::new_v4().to_string());
        let notification =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let (send, receive) = mpsc::channel();
        let reply = RcBlock::new(move |error: *mut NSError| {
            let _ = send.send(error.is_null());
        });
        center.addNotificationRequest_withCompletionHandler(&notification, Some(&reply));
        match receive.recv_timeout(Duration::from_secs(3)) {
            Ok(true) => true,
            Ok(false) => {
                // Older builds could mark permission as requested without
                // asking the OS. macOS only prompts if it has no decision.
                request(&center);
                false
            }
            Err(_) => true,
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn request_permission() {}

#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn submit(app: &tauri::AppHandle, title: &str, body: &str) -> bool {
    use std::{collections::HashMap, sync::Mutex, time::Duration};
    use zbus::{blocking::Connection, zvariant::Value};

    // Keep the sender connected. Some notification servers close messages
    // when their sender disconnects.
    static CONNECTION: Mutex<Option<Connection>> = Mutex::new(None);
    let Ok(mut connection) = CONNECTION.lock() else {
        return false;
    };
    if connection.is_none() {
        *connection = zbus::blocking::connection::Builder::session()
            .and_then(|builder| builder.method_timeout(Duration::from_secs(3)).build())
            .ok();
    }
    let Some(bus) = connection.as_ref() else {
        return false;
    };
    let name = app.config().product_name.as_deref().unwrap_or("Loupist");
    let service = "org.freedesktop.Notifications";
    let result = bus.call_method(
        Some(service),
        "/org/freedesktop/Notifications",
        Some(service),
        "Notify",
        &(
            name,
            0u32,
            "",
            title,
            body,
            Vec::<String>::new(),
            HashMap::<&str, Value<'_>>::new(),
            -1i32,
        ),
    );
    match result {
        Ok(_) => true,
        Err(error) => {
            *connection = None;
            !matches!(error, zbus::Error::MethodError(..))
        }
    }
}

#[cfg(windows)]
pub(super) fn submit(app: &tauri::AppHandle, title: &str, body: &str) -> bool {
    use tauri_winrt_notification::Toast;
    use windows::{
        core::HSTRING,
        UI::Notifications::{NotificationSetting, ToastNotificationManager},
    };

    let app_id = if tauri::is_dev() {
        Toast::POWERSHELL_APP_ID
    } else {
        app.config().identifier.as_str()
    };
    let permission = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id))
        .and_then(|notifier| notifier.Setting());
    if !matches!(permission, Ok(NotificationSetting::Enabled)) {
        return false;
    }
    Toast::new(app_id).title(title).text1(body).show().is_ok()
}
