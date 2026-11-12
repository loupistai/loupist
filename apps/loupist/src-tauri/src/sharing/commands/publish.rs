//! Publishing a shared Loupist to a GitHub repository or gist.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{ipc::Channel, State};

use super::github_account::api_error;
use super::share::{build_pack, record_identity, require_shareable, ScopeRequest, ShareRequest};
use crate::app::{AppState, CommandError};
use crate::sharing::export::Sections;
use crate::sharing::github::{self, Github};
use crate::sharing::pack::License;

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub enum PublishTarget {
    Repo { name: String },
    Update(bool),
    Gist(String),
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharePublishRequest {
    loupist_id: String,
    scope: ScopeRequest,
    sections: Sections,
    excluded_item_keys: Vec<String>,
    acknowledged_finding_ids: Vec<String>,
    #[serde(default)]
    license: Option<License>,
    #[serde(default)]
    byline: Option<String>,
    target: PublishTarget,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PublishEvent {
    Creating,
    Uploading { files: u32 },
}

#[tauri::command(rename_all = "snake_case")]
pub async fn share_publish(
    request: SharePublishRequest,
    on_event: Channel<PublishEvent>,
    state: State<'_, AppState>,
) -> Result<Value, CommandError> {
    let state = state.inner();
    if let Some(license) = request.license {
        require_shareable(license)?;
    }
    if github::client_id().is_none() {
        return Err(CommandError::new(
            "not_configured",
            "GitHub publishing isn't available in this version.",
        ));
    }
    let environment = state.credential_environment.to_string();
    let token = github::load_token(&environment)
        .ok_or_else(|| CommandError::new("signed_out", "Sign in to GitHub first."))?;
    let share = ShareRequest {
        loupist_id: request.loupist_id.clone(),
        scope: request.scope.clone(),
        sections: request.sections,
        excluded_item_keys: request.excluded_item_keys.clone(),
        acknowledged_finding_ids: request.acknowledged_finding_ids.clone(),
        license: request.license,
        byline: request.byline.clone(),
    };
    let api = Github::production();
    match request.target {
        PublishTarget::Gist(visibility) => {
            if !matches!(share.scope, ScopeRequest::Item { .. }) {
                return Err(CommandError::invalid(
                    "A gist shares one method or principle.",
                ));
            }
            let built = build_pack(state, &share, None)?;
            let _ = on_event.send(PublishEvent::Uploading {
                files: built.files.len() as u32,
            });
            let url = api
                .create_gist(&token, &built.files, &built.name, visibility == "public")
                .await
                .map_err(|error| api_error(error, &environment, state))?;
            Ok(
                serde_json::json!({"url": url, "fullName": null, "version": built.version, "commit": null}),
            )
        }
        target => {
            if !matches!(share.scope, ScopeRequest::Loupist) {
                return Err(CommandError::invalid("Publish a single method as a gist."));
            }
            // Validate everything before anything is created on GitHub.
            let checked = build_pack(state, &share, None)?;
            let created = matches!(target, PublishTarget::Repo { .. });
            let repo = match target {
                PublishTarget::Repo { name } => {
                    if !github::valid_repo_name(&name) {
                        return Err(CommandError::new(
                            "name_taken",
                            "Choose another repository name.",
                        ));
                    }
                    let _ = on_event.send(PublishEvent::Creating);
                    // The Loupist's name leaves only with its Profile.
                    let description = if share.sections.profile {
                        format!("{} — a shared Loupist", checked.name)
                    } else {
                        "A shared Loupist".to_owned()
                    };
                    let repo = api
                        .create_repo(&token, &name, &description)
                        .await
                        .map_err(|error| api_error(error, &environment, state))?;
                    let _ = api.set_topic(&token, &repo.full_name).await;
                    repo
                }
                PublishTarget::Update(false) => {
                    return Err(CommandError::invalid("Choose a repository to publish to."))
                }
                _ => {
                    let recorded = checked.identity.repo.clone().ok_or_else(|| {
                        CommandError::new("repo_gone", "No repository is recorded.")
                    })?;
                    match api
                        .recorded_repo(&token, &recorded, &checked.identity.pack_id)
                        .await
                    {
                        Ok(repo) => repo,
                        Err(github::RecordedError::Api(error)) => {
                            return Err(api_error(error, &environment, state))
                        }
                        Err(github::RecordedError::Mismatch) => {
                            return Err(CommandError::new(
                                "repo_mismatch",
                                "That repository no longer holds this shared Loupist. Publish it as a new repository.",
                            ))
                        }
                    }
                }
            };
            let result: Result<Value, CommandError> = async {
                let mut built = build_pack(state, &share, Some(repo.html_url.clone()))?;
                let _ = on_event.send(PublishEvent::Uploading {
                    files: built.files.len() as u32,
                });
                let commit = api
                    .commit_files(
                        &token,
                        &repo.full_name,
                        &repo.default_branch,
                        &built.files,
                        &format!("Loupist pack v{}", built.version),
                    )
                    .await
                    .map_err(|error| api_error(error, &environment, state))?;
                built.identity.repo = Some(repo.full_name.clone());
                built.identity.repo_url = Some(repo.html_url.clone());
                built.identity.repo_commit = Some(commit.clone());
                record_identity(state, &share.loupist_id, &built)?;
                Ok(serde_json::json!({
                    "url": repo.html_url,
                    "fullName": repo.full_name,
                    "version": built.version,
                    "commit": commit,
                }))
            }
            .await;
            match result {
                Ok(value) => Ok(value),
                Err(mut error) => {
                    let removed = api
                        .remove_empty_created_repo(&token, &repo.full_name, created)
                        .await;
                    error.message.push_str(if removed {
                        " The empty repository created for this attempt was removed."
                    } else {
                        " The repository was left on GitHub; check it before trying again."
                    });
                    Err(error)
                }
            }
        }
    }
}
