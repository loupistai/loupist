use std::{
    fs,
    path::{Path, PathBuf},
};

use tauri::State;

use crate::{
    agent_harness::AgentSkillSummary,
    app::{AppState, CommandError, LoupistAccess, LoupistRecoveryAction, QuarantineSource},
    domain::{workspace_folder_name, LoupistProfile},
    fs::loupist_root::BoundLoupistRoot,
    runs::maintenance::MaintenanceActivityKind,
    store::deletion,
    store::{LoupistStore, StoreError},
};

use super::{
    map_internal, map_store, new_id, now_ms, profile_summary, profile_summary_at,
    profile_summary_with_availability,
    types::{
        chat_dto, LoupistCreateRequest, LoupistDeleteRequest, LoupistImportRequest,
        LoupistProfileUpdateRequest, LoupistReconnectRequest, LoupistRecoverRequest,
        LoupistSummary, LoupistWorkspace,
    },
};

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_list(state: State<'_, AppState>) -> Result<Vec<LoupistSummary>, CommandError> {
    list_available_loupists(&state).await
}

pub(super) async fn recover_loupist_inner(
    state: &AppState,
    request: &LoupistRecoverRequest,
) -> Result<LoupistSummary, CommandError> {
    let _recovery = state.register_loupist_recovery(
        new_id("loupist-recovery"),
        request.loupist_id.clone(),
        request.action,
    )?;
    match request.action {
        LoupistRecoveryAction::RecoverMemory => {
            state.clear_loupist_quarantine(&request.loupist_id, QuarantineSource::MemoryWrite);
        }
    }
    let profile = state
        .store
        .get_loupist(&request.loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    profile_summary(state, &profile).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_recover(
    request: LoupistRecoverRequest,
    state: State<'_, AppState>,
) -> Result<LoupistSummary, CommandError> {
    recover_loupist_inner(&state, &request).await
}

#[tauri::command(rename_all = "snake_case")]
pub fn agent_skill_catalog(
    loupist_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<AgentSkillSummary>, CommandError> {
    state.ensure_loupist_available(&loupist_id)?;
    state
        .store
        .get_loupist(&loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    let catalog = super::skills::list(
        &state,
        &crate::scoped_config::ConfigScope::Project { loupist_id },
    )?;
    Ok(catalog
        .skills
        .into_iter()
        .filter(|skill| !skill.shadowed && skill.error.is_none())
        .map(|skill| AgentSkillSummary {
            id: skill.id,
            name: skill.name,
            description: skill.description,
            enabled: skill.enabled,
            fixed: skill.fixed,
        })
        .collect())
}

pub(super) async fn list_available_loupists(
    state: &AppState,
) -> Result<Vec<LoupistSummary>, CommandError> {
    let profiles = state.store.list_loupists().map_err(map_store)?;
    let mut summaries = Vec::with_capacity(profiles.len());
    for profile in profiles {
        let availability = match state.loupist_access(&profile.id) {
            LoupistAccess::Visible(availability) => availability,
            LoupistAccess::Hidden => continue,
        };
        summaries.push(profile_summary_with_availability(state, &profile, availability).await?);
    }
    Ok(summaries)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_select(
    loupist_id: String,
    state: State<'_, AppState>,
) -> Result<LoupistWorkspace, CommandError> {
    state.ensure_loupist_available(&loupist_id)?;
    if super::knowledge::complete_pending_world_model(state.inner(), &loupist_id).is_err() {
        eprintln!("Could not complete pending World model setup for {loupist_id}");
    }
    // Assets followed while this Loupist was closed or unavailable reach its Map now.
    if super::knowledge::sync_followed_assets(state.inner(), &loupist_id).is_err() {
        eprintln!("Could not add what the user follows to {loupist_id}");
    }
    let profile = state
        .store
        .get_loupist(&loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    let chats = if state.store.is_folder_catalog()
        && (crate::fs::loupist_root::profile_workspace(&profile).is_err()
            || state.store.lock_loupist(&profile.id).is_err())
    {
        Vec::new()
    } else {
        // Opening a Loupist lands on its home Thread, and results finished while
        // a reply was running there arrive now.
        if let Err(error) = crate::chat::home::deliver_finished_runs(&state, &profile.id) {
            eprintln!(
                "Could not prepare the home Thread for {} ({error})",
                profile.id
            );
        }
        state
            .store
            .list_chats_for_loupist(&profile.id)
            .map_err(map_store)?
    };
    let threads = chats
        .iter()
        .filter_map(|chat| match chat_dto(chat) {
            Ok(thread) => Some(thread),
            Err(error) => {
                eprintln!("Loupist skipped unreadable Chat {} ({error})", chat.id);
                None
            }
        })
        .collect();
    Ok(LoupistWorkspace {
        loupist: profile_summary(&state, &profile).await?,
        threads,
    })
}

/// Registers or opens a user-owned Loupist folder with canonical data in `.loupist`.
#[cfg(any(test, feature = "eval-harness", feature = "e2e"))]
pub(crate) async fn create_folder_loupist(
    state: &AppState,
    workspace_path: &Path,
) -> Result<LoupistSummary, CommandError> {
    create_folder_loupist_with_setup(state, workspace_path, None, None).await
}

#[cfg(any(test, feature = "eval-harness", feature = "e2e"))]
pub(super) async fn create_folder_loupist_with_setup(
    state: &AppState,
    workspace_path: &Path,
    research_focus: Option<String>,
    world_processing: Option<crate::providers::world_defaults::NewLoupistWorldSettings>,
) -> Result<LoupistSummary, CommandError> {
    let workspace = selected_workspace(state, workspace_path)?;
    create_bound_loupist(
        state,
        workspace,
        research_focus,
        world_processing,
        None,
        None,
    )
    .await
}

async fn create_bound_loupist(
    state: &AppState,
    workspace: BoundLoupistRoot,
    research_focus: Option<String>,
    world_processing: Option<crate::providers::world_defaults::NewLoupistWorldSettings>,
    template: Option<crate::domain::LoupistTemplate>,
    identity: Option<(String, String, crate::domain::LoupistTone)>,
) -> Result<LoupistSummary, CommandError> {
    let imported = state.store.folder_profile(&workspace).map_err(map_store)?;
    let world_settings = if imported.is_none() {
        Some(crate::providers::world_defaults::new_settings(
            state,
            world_processing.as_ref(),
            research_focus.as_deref(),
        )?)
    } else {
        None
    };
    let loupist_id = imported
        .as_ref()
        .map(|p| p.id.clone())
        .unwrap_or_else(|| new_id("loupist"));
    let timestamp = now_ms();
    let focus = research_focus
        .as_deref()
        .map(str::trim)
        .unwrap_or_default()
        .to_owned();
    if focus.chars().count() > 280 {
        return Err(CommandError::invalid(
            "research focus must be 280 characters or fewer",
        ));
    }
    let mut profile = LoupistProfile {
        id: loupist_id.clone(),
        name: workspace_folder_name(
            workspace
                .path()
                .to_str()
                .ok_or_else(|| CommandError::invalid("workspace path must be valid Unicode"))?,
        )
        .to_owned(),
        focus: focus.clone(),
        instructions: String::new(),
        avatar: "initial".into(),
        tone: Default::default(),
        template,
        workspace_root: workspace
            .path()
            .to_str()
            .ok_or_else(|| CommandError::invalid("workspace path must be valid Unicode"))?
            .to_owned(),
        runtime_filesystem_identity: None,
        root_filesystem_identity: workspace.identity(),
        last_model_profile_id: None,
        created_at_ms: timestamp,
        updated_at_ms: timestamp,
    };
    if let Some((name, avatar, tone)) = identity {
        profile.name = name;
        profile.avatar = avatar;
        profile.tone = tone;
    }
    if let Some(saved) = &imported {
        profile = saved.clone();
    }
    let loupist_relative = PathBuf::from("loupists").join(&loupist_id);
    let loupist_guard = state
        .artifacts
        .deletion_root
        .ensure_private_subdirectory(&loupist_relative)?;
    let loupist_identity = state
        .artifacts
        .deletion_root
        .directory_identity(&loupist_relative)?
        .ok_or_else(|| CommandError::internal("new Loupist runtime identity is missing"))?;
    profile.runtime_filesystem_identity = Some(loupist_identity.clone());
    let registration = if imported.is_some() {
        state.store.register_existing_folder(&profile)
    } else {
        state.store.create_loupist(&profile)
    };
    if let Err(error) = registration.map_err(|error| match error {
        StoreError::Conflict(_) => {
            CommandError::conflict("this folder is already registered to a Loupist")
        }
        other => map_store(other),
    }) {
        return Err(cleanup_failed_loupist_creation(
            state,
            &loupist_relative,
            &loupist_identity,
            loupist_guard,
            error,
        ));
    }
    if let Err(error) = crate::commands::knowledge::ensure_perspective(state, &loupist_id, &focus)
        .and_then(|_| {
            crate::commands::knowledge::initialize_new_world(
                state,
                &loupist_id,
                world_settings.as_ref(),
                template.map(|template| template.as_str()),
            )
        })
    {
        state.store.delete_loupist(&profile).map_err(map_store)?;
        return Err(cleanup_failed_loupist_creation(
            state,
            &loupist_relative,
            &loupist_identity,
            loupist_guard,
            error,
        ));
    }
    drop(loupist_guard);
    profile_summary_at(state, &profile, &workspace).await
}

fn selected_workspace(state: &AppState, path: &Path) -> Result<BoundLoupistRoot, CommandError> {
    if !path.is_absolute() {
        return Err(CommandError::invalid("choose an absolute workspace folder"));
    }
    let metadata = fs::symlink_metadata(path)
        .map_err(|_| CommandError::invalid("workspace folder does not exist"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(CommandError::invalid(
            "choose a regular workspace directory",
        ));
    }
    let canonical = path.canonicalize().map_err(map_internal)?;
    // Development and manual profiles also reserve their containing global
    // namespace, so a workspace cannot nest inside another app environment.
    let private_root = state
        .artifacts
        .app_data_dir
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == ".loupist"))
        .unwrap_or(&state.artifacts.app_data_dir);
    let project_data = canonical.join(".loupist");
    if canonical.starts_with(private_root)
        || project_data.starts_with(private_root)
        || private_root.starts_with(&project_data)
    {
        return Err(CommandError::invalid(
            "choose a folder outside Loupist's private storage",
        ));
    }
    BoundLoupistRoot::open_unbound(canonical)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_choose_folder() -> Result<Option<String>, CommandError> {
    #[cfg(feature = "e2e")]
    if let Some(path) = std::env::var_os("LOUPIST_E2E_WORKSPACE_DIR") {
        return PathBuf::from(path)
            .to_str()
            .map(|path| Some(path.to_owned()))
            .ok_or_else(|| CommandError::invalid("workspace path must be valid Unicode"));
    }
    rfd::AsyncFileDialog::new()
        .set_title("Choose a trusted folder for this Loupist")
        .pick_folder()
        .await
        .map(|folder| {
            folder
                .path()
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| CommandError::invalid("workspace path must be valid Unicode"))
        })
        .transpose()
}

/// Deterministic E2E registration requires an explicitly supplied real folder;
/// ordinary app startup never allocates or registers a default workspace.
#[cfg(feature = "e2e")]
pub(crate) async fn seed_e2e_workspace(state: &AppState) -> Result<(), CommandError> {
    let Some(path) = std::env::var_os("LOUPIST_E2E_WORKSPACE_DIR") else {
        return Ok(());
    };
    if state.store.list_loupists().map_err(map_store)?.is_empty() {
        create_folder_loupist(state, Path::new(&path)).await?;
    }
    Ok(())
}

pub(super) async fn reconnect_loupist_inner(
    state: &AppState,
    request: &LoupistReconnectRequest,
) -> Result<LoupistSummary, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::LoupistMutation)?;
    let _reservation = state.register_run(
        new_id("loupist-reconnect"),
        request.loupist_id.clone(),
        crate::runs::coordinator::RunKind::MemoryWrite,
        crate::runs::coordinator::RunTarget::MemoryWriteSession("workspace-reconnect".into()),
    )?;
    let expected = state
        .store
        .get_loupist(&request.loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    let workspace = selected_workspace(state, Path::new(&request.workspace_path))?;
    let mut updated = expected.clone();
    updated.workspace_root = workspace
        .path()
        .to_str()
        .ok_or_else(|| CommandError::invalid("workspace path must be valid Unicode"))?
        .to_owned();
    updated.root_filesystem_identity = workspace.identity();
    let runtime_relative = PathBuf::from("loupists").join(&expected.id);
    let existing_runtime = state
        .artifacts
        .deletion_root
        .directory_identity(&runtime_relative)?;
    if existing_runtime.is_some() && existing_runtime != expected.runtime_filesystem_identity {
        return Err(CommandError::conflict(
            "Loupist private runtime folder changed",
        ));
    }
    let runtime_guard = state
        .artifacts
        .deletion_root
        .ensure_private_subdirectory(&runtime_relative)?;
    updated.runtime_filesystem_identity = state
        .artifacts
        .deletion_root
        .directory_identity(&runtime_relative)?;
    updated.updated_at_ms = now_ms().max(expected.updated_at_ms.saturating_add(1));
    if let Err(error) = state.store.reconnect_loupist_workspace(&expected, &updated) {
        let error = map_store(error);
        if existing_runtime.is_none() {
            if let Some(identity) = &updated.runtime_filesystem_identity {
                return Err(cleanup_failed_loupist_creation(
                    state,
                    &runtime_relative,
                    identity,
                    runtime_guard,
                    error,
                ));
            }
        }
        return Err(error);
    }
    profile_summary_at(state, &updated, &workspace).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_reconnect(
    request: LoupistReconnectRequest,
    state: State<'_, AppState>,
) -> Result<LoupistSummary, CommandError> {
    reconnect_loupist_inner(state.inner(), &request).await
}

pub(super) fn cleanup_failed_loupist_creation(
    state: &AppState,
    relative: &Path,
    expected_identity: &crate::domain::RootFilesystemIdentity,
    guard: crate::fs::secure_delete::PrivateDirectoryGuard,
    primary: CommandError,
) -> CommandError {
    drop(guard);
    match state
        .artifacts
        .deletion_root
        .remove_tree_expected(relative, Some(expected_identity))
    {
        Ok(()) => primary,
        Err(cleanup) => CommandError::internal(format!(
            "Loupist setup failed and its private runtime could not be cleaned: {}; cleanup: {}",
            primary.message, cleanup.message
        )),
    }
}

pub(super) fn validate_profile_update(
    request: &LoupistProfileUpdateRequest,
) -> Result<(), CommandError> {
    if let Some(name) = &request.name {
        if !(1..=60).contains(&name.trim().chars().count()) || name.contains('\0') {
            return Err(CommandError::invalid(
                "Name must contain 1–60 characters after trimming.",
            ));
        }
    }
    for (value, limit, message) in [
        (
            request.focus.as_deref(),
            280,
            "Focus must contain at most 280 characters.",
        ),
        (
            request.instructions.as_deref(),
            4_000,
            "Instructions must contain at most 4,000 characters.",
        ),
    ] {
        if value.is_some_and(|value| value.chars().count() > limit || value.contains('\0')) {
            return Err(CommandError::invalid(message));
        }
    }
    if request
        .avatar
        .as_deref()
        .is_some_and(|avatar| !crate::domain::LOUPIST_AVATAR_IDS.contains(&avatar))
    {
        return Err(CommandError::invalid("Choose an available avatar."));
    }
    Ok(())
}

pub(super) async fn update_profile_inner(
    state: &AppState,
    request: LoupistProfileUpdateRequest,
) -> Result<LoupistSummary, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::LoupistMutation)?;
    state.ensure_loupist_available(&request.loupist_id)?;
    validate_profile_update(&request)?;
    state
        .store
        .get_loupist(&request.loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    let profile = state
        .store
        .update_loupist_profile(
            &request.loupist_id,
            crate::store::LoupistProfileEdit {
                name: request.name.as_deref().map(str::trim),
                focus: request.focus.as_deref(),
                instructions: request.instructions.as_deref(),
                avatar: request.avatar.as_deref(),
                tone: request.tone,
            },
            now_ms(),
        )
        .map_err(map_store)?;
    profile_summary(state, &profile).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_profile_update(
    request: LoupistProfileUpdateRequest,
    state: State<'_, AppState>,
) -> Result<LoupistSummary, CommandError> {
    update_profile_inner(state.inner(), request).await
}

pub(crate) async fn create_managed_loupist(
    state: &AppState,
    request: LoupistCreateRequest,
) -> Result<LoupistSummary, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::LoupistMutation)?;
    validate_profile_update(&LoupistProfileUpdateRequest {
        name: Some(request.name.clone()),
        focus: request.focus.clone(),
        avatar: request.avatar.clone(),
        ..Default::default()
    })?;
    let template = request
        .template
        .as_deref()
        .map(|id| {
            crate::domain::LoupistTemplate::parse(id)
                .ok_or_else(|| CommandError::invalid("template is not a Loupist template"))
        })
        .transpose()?;
    let name = request.name.trim().to_owned();
    let slug: String = name
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .take(40)
        .collect();
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "loupist" } else { slug };
    let suffix = new_id("workspace");
    let relative =
        PathBuf::from("loupists").join(format!("{slug}-{}", &suffix[suffix.len() - 12..]));
    let guard = state
        .artifacts
        .deletion_root
        .ensure_private_subdirectory(&relative)?;
    let workspace =
        BoundLoupistRoot::open_unbound(state.artifacts.deletion_root.absolute_path(&relative)?)?;
    let identity = workspace
        .identity()
        .ok_or_else(|| CommandError::internal("New Loupist folder identity is missing"))?;
    let result = create_bound_loupist(
        state,
        workspace,
        request.focus,
        None,
        template,
        Some((
            name,
            request.avatar.unwrap_or_else(|| "initial".into()),
            request.tone.unwrap_or_default(),
        )),
    )
    .await;
    match result {
        Ok(summary) => {
            drop(guard);
            // The new Loupist's first Source is what the user follows. Failing to
            // add it never undoes a Loupist that was created; opening it catches up.
            if crate::commands::knowledge::sync_followed_assets(state, &summary.id).is_err() {
                eprintln!(
                    "Could not add what the user follows to the new Loupist {}",
                    summary.id
                );
            }
            Ok(summary)
        }
        Err(error) => Err(cleanup_failed_loupist_creation(
            state, &relative, &identity, guard, error,
        )),
    }
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_create(
    request: LoupistCreateRequest,
    state: State<'_, AppState>,
) -> Result<LoupistSummary, CommandError> {
    create_managed_loupist(state.inner(), request).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_import(
    request: LoupistImportRequest,
    state: State<'_, AppState>,
) -> Result<LoupistSummary, CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::LoupistMutation)?;
    let workspace = selected_workspace(state.inner(), Path::new(&request.workspace_path))?;
    if state
        .store
        .folder_profile(&workspace)
        .map_err(map_store)?
        .is_none()
    {
        return Err(CommandError::invalid("Choose an existing Loupist folder."));
    }
    create_bound_loupist(state.inner(), workspace, None, None, None, None).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn loupist_delete(
    request: LoupistDeleteRequest,
    state: State<'_, AppState>,
) -> Result<(), CommandError> {
    delete_loupist_inner(state.inner(), &request.loupist_id).await
}

pub(crate) async fn delete_loupist_inner(
    state: &AppState,
    loupist_id: &str,
) -> Result<(), CommandError> {
    let _activity = state
        .maintenance
        .admit_kind(MaintenanceActivityKind::LoupistDeletion)?;
    state.ensure_loupist_available(loupist_id)?;
    let profile = state
        .store
        .get_loupist(loupist_id)
        .map_err(map_store)?
        .ok_or_else(|| CommandError::not_found("Loupist"))?;
    state.begin_loupist_deletion(loupist_id)?;
    if let Err(error) = deletion::delete_loupist(
        state.store.as_ref(),
        state.artifacts.deletion_root.as_ref(),
        &profile,
        now_ms(),
    ) {
        if !deletion::has_pending_for(state.store.as_ref(), loupist_id, loupist_id).unwrap_or(true)
        {
            state.clear_loupist_quarantine(loupist_id, QuarantineSource::Deletion);
        }
        return Err(error);
    }
    state.forget_loupist(&profile.id).await;
    Ok(())
}

#[cfg(test)]
mod loupist_profile_tests {
    use super::*;

    #[test]
    fn loupist_requests_carry_a_tone_and_no_suitability_fields() {
        let request: LoupistCreateRequest =
            serde_json::from_value(serde_json::json!({ "name": "Desk", "tone": "careful" }))
                .unwrap();
        assert_eq!(request.tone, Some(crate::domain::LoupistTone::Careful));
        let defaulted: LoupistCreateRequest =
            serde_json::from_value(serde_json::json!({ "name": "Desk" })).unwrap();
        assert_eq!(
            defaulted.tone.unwrap_or_default(),
            crate::domain::LoupistTone::Balanced
        );
        assert!(serde_json::from_value::<LoupistCreateRequest>(
            serde_json::json!({ "name": "Desk", "tone": "aggressive" })
        )
        .is_err());
        // Tone is presentation only; the app never stores a suitability profile.
        for key in [
            "risk_tolerance",
            "goals",
            "income",
            "assets",
            "horizon",
            "explanation_level",
        ] {
            let mut body = serde_json::Map::new();
            body.insert("name".into(), "Desk".into());
            body.insert(key.into(), "high".into());
            assert!(
                serde_json::from_value::<LoupistCreateRequest>(body.clone().into()).is_err(),
                "{key}"
            );
            body.insert("loupistId".into(), "loupist-a".into());
            body.remove("name");
            assert!(
                serde_json::from_value::<LoupistProfileUpdateRequest>(body.into()).is_err(),
                "{key}"
            );
        }
        // Profiles saved before tones existed still open, as Balanced.
        let mut legacy =
            serde_json::to_value(crate::test_fixtures::loupist_profile("loupist-a")).unwrap();
        legacy.as_object_mut().unwrap().remove("tone");
        let profile: crate::domain::LoupistProfile = serde_json::from_value(legacy).unwrap();
        assert_eq!(profile.tone, crate::domain::LoupistTone::Balanced);
    }

    #[test]
    fn loupist_profile_validates_boundaries_and_unicode() {
        let valid = LoupistProfileUpdateRequest {
            name: Some(format!("  {}  ", "연".repeat(60))),
            focus: Some("관".repeat(280)),
            instructions: Some("😀".repeat(4_000)),
            avatar: Some("book-open".into()),
            ..Default::default()
        };
        validate_profile_update(&valid).unwrap();
        for avatar in crate::domain::LOUPIST_AVATAR_IDS {
            validate_profile_update(&LoupistProfileUpdateRequest {
                avatar: Some((*avatar).into()),
                ..valid.clone()
            })
            .unwrap();
        }
        for invalid in [
            LoupistProfileUpdateRequest {
                name: Some(" \t".into()),
                ..valid.clone()
            },
            LoupistProfileUpdateRequest {
                name: Some("연".repeat(61)),
                ..valid.clone()
            },
            LoupistProfileUpdateRequest {
                focus: Some("관".repeat(281)),
                ..valid.clone()
            },
            LoupistProfileUpdateRequest {
                instructions: Some("😀".repeat(4_001)),
                ..valid.clone()
            },
            LoupistProfileUpdateRequest {
                avatar: Some("not-an-avatar".into()),
                ..valid.clone()
            },
        ] {
            assert_eq!(
                validate_profile_update(&invalid).unwrap_err().code,
                "invalid_request"
            );
        }
    }

    #[tokio::test]
    async fn loupist_profile_updates_persist_and_preserve_unspecified_fields() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("research");
        fs::create_dir(&folder).unwrap();
        let root = temp.path().join("app");
        let state = AppState::for_folder_test(root.clone());
        let original = create_folder_loupist(&state, &folder).await.unwrap();
        let updated = update_profile_inner(
            &state,
            LoupistProfileUpdateRequest {
                loupist_id: original.id.clone(),
                name: Some("  Research desk  ".into()),
                focus: Some("Semiconductors".into()),
                instructions: Some("Use primary evidence.".into()),
                avatar: Some("loupist-10".into()),
                tone: Some(crate::domain::LoupistTone::Careful),
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.name, "Research desk");
        assert_eq!(updated.tone, crate::domain::LoupistTone::Careful);
        assert_eq!(updated.philosophy, "Semiconductors");
        assert_eq!(updated.instructions, "Use primary evidence.");
        assert_eq!(updated.avatar, "loupist-10");
        assert_eq!(updated.workspace_path, original.workspace_path);
        let partial = update_profile_inner(
            &state,
            LoupistProfileUpdateRequest {
                loupist_id: original.id.clone(),
                focus: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(partial.name, updated.name);
        assert_eq!(partial.avatar, "loupist-10");
        assert_eq!(partial.tone, crate::domain::LoupistTone::Careful);
        assert_eq!(partial.philosophy, "");
        let rejected = update_profile_inner(
            &state,
            LoupistProfileUpdateRequest {
                loupist_id: original.id.clone(),
                name: Some(" ".into()),
                ..Default::default()
            },
        )
        .await;
        assert!(rejected.is_err());
        drop(state);
        let reopened = AppState::for_folder_test(root);
        let profile = reopened.store.get_loupist(&original.id).unwrap().unwrap();
        assert_eq!(profile.name, "Research desk");
        assert_eq!(profile.focus, "");
        assert_eq!(profile.instructions, "Use primary evidence.");
        assert_eq!(profile.avatar, "loupist-10");
        assert_eq!(profile.tone, crate::domain::LoupistTone::Careful);
    }

    #[tokio::test]
    async fn loupist_profile_managed_creation_owns_workspace_and_validates_before_allocation() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(temp.path().join("app"));
        let created = create_managed_loupist(
            &state,
            LoupistCreateRequest {
                name: "  My Loupist  ".into(),
                focus: Some("Energy".into()),
                avatar: Some("loupist-07".into()),
                template: None,
                tone: Some(crate::domain::LoupistTone::Direct),
            },
        )
        .await
        .unwrap();
        assert_eq!(created.name, "My Loupist");
        assert_eq!(created.tone, crate::domain::LoupistTone::Direct);
        assert_eq!(created.philosophy, "Energy");
        assert_eq!(created.avatar, "loupist-07");
        assert!(Path::new(&created.workspace_path)
            .starts_with(state.artifacts.app_data_dir.join("loupists")));
        assert!(Path::new(&created.workspace_path)
            .join(".loupist/loupist.sqlite3")
            .is_file());
        assert!(create_managed_loupist(
            &state,
            LoupistCreateRequest {
                name: String::new(),
                focus: None,
                avatar: None,
                template: None,
                tone: None,
            }
        )
        .await
        .is_err());
        assert_eq!(state.store.list_loupists().unwrap().len(), 1);
    }

    async fn create(
        state: &AppState,
        name: &str,
        focus: Option<&str>,
        template: Option<&str>,
    ) -> String {
        create_managed_loupist(
            state,
            LoupistCreateRequest {
                name: name.into(),
                focus: focus.map(str::to_owned),
                avatar: None,
                template: template.map(str::to_owned),
                tone: None,
            },
        )
        .await
        .unwrap()
        .id
    }

    #[tokio::test]
    async fn the_template_a_loupist_was_made_from_is_kept_on_its_profile() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(temp.path().join("app"));
        let crypto = create(&state, "Crypto desk", Some("Tokens"), Some("crypto")).await;
        let plain = create(&state, "Plain desk", Some("Anything"), None).await;
        let stored = |id: &str| state.store.get_loupist(id).unwrap().unwrap().template;
        assert_eq!(
            stored(&crypto),
            Some(crate::domain::LoupistTemplate::Crypto)
        );
        assert_eq!(stored(&plain), None);
        // The summary the renderer reads carries it under the renderer's id.
        let profile = state.store.get_loupist(&crypto).unwrap().unwrap();
        let summary = serde_json::to_value(
            crate::commands::profile_summary(&state, &profile)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(summary["template"], "crypto");
        assert_eq!(
            crate::domain::LoupistTemplate::parse("realAssets")
                .and_then(crate::domain::LoupistTemplate::asset_class),
            Some(crate::knowledge::assets::AssetClass::RealAsset)
        );
        // An unknown template is refused instead of stored.
        let refused = create_managed_loupist(
            &state,
            LoupistCreateRequest {
                name: "Odd desk".into(),
                focus: None,
                avatar: None,
                template: Some("moon".into()),
                tone: None,
            },
        )
        .await;
        assert!(refused.is_err());
    }

    #[tokio::test]
    async fn a_template_writes_one_preset_change_whatever_the_focus_says() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(temp.path().join("app"));
        for (name, template, focus) in [
            (
                "Chip desk",
                "semiconductors",
                "Chips, memory, foundries and equipment",
            ),
            ("Edited chips", "semiconductors", "Only foundries"),
            ("Typed chips", "semiconductors", ""),
        ] {
            let id = create(&state, name, Some(focus), Some(template)).await;
            let (extension, events, changes) = world_seed(&state, &id);
            assert_eq!(extension.version, 1, "{name}");
            assert!(!extension.types.is_empty() && !events.is_empty(), "{name}");
            assert_eq!(changes, vec!["preset".to_owned()], "{name}");
        }
        // A template without a curated seed, and a blank desk, leave the core alone.
        for (name, template) in [
            ("Portfolio", Some("portfolio")),
            ("Custom", Some("custom")),
            ("Blank", None),
        ] {
            let id = create(&state, name, Some("My own desk"), template).await;
            let (extension, events, changes) = world_seed(&state, &id);
            assert_eq!(extension.version, 0, "{name}");
            assert!(
                extension.types.is_empty() && events.is_empty() && changes.is_empty(),
                "{name}"
            );
        }
    }

    #[allow(clippy::type_complexity)]
    fn world_seed(
        state: &AppState,
        loupist_id: &str,
    ) -> (
        crate::knowledge::WorldExtension,
        Vec<(String, String)>,
        Vec<String>,
    ) {
        let connection = state.store.lock_loupist(loupist_id).unwrap();
        let extension = crate::knowledge::load_settings(&connection, loupist_id)
            .unwrap()
            .extension;
        let mut events = connection
            .prepare(
                "SELECT kind, subject_id FROM knowledge_growth_events WHERE loupist_id = ?1 AND kind IN ('schema_extended', 'schema_reduced') ORDER BY rowid",
            )
            .unwrap();
        let events = events
            .query_map([loupist_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        let mut changes = connection
            .prepare("SELECT producer FROM knowledge_schema_changes WHERE loupist_id = ?1 ORDER BY rowid")
            .unwrap();
        let changes = changes
            .query_map([loupist_id], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        (extension, events, changes)
    }
}
