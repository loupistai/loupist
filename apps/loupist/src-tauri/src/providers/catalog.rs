//! The model catalog: configured models, provider options and sign-in rules,
//! credential sources and the execution model lock.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{cmp::Ordering, collections::BTreeSet, env, fs::OpenOptions, io::Read, path::Path};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::{
    app::CommandError,
    fs::artifact_trust::ensure_private_regular_file,
    providers::status::{CredentialKind, ProviderStatusView},
    util::sha256,
};

const MAX_AUTH_FILE_BYTES: u64 = 64 * 1024;
const MAX_MODEL_ID_BYTES: usize = 512;
// Pi can expose large provider catalogs (for example, routing providers). Keep
// the persisted catalog bounded without treating the old manual-profile limit
// as a provider model limit.
const MAX_CONFIGURED_MODELS: usize = 4_096;
const MAX_MODEL_RUN_CONTROLS: usize = 16;
pub const PI_THINKING_LEVELS: &[&str] =
    &["off", "minimal", "low", "medium", "high", "xhigh", "max"];
pub const PROVIDER_CREDENTIAL_ENVIRONMENTS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "ANT_LING_API_KEY",
    "OPENAI_API_KEY",
    "GEMINI_API_KEY",
    "DEEPSEEK_API_KEY",
    "NVIDIA_API_KEY",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "CEREBRAS_API_KEY",
    "XAI_API_KEY",
    "OPENROUTER_API_KEY",
    "AI_GATEWAY_API_KEY",
    "HF_TOKEN",
    "FIREWORKS_API_KEY",
    "TOGETHER_API_KEY",
    "BASETEN_API_KEY",
    "KIMI_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "ZAI_API_KEY",
    "ZAI_CODING_CN_API_KEY",
    "OPENCODE_API_KEY",
    "RADIUS_API_KEY",
    "QWEN_TOKEN_PLAN_API_KEY",
    "QWEN_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_API_KEY",
    "XIAOMI_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
    "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
];

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConfiguredModel {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub input: Vec<String>,
    pub reasoning: bool,
    pub context_window: u64,
    pub max_tokens: u64,
    pub thinking_levels: Vec<String>,
    pub thinking_level_map: std::collections::BTreeMap<String, Option<String>>,
    pub run_controls: Vec<ModelRunControl>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelRunControlChoice {
    pub id: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelRunControl {
    pub id: String,
    pub label: String,
    pub default_choice: String,
    pub choices: Vec<ModelRunControlChoice>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ExecutionModelLock {
    pub profile_id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub thinking_level: String,
    pub run_options: std::collections::BTreeMap<String, String>,
}

impl ExecutionModelLock {
    pub fn from_model(
        value: &ConfiguredModel,
        thinking_level: &str,
        run_options: &std::collections::BTreeMap<String, String>,
    ) -> Self {
        Self {
            profile_id: value.id.clone(),
            name: value.name.clone(),
            provider: value.provider.clone(),
            model: value.model.clone(),
            thinking_level: thinking_level.to_owned(),
            run_options: run_options.clone(),
        }
    }

    pub fn validate(&self) -> Result<(), CommandError> {
        if !valid_model_profile_id(&self.profile_id) {
            return Err(CommandError::invalid("execution model ID is invalid"));
        }
        if self.name.trim().is_empty()
            || self.name.len() > 80
            || self.name.chars().any(char::is_control)
        {
            return Err(CommandError::invalid(
                "execution model display name is invalid",
            ));
        }
        // A lock records what answered. It stays readable after a build stops
        // offering that provider; admission decides what may run now.
        if !valid_provider_id(&self.provider) {
            return Err(CommandError::invalid("Pi model provider ID is invalid"));
        }
        validate_model_identifier(&self.model)?;
        if !PI_THINKING_LEVELS.contains(&self.thinking_level.as_str()) {
            return Err(CommandError::invalid("Pi thinking level is invalid"));
        }
        validate_run_option_map(&self.run_options)?;
        Ok(())
    }
}

impl ConfiguredModel {
    pub fn validate(&self) -> Result<(), CommandError> {
        if !valid_model_profile_id(&self.id) {
            return Err(CommandError::invalid("Pi model key is invalid"));
        }
        let name = self.name.trim();
        if name.is_empty() || name.len() > 80 || name.chars().any(char::is_control) {
            return Err(CommandError::invalid("model display name is invalid"));
        }
        if !known_provider(&self.provider) {
            return Err(CommandError::invalid("unsupported Pi model provider"));
        }
        validate_model_identifier(&self.model)?;
        if self.input.is_empty()
            || self.input.iter().any(|input| {
                !matches!(input.as_str(), "text" | "image") || input.chars().any(char::is_control)
            })
            || self
                .input
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != self.input.len()
        {
            return Err(CommandError::invalid(
                "Pi model input capabilities are invalid",
            ));
        }
        if self.context_window == 0 || self.max_tokens == 0 {
            return Err(CommandError::invalid("Pi model limits are invalid"));
        }
        if self.thinking_levels.is_empty()
            || self.thinking_levels.iter().any(|level| {
                !PI_THINKING_LEVELS.contains(&level.as_str())
                    || level.is_empty()
                    || level.chars().any(char::is_control)
            })
            || self
                .thinking_levels
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len()
                != self.thinking_levels.len()
        {
            return Err(CommandError::invalid(
                "Pi model thinking levels are invalid",
            ));
        }
        if self.thinking_level_map.keys().any(|level| {
            !PI_THINKING_LEVELS.contains(&level.as_str()) || level.chars().any(char::is_control)
        }) || self.thinking_level_map.values().any(|value| {
            value
                .as_deref()
                .is_some_and(|value| value.is_empty() || value.chars().any(char::is_control))
        }) {
            return Err(CommandError::invalid(
                "Pi model thinking level map is invalid",
            ));
        }
        if self.run_controls.len() > MAX_MODEL_RUN_CONTROLS {
            return Err(CommandError::invalid("too many Pi model run controls"));
        }
        let mut control_ids = std::collections::HashSet::new();
        for control in &self.run_controls {
            if !valid_run_option_id(&control.id)
                || !valid_run_option_label(&control.label)
                || control.choices.is_empty()
                || control.choices.len() > 16
                || !control_ids.insert(control.id.as_str())
            {
                return Err(CommandError::invalid("Pi model run control is invalid"));
            }
            let mut choice_ids = std::collections::HashSet::new();
            for choice in &control.choices {
                if !valid_run_option_id(&choice.id)
                    || !valid_run_option_label(&choice.label)
                    || choice.description.len() > 200
                    || choice.description.chars().any(char::is_control)
                    || !choice_ids.insert(choice.id.as_str())
                {
                    return Err(CommandError::invalid(
                        "Pi model run control choice is invalid",
                    ));
                }
            }
            if !choice_ids.contains(control.default_choice.as_str()) {
                return Err(CommandError::invalid(
                    "Pi model run control default is invalid",
                ));
            }
        }
        Ok(())
    }

    pub fn validate_thinking_level(&self, level: &str) -> Result<(), CommandError> {
        if self
            .thinking_levels
            .iter()
            .any(|candidate| candidate == level)
        {
            Ok(())
        } else {
            Err(CommandError::invalid(
                "thinking level is not supported by the selected Pi model",
            ))
        }
    }

    pub fn validate_run_options(
        &self,
        options: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), CommandError> {
        validate_run_option_map(options)?;
        if options.len() != self.run_controls.len()
            || self.run_controls.iter().any(|control| {
                options.get(&control.id).is_none_or(|choice| {
                    !control
                        .choices
                        .iter()
                        .any(|candidate| candidate.id == *choice)
                })
            })
        {
            return Err(CommandError::invalid(
                "run options are not supported by the selected Pi model",
            ));
        }
        Ok(())
    }
}

fn valid_run_option_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().enumerate().all(|(index, byte)| {
            byte.is_ascii_lowercase()
                || (index > 0 && (byte.is_ascii_digit() || byte == b'_' || byte == b'-'))
        })
}

fn valid_run_option_label(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 80 && !value.chars().any(char::is_control)
}

fn validate_run_option_map(
    options: &std::collections::BTreeMap<String, String>,
) -> Result<(), CommandError> {
    if options.len() > MAX_MODEL_RUN_CONTROLS
        || options
            .iter()
            .any(|(control, choice)| !valid_run_option_id(control) || !valid_run_option_id(choice))
    {
        return Err(CommandError::invalid("Pi model run options are invalid"));
    }
    Ok(())
}

pub(crate) fn valid_model_profile_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= MAX_MODEL_ID_BYTES
        && !value.chars().any(char::is_control)
}

fn validate_model_identifier(model: &str) -> Result<(), CommandError> {
    if model.trim().is_empty()
        || model.len() > MAX_MODEL_ID_BYTES
        || model.chars().any(char::is_control)
    {
        return Err(CommandError::invalid("Pi model ID is invalid"));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelCatalog {
    pub models: Vec<ConfiguredModel>,
}

impl ModelCatalog {
    pub fn validate(&self) -> Result<(), CommandError> {
        if self.models.len() > MAX_CONFIGURED_MODELS {
            return Err(CommandError::invalid("too many configured models"));
        }
        let mut ids = std::collections::HashSet::new();
        for model in &self.models {
            model.validate()?;
            if !ids.insert(model.id.as_str()) {
                return Err(CommandError::invalid("Pi model keys must be unique"));
            }
        }
        Ok(())
    }

    pub fn resolve(&self, id: &str) -> Result<ConfiguredModel, CommandError> {
        self.models
            .iter()
            .find(|model| model.id == id)
            .cloned()
            .ok_or_else(|| CommandError::not_found("Pi model"))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelVisibility {
    pub hidden_model_profile_ids: BTreeSet<String>,
}

impl ModelVisibility {
    pub fn validate(&self) -> Result<(), CommandError> {
        if self.hidden_model_profile_ids.len() > MAX_CONFIGURED_MODELS
            || self
                .hidden_model_profile_ids
                .iter()
                .any(|id| !valid_model_profile_id(id))
        {
            return Err(CommandError::invalid(
                "hidden Pi model profile IDs are invalid",
            ));
        }
        Ok(())
    }

    pub fn is_visible(&self, model_profile_id: &str) -> bool {
        !self.hidden_model_profile_ids.contains(model_profile_id)
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ModelProviderOption {
    pub id: &'static str,
    pub label: &'static str,
    pub credential_label: &'static str,
    pub description: &'static str,
    pub api_key: bool,
    pub oauth: Option<ProviderOauth>,
    pub credential_source: CredentialSource,
    pub recommended: bool,
    /// Where onboarding and the Settings picker list the provider.
    pub group: ProviderGroup,
    /// The provider's key page, opened by Rust (`provider_open_console`).
    pub console_url: Option<&'static str>,
    pub sign_in_flow: Option<SignInFlow>,
    pub billing_note: Option<BillingNote>,
    pub status: ProviderStatusView,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderGroup {
    SignIn,
    ApiKey,
    More,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignInFlow {
    Browser,
    Device,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BillingNote {
    ChatgptPlan,
    ClaudeExtraUsage,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
pub struct ProviderOauth {
    pub label: &'static str,
    #[serde(skip)]
    pub authorization: OauthAuthorizationRule,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OauthAuthorizationRule {
    host: &'static str,
    path: &'static str,
}

impl OauthAuthorizationRule {
    pub const fn exact(host: &'static str, path: &'static str) -> Self {
        Self { host, path }
    }

    pub fn allows(self, host: &str, path: &str) -> bool {
        self.host == host && path.starts_with('/') && self.path == path
    }
}

pub fn catalog_allows_authorization(host: &str, path: &str) -> bool {
    provider_options().iter().any(|provider| {
        provider
            .oauth
            .is_some_and(|oauth| oauth.authorization.allows(host, path))
    })
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    Saved,
    Environment,
    Missing,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConfiguredModelView {
    pub id: String,
    pub name: String,
    pub provider: String,
    pub model: String,
    pub input: Vec<String>,
    pub reasoning: bool,
    pub context_window: u64,
    pub max_tokens: u64,
    pub thinking_levels: Vec<String>,
    pub thinking_level_map: std::collections::BTreeMap<String, Option<String>>,
    pub run_controls: Vec<ModelRunControl>,
    pub credential_source: CredentialSource,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelCatalogView {
    pub models: Vec<ConfiguredModelView>,
    pub providers: Vec<ModelProviderOption>,
    pub custom_providers: Vec<CustomProviderView>,
    pub hidden_model_profile_ids: Vec<String>,
    /// The pending "Now using …" line above the composer (W17.5).
    pub model_notice: Option<ModelNoticeView>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ModelNoticeView {
    pub id: String,
    pub kind: String,
    /// The model's display name.
    pub model: String,
}

/// A user-added OpenAI- or Anthropic-compatible endpoint. Its key is never shown.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CustomProviderView {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api: String,
    pub model_ids: Vec<String>,
    pub credential_source: CredentialSource,
    pub status: ProviderStatusView,
}

/// The shape of any Pi provider ID, offered by this build or not.
fn valid_provider_id(provider: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= 64
        && provider
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// A built-in provider, or one in the user-added `custom-` namespace.
pub(crate) fn known_provider(provider: &str) -> bool {
    provider_options()
        .iter()
        .any(|option| option.id == provider)
        || crate::providers::custom::is_custom_id(provider)
}

pub fn provider_options() -> Vec<ModelProviderOption> {
    let options = vec![
        // One `openai` slot holds either a Sign in with ChatGPT grant or an
        // OpenAI API key (Pi's `openai-chatgpt` flow with the dynamic client).
        ModelProviderOption {
            recommended: true,
            group: ProviderGroup::SignIn,
            console_url: Some("https://platform.openai.com/api-keys"),
            sign_in_flow: Some(SignInFlow::Browser),
            billing_note: Some(BillingNote::ChatgptPlan),
            ..oauth_provider(
                "openai",
                "ChatGPT",
                "OpenAI API key",
                "Sign in with ChatGPT, or use an OpenAI API key stored by Pi in Loupist's private app data.",
                ProviderOauth {
                    label: "Continue with ChatGPT",
                    authorization: OauthAuthorizationRule::exact(
                        "auth.openai.com",
                        "/api/accounts/authorize",
                    ),
                },
                true,
            )
        },
        ModelProviderOption {
            group: ProviderGroup::SignIn,
            console_url: Some("https://console.anthropic.com/settings/keys"),
            sign_in_flow: Some(SignInFlow::Browser),
            billing_note: Some(BillingNote::ClaudeExtraUsage),
            ..oauth_provider(
                "anthropic",
                "Anthropic",
                "Anthropic API key",
                "Sign in with Claude (billed as Claude extra usage), or use an Anthropic API key stored by Pi in Loupist's private app data.",
                ProviderOauth {
                    label: "Continue with Claude",
                    authorization: OauthAuthorizationRule::exact("claude.ai", "/oauth/authorize"),
                },
                true,
            )
        },
        ModelProviderOption {
            group: ProviderGroup::ApiKey,
            console_url: Some("https://aistudio.google.com/apikey"),
            ..provider("google", "Google Gemini", "Gemini API key")
        },
        ModelProviderOption {
            group: ProviderGroup::SignIn,
            console_url: Some("https://console.x.ai"),
            sign_in_flow: Some(SignInFlow::Device),
            ..oauth_provider(
                "xai",
                "xAI",
                "xAI API key",
                "Use SuperGrok or X Premium, or an xAI API key stored by Pi in Loupist's private app data.",
                ProviderOauth {
                    label: "Continue with SuperGrok",
                    authorization: OauthAuthorizationRule::exact(
                        "accounts.x.ai",
                        "/oauth2/device",
                    ),
                },
                true,
            )
        },
        ModelProviderOption {
            group: ProviderGroup::ApiKey,
            console_url: Some("https://openrouter.ai/keys"),
            ..provider("openrouter", "OpenRouter", "OpenRouter API key")
        },
        provider("ant-ling", "Ant Ling", "Ant Ling API key"),
        provider("deepseek", "DeepSeek", "DeepSeek API key"),
        provider("nvidia", "NVIDIA NIM", "NVIDIA API key"),
        provider("mistral", "Mistral", "Mistral API key"),
        provider("groq", "Groq", "Groq API key"),
        provider("cerebras", "Cerebras", "Cerebras API key"),
        provider(
            "vercel-ai-gateway",
            "Vercel AI Gateway",
            "AI Gateway API key",
        ),
        provider("huggingface", "Hugging Face", "Hugging Face token"),
        provider("fireworks", "Fireworks", "Fireworks API key"),
        provider("together", "Together AI", "Together API key"),
        provider("baseten", "Baseten", "Baseten API key"),
        provider("kimi-coding", "Kimi For Coding", "Kimi API key"),
        provider("minimax", "MiniMax", "MiniMax API key"),
        provider("minimax-cn", "MiniMax (China)", "MiniMax China API key"),
        provider("zai", "ZAI Coding Plan", "ZAI API key"),
        provider(
            "zai-coding-cn",
            "ZAI Coding Plan (China)",
            "ZAI China API key",
        ),
        provider("opencode", "OpenCode Zen", "OpenCode API key"),
        provider("opencode-go", "OpenCode Go", "OpenCode API key"),
        provider("radius", "Radius", "Radius API key"),
        provider(
            "qwen-token-plan",
            "Qwen Token Plan",
            "Qwen Token Plan API key",
        ),
        provider(
            "qwen-token-plan-individual",
            "Qwen Token Plan (Individual)",
            "Qwen Token Plan API key",
        ),
        provider(
            "qwen-token-plan-cn",
            "Qwen Token Plan (China)",
            "Qwen Token Plan China API key",
        ),
        provider("xiaomi", "Xiaomi MiMo", "Xiaomi API key"),
        provider(
            "xiaomi-token-plan-cn",
            "Xiaomi MiMo Token Plan (China)",
            "Xiaomi Token Plan API key",
        ),
        provider(
            "xiaomi-token-plan-ams",
            "Xiaomi MiMo Token Plan (Amsterdam)",
            "Xiaomi Token Plan API key",
        ),
        provider(
            "xiaomi-token-plan-sgp",
            "Xiaomi MiMo Token Plan (Singapore)",
            "Xiaomi Token Plan API key",
        ),
    ];
    // The legacy Codex client exists only for the development e2e harness and
    // evaluations that already hold its credential; release builds drop it.
    #[cfg(debug_assertions)]
    let options = {
        let mut options = options;
        options.insert(
            1,
            ModelProviderOption {
                group: ProviderGroup::More,
                ..oauth_provider(
                    "openai-codex",
                    "ChatGPT (Codex client, development only)",
                    "ChatGPT account",
                    "Development builds only: the legacy Codex sign-in.",
                    ProviderOauth {
                        label: "Continue with ChatGPT",
                        authorization: OauthAuthorizationRule::exact(
                            "auth.openai.com",
                            "/oauth/authorize",
                        ),
                    },
                    false,
                )
            },
        );
        options
    };
    options
}

const fn provider(
    id: &'static str,
    label: &'static str,
    credential_label: &'static str,
) -> ModelProviderOption {
    ModelProviderOption {
        id,
        label,
        credential_label,
        description: "Connect with an API key stored by Pi in Loupist's private app data.",
        api_key: true,
        oauth: None,
        credential_source: CredentialSource::Missing,
        recommended: false,
        group: ProviderGroup::More,
        console_url: None,
        sign_in_flow: None,
        billing_note: None,
        status: ProviderStatusView::DISCONNECTED,
    }
}

const fn oauth_provider(
    id: &'static str,
    label: &'static str,
    credential_label: &'static str,
    description: &'static str,
    oauth: ProviderOauth,
    api_key: bool,
) -> ModelProviderOption {
    ModelProviderOption {
        id,
        label,
        credential_label,
        description,
        api_key,
        oauth: Some(oauth),
        credential_source: CredentialSource::Missing,
        recommended: false,
        group: ProviderGroup::More,
        console_url: None,
        sign_in_flow: Some(SignInFlow::Browser),
        billing_note: None,
        status: ProviderStatusView::DISCONNECTED,
    }
}

/// Only development builds read host `*_API_KEY` variables. Pi and its native
/// Bash are same-user processes and `ps -E` shows a process environment, so a
/// forwarded key would be readable by any command the agent runs. Release
/// builds use the saved credential in Pi's private auth.json instead.
const FORWARD_HOST_PROVIDER_KEYS: bool = cfg!(debug_assertions) && !cfg!(feature = "e2e");

pub fn provider_credential_from_environment(provider: &str) -> Option<(String, String)> {
    host_provider_credential(provider, FORWARD_HOST_PROVIDER_KEYS, |name| {
        env::var(name).ok()
    })
}

pub(crate) fn host_provider_credential(
    provider: &str,
    forward_host_keys: bool,
    host: impl Fn(&str) -> Option<String>,
) -> Option<(String, String)> {
    if !forward_host_keys {
        return None;
    }
    let name = provider_environment_name(provider)?;
    host(name)
        .filter(|value| !value.is_empty())
        .map(|value| (name.to_owned(), value))
}

fn provider_environment_name(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "anthropic" => "ANTHROPIC_API_KEY",
        "ant-ling" => "ANT_LING_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "google" => "GEMINI_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "nvidia" => "NVIDIA_API_KEY",
        "mistral" => "MISTRAL_API_KEY",
        "groq" => "GROQ_API_KEY",
        "cerebras" => "CEREBRAS_API_KEY",
        "xai" => "XAI_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        "vercel-ai-gateway" => "AI_GATEWAY_API_KEY",
        "huggingface" => "HF_TOKEN",
        "fireworks" => "FIREWORKS_API_KEY",
        "together" => "TOGETHER_API_KEY",
        "baseten" => "BASETEN_API_KEY",
        "kimi-coding" => "KIMI_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "minimax-cn" => "MINIMAX_CN_API_KEY",
        "zai" => "ZAI_API_KEY",
        "zai-coding-cn" => "ZAI_CODING_CN_API_KEY",
        "opencode" | "opencode-go" => "OPENCODE_API_KEY",
        "radius" => "RADIUS_API_KEY",
        "qwen-token-plan" | "qwen-token-plan-individual" => "QWEN_TOKEN_PLAN_API_KEY",
        "qwen-token-plan-cn" => "QWEN_TOKEN_PLAN_CN_API_KEY",
        "xiaomi" => "XIAOMI_API_KEY",
        "xiaomi-token-plan-cn" => "XIAOMI_TOKEN_PLAN_CN_API_KEY",
        "xiaomi-token-plan-ams" => "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
        "xiaomi-token-plan-sgp" => "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
        "openai-codex" => return None,
        _ => return None,
    })
}

/// Credential-free native smoke cannot pass onboarding step 2, which has no
/// skip. An e2e build may mark ChatGPT ready without reading a secret or
/// opening a network connection. Every other build ignores the marker.
fn credential_source_with_e2e_marker(
    provider_id: &str,
    current: CredentialSource,
    marker: Option<&str>,
) -> CredentialSource {
    #[cfg(feature = "e2e")]
    if provider_id == "openai" && current == CredentialSource::Missing && marker == Some("1") {
        return CredentialSource::Saved;
    }
    let _ = (provider_id, marker);
    current
}

fn apply_e2e_provider_ready(provider_id: &str, current: CredentialSource) -> CredentialSource {
    #[cfg(feature = "e2e")]
    let marker = std::env::var("LOUPIST_E2E_PROVIDER_READY").ok();
    #[cfg(not(feature = "e2e"))]
    let marker: Option<String> = None;
    credential_source_with_e2e_marker(provider_id, current, marker.as_deref())
}

pub fn catalog_view(
    catalog: &ModelCatalog,
    agent_data_dir: &Path,
    visibility: &ModelVisibility,
) -> Result<ModelCatalogView, CommandError> {
    visibility.validate()?;
    let provider_options = provider_options();
    // auth.json is bounded but may sit on a slower user-data volume. Read and
    // validate it once per view instead of once per model (provider catalogs
    // can contain thousands of entries) and once again per provider.
    let saved_auth = read_auth_map(&agent_data_dir.join("auth.json"))?;
    let credential_sources = provider_options
        .iter()
        .map(|provider| {
            (
                provider.id,
                apply_e2e_provider_ready(
                    provider.id,
                    credential_source_from_auth(&saved_auth, provider.id),
                ),
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let provider_order = provider_options
        .iter()
        .enumerate()
        .map(|(index, provider)| (provider.id, index))
        .collect::<std::collections::HashMap<_, _>>();
    // An unreadable models.json hides custom providers; Pi reports the file itself.
    let custom_providers = crate::providers::custom::list(agent_data_dir)
        .unwrap_or_default()
        .into_iter()
        .map(|provider| {
            let credential_source = credential_source_from_auth(&saved_auth, &provider.id);
            CustomProviderView {
                status: ProviderStatusView::from_credential(
                    if credential_source == CredentialSource::Missing {
                        CredentialKind::None
                    } else {
                        crate::providers::status::custom_kind(&provider.base_url)
                    },
                ),
                credential_source,
                id: provider.id,
                name: provider.name,
                base_url: provider.base_url,
                api: provider.api,
                model_ids: provider.model_ids,
            }
        })
        .collect::<Vec<_>>();
    let mut models = catalog
        .models
        .iter()
        .map(|model| {
            let credential_source = credential_sources
                .get(model.provider.as_str())
                .copied()
                .or_else(|| {
                    custom_providers
                        .iter()
                        .find(|provider| provider.id == model.provider)
                        .map(|provider| provider.credential_source)
                })
                .unwrap_or(CredentialSource::Missing);
            Ok(ConfiguredModelView {
                id: model.id.clone(),
                name: model.name.clone(),
                provider: model.provider.clone(),
                model: model.model.clone(),
                input: model.input.clone(),
                reasoning: model.reasoning,
                context_window: model.context_window,
                max_tokens: model.max_tokens,
                thinking_levels: model.thinking_levels.clone(),
                thinking_level_map: model.thinking_level_map.clone(),
                run_controls: model.run_controls.clone(),
                credential_source,
            })
        })
        .collect::<Result<Vec<_>, CommandError>>()?;
    models.sort_by(|left, right| {
        provider_order
            .get(left.provider.as_str())
            .unwrap_or(&usize::MAX)
            .cmp(
                provider_order
                    .get(right.provider.as_str())
                    .unwrap_or(&usize::MAX),
            )
            .then_with(|| compare_model_recency(left, right))
    });
    let providers = provider_options
        .into_iter()
        .map(|mut provider| {
            provider.credential_source = credential_sources
                .get(provider.id)
                .copied()
                .unwrap_or(CredentialSource::Missing);
            provider.status =
                ProviderStatusView::from_credential(match provider.credential_source {
                    CredentialSource::Missing => CredentialKind::None,
                    CredentialSource::Environment => CredentialKind::Environment,
                    CredentialSource::Saved => match saved_auth
                        .get(provider.id)
                        .and_then(|entry| entry.get("type"))
                        .and_then(Value::as_str)
                    {
                        Some("oauth") => CredentialKind::Plan,
                        // The e2e ready marker has no stored entry; it stands for a sign-in.
                        None => CredentialKind::Plan,
                        _ => CredentialKind::ApiKey,
                    },
                });
            Ok(provider)
        })
        .collect::<Result<Vec<_>, CommandError>>()?;
    Ok(ModelCatalogView {
        models,
        providers,
        custom_providers,
        hidden_model_profile_ids: visibility
            .hidden_model_profile_ids
            .iter()
            .cloned()
            .collect(),
        model_notice: None,
    })
}

fn compare_model_recency(left: &ConfiguredModelView, right: &ConfiguredModelView) -> Ordering {
    numeric_runs(&right.model)
        .cmp(&numeric_runs(&left.model))
        .then_with(|| model_tier_rank(right).cmp(&model_tier_rank(left)))
        .then_with(|| right.context_window.cmp(&left.context_window))
        .then_with(|| right.max_tokens.cmp(&left.max_tokens))
        .then_with(|| left.name.cmp(&right.name))
        .then_with(|| left.id.cmp(&right.id))
}

fn model_tier_rank(model: &ConfiguredModelView) -> u8 {
    let value = format!("{} {}", model.name, model.model).to_ascii_lowercase();
    if ["opus", "fable", "ultra", " pro", "-pro", " sol"]
        .iter()
        .any(|marker| value.contains(marker))
    {
        3
    } else if ["sonnet", "terra", "flash", "standard"]
        .iter()
        .any(|marker| value.contains(marker))
    {
        2
    } else if ["haiku", "mini", "nano", "luna", "spark", "lite"]
        .iter()
        .any(|marker| value.contains(marker))
    {
        1
    } else {
        2
    }
}

fn numeric_runs(value: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    let mut current = None::<u64>;
    for byte in value.bytes() {
        if byte.is_ascii_digit() {
            current = Some(
                current
                    .unwrap_or_default()
                    .saturating_mul(10)
                    .saturating_add(u64::from(byte - b'0')),
            );
        } else if let Some(number) = current.take() {
            numbers.push(number);
        }
    }
    if let Some(number) = current {
        numbers.push(number);
    }
    numbers
}

/// Returns a secret-free, process-local cache generation for the credential
/// authority Pi will use for this provider. Environment credentials take the
/// same precedence as the provider support launch path. OAuth access tokens and
/// expiry timestamps are deliberately excluded: routine token refresh must not
/// invalidate a model catalog, while a refresh-token or account replacement
/// still rotates the generation.
pub(crate) fn provider_credential_generation(
    agent_data_dir: &Path,
    provider: &str,
) -> Result<Option<String>, CommandError> {
    let (source, secret) =
        if let Some((name, secret)) = provider_credential_from_environment(provider) {
            (format!("environment:{name}"), secret)
        } else {
            let auth = read_auth_map(&agent_data_dir.join("auth.json"))?;
            let Some(entry) = auth
                .get(provider)
                .filter(|entry| valid_saved_credential(entry))
            else {
                return Ok(None);
            };
            let Some(entry) = entry.as_object() else {
                return Ok(None);
            };
            match entry.get("type").and_then(Value::as_str) {
                Some("api_key") => (
                    "saved:api_key".to_owned(),
                    entry
                        .get("key")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                Some("oauth") => (
                    "saved:oauth".to_owned(),
                    entry
                        .get("refresh")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                ),
                _ => return Ok(None),
            }
        };
    let mut material = Vec::with_capacity(provider.len() + source.len() + secret.len() + 64);
    material.extend_from_slice(b"loupist/provider-credential-generation/v1\0");
    material.extend_from_slice(provider.as_bytes());
    material.push(0);
    material.extend_from_slice(source.as_bytes());
    material.push(0);
    material.extend_from_slice(secret.as_bytes());
    Ok(Some(sha256(&material)))
}

/// Secret-free readiness metadata for host-owned background inference. Pi
/// remains the owner of token refresh and authenticated requests.
pub(crate) fn provider_credential_kind(
    agent_data_dir: &Path,
    provider: &str,
) -> Result<Option<String>, CommandError> {
    if provider_credential_from_environment(provider).is_some() {
        return Ok(Some("environment".into()));
    }
    let auth = read_auth_map(&agent_data_dir.join("auth.json"))?;
    Ok(auth
        .get(provider)
        .filter(|value| valid_saved_credential(value))
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

fn credential_source_from_auth(auth: &Map<String, Value>, provider: &str) -> CredentialSource {
    if auth.get(provider).is_some_and(valid_saved_credential) {
        CredentialSource::Saved
    } else if provider_credential_from_environment(provider).is_some() {
        CredentialSource::Environment
    } else {
        CredentialSource::Missing
    }
}

fn valid_saved_credential(value: &Value) -> bool {
    let Some(entry) = value.as_object() else {
        return false;
    };
    match entry.get("type").and_then(Value::as_str) {
        Some("api_key") => entry
            .get("key")
            .and_then(Value::as_str)
            .is_some_and(|key| !key.is_empty()),
        Some("oauth") => {
            entry
                .get("access")
                .and_then(Value::as_str)
                .is_some_and(|token| !token.is_empty())
                && entry.get("refresh").and_then(Value::as_str).is_some()
                && entry.get("expires").and_then(Value::as_u64).is_some()
        }
        _ => false,
    }
}

/// Every saved provider credential value (API keys and sign-in tokens) in
/// Pi's private `auth.json`, for the sharing private-string scan. Values are
/// compared in Rust only; they are never returned to the renderer or logged.
pub(crate) fn saved_credential_values(agent_data_dir: &Path) -> Vec<String> {
    fn collect(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(text) if text.len() >= 12 => out.push(text.clone()),
            Value::Array(items) => items.iter().for_each(|item| collect(item, out)),
            Value::Object(map) => map
                .iter()
                .filter(|(key, _)| key.as_str() != "type")
                .for_each(|(_, item)| collect(item, out)),
            _ => {}
        }
    }
    let mut values = Vec::new();
    if let Ok(auth) = read_auth_map(&agent_data_dir.join("auth.json")) {
        for entry in auth.values() {
            collect(entry, &mut values);
        }
    }
    values
}

fn read_auth_map(path: &Path) -> Result<Map<String, Value>, CommandError> {
    if !path.exists() {
        return Ok(Map::new());
    }
    ensure_private_regular_file(path).map_err(|error| CommandError::internal(error.to_string()))?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    let mut file = options
        .open(path)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    if file
        .metadata()
        .map_err(|error| CommandError::internal(error.to_string()))?
        .len()
        > MAX_AUTH_FILE_BYTES
    {
        return Err(CommandError::internal("Pi auth file is too large"));
    }
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|error| CommandError::internal(error.to_string()))?;
    if contents.trim().is_empty() {
        return Ok(Map::new());
    }
    serde_json::from_str::<Value>(&contents)
        .map_err(|_| CommandError::internal("Pi auth file is invalid"))?
        .as_object()
        .cloned()
        .ok_or_else(|| CommandError::internal("Pi auth file is invalid"))
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use super::*;

    fn write_auth_fixture(directory: &Path, value: Value) {
        let path = directory.join("auth.json");
        ensure_private_regular_file(&path).unwrap();
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        ensure_private_regular_file(&path).unwrap();
    }

    fn model(provider: &str, name: &str, thinking_levels: &[&str]) -> ConfiguredModel {
        ConfiguredModel {
            id: format!("{provider}/{name}"),
            name: name.into(),
            provider: provider.into(),
            model: name.into(),
            input: vec!["text".into()],
            reasoning: true,
            context_window: 128_000,
            max_tokens: 32_000,
            thinking_levels: thinking_levels
                .iter()
                .map(|level| (*level).into())
                .collect(),
            thinking_level_map: BTreeMap::new(),
            run_controls: vec![],
        }
    }

    #[test]
    fn every_exposed_provider_has_a_unique_id_and_environment_route() {
        let providers = provider_options();
        let unique = providers
            .iter()
            .map(|provider| provider.id)
            .collect::<HashSet<_>>();
        assert_eq!(unique.len(), providers.len());
        assert!(providers
            .iter()
            .all(|provider| { provider.api_key || provider.oauth.is_some() }));
        assert!(providers.iter().all(|provider| {
            !provider.api_key
                || provider_environment_name(provider.id)
                    .is_some_and(|name| PROVIDER_CREDENTIAL_ENVIRONMENTS.contains(&name))
        }));
    }

    #[test]
    fn host_provider_keys_are_ignored_unless_the_build_forwards_them() {
        let host = |name: &str| (name == "OPENAI_API_KEY").then(|| "fake-host-key".to_owned());
        // Release and e2e builds: a host key never becomes a Pi credential.
        assert_eq!(host_provider_credential("openai", false, host), None);
        assert_eq!(
            host_provider_credential("openai", true, host),
            Some(("OPENAI_API_KEY".into(), "fake-host-key".into()))
        );
        assert_eq!(host_provider_credential("anthropic", true, host), None);
    }

    #[test]
    fn e2e_provider_ready_marker_marks_only_a_missing_chatgpt_credential_in_e2e_builds() {
        let marked = if cfg!(feature = "e2e") {
            CredentialSource::Saved
        } else {
            CredentialSource::Missing
        };
        assert_eq!(
            credential_source_with_e2e_marker("openai", CredentialSource::Missing, Some("1")),
            marked,
        );
        for (provider, source, marker) in [
            ("openai", CredentialSource::Missing, Some("0")),
            ("openai", CredentialSource::Missing, None),
            ("openai", CredentialSource::Environment, Some("1")),
            ("openai-codex", CredentialSource::Missing, Some("1")),
            ("anthropic", CredentialSource::Missing, Some("1")),
        ] {
            assert_eq!(
                credential_source_with_e2e_marker(provider, source, marker),
                source
            );
        }
    }

    #[test]
    fn release_catalog_offers_chatgpt_sign_in_on_the_openai_slot() {
        let providers = provider_options();
        let openai = providers.iter().find(|p| p.id == "openai").unwrap();
        assert_eq!(openai.label, "ChatGPT");
        assert!(openai.api_key && openai.recommended);
        assert_eq!(openai.group, ProviderGroup::SignIn);
        assert_eq!(openai.billing_note, Some(BillingNote::ChatgptPlan));
        assert!(catalog_allows_authorization(
            "auth.openai.com",
            "/api/accounts/authorize"
        ));
        let anthropic = providers.iter().find(|p| p.id == "anthropic").unwrap();
        assert_eq!(anthropic.billing_note, Some(BillingNote::ClaudeExtraUsage));
        let xai = providers.iter().find(|p| p.id == "xai").unwrap();
        assert_eq!(xai.sign_in_flow, Some(SignInFlow::Device));
        // OpenRouter is a key provider; its browser sign-in is gone.
        let openrouter = providers.iter().find(|p| p.id == "openrouter").unwrap();
        assert!(openrouter.oauth.is_none());
        assert!(!catalog_allows_authorization("openrouter.ai", "/auth"));
        // The legacy Codex client is a development-only harness entry.
        assert_eq!(
            providers.iter().any(|p| p.id == "openai-codex"),
            cfg!(debug_assertions)
        );
        let json = serde_json::to_value(openai).unwrap();
        assert_eq!(json["group"], "sign_in");
        assert_eq!(json["sign_in_flow"], "browser");
        assert_eq!(json["billing_note"], "chatgpt_plan");
        assert_eq!(json["status"]["state"], "disconnected");
        assert!(json.get("oauth").unwrap().get("authorization").is_none());
    }

    #[test]
    fn oauth_authorization_rules_match_only_the_declared_host_and_path() {
        let openai = OauthAuthorizationRule::exact("auth.openai.com", "/oauth/authorize");
        assert!(openai.allows("auth.openai.com", "/oauth/authorize"));
        assert!(!openai.allows("auth.openai.com", "/other"));
        assert!(!openai.allows("auth.openai.com", "oauth/authorize"));
        let xai = OauthAuthorizationRule::exact("accounts.x.ai", "/oauth2/device");
        assert!(xai.allows("accounts.x.ai", "/oauth2/device"));
        assert!(!xai.allows("auth.x.ai", "/oauth2/device"));
        assert!(!xai.allows("accounts.x.ai.evil.example", "/oauth2/device"));
        assert!(catalog_allows_authorization(
            "claude.ai",
            "/oauth/authorize"
        ));
        assert!(!catalog_allows_authorization(
            "example.com",
            "/oauth/authorize"
        ));
    }

    #[test]
    fn saved_credentials_are_recognized_but_never_reach_the_catalog_view() {
        let temporary = tempfile::tempdir().unwrap();
        let provider = crate::providers::custom::prepare(
            temporary.path(),
            &crate::providers::custom::CustomProviderRequest {
                provider: None,
                name: "Local server".into(),
                base_url: "http://localhost:11434/v1".into(),
                api: "openai-completions".into(),
                api_key: None,
                model_ids: vec!["qwen2.5:7b".into()],
            },
        )
        .unwrap();
        crate::providers::custom::write(temporary.path(), &provider).unwrap();
        write_auth_fixture(
            temporary.path(),
            serde_json::json!({
                "anthropic": { "type": "api_key", "key": "api-key-secret" },
                "openai": {
                    "type": "oauth",
                    "access": "access-secret",
                    "refresh": "refresh-secret",
                    "expires": 1_900_000_000_000_u64,
                    "accountId": "account"
                },
                // Minted OAuth keys have no refresh token.
                "openrouter": {
                    "type": "oauth",
                    "access": "or-secret",
                    "refresh": "",
                    "expires": 9_007_199_254_740_991_u64
                },
                provider.id.clone(): { "type": "api_key", "key": "custom-secret" }
            }),
        );
        let custom = ConfiguredModel {
            reasoning: false,
            thinking_levels: vec!["off".into()],
            ..model(&provider.id, "qwen2.5:7b", &[])
        };
        let hidden = model("openai", "fixture", &["off", "low"]);
        let visibility = ModelVisibility {
            hidden_model_profile_ids: [hidden.id.clone()].into(),
        };
        let view = catalog_view(
            &ModelCatalog {
                models: vec![hidden.clone(), custom],
            },
            temporary.path(),
            &visibility,
        )
        .unwrap();
        assert_eq!(view.hidden_model_profile_ids, vec![hidden.id]);
        assert!(view
            .models
            .iter()
            .all(|model| model.credential_source == CredentialSource::Saved));
        assert_eq!(view.custom_providers[0].name, "Local server");
        assert_eq!(
            view.custom_providers[0].credential_source,
            CredentialSource::Saved
        );
        for id in ["openai", "anthropic", "openrouter"] {
            let provider = view.providers.iter().find(|p| p.id == id).unwrap();
            assert_eq!(provider.credential_source, CredentialSource::Saved, "{id}");
        }
        let status = |id: &str| view.providers.iter().find(|p| p.id == id).unwrap().status;
        assert_eq!(status("openai").credential_kind, CredentialKind::Plan);
        assert_eq!(status("anthropic").credential_kind, CredentialKind::ApiKey);
        assert_eq!(status("google").credential_kind, CredentialKind::None);
        assert_eq!(
            view.custom_providers[0].status.credential_kind,
            CredentialKind::Local
        );
        let serialized = serde_json::to_string(&view).unwrap();
        for secret in [
            "api-key-secret",
            "access-secret",
            "refresh-secret",
            "or-secret",
            "custom-secret",
            "auth.openai.com",
        ] {
            assert!(!serialized.contains(secret), "{secret}");
        }
    }

    #[test]
    fn provider_credential_generation_ignores_oauth_access_refresh_but_rotates_with_authority() {
        let temporary = tempfile::tempdir().unwrap();
        let generation = |access: &str, refresh: &str| {
            write_auth_fixture(
                temporary.path(),
                serde_json::json!({
                    "openai": {
                        "type": "oauth",
                        "access": access,
                        "refresh": refresh,
                        "expires": 1_900_000_000_000_u64
                    }
                }),
            );
            provider_credential_generation(temporary.path(), "openai")
                .unwrap()
                .unwrap()
        };
        let first = generation("access-one", "authority-one");
        assert_eq!(generation("access-two", "authority-one"), first);
        assert_ne!(generation("access-three", "authority-two"), first);
    }

    #[test]
    fn a_stored_lock_on_a_provider_this_build_lacks_stays_readable_but_cannot_enter_the_catalog() {
        let retired = model("retired-client", "gpt-6-luna", &["off", "high"]);
        let lock = ExecutionModelLock::from_model(&retired, "high", &BTreeMap::new());
        lock.validate().unwrap();
        assert_eq!(retired.validate().unwrap_err().code, "invalid_request");
        let mut malformed = lock.clone();
        malformed.provider = "Not A Provider".into();
        assert!(malformed.validate().is_err());
    }

    #[test]
    fn execution_locks_accept_only_declared_thinking_levels_and_run_options() {
        let mut sonnet = model("anthropic", "claude-sonnet", &["off", "low", "medium"]);
        sonnet.run_controls = vec![ModelRunControl {
            id: "performance".into(),
            label: "Performance".into(),
            default_choice: "standard".into(),
            choices: ["standard", "fast"]
                .map(|id| ModelRunControlChoice {
                    id: id.into(),
                    label: id.into(),
                    description: id.into(),
                })
                .into(),
        }];
        sonnet.validate().unwrap();
        sonnet.validate_thinking_level("low").unwrap();
        assert!(sonnet.validate_thinking_level("high").is_err());
        let fast = BTreeMap::from([("performance".into(), "fast".into())]);
        sonnet.validate_run_options(&fast).unwrap();
        assert!(sonnet
            .validate_run_options(&BTreeMap::from([("performance".into(), "turbo".into())]))
            .is_err());
        let lock = ExecutionModelLock::from_model(&sonnet, "low", &fast);
        assert_eq!(
            (lock.provider.as_str(), lock.model.as_str()),
            ("anthropic", "claude-sonnet")
        );
        assert_eq!(lock.thinking_level, "low");
        assert_eq!(lock.run_options, fast);
        lock.validate().unwrap();

        // A model may explicitly disable the off level.
        let mut always_thinking = model("openai", "always-thinking", &["high", "max"]);
        always_thinking.thinking_level_map = BTreeMap::from([
            ("off".into(), None),
            ("high".into(), Some("high".into())),
            ("max".into(), Some("max".into())),
        ]);
        always_thinking.validate().unwrap();
        always_thinking.validate_thinking_level("max").unwrap();
        assert!(always_thinking.validate_thinking_level("off").is_err());
    }

    #[test]
    fn catalog_lists_each_providers_newest_numeric_model_version_first() {
        let temporary = tempfile::tempdir().unwrap();
        let view = catalog_view(
            &ModelCatalog {
                models: [
                    "gpt-5.4",
                    "gpt-5.10",
                    "gpt-5.6 Luna",
                    "gpt-5.6 Sol",
                    "gpt-5.6 Terra",
                ]
                .map(|name| model("openai", name, &["off", "medium"]))
                .into(),
            },
            temporary.path(),
            &ModelVisibility::default(),
        )
        .unwrap();
        assert_eq!(
            view.models
                .iter()
                .map(|model| model.model.as_str())
                .collect::<Vec<_>>(),
            [
                "gpt-5.10",
                "gpt-5.6 Sol",
                "gpt-5.6 Terra",
                "gpt-5.6 Luna",
                "gpt-5.4"
            ]
        );
    }
}
