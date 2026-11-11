//! Auto: the Chat model Rust picks when nothing is chosen (providers spec §3.3).
//! Reviewed choices come from `chat-model-defaults.json`; Auto never fails
//! while any model is connected, and never moves to another paid account
//! because one hit its limit (the order ignores notices).
use std::{
    collections::{BTreeMap, HashSet},
    sync::LazyLock,
};

use serde::Deserialize;

use crate::providers::{
    catalog::{
        known_provider, ConfiguredModel, ConfiguredModelView, CredentialSource, ModelCatalog,
        ModelCatalogView,
    },
    status::ProviderStatusRow,
};

/// Research needs a model that can read at least this many tokens at once.
pub const MIN_RESEARCH_CONTEXT: u64 = 20_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatModelDefaults {
    providers: Vec<ProviderDefault>,
    family_tiers: FamilyTiers,
}

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

/// Declared tiers for model families (`family_tiers` in
/// `chat-model-defaults.json`): 3 for a provider's largest family, 1 for its
/// small, fast one. Among one provider's models of the same version the higher
/// tier lists first, so it is also what Auto's fallback reaches first.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FamilyTiers {
    /// The tier of a model that names no declared family.
    unknown: u8,
    families: BTreeMap<String, u8>,
}

impl FamilyTiers {
    /// The highest tier declared for a whole word of the model's name or ID
    /// (`gpt-5.6 Sol` and `gpt-5.6-sol` name `sol`; `gemini` is not `mini`).
    pub(crate) fn tier(&self, name: &str, model: &str) -> u8 {
        let (name, model) = (name.to_ascii_lowercase(), model.to_ascii_lowercase());
        name.split(|character: char| !character.is_ascii_alphanumeric())
            .chain(model.split(|character: char| !character.is_ascii_alphanumeric()))
            .filter_map(|word| self.families.get(word))
            .max()
            .copied()
            .unwrap_or(self.unknown)
    }
}

static DEFAULTS: LazyLock<ChatModelDefaults> = LazyLock::new(|| {
    let defaults: ChatModelDefaults =
        serde_json::from_str(include_str!("../../../chat-model-defaults.json"))
            .expect("bundled Chat model defaults");
    for group in &defaults.providers {
        assert!(!group.models.is_empty(), "Chat defaults need a model");
        for model in &group.models {
            assert!(
                crate::providers::catalog::PI_THINKING_LEVELS
                    .contains(&model.thinking_level.as_str()),
                "Chat default thinking level is invalid"
            );
        }
    }
    assert!(
        defaults.family_tiers.families.keys().all(|family| {
            !family.is_empty()
                && family
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        }),
        "a model family is one lowercase word"
    );
    defaults
});

/// The bundled family tiers that order each provider's models in the catalog.
pub(crate) fn family_tiers() -> &'static FamilyTiers {
    &DEFAULTS.family_tiers
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutoChoice {
    pub model: ConfiguredModel,
    pub thinking_level: String,
    pub run_options: BTreeMap<String, String>,
}

fn default_level(levels: &[String], preferred: Option<&str>) -> Option<String> {
    preferred
        .filter(|level| levels.iter().any(|candidate| candidate == level))
        .or_else(|| {
            levels
                .iter()
                .any(|level| level == "medium")
                .then_some("medium")
        })
        .or_else(|| levels.first().map(String::as_str))
        .map(str::to_owned)
}

/// Resolves Auto on one catalog snapshot. `statuses` supplies each provider's
/// connected date: the provider the user connected first (onboarding) leads.
pub fn resolve(
    view: &ModelCatalogView,
    catalog: &ModelCatalog,
    statuses: &BTreeMap<String, ProviderStatusRow>,
) -> Option<AutoChoice> {
    let hidden = view
        .hidden_model_profile_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let usable = |model: &&ConfiguredModelView| {
        model.credential_source != CredentialSource::Missing && !hidden.contains(model.id.as_str())
    };
    let mut order = view
        .providers
        .iter()
        .filter(|provider| provider.credential_source != CredentialSource::Missing)
        .map(|provider| provider.id.to_owned())
        .chain(
            view.custom_providers
                .iter()
                .filter(|provider| provider.credential_source != CredentialSource::Missing)
                .map(|provider| provider.id.clone()),
        )
        .collect::<Vec<_>>();
    // A sign-in that needs reconnecting answers only when no other account can.
    let needs_reconnect = |provider: &str| {
        statuses
            .get(provider)
            .is_some_and(|row| row.state == crate::providers::status::ProviderState::NeedsReconnect)
    };
    // Stable sort: providers without a stored date keep catalog order after dated ones.
    order.sort_by_key(|provider| {
        (
            needs_reconnect(provider),
            statuses
                .get(provider)
                .and_then(|row| row.connected_at_ms)
                .unwrap_or(i64::MAX),
        )
    });
    let choose = |model: &ConfiguredModelView, preferred: Option<&str>| {
        let configured = catalog.resolve(&model.id).ok()?;
        let thinking_level = default_level(&configured.thinking_levels, preferred)?;
        let run_options = configured
            .run_controls
            .iter()
            .map(|control| (control.id.clone(), control.default_choice.clone()))
            .collect();
        Some(AutoChoice {
            model: configured,
            thinking_level,
            run_options,
        })
    };
    for provider in &order {
        let models = view
            .models
            .iter()
            .filter(usable)
            .filter(|model| &model.provider == provider)
            .collect::<Vec<_>>();
        if let Some(policy) = DEFAULTS
            .providers
            .iter()
            .find(|group| &group.provider == provider)
        {
            for entry in &policy.models {
                if let Some(model) = models.iter().find(|model| model.model == entry.id) {
                    if let Some(choice) = choose(model, Some(&entry.thinking_level)) {
                        return Some(choice);
                    }
                }
            }
        }
        // Catalog views list each provider's newest models first.
        if let Some(choice) = models
            .iter()
            .filter(|model| model.reasoning && model.context_window >= MIN_RESEARCH_CONTEXT)
            .find_map(|model| choose(model, None))
        {
            return Some(choice);
        }
    }
    let mut rest = view.models.iter().filter(usable).collect::<Vec<_>>();
    rest.sort_by_key(|model| needs_reconnect(&model.provider));
    rest.into_iter().find_map(|model| choose(model, None))
}

/// The execution choice at admission: an explicit model that is still offered,
/// otherwise Auto. Auto keeps the Chat defaults' thinking level and run options
/// when the resolved model supports them, and otherwise uses the policy's and
/// the model's own defaults.
pub fn admit(
    state: &crate::app::AppState,
    model_profile_id: &str,
    thinking_level: &str,
    run_options: &BTreeMap<String, String>,
) -> Result<AutoChoice, crate::app::CommandError> {
    let catalog = state.model_catalog()?;
    if let Some(model) = offered(&catalog, model_profile_id, &known_provider) {
        return Ok(AutoChoice {
            model,
            thinking_level: thinking_level.to_owned(),
            run_options: run_options.clone(),
        });
    }
    let view = state.model_catalog_view()?;
    let statuses = crate::providers::status::load_all(state)?;
    let choice = resolve(&view, &catalog, &statuses).ok_or_else(|| {
        crate::app::CommandError::invalid(
            "Connect an AI account in Settings › Models & providers before chatting.",
        )
    })?;
    remember(state, &catalog, &choice);
    // A fallback for a model that left the catalog ignores controls chosen for that model.
    Ok(if model_profile_id.is_empty() {
        with_requested_controls(choice, thinking_level, run_options)
    } else {
        choice
    })
}

/// An explicit choice that may still run: in the catalog and on a provider
/// this build offers. Anything else is gone and falls back to Auto.
fn offered(
    catalog: &ModelCatalog,
    model_profile_id: &str,
    known: &dyn Fn(&str) -> bool,
) -> Option<ConfiguredModel> {
    if model_profile_id.is_empty() {
        return None;
    }
    catalog
        .resolve(model_profile_id)
        .ok()
        .filter(|model| known(&model.provider))
}

/// Applies the Chat defaults' controls to an Auto choice only where the
/// resolved model supports them exactly.
fn with_requested_controls(
    mut choice: AutoChoice,
    thinking_level: &str,
    run_options: &BTreeMap<String, String>,
) -> AutoChoice {
    if choice
        .model
        .thinking_levels
        .iter()
        .any(|level| level == thinking_level)
    {
        choice.thinking_level = thinking_level.to_owned();
    }
    if !run_options.is_empty() && choice.model.validate_run_options(run_options).is_ok() {
        choice.run_options = run_options.clone();
    }
    choice
}

/// Stores Auto's resolution per provider; a different resolution whose old
/// model is gone records the "Now using …" notice once.
fn remember(state: &crate::app::AppState, catalog: &ModelCatalog, choice: &AutoChoice) {
    let provider = choice.model.provider.as_str();
    let fresh = crate::providers::status::ProviderStatusRow::fresh(
        provider,
        crate::providers::status::live_credential_kind(state, provider),
        crate::commands::now_ms(),
    );
    // Only Auto's column changes, in one transaction: a plan-limit notice
    // recorded meanwhile stays.
    let Ok(previous) = state.store.provider_status_remember_auto(
        &crate::store::provider_status::ProviderStatusRecord::from(&fresh),
        &choice.model.id,
    ) else {
        return;
    };
    let previous_gone = previous
        .is_some_and(|previous| previous != choice.model.id && catalog.resolve(&previous).is_err());
    if previous_gone {
        set_notice(state, "now_using", &choice.model.name);
    }
}

fn set_notice(state: &crate::app::AppState, kind: &str, model: &str) {
    let _ = state
        .store
        .model_notice_set(&crate::store::provider_status::ModelNoticeRecord {
            id: uuid::Uuid::new_v4().to_string(),
            kind: kind.into(),
            model: model.chars().take(80).collect(),
            created_at_ms: crate::commands::now_ms(),
        });
}

/// Models a provider no longer offers: when one was the explicit Chat default,
/// Chat uses Auto from now on and the composer says so once.
pub fn note_removed_default(state: &crate::app::AppState, removed: &[ConfiguredModel]) {
    if removed.is_empty() {
        return;
    }
    let Some(default) = crate::scoped_config::resolve(&state.artifacts.app_data_dir, None)
        .ok()
        .and_then(|config| config.chat.model_profile_id)
    else {
        return;
    };
    if let Some(model) = removed.iter().find(|model| model.id == default) {
        set_notice(state, "removed", &model.name);
    }
}

/// A 404 `model_not_found`: nothing was processed, so the model leaves the
/// catalog now and the one retry resolves Auto without it.
pub fn prune_gone(state: &crate::app::AppState, model_profile_id: &str) {
    let Ok(catalog) = state.model_catalog() else {
        return;
    };
    let Ok(gone) = catalog.resolve(model_profile_id) else {
        return;
    };
    // Rust remembers it, so the next model refresh does not bring it back.
    if state
        .store
        .gone_model_add(&gone.provider, &gone.id, crate::commands::now_ms())
        .is_err()
    {
        eprintln!("Loupist could not remember a model its provider no longer offers");
    }
    let remaining = catalog
        .models
        .into_iter()
        .filter(|model| model.provider == gone.provider && model.id != gone.id)
        .collect();
    if state
        .replace_provider_models(&gone.provider, remaining)
        .is_err()
    {
        eprintln!("Loupist could not remove a model its provider no longer offers");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        catalog::{catalog_view, ModelVisibility},
        status::{transition, CredentialKind, Trigger},
    };

    fn model(provider: &str, id: &str, reasoning: bool, context: u64) -> ConfiguredModel {
        ConfiguredModel {
            id: format!("{provider}/{id}"),
            name: id.into(),
            provider: provider.into(),
            model: id.into(),
            input: vec!["text".into()],
            reasoning,
            context_window: context,
            max_tokens: 16_000,
            thinking_levels: vec!["off".into(), "low".into(), "medium".into(), "high".into()],
            thinking_level_map: BTreeMap::new(),
            run_controls: vec![],
        }
    }

    fn connected_view(
        directory: &std::path::Path,
        providers: &[(&str, &str)],
        models: Vec<ConfiguredModel>,
        hidden: &[&str],
    ) -> (ModelCatalogView, ModelCatalog) {
        let auth = providers
            .iter()
            .map(|(provider, kind)| {
                let entry = if *kind == "oauth" {
                    serde_json::json!({"type": "oauth", "access": "a", "refresh": "r", "expires": 1_900_000_000_000_u64})
                } else {
                    serde_json::json!({"type": "api_key", "key": "k"})
                };
                ((*provider).to_owned(), entry)
            })
            .collect::<serde_json::Map<_, _>>();
        let path = directory.join("auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&path).unwrap();
        std::fs::write(&path, serde_json::to_vec(&auth).unwrap()).unwrap();
        let catalog = ModelCatalog { models };
        let view = catalog_view(
            &catalog,
            directory,
            &ModelVisibility {
                hidden_model_profile_ids: hidden.iter().map(|id| (*id).to_owned()).collect(),
            },
        )
        .unwrap();
        (view, catalog)
    }

    fn dated(provider: &str, at: i64) -> (String, ProviderStatusRow) {
        (
            provider.to_owned(),
            transition(None, provider, Trigger::Connected(CredentialKind::Plan), at).unwrap(),
        )
    }

    #[test]
    fn policy_choice_of_the_first_connected_provider_wins() {
        let directory = tempfile::tempdir().unwrap();
        let (view, catalog) = connected_view(
            directory.path(),
            &[("openai", "oauth"), ("anthropic", "api_key")],
            vec![
                model("openai", "gpt-5.6-sol", true, 272_000),
                model("openai", "gpt-6.1-sol", true, 272_000),
                model("anthropic", "claude-sonnet-5-5", true, 1_000_000),
            ],
            &[],
        );
        // Anthropic was connected first (onboarding), so it leads despite catalog order.
        let statuses = [dated("anthropic", 1), dated("openai", 2)]
            .into_iter()
            .collect();
        let choice = resolve(&view, &catalog, &statuses).unwrap();
        assert_eq!(choice.model.id, "anthropic/claude-sonnet-5-5");
        assert_eq!(choice.thinking_level, "medium");
        // Without dates the catalog order applies and the policy's first present entry wins.
        let choice = resolve(&view, &catalog, &BTreeMap::new()).unwrap();
        assert_eq!(choice.model.id, "openai/gpt-6.1-sol");
    }

    #[test]
    fn falls_back_to_a_long_context_reasoning_model_then_any_connected_model() {
        let directory = tempfile::tempdir().unwrap();
        let (view, catalog) = connected_view(
            directory.path(),
            &[("groq", "api_key")],
            vec![
                model("groq", "tiny", true, 8_000),
                model("groq", "plain", false, 128_000),
                model("groq", "reasoner", true, 128_000),
            ],
            &[],
        );
        assert_eq!(
            resolve(&view, &catalog, &BTreeMap::new()).unwrap().model.id,
            "groq/reasoner"
        );
        let (view, catalog) = connected_view(
            directory.path(),
            &[("groq", "api_key")],
            vec![
                model("groq", "tiny", true, 8_000),
                model("groq", "reasoner", true, 128_000),
            ],
            &["groq/reasoner"],
        );
        // Auto never fails while any model is connected, even a short one.
        assert_eq!(
            resolve(&view, &catalog, &BTreeMap::new()).unwrap().model.id,
            "groq/tiny"
        );
    }

    #[test]
    fn hidden_missing_and_disconnected_models_are_never_chosen() {
        let directory = tempfile::tempdir().unwrap();
        let (view, catalog) = connected_view(
            directory.path(),
            &[("openai", "oauth")],
            vec![
                model("openai", "gpt-6.1-sol", true, 272_000),
                model("anthropic", "claude-sonnet-5-5", true, 1_000_000),
            ],
            &["openai/gpt-6.1-sol"],
        );
        assert_eq!(resolve(&view, &catalog, &BTreeMap::new()), None);
    }

    fn signed_in_state(directory: &std::path::Path) -> crate::app::AppState {
        let state = crate::app::AppState::for_test(directory.join("app"));
        let agent_dir = state.pi_agent_data_dir().unwrap();
        std::fs::create_dir_all(&agent_dir).unwrap();
        let path = agent_dir.join("auth.json");
        crate::fs::artifact_trust::ensure_private_regular_file(&path).unwrap();
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"openai": {"type": "oauth", "access": "a", "refresh": "r", "expires": 1_900_000_000_000_u64}})).unwrap(),
        )
        .unwrap();
        state
            .replace_provider_models(
                "openai",
                vec![
                    model("openai", "gpt-6.1-sol", true, 272_000),
                    model("openai", "gpt-5.6-sol", true, 272_000),
                ],
            )
            .unwrap();
        state
    }

    #[test]
    fn admission_resolves_auto_and_honors_an_offered_explicit_choice() {
        let directory = tempfile::tempdir().unwrap();
        let state = signed_in_state(directory.path());
        let auto = admit(&state, "", "", &BTreeMap::new()).unwrap();
        assert_eq!(auto.model.id, "openai/gpt-6.1-sol");
        assert_eq!(auto.thinking_level, "medium");
        // Chat defaults' controls apply to Auto where the resolved model supports them.
        let configured = admit(&state, "", "high", &BTreeMap::new()).unwrap();
        assert_eq!(configured.thinking_level, "high");
        let unsupported = admit(&state, "", "xhigh", &BTreeMap::new()).unwrap();
        assert_eq!(unsupported.thinking_level, "medium");
        let explicit = admit(&state, "openai/gpt-5.6-sol", "high", &BTreeMap::new()).unwrap();
        assert_eq!(
            (explicit.model.id.as_str(), explicit.thinking_level.as_str()),
            ("openai/gpt-5.6-sol", "high")
        );
        // No connected account: Auto explains instead of guessing.
        let empty = crate::app::AppState::for_test(directory.path().join("empty"));
        empty.replace_provider_models("openai", vec![]).unwrap();
        assert_eq!(
            admit(&empty, "", "", &BTreeMap::new()).unwrap_err().code,
            "invalid_request"
        );
    }

    #[test]
    fn a_gone_model_is_pruned_and_auto_says_what_it_uses_now_once() {
        let directory = tempfile::tempdir().unwrap();
        let state = signed_in_state(directory.path());
        let first = admit(&state, "", "", &BTreeMap::new()).unwrap();
        assert_eq!(first.model.id, "openai/gpt-6.1-sol");
        assert!(state.model_catalog_view().unwrap().model_notice.is_none());
        // A 404 model_not_found on that model: nothing was processed; prune it.
        crate::providers::status::record_chat_outcome(
            &state,
            &crate::providers::catalog::ExecutionModelLock::from_model(
                &first.model,
                "medium",
                &BTreeMap::new(),
            ),
            Some(crate::domain::ChatFailureKind::ModelGone),
        );
        assert!(state
            .model_catalog()
            .unwrap()
            .resolve("openai/gpt-6.1-sol")
            .is_err());
        // The one retry resolves Auto without it, and the composer is told once.
        let retry = admit(&state, "", "", &BTreeMap::new()).unwrap();
        assert_eq!(retry.model.id, "openai/gpt-5.6-sol");
        let notice = state.model_catalog_view().unwrap().model_notice.unwrap();
        assert_eq!(
            (notice.kind.as_str(), notice.model.as_str()),
            ("now_using", "gpt-5.6-sol")
        );
        admit(&state, "", "", &BTreeMap::new()).unwrap();
        assert_eq!(
            state.model_catalog_view().unwrap().model_notice.unwrap().id,
            notice.id
        );
        state.store.model_notice_ack(&notice.id).unwrap();
        assert!(state.model_catalog_view().unwrap().model_notice.is_none());
        // A saved explicit choice that left the catalog falls back to Auto at its next use.
        let fallback = admit(&state, "openai/gpt-6.1-sol", "high", &BTreeMap::new()).unwrap();
        assert_eq!(
            (fallback.model.id.as_str(), fallback.thinking_level.as_str()),
            ("openai/gpt-5.6-sol", "medium")
        );
    }

    #[test]
    fn a_sign_in_that_needs_reconnecting_answers_only_when_nothing_else_can() {
        let directory = tempfile::tempdir().unwrap();
        let openai = model("openai", "gpt-6.1-sol", true, 272_000);
        let (view, catalog) = connected_view(
            directory.path(),
            &[("openai", "oauth"), ("anthropic", "api_key")],
            vec![
                openai.clone(),
                model("anthropic", "claude-sonnet-5-5", true, 1_000_000),
            ],
            &[],
        );
        // ChatGPT was connected first but its sign-in stopped working.
        let reconnect = transition(
            Some(dated("openai", 1).1),
            "openai",
            Trigger::Failure(crate::domain::ChatFailureKind::NeedsReconnect),
            2,
        )
        .unwrap();
        let statuses = [
            ("openai".to_owned(), reconnect.clone()),
            dated("anthropic", 3),
        ]
        .into_iter()
        .collect();
        assert_eq!(
            resolve(&view, &catalog, &statuses).unwrap().model.id,
            "anthropic/claude-sonnet-5-5"
        );
        // Alone, it still answers: Auto never fails while a model is connected.
        let (view, catalog) =
            connected_view(directory.path(), &[("openai", "oauth")], vec![openai], &[]);
        let statuses = [("openai".to_owned(), reconnect)].into_iter().collect();
        assert_eq!(
            resolve(&view, &catalog, &statuses).unwrap().model.id,
            "openai/gpt-6.1-sol"
        );
    }

    #[test]
    fn an_explicit_choice_on_a_provider_this_build_lacks_is_gone() {
        let catalog = ModelCatalog {
            models: vec![
                model("openai-codex", "gpt-6-luna", true, 272_000),
                model("openai", "gpt-6-luna", true, 272_000),
            ],
        };
        let release = |provider: &str| provider != "openai-codex";
        assert!(offered(&catalog, "openai-codex/gpt-6-luna", &release).is_none());
        assert_eq!(
            offered(&catalog, "openai/gpt-6-luna", &release).unwrap().id,
            "openai/gpt-6-luna"
        );
        assert!(offered(&catalog, "", &release).is_none());
        assert!(offered(&catalog, "openai/missing", &release).is_none());
    }

    #[test]
    fn a_model_family_ranks_by_its_declared_tier_and_an_undeclared_one_sits_in_the_middle() {
        let tiers: FamilyTiers = serde_json::from_value(serde_json::json!({
            "unknown": 2,
            "families": {"nova": 3, "terra": 2, "luna": 1, "flash": 2, "lite": 1}
        }))
        .unwrap();
        // A new family is ranked by what the defaults declare for it.
        assert_eq!(tiers.tier("GPT-7 Nova", "gpt-7-nova"), 3);
        assert_eq!(tiers.tier("gpt-7-luna", "gpt-7-luna"), 1);
        assert_eq!(tiers.tier("gpt-7-comet", "gpt-7-comet"), 2);
        // Whole words only, and the highest declared tier wins.
        assert_eq!(tiers.tier("Gemini 3", "gemini-3"), 2);
        assert_eq!(tiers.tier("Gemini Flash-Lite", "gemini-3-flash-lite"), 2);
        // The bundled table keeps today's families.
        let bundled = family_tiers();
        for (model, tier) in [
            ("claude-opus-5", 3),
            ("gpt-6.1-sol", 3),
            ("gemini-2.5-pro", 3),
            ("claude-sonnet-5-5", 2),
            ("gpt-6-terra", 2),
            ("gemini-2.5-flash-lite", 2),
            ("claude-haiku-4-5", 1),
            ("gpt-6-luna", 1),
            ("gpt-5.4-mini", 1),
            ("grok-4.7", 2),
        ] {
            assert_eq!(bundled.tier(model, model), tier, "{model}");
        }
    }

    #[test]
    fn bundled_policy_parses_and_names_known_providers() {
        assert!(DEFAULTS
            .providers
            .iter()
            .any(|group| group.provider == "openai"));
        for group in DEFAULTS.providers.iter() {
            assert!(crate::providers::catalog::provider_options()
                .iter()
                .any(|option| option.id == group.provider));
        }
    }
}
