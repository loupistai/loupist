use super::{admit_mutation, load_graph_settings, with_loupist_knowledge};
use crate::{
    app::{AppState, CommandError},
    knowledge::GraphProcessingSettings,
    providers::world_defaults::{recommended_model, ResolvedWorldModel},
    store::LoupistStore,
};

fn awaits_default_model(settings: &GraphProcessingSettings) -> bool {
    settings.awaiting_default_model
        && settings.enrichment_enabled
        && settings.helper_model_profile_id.is_none()
}

pub(crate) fn complete_pending_world_model(
    state: &AppState,
    loupist_id: &str,
) -> Result<(), CommandError> {
    if !awaits_default_model(&load_graph_settings(state, loupist_id)?) {
        return Ok(());
    }
    if let Some(model) = recommended_model(state)? {
        if complete_world_model(state, loupist_id, &model)? {
            state.graph.wake();
        }
    }
    Ok(())
}

pub(crate) fn initialize_new_world(
    state: &AppState,
    loupist_id: &str,
    settings: Option<&crate::knowledge::GraphProcessingSettings>,
    template: Option<&str>,
) -> Result<(), CommandError> {
    if let Some(settings) = settings {
        with_loupist_knowledge(state, loupist_id, |tx, _| {
            crate::knowledge::store_settings(tx, loupist_id, settings)?;
            crate::knowledge::apply_preset(tx, loupist_id, template, crate::commands::now_ms())?;
            Ok(())
        })?;
    }
    Ok(())
}

// Only the pending marker written at new-Loupist creation authorizes completion.
// Applying public settings clears it, even when no model is selected.
pub(crate) fn complete_pending_world_models(state: &AppState) -> Result<(), CommandError> {
    let Some(model) = recommended_model(state)? else {
        return Ok(());
    };
    let loupists = state
        .store
        .list_loupists()
        .map_err(crate::commands::map_store)?;
    let mut changed = false;
    for loupist in loupists {
        // Unavailable folders must not turn a successful provider connection
        // into a failure. Opening that Loupist later can complete its setup.
        match complete_world_model(state, &loupist.id, &model) {
            Ok(completed) => changed |= completed,
            Err(_) => eprintln!(
                "Could not complete pending World model setup for {}",
                loupist.id
            ),
        }
    }
    if changed {
        state.graph.wake();
    }
    Ok(())
}

/// A World model on a provider this build does not offer is forgotten; the
/// Loupist then waits for the World default like a new one (providers spec §3.3).
pub(crate) fn release_unknown_world_models(state: &AppState, known: &dyn Fn(&str) -> bool) {
    let Ok(loupists) = state.store.list_loupists() else {
        return;
    };
    let mut released_any = false;
    for loupist in loupists {
        let named_unknown = load_graph_settings(state, &loupist.id).is_ok_and(|settings| {
            settings
                .helper_provider
                .as_deref()
                .is_some_and(|provider| !known(provider))
        });
        if !named_unknown {
            continue;
        }
        released_any = true;
        let released = admit_mutation(state, &loupist.id).and_then(|_activity| {
            with_loupist_knowledge(state, &loupist.id, |tx, _| {
                let mut settings = crate::knowledge::load_settings(tx, &loupist.id)?;
                if settings.helper_provider.as_deref().is_none_or(known) {
                    return Ok(());
                }
                settings.helper_provider = None;
                settings.helper_model = None;
                settings.helper_model_profile_id = None;
                settings.fast_model = None;
                settings.awaiting_default_model = settings.enrichment_enabled;
                crate::knowledge::store_settings(tx, &loupist.id, &settings)
            })
        });
        if released.is_err() {
            eprintln!("Loupist could not reset a World model it no longer offers");
        }
    }
    if released_any && complete_pending_world_models(state).is_err() {
        eprintln!("Loupist could not pick the default World model");
    }
}

#[cfg(test)]
pub(crate) fn set_world_model_for_test(state: &AppState, loupist_id: &str, profile_id: &str) {
    let (provider, model) = profile_id.split_once('/').unwrap();
    with_loupist_knowledge(state, loupist_id, |tx, _| {
        let mut settings = crate::knowledge::load_settings(tx, loupist_id)?;
        settings.enrichment_enabled = true;
        settings.awaiting_default_model = false;
        settings.helper_provider = Some(provider.into());
        settings.helper_model = Some(model.into());
        settings.helper_model_profile_id = Some(profile_id.into());
        crate::knowledge::store_settings(tx, loupist_id, &settings)
    })
    .unwrap();
}

#[cfg(test)]
pub(crate) fn world_model_for_test(state: &AppState, loupist_id: &str) -> Option<String> {
    load_graph_settings(state, loupist_id)
        .unwrap()
        .helper_model_profile_id
}

fn complete_world_model(
    state: &AppState,
    loupist_id: &str,
    model: &ResolvedWorldModel,
) -> Result<bool, CommandError> {
    let _activity = admit_mutation(state, loupist_id)?;
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        let mut settings = crate::knowledge::load_settings(tx, loupist_id)?;
        // Recheck in the same transaction as the write so a saved user choice wins.
        if !awaits_default_model(&settings) {
            return Ok(false);
        }
        model.apply(&mut settings);
        crate::knowledge::store_settings(tx, loupist_id, &settings)?;
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).enqueue_existing_sources(
            loupist_id,
            &settings,
            crate::commands::now_ms(),
        )?;
        Ok(true)
    })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        commands::loupist::{create_folder_loupist, create_folder_loupist_with_setup},
        providers::catalog::{ConfiguredModel, ModelCatalog},
        providers::world_defaults::{NewLoupistWorldSettings, WorldModelSelection},
    };
    use std::{collections::BTreeMap, fs, path::Path};

    pub(crate) fn connect_fixture(state: &AppState) {
        let path = state.pi_agent_data_dir().unwrap().join("auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&path).unwrap();
        fs::write(&path, r#"{"openai":{"type":"api_key","key":"fixture-not-a-real-key"},"anthropic":{"type":"api_key","key":"fixture-not-a-real-key"}}"#).unwrap();
        let models = [("openai", "gpt-6-luna"), ("anthropic", "claude-haiku-4-5")]
            .into_iter()
            .map(|(provider, model)| ConfiguredModel {
                id: format!("{provider}/{model}"),
                name: model.into(),
                provider: provider.into(),
                model: model.into(),
                input: vec!["text".into()],
                reasoning: true,
                context_window: 128_000,
                max_tokens: 16_000,
                thinking_levels: vec!["off".into(), "low".into(), "high".into()],
                thinking_level_map: BTreeMap::new(),
                run_controls: vec![],
            })
            .collect();
        state.set_model_catalog(ModelCatalog { models }).unwrap();
    }

    async fn create(state: &AppState, root: &Path, name: &str) -> String {
        let folder = root.join(name);
        fs::create_dir(&folder).unwrap();
        create_folder_loupist(state, &folder).await.unwrap().id
    }

    #[tokio::test]
    async fn new_world_defaults_persist_and_reopening_preserves_user_settings() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        connect_fixture(&state);
        let loupist_id = create(&state, root.path(), "research").await;
        let settings = load_graph_settings(&state, &loupist_id).unwrap();
        assert!(settings.enrichment_enabled);
        assert!(!settings.awaiting_default_model);
        assert_eq!(settings.helper_model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(settings.thinking_level, "high");
        let mut edited = settings.public_settings();
        edited.enrichment_enabled = false;
        edited.paused = true;
        edited.helper_model_profile_id = Some("anthropic/claude-haiku-4-5".into());
        edited.thinking_level = "low".into();
        edited.research_focus = "Supplier concentration".into();
        edited.max_llm_calls = 3;
        let model = state
            .model_catalog()
            .unwrap()
            .resolve(edited.helper_model_profile_id.as_deref().unwrap())
            .unwrap();
        let expected = with_loupist_knowledge(&state, &loupist_id, |tx, _| {
            let mut saved = crate::knowledge::load_settings(tx, &loupist_id)?;
            saved.apply_public(&edited, Some(&model));
            crate::knowledge::store_settings(tx, &loupist_id, &saved)?;
            Ok(saved)
        })
        .unwrap();
        drop(state);
        let reopened = AppState::for_folder_test(root.path().join("reopened-app"));
        connect_fixture(&reopened);
        // Creation defaults, including an invalid requested model, have no
        // authority over an existing folder's World preferences.
        let summary = create_folder_loupist_with_setup(
            &reopened,
            &root.path().join("research"),
            Some("Ignored focus".into()),
            Some(NewLoupistWorldSettings {
                enabled: true,
                model: Some(WorldModelSelection {
                    model_profile_id: "missing/model".into(),
                    thinking_level: "high".into(),
                    run_options: BTreeMap::new(),
                }),
            }),
        )
        .await
        .unwrap();
        assert_eq!(summary.id, loupist_id);
        complete_pending_world_models(&reopened).unwrap();
        assert_eq!(
            load_graph_settings(&reopened, &loupist_id).unwrap(),
            expected
        );
    }

    #[tokio::test]
    async fn new_world_defaults_wait_for_connection_and_leave_unconfigured_or_opted_out_loupists_alone(
    ) {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let pending = create(&state, root.path(), "pending").await;
        assert!(
            load_graph_settings(&state, &pending)
                .unwrap()
                .awaiting_default_model
        );
        let unconfigured = create(&state, root.path(), "unconfigured").await;
        with_loupist_knowledge(&state, &unconfigured, |tx, _| {
            tx.execute(
                "DELETE FROM app_settings WHERE id = ?1",
                [format!("world_processing:{unconfigured}")],
            )?;
            Ok(())
        })
        .unwrap();
        let changed = create(&state, root.path(), "changed").await;
        with_loupist_knowledge(&state, &changed, |tx, _| {
            let mut settings = crate::knowledge::load_settings(tx, &changed)?;
            let mut public = settings.public_settings();
            public.enrichment_enabled = false;
            settings.apply_public(&public, None);
            crate::knowledge::store_settings(tx, &changed, &settings)
        })
        .unwrap();
        let folder = root.path().join("opt-out");
        fs::create_dir(&folder).unwrap();
        let opted_out = create_folder_loupist_with_setup(
            &state,
            &folder,
            None,
            Some(NewLoupistWorldSettings {
                enabled: false,
                model: None,
            }),
        )
        .await
        .unwrap();
        connect_fixture(&state);
        complete_pending_world_models(&state).unwrap();
        let completed = load_graph_settings(&state, &pending).unwrap();
        assert!(completed.enrichment_enabled);
        assert!(!completed.awaiting_default_model);
        assert_eq!(completed.helper_model.as_deref(), Some("gpt-6-luna"));
        for id in [&unconfigured, &changed, &opted_out.id] {
            let settings = load_graph_settings(&state, id).unwrap();
            assert!(!settings.enrichment_enabled);
            assert!(settings.helper_model_profile_id.is_none());
        }
        // Later connections/refreshes cannot replace a resolved model.
        with_loupist_knowledge(&state, &pending, |tx, _| {
            let mut settings = crate::knowledge::load_settings(tx, &pending)?;
            settings.helper_model = Some("user-selected".into());
            crate::knowledge::store_settings(tx, &pending, &settings)
        })
        .unwrap();
        complete_pending_world_models(&state).unwrap();
        assert_eq!(
            load_graph_settings(&state, &pending)
                .unwrap()
                .helper_model
                .as_deref(),
            Some("user-selected")
        );
    }

    #[tokio::test]
    async fn selecting_a_pending_loupist_completes_only_that_loupist() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let selected = create(&state, root.path(), "selected").await;
        let other = create(&state, root.path(), "other").await;
        let unavailable = create(&state, root.path(), "unavailable").await;
        let before = load_graph_settings(&state, &other).unwrap();
        assert!(before.awaiting_default_model);
        fs::rename(root.path().join("unavailable"), root.path().join("moved")).unwrap();
        connect_fixture(&state);

        complete_pending_world_model(&state, &selected).unwrap();
        assert!(
            !load_graph_settings(&state, &selected)
                .unwrap()
                .awaiting_default_model
        );
        assert_eq!(load_graph_settings(&state, &other).unwrap(), before);

        // Provider setup may finish all available pending Loupists, while an
        // unavailable folder remains untouched until it is opened again.
        complete_pending_world_models(&state).unwrap();
        assert!(
            !load_graph_settings(&state, &other)
                .unwrap()
                .awaiting_default_model
        );
        fs::rename(root.path().join("moved"), root.path().join("unavailable")).unwrap();
        assert!(
            load_graph_settings(&state, &unavailable)
                .unwrap()
                .awaiting_default_model
        );
        complete_pending_world_model(&state, &unavailable).unwrap();
        assert!(
            !load_graph_settings(&state, &unavailable)
                .unwrap()
                .awaiting_default_model
        );
    }

    #[tokio::test]
    async fn new_world_defaults_validate_explicit_models_before_creating_a_loupist() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        connect_fixture(&state);
        let folder = root.path().join("explicit");
        fs::create_dir(&folder).unwrap();
        let selection = WorldModelSelection {
            model_profile_id: "anthropic/claude-haiku-4-5".into(),
            thinking_level: "off".into(),
            run_options: BTreeMap::new(),
        };
        let loupist = create_folder_loupist_with_setup(
            &state,
            &folder,
            Some("Energy suppliers".into()),
            Some(NewLoupistWorldSettings {
                enabled: true,
                model: Some(selection.clone()),
            }),
        )
        .await
        .unwrap();
        let settings = load_graph_settings(&state, &loupist.id).unwrap();
        assert_eq!(settings.helper_provider.as_deref(), Some("anthropic"));
        assert_eq!(settings.research_focus, "Energy suppliers");
        for (name, model) in [
            (
                "disconnected",
                WorldModelSelection {
                    model_profile_id: "google/gemini-flash".into(),
                    ..selection.clone()
                },
            ),
            (
                "bad-controls",
                WorldModelSelection {
                    thinking_level: "impossible".into(),
                    ..selection
                },
            ),
        ] {
            let folder = root.path().join(name);
            fs::create_dir(&folder).unwrap();
            assert!(create_folder_loupist_with_setup(
                &state,
                &folder,
                None,
                Some(NewLoupistWorldSettings {
                    enabled: true,
                    model: Some(model)
                })
            )
            .await
            .is_err());
            assert!(!folder.join(".loupist/loupist.sqlite3").exists());
        }
        assert_eq!(state.store.list_loupists().unwrap().len(), 1);
    }
}
