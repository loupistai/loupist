//! A `loupist://` link held for the renderer, and its delivery.
use crate::sharing::deep_link::{self, DeepLinkEvent};

#[tauri::command(rename_all = "snake_case")]
pub fn deep_link_pending() -> Option<DeepLinkEvent> {
    deep_link::held().map(|location| DeepLinkEvent::Ok {
        source: location.label(),
    })
}

#[tauri::command(rename_all = "snake_case")]
pub fn deep_link_dismiss() {
    deep_link::dismiss();
}

/// Validates each arriving `loupist://` URL in Rust and tells the renderer.
#[cfg(all(any(target_os = "macos", windows), not(feature = "e2e")))]
pub(crate) fn deliver_links(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    use tauri::Emitter;
    for url in urls {
        let event = deep_link::receive(&url);
        let _ = app.emit(crate::events::DEEP_LINK_IMPORT, event);
    }
}
