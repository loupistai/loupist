use super::*;

impl BrowserManager {
    fn record_history(&self, tab_id: &str, history: (bool, bool)) {
        let Ok(mut registry) = self.registry.lock() else {
            return;
        };
        let Some(record) = registry.tabs.get_mut(tab_id) else {
            return;
        };
        if record.history == Some(history) {
            return;
        }
        record.history = Some(history);
        if let Some(events) = &record.history_events {
            let _ = events.send(BrowserTabEvent::HistoryChanged {
                tab_id: tab_id.to_owned(),
                can_go_back: history.0,
                can_go_forward: history.1,
            });
        }
    }

    pub(super) async fn attach_history(
        &self,
        tab_id: &str,
        view: &Webview,
        channel: Channel<BrowserTabEvent>,
    ) -> Result<BrowserTabState, CommandError> {
        let (events, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let (done, initial) = tokio::sync::oneshot::channel();
        let manager = self.clone();
        let id = tab_id.to_owned();
        view.with_webview(move |native| {
            #[cfg(target_os = "linux")]
            watch_history(&native, &manager, &id);
            let history = read_history(&native);
            let snapshot = manager.registry.lock().ok().and_then(|mut registry| {
                let record = registry.tabs.get_mut(&id)?;
                record.history = history;
                record.history_events = Some(events);
                Some(record.state(&id))
            });
            let _ = done.send(snapshot);
        })
        .map_err(|error| CommandError::internal(error.to_string()))?;
        // Keep native changes in order and send IPC outside Wry's window callbacks.
        tauri::async_runtime::spawn(async move {
            while let Some(event) = receiver.recv().await {
                if channel.send(event).is_err() {
                    break;
                }
            }
        });
        initial
            .await
            .ok()
            .flatten()
            .ok_or_else(|| CommandError::not_found("Browser tab"))
    }

    pub(super) fn refresh_history(&self, tab_id: &str, view: &Webview) {
        let manager = self.clone();
        let id = tab_id.to_owned();
        let _ = view.with_webview(move |native| {
            if let Some(history) = read_history(&native) {
                manager.record_history(&id, history);
            }
        });
    }

    pub(super) fn defer_history(&self, tab_id: &str, view: Webview) {
        let manager = self.clone();
        let id = tab_id.to_owned();
        tauri::async_runtime::spawn(async move { manager.refresh_history(&id, &view) });
    }
}

#[cfg(target_os = "macos")]
fn read_history(view: &tauri::webview::PlatformWebview) -> Option<(bool, bool)> {
    // Tauri supplies this live WKWebView on the main thread for the duration of with_webview.
    unsafe {
        let wk = view.inner().cast::<objc2_web_kit::WKWebView>().as_ref()?;
        Some((wk.canGoBack(), wk.canGoForward()))
    }
}

#[cfg(target_os = "linux")]
fn read_history(view: &tauri::webview::PlatformWebview) -> Option<(bool, bool)> {
    use webkit2gtk::WebViewExt;
    let wk = view.inner();
    Some((wk.can_go_back(), wk.can_go_forward()))
}

#[cfg(target_os = "linux")]
fn watch_history(native: &tauri::webview::PlatformWebview, manager: &BrowserManager, id: &str) {
    use webkit2gtk::{glib::prelude::ObjectExt, WebViewExt};
    let view = native.inner();
    if let Some(list) = view.back_forward_list() {
        let weak = view.downgrade();
        let manager = manager.clone();
        let id = id.to_owned();
        // The bindings omit a typed handler for the signal's list of removed items.
        list.connect_local("changed", false, move |_| {
            if let Some(view) = weak.upgrade() {
                manager.record_history(&id, (view.can_go_back(), view.can_go_forward()));
            }
            None
        });
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn read_history(_: &tauri::webview::PlatformWebview) -> Option<(bool, bool)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_changes_emit_once_in_order_and_match_the_snapshot() {
        let manager = BrowserManager::new(PathBuf::from("profile"));
        let (tab, _) = manager
            .reserve(&validated_http_url("https://example.com").unwrap())
            .unwrap();
        let id = tab.tab_id;
        let (events, mut received) = tokio::sync::mpsc::unbounded_channel();
        manager
            .registry
            .lock()
            .unwrap()
            .tabs
            .get_mut(&id)
            .unwrap()
            .history_events = Some(events);
        for (back, forward) in [(false, false), (true, false), (false, true), (true, false)] {
            manager.record_history(&id, (back, forward));
            manager.record_history(&id, (back, forward));
            let state = manager.registry.lock().unwrap().tabs[&id].state(&id);
            assert_eq!(
                (state.can_go_back, state.can_go_forward),
                (Some(back), Some(forward))
            );
            assert!(
                matches!(received.try_recv(), Ok(BrowserTabEvent::HistoryChanged { tab_id, can_go_back, can_go_forward }) if tab_id == id && (can_go_back, can_go_forward) == (back, forward))
            );
            assert!(received.try_recv().is_err());
        }
    }
}
