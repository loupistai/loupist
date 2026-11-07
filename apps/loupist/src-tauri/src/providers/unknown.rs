//! State that names a provider this build does not offer. Release builds drop
//! the development-only Codex client (providers spec §9), so at startup Rust
//! deletes what still points at such a provider instead of failing every
//! catalog write: its catalog rows and status, the Chat default, scheduled
//! model choices (they use Auto) and World models (they wait for the World
//! default). This is a deletion, not a migration.
//!
//! The same startup pass deletes a Claude.ai sign-in grant an earlier build
//! saved: Anthropic does not permit third-party apps to offer that sign-in, so
//! `anthropic` connects only by API key (providers spec).
use crate::{
    app::{AppState, CommandError},
    providers::catalog::known_provider,
    scoped_config::{ConfigDocument, ConfigScope, ConfigUpdateRequest},
    store::LoupistStore,
};

/// The provider part of a `provider/model` profile ID.
pub(crate) fn profile_provider(profile_id: &str) -> &str {
    profile_id
        .split_once('/')
        .map_or(profile_id, |(provider, _)| provider)
}

/// Runs once at startup. A failure leaves the rest of startup alone; admission
/// still treats an unknown provider's model as gone.
pub(crate) fn delete_unknown_provider_state(state: &AppState) {
    if delete_with(state, &known_provider).is_err() {
        eprintln!("Loupist could not remove settings for a provider this build does not offer");
    }
    if delete_claude_sign_in(state).is_err() {
        eprintln!("Loupist could not remove a retired Claude sign-in");
    }
}

/// Deletes a saved `anthropic` OAuth grant and treats it as a disconnect: the
/// card reads Disconnected while a schedule, the Chat default or a World model
/// still uses the account. An Anthropic API key is left alone.
fn delete_claude_sign_in(state: &AppState) -> Result<(), CommandError> {
    let agent_dir = state.pi_agent_data_dir()?;
    if !crate::providers::catalog::delete_saved_oauth_credential(&agent_dir, "anthropic")? {
        return Ok(());
    }
    state.provider_model_cache.invalidate("anthropic");
    crate::providers::status::record(
        state,
        "anthropic",
        crate::providers::status::Trigger::Disconnected(crate::providers::status::used_by(
            state,
            "anthropic",
        )),
    )?;
    state.replace_provider_models("anthropic", Vec::new())
}

fn delete_with(state: &AppState, known: &dyn Fn(&str) -> bool) -> Result<(), CommandError> {
    let mut catalog = state.model_catalog()?;
    let before = catalog.models.len();
    catalog.models.retain(|model| known(&model.provider));
    if catalog.models.len() != before {
        state.set_model_catalog(catalog)?;
    }
    let map_store = |error: crate::store::StoreError| CommandError::internal(error.to_string());
    for record in state.store.provider_status_all().map_err(map_store)? {
        if !known(&record.provider) {
            state
                .store
                .provider_status_delete(&record.provider)
                .map_err(map_store)?;
        }
    }
    state
        .store
        .gone_models_delete_unknown(known)
        .map_err(map_store)?;
    clear_chat_default(state, known)?;
    clear_schedule_models(state, known);
    crate::commands::knowledge::release_unknown_world_models(state, known);
    Ok(())
}

/// The global Chat default; a Loupist's own default falls back to Auto at use.
fn clear_chat_default(state: &AppState, known: &dyn Fn(&str) -> bool) -> Result<(), CommandError> {
    let root = &state.artifacts.app_data_dir;
    let snapshot = crate::scoped_config::read_scope(root, None, ConfigScope::Global)?;
    let unknown = snapshot
        .explicit
        .as_ref()
        .and_then(|document| document.chat.as_ref())
        .and_then(|chat| chat.model_profile_id.as_deref())
        .is_some_and(|id| !known(profile_provider(id)));
    if unknown {
        crate::scoped_config::update_scope(
            root,
            None,
            &ConfigUpdateRequest {
                scope: ConfigScope::Global,
                expected_revision: snapshot.revision,
                set: ConfigDocument::default(),
                unset: vec![vec!["chat".into(), "model_profile_id".into()]],
            },
        )?;
    }
    Ok(())
}

/// Each schedule that named such a model uses Auto from its next run on.
fn clear_schedule_models(state: &AppState, known: &dyn Fn(&str) -> bool) {
    let Ok(loupists) = state.store.list_loupists() else {
        return;
    };
    for loupist in loupists {
        let Ok(tasks) = state.store.scheduled_tasks_list(&loupist.id) else {
            continue;
        };
        for task in tasks {
            if task.model_profile_id.is_empty()
                || known(profile_provider(&task.model_profile_id))
                || task.status == crate::scheduled_tasks::types::TaskStatus::Completed
            {
                continue;
            }
            let mut draft = task.draft();
            draft.model_profile_id.clear();
            draft.thinking_level.clear();
            draft.run_options.clear();
            if state
                .store
                .scheduled_task_update(
                    &loupist.id,
                    &task.id,
                    task.revision,
                    draft,
                    crate::commands::now_ms(),
                )
                .is_err()
            {
                eprintln!("Loupist could not move a schedule to Auto");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        providers::catalog::{ConfiguredModel, ModelCatalog},
        scheduled_tasks::types::{Schedule, TaskDraft, TaskStatus, TaskTarget},
    };

    fn model(provider: &str, id: &str) -> ConfiguredModel {
        ConfiguredModel {
            id: format!("{provider}/{id}"),
            name: id.into(),
            provider: provider.into(),
            model: id.into(),
            input: vec!["text".into()],
            reasoning: true,
            context_window: 272_000,
            max_tokens: 32_000,
            thinking_levels: vec!["off".into(), "low".into(), "high".into()],
            thinking_level_map: BTreeMap::new(),
            run_controls: vec![],
        }
    }

    #[tokio::test]
    async fn a_release_build_deletes_what_still_names_the_codex_client() {
        // The release provider set: everything this build knows except the
        // development-only Codex client.
        let release = |provider: &str| provider != "openai-codex" && known_provider(provider);
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let auth = state.pi_agent_data_dir().unwrap().join("auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&auth).unwrap();
        std::fs::write(
            &auth,
            r#"{"openai":{"type":"api_key","key":"fixture-not-a-real-key"}}"#,
        )
        .unwrap();
        state
            .set_model_catalog(ModelCatalog {
                models: vec![
                    model("openai-codex", "gpt-6-luna"),
                    model("openai", "gpt-6-luna"),
                ],
            })
            .unwrap();
        let folder = root.path().join("research");
        std::fs::create_dir(&folder).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &folder)
            .await
            .unwrap()
            .id;
        crate::commands::knowledge::set_world_model_for_test(
            &state,
            &loupist,
            "openai-codex/gpt-6-luna",
        );
        let task = state
            .store
            .scheduled_task_create(
                &loupist,
                TaskDraft {
                    title: "Morning notes".into(),
                    prompt: "Summarize today's notes.".into(),
                    schedule: Schedule::Interval { every_minutes: 60 },
                    target: TaskTarget::NewChat,
                    model_profile_id: "openai-codex/gpt-6-luna".into(),
                    thinking_level: "high".into(),
                    run_options: BTreeMap::new(),
                    use_memory: true,
                    update_memory: false,
                    status: TaskStatus::Active,
                },
                crate::commands::now_ms(),
            )
            .unwrap();
        let root_dir = &state.artifacts.app_data_dir;
        let snapshot =
            crate::scoped_config::read_scope(root_dir, None, ConfigScope::Global).unwrap();
        crate::scoped_config::update_scope(
            root_dir,
            None,
            &ConfigUpdateRequest {
                scope: ConfigScope::Global,
                expected_revision: snapshot.revision,
                set: serde_json::from_value(serde_json::json!({
                    "chat": {"model_profile_id": "openai-codex/gpt-6-luna", "thinking_level": "high"}
                }))
                .unwrap(),
                unset: vec![],
            },
        )
        .unwrap();
        crate::providers::status::record(
            &state,
            "openai-codex",
            crate::providers::status::Trigger::Connected(
                crate::providers::status::CredentialKind::Plan,
            ),
        )
        .unwrap();

        delete_with(&state, &release).unwrap();

        let catalog = state.model_catalog().unwrap();
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["openai/gpt-6-luna"]
        );
        // Every remaining row validates against the release provider set.
        assert!(catalog.models.iter().all(|model| release(&model.provider)));
        let chat = crate::scoped_config::resolve(root_dir, None).unwrap().chat;
        assert_eq!(chat.model_profile_id, None);
        assert_eq!(chat.thinking_level.as_deref(), Some("high"));
        let task = state
            .store
            .scheduled_task_get(&loupist, &task.id)
            .unwrap()
            .unwrap();
        assert_eq!(
            (task.model_profile_id.as_str(), task.thinking_level.as_str()),
            ("", "")
        );
        assert!(crate::providers::status::load(&state, "openai-codex")
            .unwrap()
            .is_none());
        // The World model falls back to the World default for a connected model.
        let world = crate::commands::knowledge::world_model_for_test(&state, &loupist);
        assert_eq!(world.as_deref(), Some("openai/gpt-6-luna"));
        // Nothing is left to delete the second time.
        delete_with(&state, &release).unwrap();
        assert_eq!(state.model_catalog().unwrap(), catalog);
    }

    fn write_auth(state: &AppState, auth: &str) -> std::path::PathBuf {
        let path = state.pi_agent_data_dir().unwrap().join("auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&path).unwrap();
        std::fs::write(&path, auth).unwrap();
        path
    }

    fn saved_auth(path: &std::path::Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn a_saved_claude_sign_in_is_deleted_like_a_disconnect_and_only_once() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let auth = write_auth(
            &state,
            r#"{
                "anthropic":{"type":"oauth","access":"fixture-access","refresh":"fixture-refresh","expires":1900000000000},
                "openai":{"type":"api_key","key":"fixture-not-a-real-key"}
            }"#,
        );
        state
            .set_model_catalog(ModelCatalog {
                models: vec![
                    model("anthropic", "claude-haiku-4-5"),
                    model("openai", "gpt-6-luna"),
                ],
            })
            .unwrap();
        crate::providers::status::record(
            &state,
            "anthropic",
            crate::providers::status::Trigger::Connected(
                crate::providers::status::CredentialKind::Plan,
            ),
        )
        .unwrap();
        let root_dir = &state.artifacts.app_data_dir;
        let snapshot =
            crate::scoped_config::read_scope(root_dir, None, ConfigScope::Global).unwrap();
        crate::scoped_config::update_scope(
            root_dir,
            None,
            &ConfigUpdateRequest {
                scope: ConfigScope::Global,
                expected_revision: snapshot.revision,
                set: serde_json::from_value(serde_json::json!({
                    "chat": {"model_profile_id": "anthropic/claude-haiku-4-5"}
                }))
                .unwrap(),
                unset: vec![],
            },
        )
        .unwrap();

        delete_claude_sign_in(&state).unwrap();

        let saved = saved_auth(&auth);
        assert!(saved.get("anthropic").is_none());
        assert_eq!(saved["openai"]["key"], "fixture-not-a-real-key");
        let row = crate::providers::status::load(&state, "anthropic")
            .unwrap()
            .unwrap();
        assert_eq!(
            row.state,
            crate::providers::status::ProviderState::Disconnected
        );
        assert!(row.used_by.chat_default);
        let catalog = state.model_catalog().unwrap();
        assert_eq!(
            catalog
                .models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["openai/gpt-6-luna"]
        );

        delete_claude_sign_in(&state).unwrap();
        assert_eq!(saved_auth(&auth), saved);
        assert_eq!(state.model_catalog().unwrap(), catalog);
        assert_eq!(
            crate::providers::status::load(&state, "anthropic")
                .unwrap()
                .unwrap()
                .state,
            crate::providers::status::ProviderState::Disconnected
        );
    }

    #[test]
    fn an_anthropic_api_key_survives_the_claude_sign_in_cleanup() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let contents = r#"{"anthropic":{"type":"api_key","key":"fixture-not-a-real-key"}}"#;
        let auth = write_auth(&state, contents);
        state
            .set_model_catalog(ModelCatalog {
                models: vec![model("anthropic", "claude-haiku-4-5")],
            })
            .unwrap();

        delete_claude_sign_in(&state).unwrap();

        assert_eq!(std::fs::read_to_string(&auth).unwrap(), contents);
        assert_eq!(state.model_catalog().unwrap().models.len(), 1);
        assert!(crate::providers::status::load(&state, "anthropic")
            .unwrap()
            .is_none());
    }
}
