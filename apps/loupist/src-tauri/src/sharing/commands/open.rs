//! Opening a shared pack from a GitHub link, a held deep link, a Gallery card,
//! a ZIP or a folder, through to its staged preview.
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use tauri::State;

use super::{gallery_cache, internal};
use crate::app::{AppState, CommandError};
use crate::sharing::deep_link::{self, GithubLocation};
use crate::sharing::gallery;
use crate::sharing::github::{self, ApiError, Github};
use crate::sharing::import::{self, Fetched, OpenResult};
use crate::sharing::pack::{self, Files, Rejection};

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum PackSource {
    Url {
        url: String,
    },
    Zip {
        #[allow(dead_code)]
        zip: bool,
    },
    Folder {
        #[allow(dead_code)]
        folder: bool,
    },
    Pending {
        #[allow(dead_code)]
        pending: bool,
    },
    /// A Gallery card: Rust resolves the listed repository, path and commit.
    Gallery {
        gallery: String,
    },
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(deny_unknown_fields)]
pub struct PackOpenRequest {
    source: PackSource,
}

fn link_error(error: ApiError) -> Result<Fetched, Result<OpenResult, CommandError>> {
    Err(match error {
        ApiError::NotFound | ApiError::Denied => Err(CommandError::new(
            "link_not_found",
            "This link isn't public, or it moved.",
        )),
        ApiError::TooLarge => Ok(OpenResult::Rejected {
            reason: Rejection::TooLarge,
            source_label: None,
        }),
        ApiError::RateLimited => Err(CommandError::new(
            "rate_limited",
            "GitHub asked to slow down.",
        )),
        _ => Err(CommandError::new("offline", "Couldn't reach GitHub.")),
    })
}

async fn fetch_github(
    state: &AppState,
    location: deep_link::GithubLocation,
) -> Result<Fetched, Result<OpenResult, CommandError>> {
    let api = Github::production();
    let token = github::load_token(&state.credential_environment);
    let label = Some(location.label());
    let source_url = Some(location.url());
    match &location {
        GithubLocation::Repo {
            owner,
            repo,
            reference,
            path,
        } => {
            let commit = match api
                .resolve_commit(token.as_deref(), owner, repo, reference.as_deref())
                .await
            {
                Err(ApiError::Denied) => {
                    api.resolve_commit(None, owner, repo, reference.as_deref())
                        .await
                }
                other => other,
            };
            let commit = match commit {
                Ok(commit) => commit,
                Err(error) => return link_error(error),
            };
            let bytes = match api.archive(owner, repo, &commit).await {
                Ok(bytes) => bytes,
                Err(error) => return link_error(error),
            };
            let files = import::read_zip(&bytes, path.as_deref()).map_err(|reason| {
                Ok(OpenResult::Rejected {
                    reason,
                    source_label: label.clone(),
                })
            })?;
            Ok(Fetched {
                files,
                source_url,
                label,
                repo: Some(format!("{owner}/{repo}")),
                path: path.clone(),
                commit: Some(commit),
            })
        }
        GithubLocation::Gist { user, id } => {
            let files = match api.gist(token.as_deref(), user, id).await {
                Err(ApiError::Denied) => api.gist(None, user, id).await,
                other => other,
            };
            match files {
                Ok(files) => Ok(Fetched {
                    files,
                    source_url,
                    label,
                    repo: None,
                    path: None,
                    commit: None,
                }),
                Err(error) => link_error(error),
            }
        }
    }
}

#[cfg(feature = "e2e")]
fn e2e_pack_path() -> Option<PathBuf> {
    std::env::var_os("LOUPIST_E2E_PACK_PATH").map(PathBuf::from)
}
#[cfg(not(feature = "e2e"))]
fn e2e_pack_path() -> Option<PathBuf> {
    None
}

pub(super) fn manifest_id(files: &Files) -> Option<String> {
    let value: Value = serde_json::from_slice(files.get(pack::MANIFEST)?).ok()?;
    value["id"].as_str().map(str::to_owned)
}

#[derive(Debug, Clone, Copy)]
pub(super) enum Local {
    Zip,
    Folder,
}

/// A ZIP or folder the person picks in the native dialog; None when cancelled.
pub(super) async fn fetch_local(
    kind: Local,
) -> Result<Option<Fetched>, Result<OpenResult, CommandError>> {
    let fetched = match kind {
        Local::Zip => {
            let path = match e2e_pack_path() {
                Some(path) => path,
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Choose a shared Loupist ZIP")
                    .add_filter("ZIP", &["zip"])
                    .pick_file()
                    .await
                {
                    Some(file) => file.path().to_owned(),
                    None => return Ok(None),
                },
            };
            let bytes = tauri::async_runtime::spawn_blocking(move || {
                crate::fs::artifact_trust::read_private_regular_file_bounded(
                    &path,
                    pack::MAX_BYTES as u64,
                )
            })
            .await
            .map_err(|error| Err(internal(error)))?;
            match bytes {
                Err(_) => Err(Ok(OpenResult::Rejected {
                    reason: Rejection::TooLarge,
                    source_label: None,
                })),
                Ok(bytes) => import::read_zip(&bytes, None)
                    .map(|files| Fetched {
                        files,
                        ..Fetched::default()
                    })
                    .map_err(|reason| {
                        Ok(OpenResult::Rejected {
                            reason,
                            source_label: None,
                        })
                    }),
            }
        }
        Local::Folder => {
            let folder = match e2e_pack_path() {
                Some(path) => path,
                None => match rfd::AsyncFileDialog::new()
                    .set_title("Choose a shared Loupist folder")
                    .pick_folder()
                    .await
                {
                    Some(folder) => folder.path().to_owned(),
                    None => return Ok(None),
                },
            };
            tauri::async_runtime::spawn_blocking(move || import::read_folder(&folder))
                .await
                .map_err(|error| Err(internal(error)))?
                .map(|files| Fetched {
                    files,
                    ..Fetched::default()
                })
                .map_err(|reason| {
                    Ok(OpenResult::Rejected {
                        reason,
                        source_label: None,
                    })
                })
        }
    };
    fetched.map(Some)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn pack_open(
    request: PackOpenRequest,
    state: State<'_, AppState>,
) -> Result<Option<OpenResult>, CommandError> {
    let state = state.inner();
    let fetched = match request.source {
        PackSource::Url { url } => {
            let Some(location) = deep_link::parse_github(&url) else {
                return Err(CommandError::new(
                    "link_invalid",
                    "Use a github.com or gist.github.com link.",
                ));
            };
            fetch_github(state, location).await
        }
        PackSource::Pending { .. } => {
            let Some(location) = deep_link::held() else {
                return Err(CommandError::new(
                    "link_invalid",
                    "This link can't be opened.",
                ));
            };
            let fetched = fetch_github(state, location).await;
            // A network failure keeps the link so Preview can be tried again.
            if !matches!(fetched, Err(Err(_))) {
                deep_link::dismiss();
            }
            fetched
        }
        PackSource::Zip { .. } => match fetch_local(Local::Zip).await {
            Ok(None) => return Ok(None),
            Ok(Some(fetched)) => Ok(fetched),
            Err(error) => Err(error),
        },
        PackSource::Folder { .. } => match fetch_local(Local::Folder).await {
            Ok(None) => return Ok(None),
            Ok(Some(fetched)) => Ok(fetched),
            Err(error) => Err(error),
        },
        PackSource::Gallery { gallery } => {
            // The renderer names a card; the location comes from the index Rust read.
            let entry = match gallery::cached_entry(&gallery) {
                Some(entry) => Some(entry),
                None => gallery::load(Some(&gallery_cache(state)), false)
                    .await
                    .and_then(|_| gallery::cached_entry(&gallery)),
            };
            let Some(entry) = entry else {
                return Err(CommandError::new(
                    "link_not_found",
                    "This shared Loupist is no longer listed.",
                ));
            };
            fetch_github(state, entry.location()).await
        }
    };
    let fetched = match fetched {
        Ok(fetched) => fetched,
        Err(Ok(result)) => return Ok(Some(result)),
        Err(Err(error)) => return Err(error),
    };
    let blocked = match manifest_id(&fetched.files) {
        Some(id) => {
            gallery::blocked_reason(Some(&gallery_cache(state)), &id, fetched.repo.as_deref()).await
        }
        None => None,
    };
    Ok(Some(import::stage(fetched, blocked)))
}
