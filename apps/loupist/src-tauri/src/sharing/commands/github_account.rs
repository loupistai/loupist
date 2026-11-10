//! GitHub account: device sign-in, sign-out, and what a GitHub API refusal means.
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{ipc::Channel, State};

use super::internal;
use crate::app::{AppState, CommandError};
use crate::sharing::github::{self, ApiError, Github, Poll};

pub(super) fn api_error(error: ApiError, environment: &str, state: &AppState) -> CommandError {
    match error {
        ApiError::Offline => CommandError::new("offline", "Couldn't reach GitHub."),
        ApiError::LicenseNotShareable => CommandError::new(
            "license_not_shareable",
            "This license does not allow sharing.",
        ),
        ApiError::NameTaken => CommandError::new("name_taken", "That repository name is taken."),
        ApiError::RateLimited => CommandError::new("rate_limited", "GitHub asked to slow down."),
        ApiError::Denied => {
            github::delete_token(environment);
            if let Ok(connection) = state.store.lock() {
                let _ = github::store_login(&connection, None);
            }
            CommandError::new("publish_denied", "GitHub didn't allow this. Sign in again.")
        }
        ApiError::NotFound => CommandError::new("repo_gone", "The repository is gone."),
        ApiError::TooLarge => CommandError::new("too_large", "The response was too large."),
        ApiError::Other(message) => CommandError::new("github_failed", message),
    }
}

#[derive(Debug, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct GithubAccount {
    client_configured: bool,
    index_configured: bool,
    gallery_terms_url: Option<String>,
    login: Option<String>,
}

fn account(state: &AppState) -> GithubAccount {
    let environment = state.credential_environment.to_string();
    let login = github::load_token(&environment).and_then(|_| {
        state
            .store
            .lock()
            .ok()
            .and_then(|connection| github::stored_login(&connection).ok().flatten())
    });
    GithubAccount {
        client_configured: github::client_id().is_some(),
        index_configured: github::index_url().is_some(),
        gallery_terms_url: github::gallery_terms_url().map(str::to_owned),
        login,
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_account(state: State<'_, AppState>) -> GithubAccount {
    account(state.inner())
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignInEvent {
    #[serde(rename_all = "camelCase")]
    Code {
        user_code: String,
        expires_in_sec: u64,
    },
    Waiting,
    SignedIn {
        login: String,
    },
    Failed {
        reason: &'static str,
    },
}

/// The GitHub device sign-in this app session runs. A newer sign-in or
/// Cancel advances the generation, which ends the earlier poll.
#[derive(Clone, Default)]
pub(crate) struct SignInSession {
    generation: Arc<AtomicU64>,
    /// The verification page the person opens to enter the code.
    verification: Arc<Mutex<Option<String>>>,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn github_sign_in(
    on_event: Channel<SignInEvent>,
    state: State<'_, AppState>,
) -> Result<GithubAccount, CommandError> {
    let state = state.inner();
    let fail = |reason| {
        let _ = on_event.send(SignInEvent::Failed { reason });
    };
    let Some(client_id) = github::client_id() else {
        fail("not_configured");
        return Ok(account(state));
    };
    let session = &state.sharing.sign_in;
    let generation = session.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let api = Github::production();
    let code = match api.device_code(client_id).await {
        Ok(code) => code,
        Err(_) => {
            fail("offline");
            return Ok(account(state));
        }
    };
    let uri = reqwest::Url::parse(&code.verification_uri)
        .ok()
        .filter(|url| url.scheme() == "https" && url.host_str() == Some("github.com"))
        .map(|url| url.to_string())
        .unwrap_or_else(|| "https://github.com/login/device".into());
    if let Ok(mut held) = session.verification.lock() {
        *held = Some(uri);
    }
    let _ = on_event.send(SignInEvent::Code {
        user_code: code.user_code.clone(),
        expires_in_sec: code.expires_in,
    });
    let _ = on_event.send(SignInEvent::Waiting);
    let deadline = Instant::now() + Duration::from_secs(code.expires_in.min(3_600));
    let mut interval = code.interval.max(1);
    let mut network_errors = 0;
    loop {
        tokio::time::sleep(Duration::from_secs(interval)).await;
        if session.generation.load(Ordering::SeqCst) != generation {
            return Ok(account(state));
        }
        if Instant::now() >= deadline {
            fail("expired");
            return Ok(account(state));
        }
        match api.poll_token(client_id, &code.device_code).await {
            Ok(Poll::Token(token)) => {
                let login = match api.login(&token).await {
                    Ok(login) => login,
                    Err(_) => {
                        fail("offline");
                        return Ok(account(state));
                    }
                };
                let environment = state.credential_environment.to_string();
                if !github::save_token(&environment, &token) {
                    fail("offline");
                    return Ok(account(state));
                }
                if let Ok(connection) = state.store.lock() {
                    github::store_login(&connection, Some(&login)).map_err(internal)?;
                }
                let _ = on_event.send(SignInEvent::SignedIn { login });
                return Ok(account(state));
            }
            Ok(Poll::Pending) => network_errors = 0,
            Ok(Poll::SlowDown(next)) => interval = next.unwrap_or(interval + 5).max(interval),
            Ok(Poll::Expired) => {
                fail("expired");
                return Ok(account(state));
            }
            Ok(Poll::Denied | Poll::Failed) => {
                fail("denied");
                return Ok(account(state));
            }
            Err(_) => {
                network_errors += 1;
                if network_errors >= 3 {
                    fail("offline");
                    return Ok(account(state));
                }
            }
        }
    }
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_sign_in_open(state: State<'_, AppState>) -> Result<(), CommandError> {
    let uri = state
        .sharing
        .sign_in
        .verification
        .lock()
        .ok()
        .and_then(|held| held.clone())
        .ok_or_else(|| CommandError::not_found("sign-in code"))?;
    crate::web::external_browser::open(&uri).map_err(internal)
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_sign_in_cancel(state: State<'_, AppState>) {
    state
        .sharing
        .sign_in
        .generation
        .fetch_add(1, Ordering::SeqCst);
}

#[tauri::command(rename_all = "snake_case")]
pub fn github_sign_out(state: State<'_, AppState>) -> Result<GithubAccount, CommandError> {
    let state = state.inner();
    github::delete_token(&state.credential_environment);
    if let Ok(connection) = state.store.lock() {
        github::store_login(&connection, None).map_err(internal)?;
    }
    Ok(account(state))
}
