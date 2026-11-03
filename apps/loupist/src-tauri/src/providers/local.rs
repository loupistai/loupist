//! Models on this Mac and custom endpoints (providers spec §2.2, §3.2, §7):
//! loopback-only discovery of Ollama, LM Studio and llama.cpp, the custom
//! endpoint Test, and the Upstage Solar preset. A Test key is held for the
//! call only; saving goes through the ordinary custom-provider path.
use std::time::Duration;

use futures_util::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::{
    app::{AppState, CommandError},
    providers::{
        catalog::ModelCatalogView,
        custom::{self, CustomProviderRequest},
    },
};

pub const UPSTAGE_PRESET: &str = "upstage";
const UPSTAGE_BASE_URL: &str = "https://api.upstage.ai/v1";
const UPSTAGE_NAME: &str = "Upstage Solar";
const PROBE_TIMEOUT: Duration = Duration::from_millis(600);
const TEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_LISTED_MODELS: usize = 200;
const MAX_LOCAL_MODELS: usize = 20;
const MAX_SAVED_MODELS: usize = 20;
const MAX_MODEL_ID_BYTES: usize = 128;
const MAX_KEY_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalEngineId {
    Ollama,
    LmStudio,
    LlamaCpp,
}

impl LocalEngineId {
    const ALL: [Self; 3] = [Self::Ollama, Self::LmStudio, Self::LlamaCpp];

    fn port(self) -> u16 {
        match self {
            Self::Ollama => 11434,
            Self::LmStudio => 1234,
            Self::LlamaCpp => 8080,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::LlamaCpp => "llama.cpp",
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct LocalModel {
    pub id: String,
    pub name: String,
    pub context_tokens: Option<u64>,
}

/// Exactly `LocalEngine` in `src/contracts/model.ts`.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct LocalEngine {
    pub engine: LocalEngineId,
    pub label: &'static str,
    pub base_url: String,
    pub running: bool,
    pub models: Vec<LocalModel>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct LocalDiscovery {
    pub engines: Vec<LocalEngine>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TestOutcome {
    Ok,
    Unreachable,
    Unauthorized,
    Timeout,
    NotFound,
    WrongDialect,
    Invalid,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ProviderCustomTestResult {
    pub outcome: TestOutcome,
    pub models: Vec<String>,
}

impl ProviderCustomTestResult {
    fn failed(outcome: TestOutcome) -> Self {
        Self {
            outcome,
            models: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderCustomTestRequest {
    pub base_url: String,
    pub api: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderPresetRequest {
    pub preset: String,
    pub api_key: String,
}

/// Probes the three model apps on their default loopback ports, in parallel.
#[tauri::command(rename_all = "snake_case")]
pub async fn provider_local_discover() -> Result<LocalDiscovery, CommandError> {
    let client = loopback_client(PROBE_TIMEOUT)?;
    Ok(LocalDiscovery {
        engines: join_all(LocalEngineId::ALL.map(|engine| {
            let client = client.clone();
            async move {
                probe(
                    &client,
                    engine,
                    &format!("http://127.0.0.1:{}", engine.port()),
                )
                .await
            }
        }))
        .await,
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn provider_custom_test(
    request: ProviderCustomTestRequest,
) -> Result<ProviderCustomTestResult, CommandError> {
    Ok(test_endpoint(&request, TEST_TIMEOUT).await)
}

/// Upstage Solar: a fixed Chat Completions endpoint saved as a custom
/// provider; its models load with the Test call.
#[tauri::command(rename_all = "snake_case")]
pub async fn provider_preset_save(
    request: ProviderPresetRequest,
    state: State<'_, AppState>,
) -> Result<ModelCatalogView, CommandError> {
    if request.preset != UPSTAGE_PRESET {
        return Err(CommandError::invalid("unknown provider preset"));
    }
    let api_key = request.api_key.trim();
    if api_key.is_empty() {
        return Err(key_rejected());
    }
    let tested = test_endpoint(
        &ProviderCustomTestRequest {
            base_url: UPSTAGE_BASE_URL.into(),
            api: "openai-completions".into(),
            api_key: Some(api_key.to_owned()),
        },
        TEST_TIMEOUT,
    )
    .await;
    let model_ids = preset_models(tested)?;
    let agent_dir = state.pi_agent_data_dir()?;
    let existing = custom::list(&agent_dir)?
        .into_iter()
        .find(|provider| provider.base_url == UPSTAGE_BASE_URL)
        .map(|provider| provider.id);
    crate::providers::connection::save_custom_provider(
        &state,
        CustomProviderRequest {
            provider: existing,
            name: UPSTAGE_NAME.into(),
            base_url: UPSTAGE_BASE_URL.into(),
            api: "openai-completions".into(),
            api_key: Some(api_key.to_owned()),
            model_ids,
        },
    )
    .await
}

fn key_rejected() -> CommandError {
    CommandError::new("key_rejected", "Upstage didn't accept this key.")
}

/// Chat models from Upstage's list; embeddings and document tools stay out.
fn preset_models(tested: ProviderCustomTestResult) -> Result<Vec<String>, CommandError> {
    match tested.outcome {
        TestOutcome::Ok => {}
        TestOutcome::Unauthorized => return Err(key_rejected()),
        _ => {
            return Err(CommandError::new(
                "offline",
                "Couldn't reach Upstage. Check your connection, then try again.",
            ))
        }
    }
    let models = tested
        .models
        .into_iter()
        .filter(|id| id.starts_with("solar") && !id.contains("embedding"))
        .take(MAX_SAVED_MODELS)
        .collect::<Vec<_>>();
    if models.is_empty() {
        return Err(CommandError::new(
            "offline",
            "Upstage didn't list a chat model for this key.",
        ));
    }
    Ok(models)
}

fn loopback_client(timeout: Duration) -> Result<reqwest::Client, CommandError> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|_| CommandError::internal("local model probe is unavailable"))
}

async fn json_body(response: reqwest::Response) -> Option<Value> {
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    if bytes.len() > MAX_BODY_BYTES {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

async fn get_json(client: &reqwest::Client, url: &str) -> Option<Value> {
    json_body(client.get(url).send().await.ok()?).await
}

fn model_id(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= MAX_MODEL_ID_BYTES
        && !value.chars().any(|c| c.is_control() || c.is_whitespace()))
    .then(|| value.to_owned())
}

fn context(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(Value::as_u64)
        .filter(|tokens| *tokens > 0 && *tokens <= 100_000_000)
}

/// `data[].id` from an OpenAI-style list (Ollama, LM Studio and llama.cpp all serve one).
fn listed_ids(body: &Value) -> Vec<String> {
    body.get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("id").and_then(Value::as_str).and_then(model_id))
        .take(MAX_LOCAL_MODELS)
        .collect()
}

async fn probe(client: &reqwest::Client, engine: LocalEngineId, origin: &str) -> LocalEngine {
    let models = match engine {
        LocalEngineId::Ollama => probe_ollama(client, origin).await,
        LocalEngineId::LmStudio => probe_lm_studio(client, origin).await,
        LocalEngineId::LlamaCpp => probe_llama_cpp(client, origin).await,
    };
    LocalEngine {
        engine,
        label: engine.label(),
        base_url: format!("http://localhost:{}/v1", engine.port()),
        running: models.is_some(),
        models: models.unwrap_or_default(),
    }
}

async fn probe_ollama(client: &reqwest::Client, origin: &str) -> Option<Vec<LocalModel>> {
    let tags = get_json(client, &format!("{origin}/api/tags")).await?;
    let names = tags
        .get("models")
        .and_then(Value::as_array)?
        .iter()
        .filter_map(|entry| {
            entry
                .get("model")
                .or_else(|| entry.get("name"))
                .and_then(Value::as_str)
                .and_then(model_id)
        })
        .take(MAX_LOCAL_MODELS)
        .collect::<Vec<_>>();
    Some(
        join_all(names.into_iter().map(|name| async move {
            // The context Ollama serves with is `num_ctx` when the model sets it.
            let shown = match client
                .post(format!("{origin}/api/show"))
                .json(&serde_json::json!({ "model": name }))
                .send()
                .await
            {
                Ok(response) => json_body(response).await,
                Err(_) => None,
            };
            let context_tokens = shown
                .as_ref()
                .and_then(|shown| shown.get("parameters"))
                .and_then(Value::as_str)
                .and_then(|parameters| {
                    parameters.lines().find_map(|line| {
                        let mut parts = line.split_whitespace();
                        (parts.next() == Some("num_ctx"))
                            .then(|| parts.next()?.parse::<u64>().ok())
                            .flatten()
                    })
                })
                .filter(|tokens| *tokens > 0);
            LocalModel {
                name: name.clone(),
                id: name,
                context_tokens,
            }
        }))
        .await,
    )
}

async fn probe_lm_studio(client: &reqwest::Client, origin: &str) -> Option<Vec<LocalModel>> {
    if let Some(body) = get_json(client, &format!("{origin}/api/v0/models")).await {
        let models = body
            .get("data")
            .and_then(Value::as_array)?
            .iter()
            .filter(|entry| {
                !matches!(
                    entry.get("type").and_then(Value::as_str),
                    Some("embeddings")
                )
            })
            .filter_map(|entry| {
                let id = entry.get("id").and_then(Value::as_str).and_then(model_id)?;
                Some(LocalModel {
                    name: id.clone(),
                    id,
                    context_tokens: context(entry.get("loaded_context_length"))
                        .or_else(|| context(entry.get("max_context_length"))),
                })
            })
            .take(MAX_LOCAL_MODELS)
            .collect();
        return Some(models);
    }
    let body = get_json(client, &format!("{origin}/v1/models")).await?;
    Some(
        listed_ids(&body)
            .into_iter()
            .map(|id| LocalModel {
                name: id.clone(),
                id,
                context_tokens: None,
            })
            .collect(),
    )
}

async fn probe_llama_cpp(client: &reqwest::Client, origin: &str) -> Option<Vec<LocalModel>> {
    let (models_url, props_url) = (format!("{origin}/v1/models"), format!("{origin}/props"));
    let (models, props) = tokio::join!(get_json(client, &models_url), get_json(client, &props_url));
    let models = models?;
    let context_tokens = props.as_ref().and_then(|props| {
        context(props.pointer("/default_generation_settings/n_ctx"))
            .or_else(|| context(props.get("n_ctx")))
    });
    Some(
        listed_ids(&models)
            .into_iter()
            .map(|id| LocalModel {
                name: id.clone(),
                id,
                context_tokens,
            })
            .collect(),
    )
}

fn loopback(url: &reqwest::Url) -> bool {
    matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    )
}

/// One bounded `models` call. Outcomes map one-to-one to the Test copy.
pub(crate) async fn test_endpoint(
    request: &ProviderCustomTestRequest,
    timeout: Duration,
) -> ProviderCustomTestResult {
    let Ok(base) = custom::validate_base_url(request.base_url.trim()) else {
        return ProviderCustomTestResult::failed(TestOutcome::Invalid);
    };
    if !custom::APIS.contains(&request.api.as_str()) {
        return ProviderCustomTestResult::failed(TestOutcome::Invalid);
    }
    let key = request
        .api_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty());
    if key.is_some_and(|key| key.len() > MAX_KEY_BYTES || key.chars().any(char::is_control)) {
        return ProviderCustomTestResult::failed(TestOutcome::Invalid);
    }
    let anthropic = request.api == "anthropic-messages";
    let url = if anthropic {
        format!("{base}/v1/models")
    } else {
        format!("{base}/models")
    };
    let Ok(parsed) = reqwest::Url::parse(&url) else {
        return ProviderCustomTestResult::failed(TestOutcome::Invalid);
    };
    let mut builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout);
    if loopback(&parsed) {
        builder = builder.no_proxy();
    }
    let Ok(client) = builder.build() else {
        return ProviderCustomTestResult::failed(TestOutcome::Unreachable);
    };
    let mut call = client.get(parsed);
    if anthropic {
        call = call.header("anthropic-version", "2023-06-01");
        if let Some(key) = key {
            call = call.header("x-api-key", key);
        }
    } else if let Some(key) = key {
        call = call.bearer_auth(key);
    }
    let response = match call.send().await {
        Ok(response) => response,
        Err(error) if error.is_timeout() => {
            return ProviderCustomTestResult::failed(TestOutcome::Timeout)
        }
        Err(_) => return ProviderCustomTestResult::failed(TestOutcome::Unreachable),
    };
    let status = response.status().as_u16();
    match status {
        200..=299 => {}
        401 | 403 => return ProviderCustomTestResult::failed(TestOutcome::Unauthorized),
        // An Anthropic dialect on an address that already ends in /v1 asks
        // for /v1/v1/models: the answer is "another dialect", not "add /v1".
        404 if anthropic && base.ends_with("/v1") => {
            return ProviderCustomTestResult::failed(TestOutcome::WrongDialect)
        }
        404 | 300..=399 => return ProviderCustomTestResult::failed(TestOutcome::NotFound),
        500..=599 => return ProviderCustomTestResult::failed(TestOutcome::Unreachable),
        _ => return ProviderCustomTestResult::failed(TestOutcome::WrongDialect),
    }
    let body = match tokio::time::timeout(timeout, json_body(response)).await {
        Ok(Some(body)) => body,
        Ok(None) => return ProviderCustomTestResult::failed(TestOutcome::WrongDialect),
        Err(_) => return ProviderCustomTestResult::failed(TestOutcome::Timeout),
    };
    match listed_models(&body, anthropic) {
        Some(models) => ProviderCustomTestResult {
            outcome: TestOutcome::Ok,
            models,
        },
        None => ProviderCustomTestResult::failed(TestOutcome::WrongDialect),
    }
}

/// `data[]` of model objects. Anthropic objects carry `type: "model"`; OpenAI
/// ones `object: "model"`. The other dialect's shape is a wrong-dialect answer.
fn listed_models(body: &Value, anthropic: bool) -> Option<Vec<String>> {
    let entries = body.get("data")?.as_array()?;
    let mut models = Vec::new();
    for entry in entries.iter().take(MAX_LISTED_MODELS) {
        let entry = entry.as_object()?;
        let openai_shape = entry.get("object").and_then(Value::as_str) == Some("model");
        let anthropic_shape = entry.get("type").and_then(Value::as_str) == Some("model");
        if (anthropic && openai_shape && !anthropic_shape)
            || (!anthropic && anthropic_shape && !openai_shape)
        {
            return None;
        }
        let Some(id) = entry.get("id").and_then(Value::as_str).and_then(model_id) else {
            continue;
        };
        if !models.contains(&id) {
            models.push(id);
        }
    }
    Some(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom_connections::fixture::{Response, Server};
    use serde_json::json;

    fn request(base_url: &str, api: &str, api_key: Option<&str>) -> ProviderCustomTestRequest {
        ProviderCustomTestRequest {
            base_url: base_url.into(),
            api: api.into(),
            api_key: api_key.map(str::to_owned),
        }
    }

    #[tokio::test]
    async fn local_engines_report_models_and_their_serving_context() {
        let ollama = Server::start(|_, request| match request.target.as_str() {
            "/api/tags" => Response::json(json!({"models": [
                {"name": "qwen3:32b", "model": "qwen3:32b"},
                {"name": "llama3.2:3b", "model": "llama3.2:3b"},
            ]})),
            "/api/show" if request.json()["model"] == "qwen3:32b" => {
                Response::json(json!({"parameters": "stop \"<|im_end|>\"\nnum_ctx 40960"}))
            }
            "/api/show" => Response::json(json!({"parameters": "stop \"x\""})),
            _ => Response::status(404),
        })
        .await;
        let lm_studio = Server::start(|_, request| match request.target.as_str() {
            "/api/v0/models" => Response::json(json!({"data": [
                {"id": "qwen2.5-7b-instruct", "type": "llm", "loaded_context_length": 4096, "max_context_length": 32768},
                {"id": "nomic-embed", "type": "embeddings", "max_context_length": 2048},
                {"id": "mistral-small", "type": "llm", "max_context_length": 32768},
            ]})),
            _ => Response::status(404),
        })
        .await;
        let llama_cpp = Server::start(|_, request| match request.target.as_str() {
            "/v1/models" => Response::json(
                json!({"object": "list", "data": [{"id": "gemma.gguf", "object": "model"}]}),
            ),
            "/props" => Response::json(json!({"default_generation_settings": {"n_ctx": 8192}})),
            _ => Response::status(404),
        })
        .await;
        let client = loopback_client(PROBE_TIMEOUT).unwrap();
        let found = probe(&client, LocalEngineId::Ollama, &ollama.url).await;
        assert!(found.running);
        assert_eq!(found.base_url, "http://localhost:11434/v1");
        assert_eq!(
            found.models,
            vec![
                LocalModel {
                    id: "qwen3:32b".into(),
                    name: "qwen3:32b".into(),
                    context_tokens: Some(40_960)
                },
                LocalModel {
                    id: "llama3.2:3b".into(),
                    name: "llama3.2:3b".into(),
                    context_tokens: None
                },
            ]
        );
        let found = probe(&client, LocalEngineId::LmStudio, &lm_studio.url).await;
        assert_eq!(
            found
                .models
                .iter()
                .map(|model| (model.id.as_str(), model.context_tokens))
                .collect::<Vec<_>>(),
            [
                ("qwen2.5-7b-instruct", Some(4096)),
                ("mistral-small", Some(32_768))
            ]
        );
        let found = probe(&client, LocalEngineId::LlamaCpp, &llama_cpp.url).await;
        assert_eq!(found.models[0].context_tokens, Some(8192));
        assert_eq!(found.label, "llama.cpp");
        let serialized = serde_json::to_value(&found).unwrap();
        assert_eq!(serialized["engine"], "llama_cpp");

        // A closed port is an app that isn't running.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let off = probe(&client, LocalEngineId::LmStudio, &origin).await;
        assert!(!off.running && off.models.is_empty());
    }

    #[tokio::test]
    async fn custom_test_outcomes_map_one_to_one() {
        let server = Server::start(|_, request| {
            let authorized = request.headers.get("authorization").map(String::as_str)
                == Some("Bearer good-key")
                || request.headers.get("x-api-key").map(String::as_str) == Some("good-key");
            match request.target.as_str() {
                "/v1/models" if !authorized => Response::status(401),
                "/v1/models" => Response::json(json!({"object": "list", "data": [
                    {"id": "qwen2.5-72b", "object": "model"}, {"id": "qwen2.5-72b", "object": "model"},
                    {"id": "has space", "object": "model"},
                ]})),
                "/anthropic/v1/models" => Response::json(json!({"data": [{"id": "claude-x", "type": "model"}]})),
                "/odd/models" => Response::json(json!({"models": []})),
                "/broken/models" => Response::status(500),
                "/teapot/models" => Response::status(418),
                _ => Response::status(404),
            }
        })
        .await;
        let base = format!("{}/v1", server.url);
        let ok = test_endpoint(
            &request(&base, "openai-completions", Some("good-key")),
            TEST_TIMEOUT,
        )
        .await;
        assert_eq!(
            ok,
            ProviderCustomTestResult {
                outcome: TestOutcome::Ok,
                models: vec!["qwen2.5-72b".into()]
            }
        );
        let responses = test_endpoint(
            &request(&base, "openai-responses", Some("good-key")),
            TEST_TIMEOUT,
        )
        .await;
        assert_eq!(responses.outcome, TestOutcome::Ok);
        for (base_url, api, key, expected) in [
            (
                base.clone(),
                "openai-completions",
                None,
                TestOutcome::Unauthorized,
            ),
            (
                base.clone(),
                "openai-completions",
                Some("bad"),
                TestOutcome::Unauthorized,
            ),
            (
                server.url.clone(),
                "openai-completions",
                Some("good-key"),
                TestOutcome::NotFound,
            ),
            (
                format!("{}/odd", server.url),
                "openai-completions",
                None,
                TestOutcome::WrongDialect,
            ),
            (
                format!("{}/broken", server.url),
                "openai-completions",
                None,
                TestOutcome::Unreachable,
            ),
            (
                format!("{}/teapot", server.url),
                "openai-completions",
                None,
                TestOutcome::WrongDialect,
            ),
            // The Anthropic dialect against an OpenAI-style /v1 address.
            (
                base.clone(),
                "anthropic-messages",
                Some("good-key"),
                TestOutcome::WrongDialect,
            ),
            (
                "http://example.com/v1".into(),
                "openai-completions",
                None,
                TestOutcome::Invalid,
            ),
            (
                "https://user:pass@example.com/v1".into(),
                "openai-completions",
                None,
                TestOutcome::Invalid,
            ),
            (base.clone(), "gemini", None, TestOutcome::Invalid),
            (
                base.clone(),
                "openai-completions",
                Some("line\nbreak"),
                TestOutcome::Invalid,
            ),
        ] {
            assert_eq!(
                test_endpoint(&request(&base_url, api, key), TEST_TIMEOUT)
                    .await
                    .outcome,
                expected,
                "{base_url} {api} {key:?}"
            );
        }
        // The Anthropic dialect lists Anthropic-shaped models from {base}/v1/models.
        let anthropic = test_endpoint(
            &request(
                &format!("{}/anthropic", server.url),
                "anthropic-messages",
                Some("good-key"),
            ),
            TEST_TIMEOUT,
        )
        .await;
        assert_eq!(anthropic.models, vec!["claude-x".to_owned()]);
        assert!(server
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|request| request.headers.contains_key("anthropic-version")));

        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}/v1", closed.local_addr().unwrap());
        drop(closed);
        assert_eq!(
            test_endpoint(&request(&origin, "openai-completions", None), TEST_TIMEOUT)
                .await
                .outcome,
            TestOutcome::Unreachable
        );
    }

    #[tokio::test]
    async fn custom_test_reports_a_slow_server_as_a_timeout() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let hold = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(5)).await;
        });
        let outcome = test_endpoint(
            &request(&format!("http://{address}/v1"), "openai-completions", None),
            Duration::from_millis(200),
        )
        .await
        .outcome;
        assert_eq!(outcome, TestOutcome::Timeout);
        hold.abort();
    }

    #[test]
    fn the_upstage_preset_keeps_chat_models_and_refuses_a_rejected_key() {
        assert_eq!(
            preset_models(ProviderCustomTestResult {
                outcome: TestOutcome::Ok,
                models: vec![
                    "solar-pro2".into(),
                    "embedding-query".into(),
                    "solar-embedding-1".into(),
                    "solar-mini".into()
                ],
            })
            .unwrap(),
            vec!["solar-pro2".to_owned(), "solar-mini".to_owned()]
        );
        assert_eq!(
            preset_models(ProviderCustomTestResult::failed(TestOutcome::Unauthorized))
                .unwrap_err()
                .code,
            "key_rejected"
        );
        assert_eq!(
            preset_models(ProviderCustomTestResult::failed(TestOutcome::Timeout))
                .unwrap_err()
                .code,
            "offline"
        );
    }
}
