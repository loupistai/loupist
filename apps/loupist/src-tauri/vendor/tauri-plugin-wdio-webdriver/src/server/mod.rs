use std::net::SocketAddr;
use std::sync::Arc;

use tauri::{AppHandle, Manager, Runtime};
use tokio::runtime::Runtime as TokioRuntime;
use tokio::sync::RwLock;

pub mod handlers;
pub mod response;
pub mod router;

use crate::platform::{create_executor, FrameId, PlatformExecutor};
use crate::server::response::WebDriverErrorResponse;
use crate::webdriver::{SessionManager, Timeouts};

/// Shared state for the `WebDriver` server
pub struct AppState<R: Runtime> {
    pub app: AppHandle<R>,
    pub sessions: RwLock<SessionManager>,
}

impl<R: Runtime + 'static> AppState<R> {
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            sessions: RwLock::new(SessionManager::new()),
        }
    }

    /// Get a platform executor for a specific window by label
    pub fn get_executor_for_window(
        &self,
        window_label: &str,
        timeouts: Timeouts,
        frame_context: Vec<FrameId>,
    ) -> Result<Arc<dyn PlatformExecutor<R>>, WebDriverErrorResponse> {
        self.app
            .webviews()
            .get(window_label)
            .cloned()
            .map(|window| create_executor(window, timeouts, frame_context))
            .ok_or_else(WebDriverErrorResponse::no_such_window)
    }

    /// Whether a window with this label is still registered
    pub fn has_window_label(&self, window_label: &str) -> bool {
        self.app.webviews().contains_key(window_label)
    }

    /// Get all window labels
    pub fn get_window_labels(&self) -> Vec<String> {
        {
            let mut labels: Vec<String> = self.app.webviews().keys().cloned().collect();
            // Keep the app surface the default even when an external child exists.
            labels.sort_by_key(|label| (label != "main", label.clone()));
            labels
        }
    }
}

/// Start the `WebDriver` HTTP server on the specified port
pub fn start<R: Runtime + 'static>(app: AppHandle<R>, port: u16) {
    std::thread::spawn(move || {
        let rt = match TokioRuntime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("Failed to create Tokio runtime for WebDriver server: {}", e);
                return;
            }
        };

        rt.block_on(async {
            let state = Arc::new(AppState::new(app));
            let router = router::create_router(state);

            // On Android, bind to all interfaces for WiFi accessibility
            // On other platforms, bind to localhost only for security
            #[cfg(target_os = "android")]
            let addr = SocketAddr::from(([0, 0, 0, 0], port));
            #[cfg(not(target_os = "android"))]
            let addr = SocketAddr::from(([127, 0, 0, 1], port));

            let listener = match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!(
                        "Failed to bind WebDriver server to {} — port may already be in use: {}",
                        addr,
                        e
                    );
                    return;
                }
            };

            tracing::info!("WebDriver server listening on http://{}", addr);

            if let Err(e) = axum::serve(listener, router).await {
                tracing::error!("WebDriver server error: {}", e);
            }
        });
    });
}

#[cfg(test)]
mod multiview_tests {
    use super::*;
    use tauri::{
        test::{mock_builder, mock_context, noop_assets},
        LogicalPosition, LogicalSize, WebviewBuilder, WebviewUrl, WebviewWindowBuilder,
    };

    #[tokio::test]
    async fn child_context_keeps_main_executor_and_closes_independently() {
        let app = mock_builder().build(mock_context(noop_assets())).unwrap();
        let main = WebviewWindowBuilder::new(&app, "main", WebviewUrl::App("index.html".into()))
            .build()
            .unwrap();
        let child = main
            .as_ref()
            .window()
            .add_child(
                WebviewBuilder::new(
                    "browser-test",
                    WebviewUrl::External("https://example.com".parse().unwrap()),
                ),
                LogicalPosition::new(0, 0),
                LogicalSize::new(100, 100),
            )
            .unwrap();
        let state = Arc::new(AppState::new(app.handle().clone()));
        // Reproduces the upstream lookup failure without creating an OS window.
        assert!(!app.webview_windows().contains_key("main"));
        assert_eq!(state.get_window_labels(), vec!["main", "browser-test"]);
        assert_eq!(
            state
                .get_executor_for_window("main", Timeouts::default(), vec![])
                .unwrap()
                .window()
                .label(),
            "main"
        );
        assert_eq!(
            state
                .get_executor_for_window("browser-test", Timeouts::default(), vec![])
                .unwrap()
                .window()
                .url()
                .unwrap()
                .as_str(),
            "https://example.com/"
        );
        assert!(state
            .get_executor_for_window("missing", Timeouts::default(), vec![])
            .is_err());
        let session_id = state
            .sessions
            .write()
            .await
            .create("browser-test".into())
            .id
            .clone();
        handlers::window::close_window(
            axum::extract::State(state.clone()),
            axum::extract::Path(session_id),
        )
        .await
        .unwrap();
        assert!(state.has_window_label("main"));
        assert!(!state.has_window_label(child.label()));
    }
}
