//! The bundled World model list and what it recommends.
//!
//! A Loupist's World model is Recommended until the user picks one in Settings
//! (Chosen). Recommended resolves whenever the host uses it, never once at
//! creation: the first model in the list, in its provider and model order,
//! that a connected account offers with the listed effort, trying the provider
//! of the Loupist's Chat default first. The list holds reviewed choices only:
//! Loupist never guesses a model ID or picks an arbitrary (possibly specialist
//! or expensive) model from a provider catalog.
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use serde::{Deserialize, Serialize};

use crate::{
    app::{AppState, CommandError},
    knowledge::{GraphProcessingSettings, WorldModel, WorldModelChoice},
    providers::catalog::{
        catalog_view, ConfiguredModelView, CredentialSource, ModelCatalog, ModelCatalogView,
        ModelVisibility,
    },
    providers::inference::supports_isolated_inference,
    providers::manifest::development_alias,
    store::SqliteStore,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderDefault {
    provider: String,
    models: Vec<ModelDefault>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelDefault {
    id: String,
    thinking_level: String,
}

static DEFAULTS: LazyLock<Vec<ProviderDefault>> = LazyLock::new(|| {
    let json = include_str!("../../../world-model-defaults.json");
    if let Err(problem) = super::world_model_list::check(json) {
        panic!("bundled World model list: {problem}");
    }
    serde_json::from_str(json).expect("bundled World model list")
});

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EarlierChoice {
    provider: String,
    id: String,
    thinking_level: String,
}

/// Every model and effort a bundled list named before Loupists recorded their
/// choice. Frozen: later lists never change how an older row reads.
static EARLIER: LazyLock<Vec<EarlierChoice>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("world_defaults_earlier.json"))
        .expect("earlier World model lists")
});

/// Whether a build before the Recommended/Chosen record could have saved this
/// model for a new Loupist on its own.
pub(crate) fn an_earlier_list_chose(provider: &str, model: &str, thinking_level: &str) -> bool {
    EARLIER.iter().any(|choice| {
        (choice.provider == provider || development_alias(&choice.provider) == Some(provider))
            && choice.id == model
            && choice.thinking_level == thinking_level
    })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldModelSelection {
    pub model_profile_id: String,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewLoupistWorldSettings {
    pub enabled: bool,
    /// A model fixes the new Loupist's choice; none leaves it Recommended.
    pub model: Option<WorldModelSelection>,
}

/// The models the connected accounts offer World building.
struct Offered {
    view: ModelCatalogView,
}

impl Offered {
    /// Isolated inference, text input and a connected account. A model hidden
    /// from menus may still be chosen, but is never recommended.
    fn usable(&self, model: &ConfiguredModelView) -> bool {
        supports_isolated_inference(&model.provider)
            && model.input.iter().any(|input| input == "text")
            && model.credential_source != CredentialSource::Missing
            && self.view.providers.iter().any(|provider| {
                provider.id == model.provider
                    && provider.credential_source != CredentialSource::Missing
            })
    }

    fn hidden(&self, model: &ConfiguredModelView) -> bool {
        self.view
            .hidden_model_profile_ids
            .iter()
            .any(|hidden| hidden == &model.id)
    }

    /// `chat_provider` is the provider of the Chat default, the account the
    /// user evidently uses; the list's own order decides among the rest.
    fn recommend(&self, chat_provider: Option<&str>) -> Option<WorldModel> {
        let preferred = DEFAULTS.iter().filter(|listed| {
            chat_provider.is_some_and(|chat| {
                listed.provider == chat || development_alias(&listed.provider) == Some(chat)
            })
        });
        preferred
            .chain(DEFAULTS.iter())
            .find_map(|listed| self.first_offered(listed))
    }

    fn first_offered(&self, listed: &ProviderDefault) -> Option<WorldModel> {
        listed.models.iter().find_map(|candidate| {
            let offered = |provider: &str| {
                self.view.models.iter().find(|model| {
                    model.provider == provider
                        && model.model == candidate.id
                        && model.thinking_levels.contains(&candidate.thinking_level)
                        && self.usable(model)
                        && !self.hidden(model)
                })
            };
            // Debug builds keep the legacy Codex client for harnesses; it serves
            // the same reviewed OpenAI models, used only when no `openai`
            // sign-in or key matches.
            offered(&listed.provider)
                .or_else(|| development_alias(&listed.provider).and_then(offered))
                .map(|model| WorldModel {
                    profile_id: model.id.clone(),
                    provider: model.provider.clone(),
                    model: model.model.clone(),
                    thinking_level: candidate.thinking_level.clone(),
                    run_options: model
                        .run_controls
                        .iter()
                        .map(|control| (control.id.clone(), control.default_choice.clone()))
                        .collect(),
                })
        })
    }
}

/// One catalog, credential and visibility snapshot for World model choices.
pub(crate) struct WorldModels {
    catalog: ModelCatalog,
    offered: Offered,
    /// The Chat default's provider where a Loupist names no Chat model of its own.
    chat_auto_provider: Option<String>,
    app_data_dir: PathBuf,
    store: Arc<SqliteStore>,
}

impl WorldModels {
    pub(crate) fn load(state: &AppState) -> Result<Self, CommandError> {
        Self::from_parts(
            state.model_catalog()?,
            &state.pi_agent_data_dir()?,
            &state.artifacts.app_data_dir,
            state.store.clone(),
        )
    }

    pub(crate) fn from_parts(
        catalog: ModelCatalog,
        agent_data_dir: &Path,
        app_data_dir: &Path,
        store: Arc<SqliteStore>,
    ) -> Result<Self, CommandError> {
        let visibility = ModelVisibility {
            hidden_model_profile_ids: crate::scoped_config::read_hidden_model_profile_ids(
                app_data_dir,
            )?
            .into_iter()
            .collect(),
        };
        let view = catalog_view(&catalog, agent_data_dir, &visibility)?;
        let statuses = crate::providers::status::rows(&store);
        let chat_auto_provider = crate::providers::auto::resolve(&view, &catalog, &statuses)
            .map(|choice| choice.model.provider);
        Ok(Self {
            catalog,
            offered: Offered { view },
            chat_auto_provider,
            app_data_dir: app_data_dir.to_owned(),
            store,
        })
    }

    /// What the list recommends now for a Loupist, or for a new one (None).
    pub(crate) fn recommend(&self, loupist_id: Option<&str>) -> Option<WorldModel> {
        self.offered
            .recommend(self.chat_provider(loupist_id).as_deref())
    }

    /// The provider of the model this Loupist's new chats use.
    fn chat_provider(&self, loupist_id: Option<&str>) -> Option<String> {
        let directory = loupist_id.and_then(|id| {
            let profile = self.store.get_loupist(id).ok()??;
            let root = crate::fs::loupist_root::profile_workspace(&profile).ok()?;
            Some(root.path().join(".loupist"))
        });
        crate::scoped_config::resolve(&self.app_data_dir, directory.as_deref())
            .ok()
            .and_then(|config| config.chat.model_profile_id)
            .filter(|id| !crate::providers::auto::uses_default(id))
            .and_then(|id| {
                let view = &self.offered.view;
                view.models
                    .iter()
                    .find(|model| {
                        model.id == id
                            && model.credential_source != CredentialSource::Missing
                            && !self.offered.hidden(model)
                    })
                    .map(|model| model.provider.clone())
            })
            .or_else(|| self.chat_auto_provider.clone())
    }

    /// The model the user picked, once a connected account offers it with
    /// these controls.
    pub(crate) fn chosen(
        &self,
        selection: &WorldModelSelection,
    ) -> Result<WorldModel, CommandError> {
        if !self
            .offered
            .view
            .models
            .iter()
            .any(|model| model.id == selection.model_profile_id && self.offered.usable(model))
        {
            return Err(CommandError::invalid(
                "The World model is no longer connected or supported. Choose another model.",
            ));
        }
        let model = self.catalog.resolve(&selection.model_profile_id)?;
        model.validate_thinking_level(&selection.thinking_level)?;
        model.validate_run_options(&selection.run_options)?;
        Ok(WorldModel {
            profile_id: model.id,
            provider: model.provider,
            model: model.model,
            thinking_level: selection.thinking_level.clone(),
            run_options: selection.run_options.clone(),
        })
    }
}

pub fn new_settings(
    state: &AppState,
    request: Option<&NewLoupistWorldSettings>,
    research_focus: Option<&str>,
) -> Result<GraphProcessingSettings, CommandError> {
    let models = WorldModels::load(state)?;
    let mut settings = GraphProcessingSettings {
        enrichment_enabled: request.is_none_or(|request| request.enabled),
        research_focus: research_focus.unwrap_or_default().trim().to_owned(),
        ..Default::default()
    };
    match request.and_then(|request| request.model.as_ref()) {
        Some(selection) => {
            settings.world_model_choice = WorldModelChoice::Chosen;
            settings.set_world_model(Some(&models.chosen(selection)?));
        }
        None => settings.set_world_model(models.recommend(None).as_ref()),
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::catalog::{provider_options, ModelRunControl, ModelRunControlChoice};

    fn model(provider: &str, id: &str) -> ConfiguredModelView {
        ConfiguredModelView {
            id: format!("{provider}/{id}"),
            name: id.into(),
            provider: provider.into(),
            model: id.into(),
            input: vec!["text".into()],
            reasoning: true,
            context_window: 128_000,
            max_tokens: 16_000,
            thinking_levels: vec!["off".into(), "high".into(), "low".into(), "medium".into()],
            thinking_level_map: BTreeMap::new(),
            credential_source: CredentialSource::Saved,
            run_controls: vec![ModelRunControl {
                id: "performance".into(),
                label: "Performance".into(),
                default_choice: "standard".into(),
                choices: vec![ModelRunControlChoice {
                    id: "standard".into(),
                    label: "Standard".into(),
                    description: "Standard service".into(),
                }],
            }],
        }
    }

    fn catalog(models: Vec<ConfiguredModelView>) -> ModelCatalogView {
        let mut providers = provider_options();
        for provider in &mut providers {
            provider.credential_source = CredentialSource::Saved;
        }
        ModelCatalogView {
            default_selection: None,
            models,
            providers,
            custom_providers: vec![],
            hidden_model_profile_ids: vec![],
            model_notice: None,
        }
    }

    fn offered(models: Vec<ConfiguredModelView>) -> Offered {
        Offered {
            view: catalog(models),
        }
    }

    fn recommended(models: Vec<ConfiguredModelView>, chat: Option<&str>) -> Option<String> {
        offered(models)
            .recommend(chat)
            .map(|model| model.profile_id)
    }

    fn usable(offered: &Offered) -> usize {
        offered
            .view
            .models
            .iter()
            .filter(|model| offered.usable(model))
            .count()
    }

    #[test]
    fn one_account_gets_its_first_listed_model_with_the_listed_effort_and_standard_service() {
        let selection = offered(vec![model("openai", "gpt-6-luna")])
            .recommend(None)
            .unwrap();
        assert_eq!(selection.profile_id, "openai/gpt-6-luna");
        assert_eq!(
            (selection.provider.as_str(), selection.model.as_str()),
            ("openai", "gpt-6-luna")
        );
        assert_eq!(selection.thinking_level, "high");
        assert_eq!(selection.run_options["performance"], "standard");
    }

    #[test]
    fn two_accounts_follow_the_list_order_whichever_order_the_catalog_has() {
        let mut models = vec![
            model("anthropic", "claude-haiku-5-5"),
            model("openai", "gpt-6-luna"),
            model("xai", "grok-4.7"),
        ];
        for _ in 0..models.len() {
            assert_eq!(
                recommended(models.clone(), None).as_deref(),
                Some("openai/gpt-6-luna")
            );
            models.rotate_left(1);
        }
        // Without OpenAI, the next listed provider: Anthropic before xAI.
        for models in [
            vec![
                model("xai", "grok-4.7"),
                model("anthropic", "claude-haiku-5-5"),
            ],
            vec![
                model("anthropic", "claude-haiku-5-5"),
                model("xai", "grok-4.7"),
            ],
        ] {
            assert_eq!(
                recommended(models, None).as_deref(),
                Some("anthropic/claude-haiku-5-5")
            );
        }
    }

    #[test]
    fn the_chat_default_provider_goes_first_and_the_list_decides_the_rest() {
        let both = || {
            vec![
                model("openai", "gpt-6-luna"),
                model("anthropic", "claude-haiku-5-5"),
            ]
        };
        assert_eq!(
            recommended(both(), Some("anthropic")).as_deref(),
            Some("anthropic/claude-haiku-5-5")
        );
        assert_eq!(
            recommended(both(), Some("openai")).as_deref(),
            Some("openai/gpt-6-luna")
        );
        // A Chat provider the list does not name, or that offers no listed
        // model, leaves the list order.
        for chat in ["google", "xai"] {
            assert_eq!(
                recommended(both(), Some(chat)).as_deref(),
                Some("openai/gpt-6-luna"),
                "{chat}"
            );
        }
    }

    #[test]
    fn no_connected_account_has_no_recommendation() {
        assert!(recommended(vec![], None).is_none());
        let mut disconnected = model("openai", "gpt-6-luna");
        disconnected.credential_source = CredentialSource::Missing;
        assert!(recommended(vec![disconnected], Some("openai")).is_none());
        let mut signed_out = offered(vec![model("anthropic", "claude-haiku-5-5")]);
        for provider in &mut signed_out.view.providers {
            provider.credential_source = CredentialSource::Missing;
        }
        assert!(signed_out.recommend(None).is_none());
    }

    #[test]
    fn a_hidden_model_is_never_recommended_but_stays_choosable() {
        let mut offered = offered(vec![
            model("openai", "gpt-5.6-luna"),
            model("openai", "gpt-6-luna"),
        ]);
        offered.view.hidden_model_profile_ids = vec!["openai/gpt-6-luna".into()];
        assert_eq!(
            offered.recommend(None).unwrap().profile_id,
            "openai/gpt-5.6-luna"
        );
        assert_eq!(usable(&offered), 2);
        offered
            .view
            .hidden_model_profile_ids
            .push("openai/gpt-5.6-luna".into());
        assert!(offered.recommend(None).is_none());
    }

    #[test]
    fn world_defaults_require_the_reviewed_model_and_effort_pair() {
        let mut luna = model("openai", "gpt-6-luna");
        luna.thinking_levels = vec!["off".into(), "max".into()];
        let selection = offered(vec![luna, model("openai", "gpt-5.4-mini")])
            .recommend(None)
            .unwrap();
        assert_eq!(selection.profile_id, "openai/gpt-5.4-mini");
        assert_eq!(selection.thinking_level, "high");
        for (provider, id, effort) in [
            ("anthropic", "claude-haiku-5-5", "medium"),
            ("anthropic", "claude-haiku-4-5", "low"),
            ("anthropic", "claude-sonnet-5-5", "low"),
            ("deepseek", "deepseek-flash", "low"),
            ("groq", "openai/gpt-oss-120b", "medium"),
            ("minimax", "MiniMax-M3.1-Flash", "low"),
            ("minimax", "MiniMax-M3", "off"),
            ("kimi-coding", "kimi-for-coding", "low"),
        ] {
            let selection = offered(vec![model(provider, id)]).recommend(None).unwrap();
            assert_eq!(selection.thinking_level, effort);
        }
    }

    #[test]
    fn a_provider_prefers_its_newest_reviewed_model_and_keeps_older_ones_as_fallbacks() {
        for (provider, newest, older) in [
            ("openai", "gpt-6.1-terra", "gpt-5.6-terra"),
            ("anthropic", "claude-haiku-5-5", "claude-haiku-4-5"),
            ("anthropic", "claude-sonnet-5-5", "claude-sonnet-4-6"),
            ("minimax", "MiniMax-M3.1-Flash", "MiniMax-M3"),
            (
                "baseten",
                "deepseek-ai/DeepSeek-V4.1-Flash-1017",
                "deepseek-ai/DeepSeek-V4.1-Flash",
            ),
        ] {
            assert_eq!(
                recommended(vec![model(provider, older), model(provider, newest)], None),
                Some(format!("{provider}/{newest}"))
            );
            assert_eq!(
                recommended(vec![model(provider, older)], None),
                Some(format!("{provider}/{older}"))
            );
        }
    }

    #[test]
    fn retiring_models_are_never_recommended() {
        for (provider, id) in [
            ("openai", "gpt-5-mini"),
            ("openai", "gpt-4.1-mini"),
            ("anthropic", "claude-sonnet-4-5"),
            ("groq", "llama-3.3-70b-versatile"),
        ] {
            let offered = offered(vec![model(provider, id)]);
            assert!(offered.recommend(None).is_none(), "{provider}/{id}");
            assert_eq!(usable(&offered), 1, "manual selection remains available");
        }
    }

    #[test]
    fn debug_builds_give_the_codex_client_the_openai_world_default() {
        let selection = recommended(vec![model("openai-codex", "gpt-6-luna")], None);
        if cfg!(debug_assertions) {
            assert_eq!(selection.as_deref(), Some("openai-codex/gpt-6-luna"));
        } else {
            assert!(selection.is_none());
        }
        // The debug-only Codex client never outranks an `openai` connection.
        assert_eq!(
            recommended(
                vec![
                    model("openai-codex", "gpt-6-luna"),
                    model("openai", "gpt-6-luna")
                ],
                Some("openai-codex")
            )
            .as_deref(),
            Some("openai/gpt-6-luna")
        );
    }

    #[test]
    fn world_defaults_filter_disconnected_unsupported_and_nontext_models() {
        let mut disconnected = model("openai-codex", "gpt-6-luna");
        disconnected.credential_source = CredentialSource::Missing;
        let mut image = model("openai", "gpt-6-luna");
        image.input = vec!["image".into()];
        let mut offered = offered(vec![
            disconnected,
            image,
            model("google", "gemini-flash"),
            model("anthropic", "claude-haiku-4-5"),
            model("deepseek", "deepseek-flash"),
        ]);
        offered
            .view
            .providers
            .iter_mut()
            .find(|provider| provider.id == "anthropic")
            .unwrap()
            .credential_source = CredentialSource::Missing;
        assert_eq!(usable(&offered), 1);
        assert_eq!(
            offered.recommend(Some("anthropic")).unwrap().profile_id,
            "deepseek/deepseek-flash"
        );
    }

    #[test]
    fn world_defaults_use_available_fallbacks_without_guessing_or_escalating_to_a_flagship() {
        assert_eq!(
            recommended(
                vec![
                    model("openai", "gpt-6-astra"),
                    model("openai", "gpt-5.4-mini")
                ],
                None
            )
            .as_deref(),
            Some("openai/gpt-5.4-mini")
        );
        let astra = offered(vec![model("openai", "gpt-6-astra")]);
        assert!(astra.recommend(None).is_none());
        assert_eq!(usable(&astra), 1, "manual selection remains available");
    }

    #[test]
    fn what_every_earlier_list_saved_reads_as_recommended_and_other_models_as_chosen() {
        // Each version's first choice per provider (git log -p -- world-model-defaults.json),
        // and later entries a Loupist got when the first was not offered.
        for (provider, model, effort) in [
            ("openai", "gpt-6-luna", "high"),
            ("anthropic", "claude-haiku-4-5", "low"),
            ("anthropic", "claude-haiku-5-5", "medium"),
            ("deepseek", "deepseek-flash", "low"),
            ("xai", "grok-4.3", "medium"),
            ("xai", "grok-4.7", "medium"),
            ("groq", "openai/gpt-oss-120b", "medium"),
            ("cerebras", "gpt-oss-120b", "medium"),
            ("baseten", "openai/gpt-oss-120b", "medium"),
            ("together", "openai/gpt-oss-120b", "medium"),
            ("minimax", "MiniMax-M3", "off"),
            ("minimax", "MiniMax-M3.1-Flash", "low"),
            ("kimi-coding", "kimi-for-coding", "low"),
            ("openai", "gpt-5.4-mini", "high"),
            ("openai", "gpt-4.1-mini", "off"),
            ("anthropic", "claude-sonnet-4-5", "low"),
            ("groq", "llama-3.3-70b-versatile", "off"),
            ("together", "meta-llama/Llama-3.3-70B-Instruct-Turbo", "off"),
            ("baseten", "deepseek-ai/DeepSeek-V4.1-Flash", "low"),
            ("baseten", "deepseek-ai/DeepSeek-V4.1-Flash", "high"),
        ] {
            assert!(
                an_earlier_list_chose(provider, model, effort),
                "{provider}/{model} {effort}"
            );
        }
        for (provider, model, effort) in [
            ("openai", "gpt-6-luna", "medium"),
            ("openai", "gpt-6-astra", "high"),
            ("anthropic", "claude-opus-5-5", "low"),
            ("anthropic", "claude-haiku-5-5", "low"),
            ("xai", "grok-4.7", "high"),
            ("my-server", "gpt-oss-120b", "medium"),
        ] {
            assert!(
                !an_earlier_list_chose(provider, model, effort),
                "{provider}/{model} {effort}"
            );
        }
        assert_eq!(
            an_earlier_list_chose("openai-codex", "gpt-6-luna", "high"),
            cfg!(debug_assertions)
        );
    }
}
