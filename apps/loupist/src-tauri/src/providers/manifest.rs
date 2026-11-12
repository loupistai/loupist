//! The bundled provider manifest (`apps/loupist/provider-manifest.json`): one
//! entry per built-in Pi provider with everything Loupist decides about it,
//! in the order Settings and Auto list providers. The catalog options, the
//! credential environment allowlist, isolated inference, native search, the
//! sign-in paste rule and the usage page are all read from here; the renderer
//! imports the same file for its featured rows and product names, and
//! `agent/provider-manifest.test.mjs` checks it against the pinned Pi catalog.
use std::sync::LazyLock;

use serde::Deserialize;

use crate::providers::catalog::{BillingNote, ProviderGroup, SignInFlow};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderManifest {
    pub support_bootstrap: SupportBootstrapSpec,
    pub presets: Vec<PresetSpec>,
    pub providers: Vec<ProviderSpec>,
}

/// The startup model of a provider support launch (`PiSupportLaunchConfig`).
/// The bundled provider extension handles every support command before any
/// model request, so this model is never called; it names an API-key
/// provider so Pi can start with a non-secret placeholder key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SupportBootstrapSpec {
    pub provider: String,
    pub model: String,
}

/// A fixed endpoint Rust saves as a custom provider (Upstage Solar).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PresetSpec {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api: String,
    pub console_url: String,
    /// Its place among the renderer's featured API-key rows.
    pub featured_row: u8,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderSpec {
    pub id: String,
    pub label: String,
    pub credential_label: String,
    #[serde(default = "default_description")]
    pub description: String,
    /// Product names for renderer sentences; Rust only validates them.
    #[serde(default)]
    names: ProviderNames,
    #[serde(default = "default_group")]
    pub group: ProviderGroup,
    #[serde(default)]
    pub recommended: bool,
    /// The provider's key page, opened by Rust (`provider_open_console`).
    #[serde(default)]
    pub console_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<ApiKeySpec>,
    #[serde(default)]
    pub sign_in: Option<SignInSpec>,
    /// Pi's model adapters for this provider support the isolated-inference
    /// fetch guard. The Pi process checks the selected model's API again.
    #[serde(default)]
    pub isolated_inference: bool,
    #[serde(default)]
    pub native_search: Option<NativeSearch>,
    /// Offered only by debug builds (the legacy Codex client).
    #[serde(default)]
    pub development_only: bool,
    /// The provider whose reviewed models this development client also serves.
    #[serde(default)]
    pub alias_of: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderNames {
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    sign_in: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApiKeySpec {
    /// The host variable development builds forward and every launch scrubs.
    pub environment: String,
    /// Its place among the renderer's featured API-key rows.
    #[serde(default)]
    pub featured_row: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SignInSpec {
    pub label: String,
    pub flow: SignInFlow,
    /// The only sign-in page Rust opens for this provider.
    pub authorization: AuthorizationSpec,
    /// What the paste fallback accepts; device flows have none.
    #[serde(default)]
    pub paste: Option<PasteSpec>,
    #[serde(default)]
    pub billing_note: Option<BillingNote>,
    /// Where the plan's usage is managed.
    #[serde(default)]
    pub usage_url: Option<String>,
    /// A signed-in plan lists its own models (`/loupist-provider-models --live`).
    #[serde(default)]
    pub live_models: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuthorizationSpec {
    pub host: String,
    pub path: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PasteSpec {
    ChatgptLoopback,
    BrowserCallback { port: u16, path: String },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NativeSearch {
    /// Automatic routing tries it before keyless Exa; model-only uses it.
    Automatic,
    /// Only the model-only routing setting uses it.
    ModelOnly,
}

fn default_description() -> String {
    "Connect with an API key stored by Pi in Loupist's private app data.".into()
}

fn default_group() -> ProviderGroup {
    ProviderGroup::More
}

static MANIFEST: LazyLock<ProviderManifest> = LazyLock::new(|| {
    parse(include_str!("../../../provider-manifest.json"))
        .unwrap_or_else(|error| panic!("bundled provider manifest: {error}"))
});

pub(crate) fn parse(source: &str) -> Result<ProviderManifest, String> {
    let manifest: ProviderManifest =
        serde_json::from_str(source).map_err(|error| error.to_string())?;
    manifest.validate()?;
    Ok(manifest)
}

/// Every provider in the manifest, development-only ones included.
pub(crate) fn all() -> impl Iterator<Item = &'static ProviderSpec> {
    MANIFEST.providers.iter()
}

/// The providers this build offers, in display order.
pub(crate) fn offered() -> impl Iterator<Item = &'static ProviderSpec> {
    all().filter(|spec| spec.offered_by_this_build())
}

/// A provider's entry whether or not this build offers it; capability checks
/// use this so a stored record on a development client keeps its meaning.
pub(crate) fn spec(provider: &str) -> Option<&'static ProviderSpec> {
    all().find(|spec| spec.id == provider)
}

pub(crate) fn preset(id: &str) -> Option<&'static PresetSpec> {
    MANIFEST.presets.iter().find(|preset| preset.id == id)
}

/// The development client that serves `provider`'s models in debug builds.
pub(crate) fn development_alias(provider: &str) -> Option<&'static str> {
    if !cfg!(debug_assertions) {
        return None;
    }
    all()
        .find(|spec| spec.development_only && spec.alias_of.as_deref() == Some(provider))
        .map(|spec| spec.id.as_str())
}

/// Whether `name` is a provider credential variable: allowed into a Pi
/// launch only as that provider's credential, and scrubbed otherwise.
pub(crate) fn is_credential_environment(name: &str) -> bool {
    all().any(|spec| spec.environment() == Some(name))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SupportBootstrap {
    pub provider: &'static str,
    pub model: &'static str,
    /// The bootstrap provider's API-key variable, which gets a placeholder.
    pub environment: &'static str,
}

pub(crate) fn support_bootstrap() -> SupportBootstrap {
    let bootstrap = &MANIFEST.support_bootstrap;
    SupportBootstrap {
        provider: bootstrap.provider.as_str(),
        model: bootstrap.model.as_str(),
        environment: spec(&bootstrap.provider)
            .and_then(ProviderSpec::environment)
            .expect("validated support bootstrap provider"),
    }
}

impl ProviderSpec {
    pub(crate) fn offered_by_this_build(&self) -> bool {
        !self.development_only || cfg!(debug_assertions)
    }

    pub(crate) fn environment(&self) -> Option<&str> {
        self.api_key.as_ref().map(|key| key.environment.as_str())
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn valid_https_url(value: &str) -> bool {
    value.len() <= 256
        && reqwest::Url::parse(value).is_ok_and(|url| {
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

fn valid_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 253
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b".-".contains(&byte))
}

fn valid_environment_name(value: &str) -> bool {
    value.len() <= 64
        && value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_uppercase())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

impl ProviderManifest {
    fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        let mut rows = std::collections::HashSet::new();
        for spec in &self.providers {
            let id = spec.id.as_str();
            if !crate::providers::catalog::valid_provider_id(id)
                || crate::providers::custom::is_custom_id(id)
                || !ids.insert(id)
            {
                return Err(format!("provider ID `{id}` is invalid or repeated"));
            }
            if !valid_text(&spec.label, 80)
                || !valid_text(&spec.credential_label, 80)
                || !valid_text(&spec.description, 200)
            {
                return Err(format!(
                    "`{id}` needs a label, credential label and description"
                ));
            }
            if [&spec.names.key, &spec.names.sign_in]
                .into_iter()
                .flatten()
                .any(|name| !valid_text(name, 40))
            {
                return Err(format!("`{id}` has an empty product name"));
            }
            if spec.api_key.is_none() && spec.sign_in.is_none() {
                return Err(format!("`{id}` offers neither an API key nor a sign-in"));
            }
            if let Some(key) = &spec.api_key {
                if !valid_environment_name(&key.environment) {
                    return Err(format!("`{id}` names an invalid credential variable"));
                }
                if let Some(row) = key.featured_row {
                    if spec.development_only || !rows.insert(row) {
                        return Err(format!("`{id}` repeats featured row {row}"));
                    }
                }
            }
            if let Some(sign_in) = &spec.sign_in {
                let rule = &sign_in.authorization;
                if !valid_text(&sign_in.label, 80)
                    || !valid_host(&rule.host)
                    || !rule.path.starts_with('/')
                    || rule.path.chars().any(char::is_control)
                    || sign_in
                        .usage_url
                        .as_deref()
                        .is_some_and(|url| !valid_https_url(url))
                {
                    return Err(format!("`{id}` has an invalid sign-in"));
                }
                if let Some(PasteSpec::BrowserCallback { port, path }) = &sign_in.paste {
                    if *port == 0 || !path.starts_with('/') {
                        return Err(format!("`{id}` has an invalid paste callback"));
                    }
                }
            }
            if spec.group == ProviderGroup::SignIn && spec.sign_in.is_none() {
                return Err(format!("`{id}` is listed with sign-ins but has none"));
            }
            if spec.group != ProviderGroup::More && spec.development_only {
                return Err(format!("development-only `{id}` cannot be featured"));
            }
            if spec
                .console_url
                .as_deref()
                .is_some_and(|url| !valid_https_url(url))
            {
                return Err(format!("`{id}` has an invalid console page"));
            }
        }
        for spec in &self.providers {
            if let Some(alias) = &spec.alias_of {
                if alias == &spec.id
                    || !self
                        .providers
                        .iter()
                        .any(|target| &target.id == alias && !target.development_only)
                {
                    return Err(format!("`{}` aliases an unknown provider", spec.id));
                }
            }
        }
        for preset in &self.presets {
            if !crate::providers::catalog::valid_provider_id(&preset.id)
                || !ids.insert(preset.id.as_str())
                || !valid_text(&preset.name, 80)
                || !valid_https_url(&preset.base_url)
                || !valid_https_url(&preset.console_url)
                || !matches!(
                    preset.api.as_str(),
                    "openai-responses" | "openai-completions" | "anthropic-messages"
                )
                || !rows.insert(preset.featured_row)
            {
                return Err(format!("preset `{}` is invalid", preset.id));
            }
        }
        let bootstrap = &self.support_bootstrap;
        if !self.providers.iter().any(|spec| {
            spec.id == bootstrap.provider && !spec.development_only && spec.api_key.is_some()
        }) || !valid_text(&bootstrap.model, 128)
        {
            return Err("the support bootstrap needs an offered API-key provider".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUNDLED: &str = include_str!("../../../provider-manifest.json");

    fn edited(edit: impl FnOnce(&mut serde_json::Value)) -> Result<ProviderManifest, String> {
        let mut value: serde_json::Value = serde_json::from_str(BUNDLED).unwrap();
        edit(&mut value);
        parse(&value.to_string())
    }

    fn provider(value: &serde_json::Value, id: &str) -> serde_json::Value {
        value["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["id"] == id)
            .unwrap()
            .clone()
    }

    #[test]
    fn the_bundled_manifest_is_valid_and_derives_one_entry_per_provider() {
        let manifest = parse(BUNDLED).unwrap();
        assert_eq!(all().count(), manifest.providers.len());
        // Every API-key provider names its credential variable by construction;
        // a sign-in-only provider names none and is never forwarded from the host.
        assert!(all().all(|spec| spec.api_key.is_some() || spec.sign_in.is_some()));
        assert_eq!(
            spec("openai-codex").and_then(ProviderSpec::environment),
            None
        );
        assert!(is_credential_environment("GEMINI_API_KEY"));
        assert!(!is_credential_environment("PATH"));
        assert_eq!(
            offered().any(|spec| spec.id == "openai-codex"),
            cfg!(debug_assertions)
        );
        assert_eq!(
            development_alias("openai"),
            cfg!(debug_assertions).then_some("openai-codex")
        );
        assert_eq!(development_alias("anthropic"), None);
        assert_eq!(
            support_bootstrap(),
            SupportBootstrap {
                provider: "google",
                model: "gemini-3.5-flash-lite",
                environment: "GEMINI_API_KEY",
            }
        );
        assert_eq!(
            preset("upstage").unwrap().base_url,
            "https://api.upstage.ai/v1"
        );
    }

    #[test]
    fn a_manifest_that_breaks_a_provider_rule_does_not_load() {
        type Edit = fn(&mut serde_json::Value);
        let cases: [(&str, Edit); 9] = [
            ("repeated ID", |value| {
                let copy = provider(value, "groq");
                value["providers"].as_array_mut().unwrap().push(copy);
            }),
            ("unknown field", |value| {
                value["providers"][0]["searches"] = true.into()
            }),
            ("no credential", |value| {
                value["providers"][2]
                    .as_object_mut()
                    .unwrap()
                    .remove("api_key");
            }),
            ("bad variable", |value| {
                value["providers"][2]["api_key"]["environment"] = "anthropic key".into();
            }),
            ("sign-in group without sign-in", |value| {
                value["providers"][2]["group"] = "sign_in".into();
            }),
            ("repeated featured row", |value| {
                value["providers"][2]["api_key"]["featured_row"] = 2.into();
            }),
            ("plain-http page", |value| {
                value["providers"][2]["console_url"] = "http://console.anthropic.com".into();
            }),
            ("custom namespace", |value| {
                value["providers"][2]["id"] = "custom-x".into()
            }),
            ("sign-in-only bootstrap", |value| {
                value["support_bootstrap"]["provider"] = "openai-codex".into();
            }),
        ];
        for (name, edit) in cases {
            assert!(edited(edit).is_err(), "{name}");
        }
    }
}
