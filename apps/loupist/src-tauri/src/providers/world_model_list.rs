//! The rules for `world-model-defaults.json`, the bundled World model list.
//!
//! `build.rs` runs [`check`] on the list, so a build stops on a broken list, and
//! the app checks it again when it reads it. This file uses only `std` and
//! `serde_json` because the build script includes it as well.

use std::collections::HashSet;

use serde_json::Value;

/// The thinking levels Pi accepts (`catalog::PI_THINKING_LEVELS`).
pub const THINKING_LEVELS: &[&str] = &["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// The list is an array of `{"provider", "models"}`, each model
/// `{"id", "thinking_level"}` and nothing else. A provider appears once and
/// lists each model id once, and every thinking level is one Pi accepts.
pub fn check(json: &str) -> Result<(), String> {
    let list: Value =
        serde_json::from_str(json).map_err(|error| format!("the list is not JSON: {error}"))?;
    let providers = list
        .as_array()
        .filter(|providers| !providers.is_empty())
        .ok_or("the list is a non-empty array of providers")?;
    let mut seen = HashSet::new();
    for entry in providers {
        let entry = entry
            .as_object()
            .ok_or("each provider entry is an object")?;
        if let Some(key) = entry
            .keys()
            .find(|key| !matches!(key.as_str(), "provider" | "models"))
        {
            return Err(format!("a provider entry has the unknown field `{key}`"));
        }
        let provider = entry
            .get("provider")
            .and_then(Value::as_str)
            .filter(|provider| !provider.is_empty())
            .ok_or("each provider entry names its provider")?;
        if !seen.insert(provider) {
            return Err(format!("`{provider}` is listed more than once"));
        }
        let models = entry
            .get("models")
            .and_then(Value::as_array)
            .filter(|models| !models.is_empty())
            .ok_or_else(|| format!("`{provider}` lists no model"))?;
        let mut ids = HashSet::new();
        for model in models {
            let model = model
                .as_object()
                .ok_or_else(|| format!("each `{provider}` model is an object"))?;
            if let Some(key) = model
                .keys()
                .find(|key| !matches!(key.as_str(), "id" | "thinking_level"))
            {
                return Err(format!(
                    "a `{provider}` model has the unknown field `{key}`"
                ));
            }
            let id = model
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(|| format!("each `{provider}` model names its id"))?;
            if !ids.insert(id) {
                return Err(format!("`{provider}` lists `{id}` more than once"));
            }
            let level = model
                .get("thinking_level")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !THINKING_LEVELS.contains(&level) {
                return Err(format!(
                    "`{provider}/{id}` has the thinking level `{level}`; use one of {}",
                    THINKING_LEVELS.join(", ")
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_list_passes_and_names_pi_thinking_levels() {
        assert_eq!(
            THINKING_LEVELS,
            crate::providers::catalog::PI_THINKING_LEVELS
        );
        check(include_str!("../../../world-model-defaults.json")).unwrap();
    }

    #[test]
    fn a_broken_list_says_what_to_fix() {
        let model =
            |id: &str, level: &str| format!(r#"{{"id":"{id}","thinking_level":"{level}"}}"#);
        let provider = |id: &str, models: &[String]| {
            format!(r#"{{"provider":"{id}","models":[{}]}}"#, models.join(","))
        };
        let good = model("gpt-6-luna", "high");
        let openai = provider("openai", std::slice::from_ref(&good));
        assert!(check(&format!("[{openai}]")).is_ok());
        for (list, problem) in [
            ("{}".to_owned(), "non-empty array"),
            ("[]".to_owned(), "non-empty array"),
            ("[1]".to_owned(), "is an object"),
            (
                format!("[{openai},{openai}]"),
                "`openai` is listed more than once",
            ),
            (format!("[{}]", provider("openai", &[])), "`openai` lists no model"),
            (
                format!("[{}]", provider("openai", &[good.clone(), good.clone()])),
                "`openai` lists `gpt-6-luna` more than once",
            ),
            (
                format!("[{}]", provider("openai", &[model("gpt-6-luna", "hgih")])),
                "`openai/gpt-6-luna` has the thinking level `hgih`",
            ),
            (
                format!("[{}]", provider("openai", &[model("", "high")])),
                "names its id",
            ),
            (
                r#"[{"provider":"openai","models":[{"id":"a","thinking_level":"high","note":"cheap"}]}]"#.to_owned(),
                "unknown field `note`",
            ),
            (
                r#"[{"provider":"openai","models":[],"_comment":"x"}]"#.to_owned(),
                "unknown field `_comment`",
            ),
        ] {
            let error = check(&list).unwrap_err();
            assert!(error.contains(problem), "{list}: {error}");
        }
    }
}
