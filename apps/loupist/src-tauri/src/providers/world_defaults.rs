//! Initial World choices only. Saved Loupist preferences never follow catalog changes.
use std::{collections::BTreeMap, sync::LazyLock};

use serde::{Deserialize, Serialize};

use crate::{
    app::{AppState, CommandError},
    knowledge::GraphProcessingSettings,
    providers::catalog::{
        catalog_view, ConfiguredModel, CredentialSource, ModelCatalog, ModelCatalogView,
    },
    providers::inference::supports_isolated_inference,
};

#[derive(Deserialize)]
struct ProviderDefault {
    provider: String,
    models: Vec<ModelDefault>,
}

#[derive(Deserialize)]
struct ModelDefault {
    id: String,
    thinking_level: String,
}

// Ordered, reviewed choices; never guess a model ID or silently select an
// arbitrary (potentially specialist or expensive) model from a provider catalog.
static DEFAULTS: LazyLock<Vec<ProviderDefault>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../world-model-defaults.json"))
        .expect("bundled World model defaults")
});

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
    pub model: Option<WorldModelSelection>,
}

struct LoupistCreationOptions {
    catalog: ModelCatalogView,
    default_model: Option<WorldModelSelection>,
}

/// Keep eligibility, provider identity and model controls on one catalog snapshot.
struct WorldModelCatalog {
    models: ModelCatalog,
    options: LoupistCreationOptions,
}

impl WorldModelCatalog {
    fn load(state: &AppState) -> Result<Self, CommandError> {
        let models = state.model_catalog()?;
        let view = catalog_view(
            &models,
            &state.pi_agent_data_dir()?,
            &state.model_visibility()?,
        )?;
        Ok(Self {
            models,
            options: options_from_catalog(view),
        })
    }

    fn resolve(&self, selection: &WorldModelSelection) -> Result<ResolvedWorldModel, CommandError> {
        if !self
            .options
            .catalog
            .models
            .iter()
            .any(|model| model.id == selection.model_profile_id)
        {
            return Err(CommandError::invalid(
                "The World model is no longer connected or supported. Choose another model.",
            ));
        }
        let model = self.models.resolve(&selection.model_profile_id)?;
        model.validate_thinking_level(&selection.thinking_level)?;
        model.validate_run_options(&selection.run_options)?;
        Ok(ResolvedWorldModel {
            model,
            selection: selection.clone(),
        })
    }
}

pub struct ResolvedWorldModel {
    model: ConfiguredModel,
    selection: WorldModelSelection,
}

impl ResolvedWorldModel {
    pub fn apply(&self, settings: &mut GraphProcessingSettings) {
        settings.helper_provider = Some(self.model.provider.clone());
        settings.helper_model = Some(self.model.model.clone());
        settings.helper_model_profile_id = Some(self.model.id.clone());
        settings.thinking_level = self.selection.thinking_level.clone();
        settings.run_options = self.selection.run_options.clone();
        settings.awaiting_default_model = false;
    }
}

pub fn recommended_model(state: &AppState) -> Result<Option<ResolvedWorldModel>, CommandError> {
    let catalog = WorldModelCatalog::load(state)?;
    catalog
        .options
        .default_model
        .as_ref()
        .map(|selection| catalog.resolve(selection))
        .transpose()
}

/// Debug builds keep the legacy Codex client for harnesses; it serves the same
/// reviewed OpenAI models, used only when no `openai` sign-in or key matches.
fn debug_alias(provider: &str) -> Option<&'static str> {
    (cfg!(debug_assertions) && provider == "openai").then_some("openai-codex")
}

fn options_from_catalog(mut catalog: ModelCatalogView) -> LoupistCreationOptions {
    catalog.models.retain(|model| {
        supports_isolated_inference(&model.provider)
            && model.input.iter().any(|input| input == "text")
            && model.credential_source != CredentialSource::Missing
            && catalog.providers.iter().any(|provider| {
                provider.id == model.provider
                    && provider.credential_source != CredentialSource::Missing
            })
    });
    let default_model = DEFAULTS.iter().find_map(|preferred| {
        preferred.models.iter().find_map(|candidate| {
            let matching = |provider: &str| {
                catalog.models.iter().find(|model| {
                    model.provider == provider
                        && model.model == candidate.id
                        && model.thinking_levels.contains(&candidate.thinking_level)
                })
            };
            matching(&preferred.provider)
                .or_else(|| debug_alias(&preferred.provider).and_then(matching))
                .map(|model| WorldModelSelection {
                    model_profile_id: model.id.clone(),
                    thinking_level: candidate.thinking_level.clone(),
                    run_options: model
                        .run_controls
                        .iter()
                        .map(|control| (control.id.clone(), control.default_choice.clone()))
                        .collect(),
                })
        })
    });
    LoupistCreationOptions {
        catalog,
        default_model,
    }
}

pub fn new_settings(
    state: &AppState,
    request: Option<&NewLoupistWorldSettings>,
    research_focus: Option<&str>,
) -> Result<GraphProcessingSettings, CommandError> {
    let catalog = WorldModelCatalog::load(state)?;
    let enabled = request.is_none_or(|request| request.enabled);
    let selection = request
        .and_then(|request| request.model.as_ref())
        .or(catalog.options.default_model.as_ref());
    let mut settings = GraphProcessingSettings {
        enrichment_enabled: enabled,
        awaiting_default_model: enabled && selection.is_none(),
        research_focus: research_focus.unwrap_or_default().trim().to_owned(),
        ..Default::default()
    };
    if let Some(selection) = selection {
        catalog.resolve(selection)?.apply(&mut settings);
    }
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::catalog::{
        provider_options, ConfiguredModelView, ModelRunControl, ModelRunControlChoice,
    };

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
            models,
            providers,
            custom_providers: vec![],
            hidden_model_profile_ids: vec![],
            model_notice: None,
        }
    }

    #[test]
    fn world_defaults_are_order_independent_and_use_reviewed_effort_and_standard_service() {
        let mut models = vec![
            model("anthropic", "claude-haiku-4-5"),
            model("openai", "gpt-5.6-luna"),
            model("openai", "gpt-6-luna"),
            // The debug-only Codex client never outranks an `openai` connection.
            model("openai-codex", "gpt-6-luna"),
        ];
        for _ in 0..models.len() {
            let mut catalog = catalog(models.clone());
            catalog.hidden_model_profile_ids = vec!["openai/gpt-6-luna".into()];
            let selection = options_from_catalog(catalog).default_model.unwrap();
            assert_eq!(selection.model_profile_id, "openai/gpt-6-luna");
            assert_eq!(selection.thinking_level, "high");
            assert_eq!(selection.run_options["performance"], "standard");
            models.rotate_left(1);
        }
    }

    #[test]
    fn world_defaults_require_the_reviewed_model_and_effort_pair() {
        let mut luna = model("openai", "gpt-6-luna");
        luna.thinking_levels = vec!["off".into(), "max".into()];
        let options = options_from_catalog(catalog(vec![luna, model("openai", "gpt-4.1-mini")]));
        let selection = options.default_model.unwrap();
        assert_eq!(selection.model_profile_id, "openai/gpt-4.1-mini");
        assert_eq!(selection.thinking_level, "off");
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
            let selection = options_from_catalog(catalog(vec![model(provider, id)]))
                .default_model
                .unwrap();
            assert_eq!(selection.thinking_level, effort);
        }
    }

    #[test]
    fn a_provider_prefers_its_newest_reviewed_model_and_keeps_older_ones_as_fallbacks() {
        for (provider, newest, older) in [
            ("anthropic", "claude-haiku-5-5", "claude-haiku-4-5"),
            ("anthropic", "claude-sonnet-5-5", "claude-sonnet-4-6"),
            ("minimax", "MiniMax-M3.1-Flash", "MiniMax-M3"),
            (
                "baseten",
                "deepseek-ai/DeepSeek-V4.1-Flash-1017",
                "deepseek-ai/DeepSeek-V4.1-Flash",
            ),
        ] {
            let both = options_from_catalog(catalog(vec![
                model(provider, older),
                model(provider, newest),
            ]));
            assert_eq!(
                both.default_model.unwrap().model_profile_id,
                format!("{provider}/{newest}")
            );
            let older_only = options_from_catalog(catalog(vec![model(provider, older)]));
            assert_eq!(
                older_only.default_model.unwrap().model_profile_id,
                format!("{provider}/{older}")
            );
        }
    }

    #[test]
    fn debug_builds_give_the_codex_client_the_openai_world_default() {
        let selection =
            options_from_catalog(catalog(vec![model("openai-codex", "gpt-6-luna")])).default_model;
        if cfg!(debug_assertions) {
            assert_eq!(
                selection.unwrap().model_profile_id,
                "openai-codex/gpt-6-luna"
            );
        } else {
            assert!(selection.is_none());
        }
    }

    #[test]
    fn world_defaults_filter_disconnected_unsupported_and_nontext_models() {
        let mut disconnected = model("openai-codex", "gpt-6-luna");
        disconnected.credential_source = CredentialSource::Missing;
        let mut image = model("openai", "gpt-6-luna");
        image.input = vec!["image".into()];
        let mut catalog = catalog(vec![
            disconnected,
            image,
            model("google", "gemini-flash"),
            model("anthropic", "claude-haiku-4-5"),
            model("deepseek", "deepseek-flash"),
        ]);
        catalog
            .providers
            .iter_mut()
            .find(|provider| provider.id == "anthropic")
            .unwrap()
            .credential_source = CredentialSource::Missing;
        let options = options_from_catalog(catalog);
        assert_eq!(options.catalog.models.len(), 1);
        assert_eq!(
            options.default_model.unwrap().model_profile_id,
            "deepseek/deepseek-flash"
        );
    }

    #[test]
    fn world_defaults_use_available_fallbacks_without_guessing_or_escalating_to_a_flagship() {
        let options = options_from_catalog(catalog(vec![
            model("openai", "gpt-6-astra"),
            model("openai", "gpt-4.1-mini"),
        ]));
        assert_eq!(
            options.default_model.unwrap().model_profile_id,
            "openai/gpt-4.1-mini"
        );
        let options = options_from_catalog(catalog(vec![model("openai", "gpt-6-astra")]));
        assert!(options.default_model.is_none());
        assert_eq!(
            options.catalog.models.len(),
            1,
            "manual selection remains available"
        );
        assert!(options_from_catalog(catalog(vec![]))
            .default_model
            .is_none());
    }
}
