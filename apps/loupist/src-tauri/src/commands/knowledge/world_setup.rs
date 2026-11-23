use super::{admit_mutation, load_graph_settings, with_loupist_knowledge};
use crate::{
    app::{AppState, CommandError},
    knowledge::{WorldModel, WorldModelChoice},
    providers::world_defaults::WorldModels,
};

/// Point this Loupist's Recommended World model at what the bundled list
/// recommends with the accounts connected now. A Chosen model never changes.
pub(crate) fn follow_recommended_world_model(
    state: &AppState,
    loupist_id: &str,
) -> Result<(), CommandError> {
    if load_graph_settings(state, loupist_id)?.world_model_choice != WorldModelChoice::Recommended {
        return Ok(());
    }
    let recommended = WorldModels::load(state)?.recommend(Some(loupist_id));
    if follow(state, loupist_id, recommended.as_ref())? {
        state.graph.wake();
    }
    Ok(())
}

/// After accounts, models, visibility or the Chat default change, and at
/// startup, every Recommended Loupist follows the new recommendation.
pub(crate) fn follow_recommended_world_models(state: &AppState) -> Result<(), CommandError> {
    let models = WorldModels::load(state)?;
    let loupists = state
        .store
        .list_loupists()
        .map_err(crate::commands::map_store)?;
    let mut changed = false;
    for loupist in loupists {
        // An unavailable folder must not fail the change that prompted this;
        // opening that Loupist later follows the recommendation then.
        match follow(
            state,
            &loupist.id,
            models.recommend(Some(&loupist.id)).as_ref(),
        ) {
            Ok(followed) => changed |= followed,
            Err(_) => eprintln!(
                "Could not update the recommended World model for {}",
                loupist.id
            ),
        }
    }
    if changed {
        state.graph.wake();
    }
    Ok(())
}

/// For a change that has already succeeded: a failure here is logged, and the
/// next World job resolves the recommendation again before it runs.
pub(crate) fn follow_recommended_world_models_quietly(state: &AppState) {
    if follow_recommended_world_models(state).is_err() {
        eprintln!("Could not update the recommended World models");
    }
}

fn follow(
    state: &AppState,
    loupist_id: &str,
    recommended: Option<&WorldModel>,
) -> Result<bool, CommandError> {
    let settings = load_graph_settings(state, loupist_id)?;
    if settings.world_model_choice != WorldModelChoice::Recommended
        || settings.world_model().as_ref() == recommended
    {
        return Ok(false);
    }
    let _activity = admit_mutation(state, loupist_id)?;
    // The write rechecks the choice in its transaction, so a model the user
    // just chose wins.
    with_loupist_knowledge(state, loupist_id, |tx, blobs| {
        crate::knowledge::KnowledgeWrite::bind(tx, blobs).follow_recommended_world_model(
            loupist_id,
            recommended,
            crate::commands::now_ms(),
        )
    })
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
            if let Some(template) = template {
                crate::knowledge::apply_preset(
                    tx,
                    loupist_id,
                    template,
                    None,
                    crate::commands::now_ms(),
                )?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// A World model on a provider this build does not offer is forgotten, and the
/// Loupist goes back to Recommended (providers spec §3.3).
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
                settings.world_model_choice = WorldModelChoice::Recommended;
                settings.set_world_model(None);
                settings.fast_model = None;
                crate::knowledge::store_settings(tx, &loupist.id, &settings)
            })
        });
        if released.is_err() {
            eprintln!("Loupist could not reset a World model it no longer offers");
        }
    }
    if released_any {
        follow_recommended_world_models_quietly(state);
    }
}

/// Fix this Loupist's World model as if the user had chosen it.
#[cfg(test)]
pub(crate) fn set_world_model_for_test(state: &AppState, loupist_id: &str, profile_id: &str) {
    let (provider, model) = profile_id.split_once('/').unwrap();
    with_loupist_knowledge(state, loupist_id, |tx, _| {
        let mut settings = crate::knowledge::load_settings(tx, loupist_id)?;
        settings.enrichment_enabled = true;
        settings.world_model_choice = WorldModelChoice::Chosen;
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

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::{
        commands::loupist::{create_folder_loupist, create_folder_loupist_with_setup},
        knowledge::{GraphProcessingSettings, GraphProcessingView, GraphPublicSettings},
        providers::catalog::{ConfiguredModel, ModelCatalog},
        providers::world_defaults::{NewLoupistWorldSettings, WorldModelSelection},
        scoped_config::{ConfigScope, ConfigUpdateRequest},
    };
    use std::{collections::BTreeMap, fs, path::Path};

    use WorldModelChoice::{Chosen, Recommended};

    /// Sign in to exactly these accounts. Nothing else is told, as when an
    /// account changes between two World jobs.
    fn sign_in(state: &AppState, accounts: &[&str]) {
        let path = state.pi_agent_data_dir().unwrap().join("auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&path).unwrap();
        let auth = accounts
            .iter()
            .map(|account| {
                (
                    account.to_string(),
                    serde_json::json!({"type": "api_key", "key": "fixture-not-a-real-key"}),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        fs::write(&path, serde_json::to_vec(&auth).unwrap()).unwrap();
    }

    fn offer(state: &AppState, models: &[(&str, &str)]) {
        let models = models
            .iter()
            .map(|(provider, model)| ConfiguredModel {
                id: format!("{provider}/{model}"),
                name: model.to_string(),
                provider: provider.to_string(),
                model: model.to_string(),
                input: vec!["text".into()],
                reasoning: true,
                context_window: 128_000,
                max_tokens: 16_000,
                thinking_levels: vec!["off".into(), "low".into(), "medium".into(), "high".into()],
                thinking_level_map: BTreeMap::new(),
                run_controls: vec![],
            })
            .collect();
        state.set_model_catalog(ModelCatalog { models }).unwrap();
    }

    pub(crate) fn connect_fixture(state: &AppState) {
        sign_in(state, &["openai", "anthropic"]);
        offer(
            state,
            &[("openai", "gpt-6-luna"), ("anthropic", "claude-haiku-4-5")],
        );
    }

    async fn create(state: &AppState, root: &Path, name: &str) -> String {
        let folder = root.join(name);
        fs::create_dir(&folder).unwrap();
        create_folder_loupist(state, &folder).await.unwrap().id
    }

    fn settings(state: &AppState, loupist_id: &str) -> GraphProcessingSettings {
        load_graph_settings(state, loupist_id).unwrap()
    }

    fn world_model(state: &AppState, loupist_id: &str) -> (WorldModelChoice, Option<String>) {
        let settings = settings(state, loupist_id);
        (
            settings.world_model_choice,
            settings.helper_model_profile_id,
        )
    }

    fn save(
        state: &AppState,
        loupist_id: &str,
        edit: impl FnOnce(&mut GraphPublicSettings),
    ) -> GraphProcessingView {
        let mut public = settings(state, loupist_id).public_settings();
        edit(&mut public);
        super::super::set_graph_settings(
            state,
            super::super::KnowledgeGraphSettingsRequest {
                loupist_id: loupist_id.into(),
                settings: public,
            },
        )
        .unwrap()
    }

    fn choose(public: &mut GraphPublicSettings, profile_id: &str, level: &str) {
        public.world_model_choice = Chosen;
        public.helper_model_profile_id = Some(profile_id.into());
        public.thinking_level = level.into();
        public.run_options = BTreeMap::new();
    }

    fn note(state: &AppState, loupist_id: &str) -> (serde_json::Value, serde_json::Value) {
        let view =
            super::super::graph_settings_view(state, loupist_id, &settings(state, loupist_id))
                .unwrap();
        let readiness = serde_json::to_value(view.readiness).unwrap();
        assert!(readiness.get("message").is_none(), "{readiness}");
        (readiness["status"].clone(), readiness["note"].clone())
    }

    fn json(status: &str, note: &str) -> (serde_json::Value, serde_json::Value) {
        (serde_json::json!(status), serde_json::json!(note))
    }

    fn set_chat_default(state: &AppState, loupist_id: Option<&str>, profile_id: &str) {
        let root = &state.artifacts.app_data_dir;
        let directory =
            loupist_id.map(|id| crate::commands::loupist_config_directory(state, id).unwrap());
        let scope = loupist_id.map_or(ConfigScope::Global, |id| ConfigScope::Project {
            loupist_id: id.into(),
        });
        let snapshot =
            crate::scoped_config::read_scope(root, directory.as_deref(), scope.clone()).unwrap();
        crate::scoped_config::update_scope(
            root,
            directory.as_deref(),
            &ConfigUpdateRequest {
                scope,
                expected_revision: snapshot.revision,
                set: serde_json::from_value(
                    serde_json::json!({"chat": {"model_profile_id": profile_id}}),
                )
                .unwrap(),
                unset: vec![],
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn a_new_loupist_is_recommended_and_a_chosen_model_survives_reopening() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        connect_fixture(&state);
        let loupist_id = create(&state, root.path(), "research").await;
        let created = settings(&state, &loupist_id);
        assert!(created.enrichment_enabled);
        assert_eq!(created.world_model_choice, Recommended);
        assert_eq!(created.helper_model.as_deref(), Some("gpt-6-luna"));
        assert_eq!(created.thinking_level, "high");
        let view = save(&state, &loupist_id, |public| {
            choose(public, "anthropic/claude-haiku-4-5", "low");
            public.enrichment_enabled = false;
            public.paused = true;
            public.research_focus = "Supplier concentration".into();
            public.max_llm_calls = 3;
        });
        // The recommendation stays on offer beside the chosen model.
        assert_eq!(
            view.recommended.unwrap().model_profile_id,
            "openai/gpt-6-luna"
        );
        let expected = settings(&state, &loupist_id);
        assert_eq!(
            (
                expected.world_model_choice,
                expected.thinking_level.as_str()
            ),
            (Chosen, "low")
        );
        // Saving woke the World loop; the folder is free once it stops.
        state.graph.shutdown().await.unwrap();
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
        follow_recommended_world_models(&reopened).unwrap();
        assert_eq!(settings(&reopened, &loupist_id), expected);
    }

    #[tokio::test]
    async fn a_recommended_loupist_waits_for_an_account_and_follows_the_one_that_connects() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let pending = create(&state, root.path(), "pending").await;
        let waiting = settings(&state, &pending);
        assert!(waiting.enrichment_enabled);
        assert_eq!(world_model(&state, &pending), (Recommended, None));
        assert_eq!(
            note(&state, &pending),
            json("model_required", "awaiting_provider")
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
        follow_recommended_world_models(&state).unwrap();
        assert_eq!(
            world_model(&state, &pending),
            (Recommended, Some("openai/gpt-6-luna".into()))
        );
        // Turned off stays off; the recommendation is ready for when it is on.
        for id in [&unconfigured, &opted_out.id] {
            let settings = settings(&state, id);
            assert!(!settings.enrichment_enabled);
            assert_eq!(settings.world_model_choice, Recommended);
        }
        // A model the user chose does not follow later connections.
        save(&state, &pending, |public| {
            choose(public, "anthropic/claude-haiku-4-5", "low")
        });
        sign_in(&state, &["anthropic"]);
        follow_recommended_world_models(&state).unwrap();
        sign_in(&state, &["openai", "anthropic"]);
        follow_recommended_world_models(&state).unwrap();
        assert_eq!(
            world_model(&state, &pending),
            (Chosen, Some("anthropic/claude-haiku-4-5".into()))
        );
    }

    #[tokio::test]
    async fn selecting_a_waiting_loupist_follows_only_that_loupist() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let selected = create(&state, root.path(), "selected").await;
        let other = create(&state, root.path(), "other").await;
        let unavailable = create(&state, root.path(), "unavailable").await;
        let before = settings(&state, &other);
        fs::rename(root.path().join("unavailable"), root.path().join("moved")).unwrap();
        connect_fixture(&state);

        follow_recommended_world_model(&state, &selected).unwrap();
        assert!(world_model(&state, &selected).1.is_some());
        assert_eq!(settings(&state, &other), before);

        // Provider setup updates every available Loupist, while an unavailable
        // folder stays untouched until it is opened again.
        follow_recommended_world_models(&state).unwrap();
        assert!(world_model(&state, &other).1.is_some());
        fs::rename(root.path().join("moved"), root.path().join("unavailable")).unwrap();
        assert!(world_model(&state, &unavailable).1.is_none());
        follow_recommended_world_model(&state, &unavailable).unwrap();
        assert!(world_model(&state, &unavailable).1.is_some());
    }

    #[tokio::test]
    async fn an_explicit_new_loupist_model_is_chosen_and_validated_before_the_loupist_exists() {
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
        let created = settings(&state, &loupist.id);
        assert_eq!(created.world_model_choice, Chosen);
        assert_eq!(created.helper_provider.as_deref(), Some("anthropic"));
        assert_eq!(created.thinking_level, "off");
        assert_eq!(created.research_focus, "Energy suppliers");
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

    #[tokio::test]
    async fn two_accounts_follow_the_list_with_the_chat_default_provider_first() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        sign_in(&state, &["anthropic", "openai"]);
        offer(
            &state,
            &[("anthropic", "claude-haiku-5-5"), ("openai", "gpt-6-luna")],
        );
        let first = create(&state, root.path(), "first").await;
        let second = create(&state, root.path(), "second").await;
        // Chat on Auto picks an OpenAI model here, so OpenAI goes first. (With
        // no Chat preference the list's order also puts OpenAI first; the
        // resolver's own tests cover both catalog orders.)
        assert_eq!(
            world_model(&state, &first).1.as_deref(),
            Some("openai/gpt-6-luna")
        );
        // The app's Chat default names Anthropic: World uses that account.
        set_chat_default(&state, None, "anthropic/claude-haiku-5-5");
        follow_recommended_world_models(&state).unwrap();
        for id in [&first, &second] {
            let saved = settings(&state, id);
            assert_eq!(
                saved.helper_model_profile_id.as_deref(),
                Some("anthropic/claude-haiku-5-5")
            );
            assert_eq!(saved.thinking_level, "medium");
        }
        // A Loupist's own Chat default comes before the app's.
        set_chat_default(&state, Some(&second), "openai/gpt-6-luna");
        follow_recommended_world_models(&state).unwrap();
        assert_eq!(
            world_model(&state, &first).1.as_deref(),
            Some("anthropic/claude-haiku-5-5")
        );
        assert_eq!(
            world_model(&state, &second).1.as_deref(),
            Some("openai/gpt-6-luna")
        );
    }

    #[tokio::test]
    async fn the_next_world_job_follows_an_account_change_and_a_chosen_model_never_does() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        connect_fixture(&state);
        let recommended = create(&state, root.path(), "recommended").await;
        let chosen = create(&state, root.path(), "chosen").await;
        set_world_model_for_test(&state, &chosen, "openai/gpt-6-luna");
        let job = |id: &str| {
            state
                .graph
                .settings_for_work(id)
                .unwrap()
                .helper_model_profile_id
        };
        assert_eq!(job(&recommended).as_deref(), Some("openai/gpt-6-luna"));
        // OpenAI is signed out between two jobs, and nothing else ran.
        sign_in(&state, &["anthropic"]);
        assert_eq!(
            job(&recommended).as_deref(),
            Some("anthropic/claude-haiku-4-5")
        );
        assert_eq!(
            world_model(&state, &recommended),
            (Recommended, Some("anthropic/claude-haiku-4-5".into()))
        );
        assert_eq!(job(&chosen).as_deref(), Some("openai/gpt-6-luna"));
        // No account at all: the Recommended Loupist waits, with today's wording.
        sign_in(&state, &[]);
        assert_eq!(job(&recommended), None);
        assert_eq!(
            note(&state, &recommended),
            json("model_required", "awaiting_provider")
        );
        sign_in(&state, &["openai", "anthropic"]);
        assert_eq!(job(&recommended).as_deref(), Some("openai/gpt-6-luna"));
        assert_eq!(
            world_model(&state, &chosen),
            (Chosen, Some("openai/gpt-6-luna".into()))
        );
    }

    #[tokio::test]
    async fn a_hidden_model_moves_a_recommended_loupist_to_the_next_listed_one() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        connect_fixture(&state);
        let loupist_id = create(&state, root.path(), "research").await;
        state.set_model_visible("openai/gpt-6-luna", false).unwrap();
        follow_recommended_world_models(&state).unwrap();
        assert_eq!(
            world_model(&state, &loupist_id),
            (Recommended, Some("anthropic/claude-haiku-4-5".into()))
        );
        state.set_model_visible("openai/gpt-6-luna", true).unwrap();
        follow_recommended_world_models(&state).unwrap();
        assert_eq!(
            world_model(&state, &loupist_id).1.as_deref(),
            Some("openai/gpt-6-luna")
        );
    }

    #[tokio::test]
    async fn a_chosen_model_that_disappears_asks_for_another_and_can_go_back_to_recommended() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        connect_fixture(&state);
        let loupist_id = create(&state, root.path(), "research").await;
        save(&state, &loupist_id, |public| {
            choose(public, "anthropic/claude-haiku-4-5", "low")
        });
        state.replace_provider_models("anthropic", vec![]).unwrap();
        follow_recommended_world_models(&state).unwrap();
        assert_eq!(
            world_model(&state, &loupist_id),
            (Chosen, Some("anthropic/claude-haiku-4-5".into()))
        );
        assert_eq!(
            note(&state, &loupist_id),
            json("model_unavailable", "model_unavailable")
        );
        let view = super::super::graph_settings(&state, &loupist_id).unwrap();
        let offered = view.recommended.unwrap();
        assert_eq!(
            (
                offered.model_profile_id.as_str(),
                offered.thinking_level.as_str()
            ),
            ("openai/gpt-6-luna", "high")
        );
        // Recommended ignores the model fields sent with it.
        let view = save(&state, &loupist_id, |public| {
            public.world_model_choice = Recommended;
            public.thinking_level = "off".into();
        });
        assert_eq!(view.settings.world_model_choice, Recommended);
        assert_eq!(
            view.settings.helper_model_profile_id.as_deref(),
            Some("openai/gpt-6-luna")
        );
        assert_eq!(view.settings.thinking_level, "high");
        assert_ne!(
            note(&state, &loupist_id).0,
            serde_json::json!("model_unavailable")
        );
    }

    #[tokio::test]
    async fn a_loupist_made_under_one_provider_follows_the_account_used_now() {
        // The owner's profile: World was saved for xAI by an earlier build, and
        // only OpenAI is connected now.
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        sign_in(&state, &["openai"]);
        offer(&state, &[("openai", "gpt-6-luna")]);
        let listed = create(&state, root.path(), "listed").await;
        let edited = create(&state, root.path(), "edited").await;
        for (id, level) in [(&listed, "medium"), (&edited, "high")] {
            let mut row = serde_json::to_value(settings(&state, id)).unwrap();
            let row = row.as_object_mut().unwrap();
            row.remove("worldModelChoice");
            row.insert("awaitingDefaultModel".into(), false.into());
            row.insert("helperProvider".into(), "xai".into());
            row.insert("helperModel".into(), "grok-4.3".into());
            row.insert("helperModelProfileId".into(), "xai/grok-4.3".into());
            row.insert("thinkingLevel".into(), level.into());
            with_loupist_knowledge(&state, id, |tx, _| {
                tx.execute(
                    "UPDATE app_settings SET data_json = ?2 WHERE id = ?1",
                    [
                        format!("world_processing:{id}"),
                        serde_json::to_string(&row)?,
                    ],
                )?;
                Ok(())
            })
            .unwrap();
        }
        assert_eq!(
            world_model(&state, &listed),
            (Recommended, Some("xai/grok-4.3".into()))
        );
        // A changed effort means the user picked it.
        assert_eq!(
            world_model(&state, &edited),
            (Chosen, Some("xai/grok-4.3".into()))
        );
        follow_recommended_world_models(&state).unwrap();
        assert_eq!(
            world_model(&state, &listed),
            (Recommended, Some("openai/gpt-6-luna".into()))
        );
        assert_eq!(
            world_model(&state, &edited),
            (Chosen, Some("xai/grok-4.3".into()))
        );
        assert_eq!(
            note(&state, &edited),
            json("model_unavailable", "model_unavailable")
        );
    }

    #[tokio::test]
    async fn world_readiness_names_its_note_by_code_for_the_interface_to_word() {
        let root = tempfile::tempdir().unwrap();
        let state = AppState::for_folder_test(root.path().join("app"));
        let loupist_id = create(&state, root.path(), "waiting").await;
        assert_eq!(
            note(&state, &loupist_id),
            json("model_required", "awaiting_provider")
        );
        let world_model = |model: Option<(&str, &str)>| {
            with_loupist_knowledge(&state, &loupist_id, |tx, _| {
                let mut settings = crate::knowledge::load_settings(tx, &loupist_id)?;
                settings.world_model_choice = Chosen;
                settings.helper_provider = model.map(|(provider, _)| provider.to_owned());
                settings.helper_model = model.map(|(_, model)| model.to_owned());
                settings.helper_model_profile_id =
                    model.map(|(provider, model)| format!("{provider}/{model}"));
                crate::knowledge::store_settings(tx, &loupist_id, &settings)
            })
            .unwrap();
        };
        // With no AI account at all, World waits for one, whatever its model setting.
        world_model(None);
        assert_eq!(
            note(&state, &loupist_id),
            json("model_required", "no_account")
        );
        world_model(Some(("xai", "grok-4.3")));
        assert_eq!(
            note(&state, &loupist_id),
            json("model_unavailable", "no_account")
        );
        // Once accounts exist, a model outside them is a model to change.
        connect_fixture(&state);
        assert_eq!(
            note(&state, &loupist_id),
            json("model_unavailable", "model_unavailable")
        );
        world_model(None);
        assert_eq!(
            note(&state, &loupist_id),
            json("model_required", "choose_model")
        );
        assert_eq!(
            serde_json::to_value(crate::knowledge::ReadinessNote::Provider(
                crate::providers::inference::UnreadyReason::AccountWaiting
            ))
            .unwrap(),
            "account_waiting"
        );
    }
}
