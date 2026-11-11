//! Model endpoints the user adds that speak an API Pi already supports
//! (OpenAI- or Anthropic-compatible). Rust owns the `custom-` provider entries in
//! Pi's `models.json`; the key goes to Pi's credential store like any other
//! API-key provider, so no key, `$NAME` interpolation or `!command` is ever
//! written to `models.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{
    app::CommandError,
    fs::artifact_trust::{
        ensure_private_regular_file, read_private_regular_file_bounded, ArtifactTrustError,
    },
};

pub const PREFIX: &str = "custom-";
/// Stored in Pi's credential store for a local server that needs no key, so Pi
/// treats the provider as connected. Servers such as Ollama ignore it.
pub const NO_KEY: &str = "loupist-no-key";
const FILE: &str = "models.json";
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_PROVIDERS: usize = 20;
const MAX_MODELS: usize = 20;
const MAX_NAME_CHARS: usize = 60;
const MAX_URL_BYTES: usize = 512;
const MAX_MODEL_ID_BYTES: usize = 128;
const MAX_ID_SLUG: usize = 32;
pub const APIS: [&str; 3] = [
    "openai-responses",
    "openai-completions",
    "anthropic-messages",
];

#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(optional_fields = nullable))]
#[serde(deny_unknown_fields)]
pub struct CustomProviderRequest {
    /// Present when editing an existing custom provider.
    pub provider: Option<String>,
    pub name: String,
    pub base_url: String,
    pub api: String,
    pub api_key: Option<String>,
    pub model_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct CustomProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api: String,
    pub model_ids: Vec<String>,
}

/// A custom provider ID has the reserved prefix and a short ASCII slug.
pub fn is_custom_id(id: &str) -> bool {
    id.strip_prefix(PREFIX).is_some_and(|slug| {
        !slug.is_empty()
            && slug.len() <= MAX_ID_SLUG + 3
            && slug
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

pub fn list(agent_dir: &Path) -> Result<Vec<CustomProvider>, CommandError> {
    let document = read(agent_dir)?;
    let Some(providers) = document.get("providers").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };
    Ok(providers
        .iter()
        .filter(|(id, _)| is_custom_id(id))
        .filter_map(|(id, entry)| parse_entry(id, entry))
        .collect())
}

pub fn find(agent_dir: &Path, id: &str) -> Result<Option<CustomProvider>, CommandError> {
    Ok(list(agent_dir)?
        .into_iter()
        .find(|provider| provider.id == id))
}

/// Validates a request into the provider that will be written. The ID is kept
/// when editing; a new provider gets a unique ID from its name.
pub fn prepare(
    agent_dir: &Path,
    request: &CustomProviderRequest,
) -> Result<CustomProvider, CommandError> {
    let name = request.name.trim();
    if name.is_empty()
        || name.chars().count() > MAX_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(CommandError::invalid(
            "Enter a provider name of up to 60 characters.",
        ));
    }
    let base_url = validate_base_url(request.base_url.trim())?;
    if !APIS.contains(&request.api.as_str()) {
        return Err(CommandError::invalid("Choose a supported API type."));
    }
    let mut model_ids = Vec::new();
    for id in request
        .model_ids
        .iter()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
    {
        if id.len() > MAX_MODEL_ID_BYTES || id.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err(CommandError::invalid(
                "A model ID can't contain spaces and must be 128 characters or fewer.",
            ));
        }
        if !model_ids.iter().any(|known: &String| known == id) {
            model_ids.push(id.to_owned());
        }
    }
    if model_ids.is_empty() || model_ids.len() > MAX_MODELS {
        return Err(CommandError::invalid("Enter between 1 and 20 model IDs."));
    }
    let existing = list(agent_dir)?;
    let id = match request.provider.as_deref() {
        Some(id) => {
            if !existing.iter().any(|provider| provider.id == id) {
                return Err(CommandError::not_found("Custom provider"));
            }
            id.to_owned()
        }
        None => {
            if existing.len() >= MAX_PROVIDERS {
                return Err(CommandError::invalid(
                    "You can add up to 20 custom providers.",
                ));
            }
            unique_id(name, &existing)
        }
    };
    Ok(CustomProvider {
        id,
        name: name.to_owned(),
        base_url,
        api: request.api.clone(),
        model_ids,
    })
}

/// Writes or replaces one custom provider entry, keeping every other entry.
pub fn write(agent_dir: &Path, provider: &CustomProvider) -> Result<(), CommandError> {
    let mut document = read(agent_dir)?;
    providers_mut(&mut document)?.insert(
        provider.id.clone(),
        json!({
            "name": provider.name,
            "baseUrl": provider.base_url,
            "api": provider.api,
            "models": provider
                .model_ids
                .iter()
                .map(|id| json!({ "id": id, "name": id }))
                .collect::<Vec<_>>(),
        }),
    );
    persist(agent_dir, &document)
}

pub fn remove(agent_dir: &Path, id: &str) -> Result<(), CommandError> {
    if !is_custom_id(id) {
        return Err(CommandError::invalid("unsupported Pi model provider"));
    }
    let mut document = read(agent_dir)?;
    if providers_mut(&mut document)?.remove(id).is_some() {
        persist(agent_dir, &document)?;
    }
    Ok(())
}

pub(crate) fn validate_base_url(value: &str) -> Result<String, CommandError> {
    let invalid = || {
        CommandError::invalid(
            "Enter an https:// address, or an http:// address on this computer (localhost).",
        )
    };
    if value.is_empty() || value.len() > MAX_URL_BYTES {
        return Err(invalid());
    }
    let url = reqwest::Url::parse(value).map_err(|_| invalid())?;
    let host = url.host_str().ok_or_else(invalid)?;
    let loopback = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1");
    let allowed_scheme = url.scheme() == "https" || (url.scheme() == "http" && loopback);
    if !allowed_scheme
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid());
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

fn unique_id(name: &str, existing: &[CustomProvider]) -> String {
    let mut slug = String::new();
    for character in name.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
        if slug.len() >= MAX_ID_SLUG {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    let base = format!(
        "{PREFIX}{}",
        if slug.is_empty() { "provider" } else { slug }
    );
    if !existing.iter().any(|provider| provider.id == base) {
        return base;
    }
    (2..)
        .map(|suffix| format!("{base}-{suffix}"))
        .find(|candidate| !existing.iter().any(|provider| &provider.id == candidate))
        .expect("an unused suffix exists")
}

fn parse_entry(id: &str, entry: &Value) -> Option<CustomProvider> {
    let entry = entry.as_object()?;
    let text = |key: &str| entry.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(CustomProvider {
        id: id.to_owned(),
        name: text("name").unwrap_or_else(|| id.trim_start_matches(PREFIX).to_owned()),
        base_url: text("baseUrl")?,
        api: text("api")?,
        model_ids: entry
            .get("models")?
            .as_array()?
            .iter()
            .filter_map(|model| model.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect(),
    })
}

fn read(agent_dir: &Path) -> Result<Value, CommandError> {
    let path = agent_dir.join(FILE);
    match read_private_regular_file_bounded(&path, MAX_FILE_BYTES) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map_err(|_| CommandError::invalid("The model provider file is not valid JSON.")),
        Err(ArtifactTrustError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(json!({}))
        }
        Err(ArtifactTrustError::Oversized) => Err(CommandError::invalid(
            "The model provider file is too large.",
        )),
        Err(_) => Err(CommandError::internal(
            "The model provider file is not trusted.",
        )),
    }
}

fn providers_mut(document: &mut Value) -> Result<&mut Map<String, Value>, CommandError> {
    let root = document
        .as_object_mut()
        .ok_or_else(|| CommandError::invalid("The model provider file is not valid JSON."))?;
    root.entry("providers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| CommandError::invalid("The model provider file is not valid JSON."))
}

fn persist(agent_dir: &Path, document: &Value) -> Result<(), CommandError> {
    let path = agent_dir.join(FILE);
    let io = |_| CommandError::internal("The model provider file could not be saved.");
    let bytes = serde_json::to_vec_pretty(document).map_err(|_| io(()))?;
    let mut temporary = tempfile::NamedTempFile::new_in(agent_dir).map_err(|_| io(()))?;
    std::io::Write::write_all(&mut temporary, &bytes).map_err(|_| io(()))?;
    temporary.as_file().sync_all().map_err(|_| io(()))?;
    temporary.persist(&path).map_err(|_| io(()))?;
    ensure_private_regular_file(&path).map_err(|_| io(()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(name: &str, url: &str) -> CustomProviderRequest {
        CustomProviderRequest {
            provider: None,
            name: name.into(),
            base_url: url.into(),
            api: "openai-completions".into(),
            api_key: None,
            model_ids: vec!["qwen2.5:7b".into(), " qwen2.5:7b ".into(), "llama3".into()],
        }
    }

    #[test]
    fn writes_only_literal_custom_entries_and_keeps_other_providers() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(FILE),
            r#"{"providers":{"ollama":{"baseUrl":"http://localhost:11434/v1"}},"modelOverrides":{}}"#,
        )
        .unwrap();
        let provider = prepare(
            dir.path(),
            &request("My Ollama", "http://localhost:11434/v1/"),
        )
        .unwrap();
        assert_eq!(provider.id, "custom-my-ollama");
        assert_eq!(provider.base_url, "http://localhost:11434/v1");
        assert_eq!(provider.model_ids, ["qwen2.5:7b", "llama3"]);
        write(dir.path(), &provider).unwrap();
        let saved: Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(FILE)).unwrap()).unwrap();
        assert!(saved["providers"]["ollama"].is_object());
        assert!(saved["modelOverrides"].is_object());
        let entry = &saved["providers"]["custom-my-ollama"];
        // No key, header or interpolation ever reaches models.json.
        assert!(entry.get("apiKey").is_none() && entry.get("headers").is_none());
        assert_eq!(list(dir.path()).unwrap(), vec![provider.clone()]);
        let second = prepare(dir.path(), &request("My Ollama", "https://example.com/v1")).unwrap();
        assert_eq!(second.id, "custom-my-ollama-2");
        remove(dir.path(), &provider.id).unwrap();
        assert!(list(dir.path()).unwrap().is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join(FILE))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn rejects_remote_http_credentials_in_urls_and_bad_ids() {
        let dir = tempfile::tempdir().unwrap();
        for url in [
            "http://example.com/v1",
            "https://user:pass@example.com/v1",
            "https://example.com/v1?key=1",
            "file:///etc/passwd",
            "ftp://localhost/v1",
        ] {
            assert!(
                prepare(dir.path(), &request("Remote", url)).is_err(),
                "{url}"
            );
        }
        for url in [
            "https://api.example.com/v1",
            "http://127.0.0.1:1234/v1",
            "http://[::1]:8080",
        ] {
            assert!(
                prepare(dir.path(), &request("Server", url)).is_ok(),
                "{url}"
            );
        }
        let mut spaced = request("Server", "https://api.example.com/v1");
        spaced.model_ids = vec!["two words".into()];
        assert!(prepare(dir.path(), &spaced).is_err());
        let mut none = request("Server", "https://api.example.com/v1");
        none.model_ids = vec![" ".into()];
        assert!(prepare(dir.path(), &none).is_err());
        let mut api = request("Server", "https://api.example.com/v1");
        api.api = "custom-stream".into();
        assert!(prepare(dir.path(), &api).is_err());
        let mut edit = request("Renamed", "https://api.example.com/v1");
        edit.provider = Some("custom-missing".into());
        assert!(prepare(dir.path(), &edit).is_err());
    }
}
