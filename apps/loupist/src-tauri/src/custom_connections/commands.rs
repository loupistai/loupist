use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};

use serde::Deserialize;
use tauri::State;

use super::{
    config::{self, ParsedConnection, TransportConfig},
    credentials::Secrets,
    policy::ApprovalChoice,
    service::{for_state, ConnectionView},
    ConnectionError,
};
use crate::app::{AppState, CommandError};

fn error(error: ConnectionError) -> CommandError {
    let code = match error {
        ConnectionError::ApprovalRequired => "needs_approval",
        ConnectionError::ClientIdRequired => "client_id_required",
        ConnectionError::Authorization => "authorization_required",
        ConnectionError::CredentialStore => "credential_store_unavailable",
        _ => "custom_connection_failed",
    };
    CommandError::new(code, error.to_string())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddUrlRequest {
    pub url: String,
    pub name: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddJsonRequest {
    pub json: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddCommandRequest {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub name: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentityRequest {
    pub connection_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfirmRequest {
    pub connection_id: String,
    /// Loupist that opened the Add dialog. Confirm enables the connection for this Loupist.
    #[serde(default)]
    pub loupist_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListRequest {
    pub loupist_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRequest {
    pub connection_id: String,
    pub dont_save_to_memory: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnabledRequest {
    pub loupist_id: String,
    pub connection_id: String,
    pub enabled: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproveRequest {
    pub loupist_id: String,
    pub connection_id: String,
    pub tool_name: String,
    pub definition_hash: String,
    pub choice: ApprovalChoice,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorizeRequest {
    pub connection_id: String,
    pub issuer: String,
    pub client_id: String,
    pub client_secret: Option<String>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_add_url(
    request: AddUrlRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    let url = super::http::canonical_resource(&request.url).map_err(error)?;
    let name = request.name.unwrap_or_else(|| {
        super::http::checked_url(&url)
            .expect("validated URL")
            .host_str()
            .unwrap()
            .into()
    });
    let secrets = Secrets {
        headers: request.headers,
        ..Default::default()
    };
    let manager = for_state(&state).map_err(error)?;
    let mut service = manager.lock().await;
    let pending = service
        .add_pending(ParsedConnection {
            name,
            transport: TransportConfig::Http { url },
            secrets,
        })
        .map_err(error)?;
    service.probe(&pending.id).await.map_err(error)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_add_json(
    request: AddJsonRequest,
    state: State<'_, AppState>,
) -> Result<Vec<ConnectionView>, CommandError> {
    let parsed = config::parse_json(&request.json).map_err(error)?;
    let manager = for_state(&state).map_err(error)?;
    let mut service = manager.lock().await;
    let mut views: Vec<ConnectionView> = Vec::new();
    for entry in parsed {
        match service.add_pending(entry) {
            Ok(view) => views.push(view),
            Err(failure) => {
                for view in &views {
                    let _ = service.remove(&view.id).await;
                }
                return Err(error(failure));
            }
        }
    }
    Ok(views)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_add_command(
    request: AddCommandRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    config::validate_command(&request.command, &request.args).map_err(error)?;
    let name = request.name.unwrap_or_else(|| request.command.clone());
    let manager = for_state(&state).map_err(error)?;
    let mut service = manager.lock().await;
    service
        .add_pending(ParsedConnection {
            name,
            transport: TransportConfig::Stdio {
                command: request.command,
                args: request.args,
            },
            secrets: Secrets {
                env: request.env,
                ..Default::default()
            },
        })
        .map_err(error)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_confirm(
    request: ConfirmRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    let loupist = request
        .loupist_id
        .as_deref()
        .map(str::trim)
        .filter(|loupist| !loupist.is_empty());
    if let Some(loupist) = loupist {
        state.ensure_loupist_available(loupist)?;
    }
    let manager = for_state(&state).map_err(error)?;
    let result = manager
        .lock()
        .await
        .confirm_for_loupist(&request.connection_id, loupist)
        .await
        .map_err(error);
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_list(
    request: ListRequest,
    state: State<'_, AppState>,
) -> Result<Vec<ConnectionView>, CommandError> {
    if let Some(loupist) = &request.loupist_id {
        state.ensure_loupist_available(loupist)?;
    }
    // Ensure startup recovery, but do not wait for an interactive OAuth flow's
    // connection lock merely to render its in-progress state.
    for_state(&state).map_err(error)?;
    super::service::views_for_state(&state, request.loupist_id.as_deref()).map_err(error)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_remove(
    request: IdentityRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    let manager = for_state(&state).map_err(error)?;
    let result = manager
        .lock()
        .await
        .remove(&request.connection_id)
        .await
        .map_err(error);
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_set_memory(
    request: MemoryRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    let manager = for_state(&state).map_err(error)?;
    let result = manager
        .lock()
        .await
        .set_memory(&request.connection_id, request.dont_save_to_memory)
        .map_err(error);
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_set_enabled(
    request: EnabledRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    let manager = for_state(&state).map_err(error)?;
    let result = manager
        .lock()
        .await
        .set_enabled(&request.connection_id, &request.loupist_id, request.enabled)
        .map_err(error);
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_tool_approve(
    request: ApproveRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    state.ensure_loupist_available(&request.loupist_id)?;
    let manager = for_state(&state).map_err(error)?;
    let result = manager
        .lock()
        .await
        .approve(
            &request.loupist_id,
            &request.connection_id,
            &request.tool_name,
            &request.definition_hash,
            request.choice,
        )
        .await
        .map_err(error);
    result
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_authorize(
    request: AuthorizeRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    let manager = for_state(&state).map_err(error)?;
    let id = request.connection_id.clone();
    let mut log = crate::runs::log::RunLog::new(
        "connection_sign_in",
        "",
        &uuid::Uuid::new_v4().to_string(),
        crate::commands::now_ms(),
    );
    log.loupist_id = None;
    let result = run_sign_in(manager, &id, Some(request)).await;
    finish_sign_in_log(&state, log, &result);
    result
}

type SignIns = BTreeMap<String, tokio::sync::watch::Sender<bool>>;
fn sign_ins() -> &'static Mutex<SignIns> {
    static SIGN_INS: OnceLock<Mutex<SignIns>> = OnceLock::new();
    SIGN_INS.get_or_init(Default::default)
}

struct SignInLease(String);
impl Drop for SignInLease {
    fn drop(&mut self) {
        if let Ok(mut sign_ins) = sign_ins().lock() {
            sign_ins.remove(&self.0);
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn custom_connection_sign_in(
    request: IdentityRequest,
    state: State<'_, AppState>,
) -> Result<ConnectionView, CommandError> {
    let manager = for_state(&state).map_err(error)?;
    let mut log = crate::runs::log::RunLog::new(
        "connection_sign_in",
        "",
        &uuid::Uuid::new_v4().to_string(),
        crate::commands::now_ms(),
    );
    log.loupist_id = None;
    let result = run_sign_in(manager, &request.connection_id, None).await;
    finish_sign_in_log(&state, log, &result);
    result
}

fn finish_sign_in_log(
    state: &AppState,
    mut log: crate::runs::log::RunLog,
    result: &Result<ConnectionView, CommandError>,
) {
    match result {
        Ok(view)
            if view.auth_state == super::service::AuthState::SignInNeeded
                && view.last_error.is_none() =>
        {
            log.finish("cancelled", None)
        }
        Ok(view) if view.last_error.is_some() => {
            log.finish("failed", Some("custom_connection_failed"))
        }
        Ok(_) => log.finish("ok", None),
        Err(error) => log.finish("failed", Some(&error.code)),
    }
    log.record_app(&state.artifacts.app_data_dir);
}

async fn run_sign_in(
    manager: std::sync::Arc<tokio::sync::Mutex<super::service::Service>>,
    connection_id: &str,
    client: Option<AuthorizeRequest>,
) -> Result<ConnectionView, CommandError> {
    let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
    {
        let mut pending = sign_ins()
            .lock()
            .map_err(|_| error(ConnectionError::Configuration))?;
        if pending.contains_key(connection_id) {
            return Err(CommandError::conflict("Sign-in is already in progress"));
        }
        pending.insert(connection_id.to_owned(), cancel);
    }
    let _lease = SignInLease(connection_id.to_owned());
    let mut service = manager.lock().await;
    if client.is_none() {
        service.begin_sign_in(connection_id).map_err(error)?;
    }
    if *cancelled.borrow() {
        return service.cancel_sign_in(connection_id).map_err(error);
    }
    tokio::select! {
        biased;
        _ = cancelled.changed() => service.cancel_sign_in(connection_id).map_err(error),
        result = async {
            if let Some(client) = client {
                service.set_client(connection_id, &client.issuer, client.client_id, client.client_secret).await
            } else {
                service.sign_in(connection_id).await
            }
        } => result.map_err(error),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn custom_connection_sign_in_cancel(request: IdentityRequest) -> Result<(), CommandError> {
    let pending = sign_ins()
        .lock()
        .map_err(|_| error(ConnectionError::Configuration))?;
    if let Some(cancel) = pending.get(&request.connection_id) {
        let _ = cancel.send(true);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::{
        net::TcpListener,
        time::{timeout, Duration},
    };

    #[tokio::test]
    async fn ordinary_and_advanced_sign_in_share_cancellation_and_release_the_registry() {
        for advanced in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/mcp", listener.local_addr().unwrap());
            let directory = tempfile::tempdir().unwrap();
            let mut service = super::super::service::Service::open(
                directory.path().join("connections"),
                format!("cancel-test-{}", uuid::Uuid::new_v4()),
            )
            .unwrap();
            let view = service
                .add_pending(ParsedConnection {
                    name: "Cancelable fixture".into(),
                    transport: TransportConfig::Http { url: url.clone() },
                    secrets: Secrets::default(),
                })
                .unwrap();
            let manager = Arc::new(tokio::sync::Mutex::new(service));
            let id = view.id.clone();
            let client = advanced.then(|| AuthorizeRequest {
                connection_id: id.clone(),
                issuer: url,
                client_id: "fixture-client".into(),
                client_secret: None,
            });
            let running = tokio::spawn({
                let manager = manager.clone();
                let id = id.clone();
                async move { run_sign_in(manager, &id, client).await }
            });
            // Hold the first HTTP request open, before OAuth can open a browser.
            let (_socket, _) = timeout(Duration::from_secs(3), listener.accept())
                .await
                .unwrap()
                .unwrap();
            assert!(sign_ins().lock().unwrap().contains_key(&id));
            assert!(run_sign_in(manager.clone(), &id, None).await.is_err());
            custom_connection_sign_in_cancel(IdentityRequest {
                connection_id: id.clone(),
            })
            .unwrap();
            let cancelled = timeout(Duration::from_secs(3), running)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(
                cancelled.auth_state,
                super::super::service::AuthState::SignInNeeded
            );
            assert!(!sign_ins().lock().unwrap().contains_key(&id));
            assert!(
                manager.try_lock().is_ok(),
                "cancellation releases the connection lock"
            );
        }
    }
}
