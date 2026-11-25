//! Host-mediated model calls for the graph worker.
//!
//! The worker never receives provider credentials. Enrichment stays off until
//! a helper model is saved. A Chat provider selection does not enable it.
//! One attempt is made. A timeout after the request is sent is uncertain and
//! is not retried here.

use std::path::Path;

use serde_json::{json, Value};

use super::graph_jobs::{GraphProcessingSettings, MAX_TIMEOUT_MS};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceDisposition {
    NotSent,
    Completed,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InferenceReply {
    pub disposition: InferenceDisposition,
    pub body: Value,
}

/// Why a call went unsent: the saved World settings no longer match the ones
/// its attempt started with. The attempt is then requeued on the new settings
/// ([`super::graph_jobs::Unpublished::settings_changed`]).
const SETTINGS_CHANGED: &str = "settings_changed";

/// The controls a World attempt keeps from its start. A saved change to any of
/// them stops the attempt.
pub(crate) fn controls_changed(
    saved: &GraphProcessingSettings,
    frozen: &GraphProcessingSettings,
) -> bool {
    saved.helper_provider != frozen.helper_provider
        || saved.helper_model != frozen.helper_model
        || saved.thinking_level != frozen.thinking_level
        || saved.run_options != frozen.run_options
        || saved.max_llm_calls != frozen.max_llm_calls
        || saved.timeout_ms != frozen.timeout_ms
}

/// A call refused before dispatch because the World settings changed.
pub(crate) fn settings_changed_reply() -> InferenceReply {
    InferenceReply {
        disposition: InferenceDisposition::NotSent,
        body: json!({"status": "not_sent", "reason": SETTINGS_CHANGED}),
    }
}

pub(crate) fn refused_for_settings(reply: &InferenceReply) -> bool {
    reply.disposition == InferenceDisposition::NotSent && reply.body["reason"] == SETTINGS_CHANGED
}

/// World settings name the bundled local model by this id, whichever model
/// is bundled. Stored vectors are tied to the asset fingerprint, not to it.
pub(crate) const LOCAL_EMBEDDING_PROFILE: &str = "local-en-128";

/// Local embeddings are a CPU operation. They do not require paid
/// enrichment or provider credentials.
pub fn admit_local_embedding(settings: &GraphProcessingSettings) -> Result<(), &'static str> {
    if settings.paused {
        return Err("graph processing is paused");
    }
    if settings.embedding_profile != LOCAL_EMBEDDING_PROFILE {
        return Err("graph embedding profile is not the bundled local model");
    }
    if settings.max_embedding_calls == 0 {
        return Err("local embedding budget is closed");
    }
    if settings.timeout_ms == 0 || settings.timeout_ms > MAX_TIMEOUT_MS {
        return Err("graph helper timeout is invalid");
    }
    Ok(())
}

pub fn authorize(settings: &GraphProcessingSettings, kind: &str) -> Result<(), &'static str> {
    if kind == "embedding" && settings.embedding_profile == LOCAL_EMBEDDING_PROFILE {
        return admit_local_embedding(settings);
    }
    if !settings.enrichment_enabled {
        return Err("paid graph enrichment is not enabled");
    }
    if settings.helper_provider.as_deref().unwrap_or("").is_empty()
        || settings.helper_model.as_deref().unwrap_or("").is_empty()
    {
        return Err("graph helper model is not configured");
    }
    if kind == "llm" || kind == "embedding" {
        return Ok(());
    }
    Err("inference kind is unsupported")
}

const MAX_PROMPT_CHARS: usize = 192 * 1024;
const MAX_EMBEDDING_CHARS: usize = 8_000;
pub(super) const MAX_ANSWER_TOKENS: u32 = 8_192;
const REASONING_ALLOWANCE_TOKENS: u32 = 8_192;
// Graphiti asks for answer tokens; reasoning APIs share that cap with hidden
// thinking. Reserve room without allowing worker-controlled unbounded output.
fn output_allowance(requested: Option<u64>, thinking_level: &str) -> u32 {
    let answer = requested.unwrap_or(256).clamp(1, MAX_ANSWER_TOKENS as u64) as u32;
    if thinking_level == "off" {
        answer
    } else {
        answer + REASONING_ALLOWANCE_TOKENS
    }
}

/// Availability comes from the host's shared Providers registry, never worker input.
pub fn admit_helper(_provider: &str, auth_mode: &str) -> Result<(), &'static str> {
    if matches!(auth_mode, "api_key" | "oauth" | "environment") {
        return Ok(());
    }
    #[cfg(any(test, feature = "e2e"))]
    if auth_mode == "fixture" {
        return Ok(());
    }
    Err("World model provider is disconnected")
}

fn embed_locally(
    settings: &GraphProcessingSettings,
    assets: &Path,
    request: &Value,
) -> InferenceReply {
    if let Err(message) = admit_local_embedding(settings) {
        return not_sent(message);
    }
    if let Some(loupist) = request.pointer("/scope/loupist_id").and_then(Value::as_str) {
        if settings.inactive_loupists.iter().any(|id| id == loupist) {
            return not_sent("graph processing is inactive for this loupist");
        }
    }
    let timeout_ms = request
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(settings.timeout_ms);
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return not_sent("graph helper timeout is invalid");
    }
    let inputs = match request.get("inputs").and_then(Value::as_array) {
        Some(inputs) if !inputs.is_empty() && inputs.len() <= 256 => inputs,
        _ => return not_sent("graph embedding inputs are invalid"),
    };
    let mut texts = Vec::with_capacity(inputs.len());
    for input in inputs {
        let text = input.as_str().unwrap_or("");
        if text.len() > MAX_EMBEDDING_CHARS {
            return not_sent("graph embedding input exceeds the request budget");
        }
        texts.push(text.to_owned());
    }
    let vectors = match embed_local_batch(assets, &texts) {
        Ok(vectors) => vectors,
        Err(_) => return uncertain("local embedding failed"),
    };
    InferenceReply {
        disposition: InferenceDisposition::Completed,
        body: json!({
            "status": "ok",
            "json_object": null,
            "vectors": vectors,
            "usage": {"input_tokens": 0, "output_tokens": 0, "certainty": "accounted"}
        }),
    }
}

/// Small Graphiti calls may name the saved fast model. Any other profile stays
/// on the Loupist's processing model, so the worker cannot select an arbitrary one.
fn dispatch_provider(settings: &GraphProcessingSettings, request: &Value) -> (String, String) {
    let requested = request
        .get("model_profile")
        .and_then(Value::as_str)
        .unwrap_or("");
    if let Some(saved) = settings
        .fast_model
        .as_deref()
        .filter(|saved| !saved.is_empty())
    {
        if saved == requested {
            if let Some((provider, model)) = saved.split_once('/') {
                if !provider.is_empty() && !model.is_empty() {
                    return (provider.to_string(), model.to_string());
                }
            }
        }
    }
    (
        settings.helper_provider.clone().unwrap_or_default(),
        settings.helper_model.clone().unwrap_or_default(),
    )
}

pub async fn dispatch_inference(
    settings: &GraphProcessingSettings,
    service: Option<&crate::providers::inference::ProviderInferenceService>,
    assets: &Path,
    request: &Value,
) -> InferenceReply {
    let kind = request.get("kind").and_then(Value::as_str).unwrap_or("");
    if kind == "embedding" && settings.embedding_profile == LOCAL_EMBEDDING_PROFILE {
        // Encoding, and waiting for a first load of the model, stay off the
        // async runtime's threads.
        let (settings, assets, request) = (settings.clone(), assets.to_path_buf(), request.clone());
        return tokio::task::spawn_blocking(move || embed_locally(&settings, &assets, &request))
            .await
            .unwrap_or_else(|_| uncertain("local embedding failed"));
    }
    if settings.paused {
        return not_sent("World processing is paused");
    }
    if let Err(message) = authorize(settings, kind) {
        return not_sent(message);
    }
    if kind != "llm" {
        return not_sent("World embeddings must use the local model");
    }
    let Some(service) = service else {
        return not_sent("shared provider runtime is unavailable");
    };
    let messages = match request.get("messages").and_then(Value::as_array) {
        Some(messages) if !messages.is_empty() && messages.len() <= 16 => messages,
        _ => return not_sent("World model messages are invalid"),
    };
    let mut converted = Vec::new();
    let mut prompt_chars = 0usize;
    for message in messages {
        let Some(content) = message.get("content").and_then(Value::as_str) else {
            return not_sent("World model message content is invalid");
        };
        let role = message.get("role").and_then(Value::as_str).unwrap_or("");
        if !matches!(role, "system" | "user" | "assistant") {
            return not_sent("World model message role is invalid");
        }
        prompt_chars = prompt_chars.saturating_add(content.len());
        converted.push(crate::providers::inference::ProviderInferenceMessage {
            role: role.into(),
            content: content.into(),
        });
    }
    if prompt_chars > MAX_PROMPT_CHARS {
        return not_sent("World model prompt exceeds the request budget");
    }
    let timeout_ms = request
        .get("timeout_ms")
        .and_then(Value::as_u64)
        .unwrap_or(settings.timeout_ms)
        .min(settings.timeout_ms);
    if timeout_ms == 0 || timeout_ms > MAX_TIMEOUT_MS {
        return not_sent("World model timeout is invalid");
    }
    let schema = match request.get("response_schema") {
        Some(value) if value.is_object() => value.clone(),
        Some(Value::Null) | None => json!({"type":"object"}),
        _ => return not_sent("World response schema is invalid"),
    };
    let (provider, model) = dispatch_provider(settings, request);
    let reply = service
        .infer(crate::providers::inference::ProviderInferenceRequest {
            provider,
            model,
            messages: converted,
            schema,
            max_output_tokens: output_allowance(
                request.get("max_tokens").and_then(Value::as_u64),
                &settings.thinking_level,
            ),
            timeout_ms,
            thinking_level: settings.thinking_level.clone(),
            run_options: settings.run_options.clone(),
        })
        .await;
    use crate::providers::inference::ProviderInferenceDisposition;
    match reply.disposition {
        ProviderInferenceDisposition::NotSent => not_sent("World provider request was not sent"),
        ProviderInferenceDisposition::Uncertain => uncertain("World provider outcome is uncertain"),
        ProviderInferenceDisposition::Completed => InferenceReply {
            disposition: InferenceDisposition::Completed,
            body: json!({"status":"ok", "json_object":reply.body, "vectors":null, "usage":graph_usage(reply.usage.as_ref())}),
        },
    }
}

fn graph_usage(usage: Option<&Value>) -> Value {
    let input = usage.and_then(|usage| {
        usage
            .get("input")?
            .as_u64()?
            .checked_add(usage.get("cacheRead")?.as_u64()?)?
            .checked_add(usage.get("cacheWrite")?.as_u64()?)
    });
    let output = usage
        .and_then(|usage| usage.get("output"))
        .and_then(Value::as_u64);
    json!({"input_tokens":input, "output_tokens":output,
        "input":usage.and_then(|u| u.get("input")).and_then(Value::as_u64),
        "cacheRead":usage.and_then(|u| u.get("cacheRead")).and_then(Value::as_u64),
        "cacheWrite":usage.and_then(|u| u.get("cacheWrite")).and_then(Value::as_u64),
        "cost":usage.and_then(|u| u.get("cost")),
        "certainty":if input.is_some() && output.is_some() {"accounted"} else {"uncertain"}})
}

/// The bundled embedding model, loaded from one verified set of asset bytes.
/// Clones share one loaded model.
#[derive(Clone)]
pub struct LocalModel(std::sync::Arc<LoadedModel>);

struct LoadedModel {
    model: model2vec_rs::model::StaticModel,
    /// The exact loaded bytes plus encoding settings. Changing bundled assets
    /// across app runs cannot reuse another model's document vectors.
    fingerprint: String,
}

impl std::fmt::Debug for LocalModel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("LocalModel")
            .field(&self.0.fingerprint)
            .finish()
    }
}

impl LocalModel {
    pub(crate) fn fingerprint(&self) -> &str {
        &self.0.fingerprint
    }

    /// Width of the model's vectors.
    pub(crate) fn dimension(&self) -> usize {
        self.0.model.dim()
    }

    /// Bounded tokenization batches preserve the single-text model's exact pooling.
    pub(crate) fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let vectors = self.0.model.encode_with_args(texts, Some(512), 128);
        let dimension = self.dimension();
        if vectors.len() != texts.len() || vectors.iter().any(|vector| vector.len() != dimension) {
            return Err("local embedding dimensions are invalid".to_owned());
        }
        Ok(vectors)
    }

    pub(crate) fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        self.embed_batch(&[text.to_owned()])?
            .pop()
            .ok_or_else(|| "local embedding is missing".to_owned())
    }

    #[cfg(test)]
    pub(crate) fn same(&self, other: &LocalModel) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The model for `assets`, loading it first if needed. A load another caller
/// started is awaited, not repeated. For background work, which may wait.
pub(crate) fn local_model(assets: &Path) -> Result<LocalModel, String> {
    local_models().get(assets)
}

/// What an interactive search can rank with now.
#[derive(Clone, Debug, Default)]
pub(crate) struct ModelLookup {
    /// The loaded model, if it has loaded.
    pub(crate) model: Option<LocalModel>,
    /// No model yet, and a load is under way: one this lookup started or one
    /// already running. A search without the model says so.
    pub(crate) loading: bool,
}

/// The model for `assets` if it is already loaded. Otherwise this starts one
/// load on its own thread and returns without waiting, so an interactive
/// search can answer from words alone meanwhile. Both answers come from one
/// look at the cache: a load that ends right after cannot make a search that
/// ranked without the model look complete. A load that failed is not retried
/// until the asset files change.
pub(crate) fn loaded_local_model(assets: &Path) -> ModelLookup {
    local_models().loaded(assets)
}

pub(crate) fn embed_local_batch(assets: &Path, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
    local_model(assets)?.embed_batch(texts)
}

fn local_models() -> &'static LocalModels {
    static MODELS: std::sync::OnceLock<LocalModels> = std::sync::OnceLock::new();
    MODELS.get_or_init(LocalModels::default)
}

const LOCAL_ASSET_NAMES: [&str; 3] = ["tokenizer.json", "model.safetensors", "config.json"];

#[derive(Clone, PartialEq, Eq)]
struct AssetStamp {
    len: u64,
    modified: std::time::SystemTime,
    // Also detect same-size replacements or writes whose mtime was restored.
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

fn asset_stamps(assets: &Path) -> Result<[AssetStamp; 3], String> {
    let stamps = LOCAL_ASSET_NAMES.map(|name| -> Result<AssetStamp, String> {
        let metadata = std::fs::metadata(assets.join(name)).map_err(|error| error.to_string())?;
        Ok(AssetStamp {
            len: metadata.len(),
            modified: metadata.modified().map_err(|error| error.to_string())?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    });
    let [tokenizer, weights, config] = stamps;
    Ok([tokenizer?, weights?, config?])
}

struct VerifiedAssets {
    stamps: [AssetStamp; 3],
    model: LocalModel,
}

/// Loaded models and the asset directories verified in this process. The
/// lock is held only for bookkeeping; reading, hashing and building a model
/// happen outside it, so a lookup never waits behind another directory's load.
#[derive(Default)]
struct LocalModels {
    cache: std::sync::Mutex<LocalModelCache>,
    loaded: std::sync::Condvar,
}

#[derive(Default)]
struct LocalModelCache {
    /// Verified asset directories by canonical path.
    assets: std::collections::HashMap<std::path::PathBuf, VerifiedAssets>,
    /// Loaded models by fingerprint: the same bytes from another path share one.
    models: std::collections::HashMap<String, LocalModel>,
    /// Directories being verified and loaded now.
    loading: std::collections::HashSet<std::path::PathBuf>,
    /// The stamps a directory's last load failed with.
    failed: std::collections::HashMap<std::path::PathBuf, [AssetStamp; 3]>,
    #[cfg(test)]
    verifications: usize,
}

impl LocalModelCache {
    fn verified(&self, assets: &Path, stamps: &[AssetStamp; 3]) -> Option<LocalModel> {
        self.assets
            .get(assets)
            .filter(|verified| &verified.stamps == stamps)
            .map(|verified| verified.model.clone())
    }
}

/// Clears a directory's loading mark however its load ends, panics included,
/// and wakes the callers waiting for it.
struct LoadingMark<'a> {
    models: &'a LocalModels,
    assets: std::path::PathBuf,
}

impl Drop for LoadingMark<'_> {
    fn drop(&mut self) {
        if let Ok(mut cache) = self.models.cache.lock() {
            cache.loading.remove(&self.assets);
        }
        self.models.loaded.notify_all();
    }
}

impl LocalModels {
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, LocalModelCache>, String> {
        self.cache
            .lock()
            .map_err(|_| "embedding initialization failed".to_owned())
    }

    fn get(&self, assets: &Path) -> Result<LocalModel, String> {
        let assets = assets.canonicalize().map_err(|error| error.to_string())?;
        let mut cache = self.lock()?;
        loop {
            let stamps = asset_stamps(&assets)?;
            if let Some(model) = cache.verified(&assets, &stamps) {
                return Ok(model);
            }
            if cache.loading.insert(assets.clone()) {
                break;
            }
            cache = self
                .loaded
                .wait(cache)
                .map_err(|_| "embedding initialization failed".to_owned())?;
        }
        drop(cache);
        self.load(assets)
    }

    fn loaded(&'static self, assets: &Path) -> ModelLookup {
        let unavailable = ModelLookup::default();
        let Ok(assets) = assets.canonicalize() else {
            return unavailable;
        };
        let Ok(stamps) = asset_stamps(&assets) else {
            return unavailable;
        };
        let Ok(mut cache) = self.lock() else {
            return unavailable;
        };
        if let Some(model) = cache.verified(&assets, &stamps) {
            return ModelLookup {
                model: Some(model),
                loading: false,
            };
        }
        if cache.failed.get(&assets) == Some(&stamps) {
            return unavailable;
        }
        let loading = ModelLookup {
            model: None,
            loading: true,
        };
        if !cache.loading.insert(assets.clone()) {
            return loading;
        }
        drop(cache);
        let background = assets.clone();
        let started = std::thread::Builder::new()
            .name("loupist-embedding-load".into())
            .spawn(move || {
                let _ = self.load(background);
            });
        if started.is_err() {
            // Clear the mark the thread would have cleared.
            drop(LoadingMark {
                models: self,
                assets,
            });
            return unavailable;
        }
        loading
    }

    /// Verify and load `assets`, whose loading mark the caller set.
    fn load(&self, assets: std::path::PathBuf) -> Result<LocalModel, String> {
        let _mark = LoadingMark {
            models: self,
            assets: assets.clone(),
        };
        let stamps = asset_stamps(&assets)?;
        let loaded = self.verify_and_build(&assets, &stamps);
        let mut cache = self.lock()?;
        #[cfg(test)]
        {
            cache.verifications += 1;
        }
        match &loaded {
            Ok(model) => {
                cache.failed.remove(&assets);
                cache.assets.insert(
                    assets,
                    VerifiedAssets {
                        stamps,
                        model: model.clone(),
                    },
                );
            }
            Err(_) => {
                cache.failed.insert(assets, stamps);
            }
        }
        loaded
    }

    fn verify_and_build(
        &self,
        assets: &Path,
        stamps: &[AssetStamp; 3],
    ) -> Result<LocalModel, String> {
        // Hash the exact bytes passed to the loader. Metadata only skips work
        // for assets already verified in this process; it never identifies
        // persisted vectors or authorizes a different model's bytes.
        let read = |name: &str| std::fs::read(assets.join(name)).map_err(|error| error.to_string());
        let (tokenizer, weights, config) = (
            read("tokenizer.json")?,
            read("model.safetensors")?,
            read("config.json")?,
        );
        if &asset_stamps(assets)? != stamps {
            return Err("embedding assets changed during initialization".to_owned());
        }
        // The version names the runtime's tokenization and pooling: bump it
        // when model2vec-rs could map the same bytes to different vectors.
        let fingerprint = crate::util::sha256(
            format!(
                "local/model2vec-rs-0.4/normalize/512-tokens/8000-chars/v2\n{}\n{}\n{}",
                crate::util::sha256(&tokenizer),
                crate::util::sha256(&weights),
                crate::util::sha256(&config),
            )
            .as_bytes(),
        );
        if let Some(model) = self.lock()?.models.get(&fingerprint) {
            return Ok(model.clone());
        }
        let model =
            model2vec_rs::model::StaticModel::from_bytes(&tokenizer, &weights, &config, Some(true))
                .map_err(|error| error.to_string())?;
        let model = LocalModel(std::sync::Arc::new(LoadedModel {
            model,
            fingerprint: fingerprint.clone(),
        }));
        Ok(self
            .lock()?
            .models
            .entry(fingerprint)
            .or_insert(model)
            .clone())
    }
}

/// A model2vec model small enough to load in milliseconds, for tests of
/// how models are verified, shared and reloaded rather than of vectors.
#[cfg(test)]
pub(crate) fn fixture_model(directory: &Path) {
    let words = [
        "[UNK]",
        "[PAD]",
        "memory",
        "the",
        "packaging",
        "bandwidth",
        "and",
        "equipment",
    ];
    let vocab: serde_json::Map<String, Value> = words
        .iter()
        .enumerate()
        .map(|(id, word)| ((*word).to_owned(), json!(id)))
        .collect();
    let tokenizer = json!({
        "version": "1.0", "truncation": null, "padding": null, "added_tokens": [],
        "normalizer": {"type": "BertNormalizer", "clean_text": true,
            "handle_chinese_chars": true, "strip_accents": null, "lowercase": true},
        "pre_tokenizer": {"type": "BertPreTokenizer"},
        "post_processor": null, "decoder": null,
        "model": {"type": "WordPiece", "unk_token": "[UNK]",
            "continuing_subword_prefix": "##", "max_input_chars_per_word": 100,
            "vocab": vocab},
    });
    std::fs::write(
        directory.join("tokenizer.json"),
        serde_json::to_vec(&tokenizer).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("config.json"), br#"{"normalize": true}"#).unwrap();
    let width = 8;
    let table: Vec<u8> = (0..words.len() * width)
        .flat_map(|index| ((index as f32 * 0.37).sin()).to_le_bytes())
        .collect();
    let mut header = serde_json::to_vec(&json!({"embeddings": {
        "dtype": "F32", "shape": [words.len(), width], "data_offsets": [0, table.len()]}}))
    .unwrap();
    header.resize(header.len().next_multiple_of(8), b' ');
    let mut weights = (header.len() as u64).to_le_bytes().to_vec();
    weights.extend(header);
    weights.extend(table);
    std::fs::write(directory.join("model.safetensors"), weights).unwrap();
}

fn not_sent(message: &str) -> InferenceReply {
    InferenceReply {
        disposition: InferenceDisposition::NotSent,
        body: json!({"status": "not_sent", "message": message}),
    }
}

fn uncertain(message: &str) -> InferenceReply {
    InferenceReply {
        disposition: InferenceDisposition::Uncertain,
        body: json!({
            "status": "uncertain",
            "message": message,
            "usage": {"certainty": "uncertain"}
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run alone with --ignored --nocapture in debug and release profiles.
    #[test]
    #[ignore = "local embedding timing probe"]
    fn local_embedding_batch_timings() {
        let assets = crate::knowledge::staged_embed_assets();
        let texts: Vec<String> = (0..32)
            .map(|index| {
                format!(
                    "Company {index} reported higher memory revenue and operating profit. \
                     Advanced packaging capacity and HBM bandwidth increased while equipment \
                     orders reflected customer demand for semiconductor manufacturing."
                )
            })
            .collect();
        let settings = GraphProcessingSettings::default();
        let request = json!({"kind":"embedding", "inputs":texts});
        let started = std::time::Instant::now();
        let model = local_model(&assets).unwrap();
        println!(
            "embedding_timing cold_model_ms={:.3}",
            started.elapsed().as_secs_f64() * 1000.0
        );
        let expected = model.0.model.encode_with_args(&texts, Some(512), 128);
        for sample in 0..3 {
            let started = std::time::Instant::now();
            for _ in 0..10 {
                assert_eq!(
                    local_model(&assets).unwrap().fingerprint(),
                    model.fingerprint()
                );
            }
            let fingerprint_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = std::time::Instant::now();
            let reply = embed_locally(&settings, &assets, &request);
            let callback_ms = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(reply.disposition, InferenceDisposition::Completed);
            assert_eq!(reply.body["vectors"], json!(expected));
            let started = std::time::Instant::now();
            let vectors = embed_local_batch(&assets, &texts).unwrap();
            let batch_ms = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(vectors, expected);
            println!(
                "embedding_timing debug={} texts={} sample={sample} fingerprint_10_ms={fingerprint_ms:.3} callback_ms={callback_ms:.3} batch_ms={batch_ms:.3}",
                cfg!(debug_assertions), texts.len()
            );
        }
    }

    #[test]
    fn shared_provider_usage_is_translated_without_inventing_missing_counts() {
        let tokens = |usage: Value| {
            let result = graph_usage(Some(&usage));
            (
                result["input_tokens"].clone(),
                result["output_tokens"].clone(),
                result["certainty"].clone(),
            )
        };
        // Cached input is billed input.
        assert_eq!(
            tokens(json!({"input":12,"cacheRead":9,"cacheWrite":3,"output":7})),
            (json!(24), json!(7), json!("accounted"))
        );
        // A missing or overflowing count is unknown, never zero.
        assert_eq!(
            tokens(json!({"input":12,"cacheWrite":3,"output":7})),
            (Value::Null, json!(7), json!("uncertain"))
        );
        assert_eq!(
            tokens(json!({"input":u64::MAX,"cacheRead":1,"cacheWrite":0,"output":7})),
            (Value::Null, json!(7), json!("uncertain"))
        );
        assert_eq!(
            tokens(json!({"input":12,"cacheRead":0,"cacheWrite":0})),
            (json!(12), Value::Null, json!("uncertain"))
        );
    }
    #[test]
    fn reasoning_has_answer_room_with_a_bounded_total_allowance() {
        assert_eq!(output_allowance(Some(4096), "off"), 4096);
        assert_eq!(output_allowance(Some(4096), "high"), 12_288);
        assert_eq!(output_allowance(Some(u64::MAX), "max"), 16_384);
        assert_eq!(output_allowance(Some(u64::MAX), "off"), 8_192);
    }

    #[test]
    fn disabled_enrichment_does_not_authorize_a_model_call() {
        assert!(authorize(&GraphProcessingSettings::default(), "llm").is_err());
        assert!(admit_helper("openai-codex", "oauth").is_ok());
        assert!(admit_helper("openai", "api_key").is_ok());
        assert!(admit_helper("openai", "missing").is_err());
    }

    fn verifications(models: &LocalModels) -> usize {
        models.lock().unwrap().verifications
    }

    #[test]
    fn unchanged_embedding_assets_are_verified_once_even_with_concurrent_use() {
        let directory = tempfile::tempdir().unwrap();
        fixture_model(directory.path());
        let models = LocalModels::default();
        let loaded = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| models.get(directory.path()).unwrap()))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        for model in loaded {
            assert!(model.same(&models.get(directory.path()).unwrap()));
        }
        assert_eq!(verifications(&models), 1);
    }

    /// Writes `bytes` and moves the file's mtime past every earlier rewrite. A filesystem with a coarse
    /// clock (Linux CI) can give a same-size rewrite the same ctime and mtime within one tick, which no
    /// real asset update does; the stamp check is what these tests exercise, not the clock.
    fn rewrite(path: &std::path::Path, bytes: impl AsRef<[u8]>) {
        static TICK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        std::fs::write(path, bytes).unwrap();
        let later = std::time::SystemTime::now()
            + std::time::Duration::from_secs(
                TICK.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            );
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(later)
            .unwrap();
    }

    #[test]
    fn embedding_asset_changes_are_reverified_and_failed_loads_can_recover() {
        let directory = tempfile::tempdir().unwrap();
        fixture_model(directory.path());
        let models = LocalModels::default();
        let first = models.get(directory.path()).unwrap();
        let path = directory.path().join("config.json");
        let config = std::fs::read_to_string(&path).unwrap();
        // Same-size edits must invalidate the verification too.
        let changed = config.replacen("true", "null", 1);
        assert_eq!(config.len(), changed.len());
        rewrite(&path, changed);
        let second = models.get(directory.path()).unwrap();
        assert_ne!(first.fingerprint(), second.fingerprint());
        assert_eq!(verifications(&models), 2);
        rewrite(&path, b"{");
        assert!(models.get(directory.path()).is_err());
        rewrite(&path, config);
        assert!(first.same(&models.get(directory.path()).unwrap()));
        assert_eq!(verifications(&models), 4);
        std::fs::remove_file(path).unwrap();
        assert!(models.get(directory.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn same_size_asset_replacement_with_restored_mtime_is_reverified() {
        let directory = tempfile::tempdir().unwrap();
        fixture_model(directory.path());
        let models = LocalModels::default();
        let first = models.get(directory.path()).unwrap();
        let path = directory.path().join("config.json");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let config = std::fs::read_to_string(&path).unwrap();
        let replacement = directory.path().join("replacement.json");
        std::fs::write(&replacement, config.replacen("true", "null", 1)).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        std::fs::rename(replacement, &path).unwrap();
        let second = models.get(directory.path()).unwrap();
        assert_ne!(first.fingerprint(), second.fingerprint());
        assert_eq!(verifications(&models), 2);
    }

    fn eventually<T>(mut probe: impl FnMut() -> Option<T>) -> T {
        let started = std::time::Instant::now();
        loop {
            if let Some(value) = probe() {
                return value;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(60));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn an_unloaded_model_is_loaded_in_the_background_without_waiting() {
        let directory = tempfile::tempdir().unwrap();
        fixture_model(directory.path());
        let models: &'static LocalModels = Box::leak(Box::default());
        let loading = || !models.lock().unwrap().loading.is_empty();
        let loaded = || models.loaded(directory.path()).model;
        // The first lookup returns before the model exists, starts its load
        // and says so, however soon that load ends.
        let first = models.loaded(directory.path());
        assert!(first.model.is_none() && first.loading);
        let model = eventually(loaded);
        assert!(first.loading);
        assert!(!models.loaded(directory.path()).loading);
        assert!(model.same(&models.get(directory.path()).unwrap()));
        assert_eq!(verifications(models), 1);
        // A failed load is not retried until the files change, and is not loading.
        let config = directory.path().join("config.json");
        let saved = std::fs::read(&config).unwrap();
        std::fs::write(&config, b"{").unwrap();
        assert!(loaded().is_none());
        eventually(|| (verifications(models) == 2 && !loading()).then_some(()));
        let failed = models.loaded(directory.path());
        assert!(failed.model.is_none() && !failed.loading);
        assert!(!loading());
        assert_eq!(verifications(models), 2);
        std::fs::write(&config, saved).unwrap();
        assert!(eventually(loaded).same(&model));
        assert_eq!(verifications(models), 3);
    }

    #[test]
    fn local_embedding_callback_matches_batched_vectors_and_checks_every_input() {
        let assets = crate::knowledge::staged_embed_assets();
        let texts: Vec<String> = (0..131)
            .map(|index| format!("Memory packaging and bandwidth {index}"))
            .collect();
        let expected = embed_local_batch(&assets, &texts).unwrap();
        let settings = GraphProcessingSettings::default();
        let mut request = json!({"inputs":texts});
        let reply = embed_locally(&settings, &assets, &request);
        assert_eq!(reply.disposition, InferenceDisposition::Completed);
        assert_eq!(reply.body["vectors"], json!(expected));
        request["inputs"][130] = json!("x".repeat(MAX_EMBEDDING_CHARS + 1));
        assert_eq!(
            embed_locally(&settings, &assets, &request).disposition,
            InferenceDisposition::NotSent
        );
        assert_eq!(
            embed_locally(&settings, &assets, &json!({"inputs":[]})).disposition,
            InferenceDisposition::NotSent
        );
        assert_eq!(
            embed_locally(&settings, &assets, &json!({"inputs":vec!["text"; 257]})).disposition,
            InferenceDisposition::NotSent
        );
    }

    #[test]
    fn knowledge_embedding_cache_distinguishes_asset_directories_and_replacements() {
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        fixture_model(first_dir.path());
        fixture_model(second_dir.path());
        let first = local_model(first_dir.path()).unwrap();
        // The same bytes from another path share a model; changed assets cannot.
        assert!(first.same(&local_model(second_dir.path()).unwrap()));
        let mut tokenizer: serde_json::Value = serde_json::from_slice(
            &std::fs::read(second_dir.path().join("tokenizer.json")).unwrap(),
        )
        .unwrap();
        let memory = tokenizer["model"]["vocab"]["memory"].clone();
        let other = tokenizer["model"]["vocab"]["the"].clone();
        tokenizer["model"]["vocab"]["memory"] = other;
        tokenizer["model"]["vocab"]["the"] = memory;
        rewrite(
            &second_dir.path().join("tokenizer.json"),
            serde_json::to_vec(&tokenizer).unwrap(),
        );
        let second = local_model(second_dir.path()).unwrap();
        assert_ne!(first.fingerprint(), second.fingerprint());
        assert!(!first.same(&second));
        assert!(first.same(&local_model(first_dir.path()).unwrap()));
        assert_eq!(second.embed("memory").unwrap().len(), second.dimension());
        assert_ne!(
            first.embed("memory").unwrap(),
            second.embed("memory").unwrap()
        );
        std::fs::remove_file(second_dir.path().join("config.json")).unwrap();
        assert!(local_model(second_dir.path()).is_err());
    }

    #[test]
    fn batched_local_embeddings_match_single_texts_across_batch_boundaries() {
        let model = local_model(&crate::knowledge::staged_embed_assets()).unwrap();
        let texts: Vec<String> = (0..131)
            .map(|index| match index % 4 {
                0 => String::new(),
                1 => format!("HBM4 bandwidth {index} Gbps versus TB/s"),
                2 => "반도체 세정 식각 포토트랙 🧪".repeat(600),
                _ => "memory and packaging equipment ".repeat(1000),
            })
            .collect();
        let expected: Vec<_> = texts
            .iter()
            .map(|text| model.0.model.encode_single(text))
            .collect();
        assert!(expected
            .iter()
            .all(|vector| vector.len() == model.dimension()));
        assert_eq!(model.embed_batch(&texts).unwrap(), expected);
        assert!(model.embed_batch(&[]).unwrap().is_empty());
    }

    #[tokio::test]
    async fn local_embedding_needs_no_provider_and_obeys_pause_and_budget() {
        let assets = crate::knowledge::staged_embed_assets();
        let mut settings = GraphProcessingSettings::default();
        let request = json!({"kind":"embedding", "inputs":["Ada described the engine"]});
        let reply = dispatch_inference(&settings, None, &assets, &request).await;
        assert_eq!(reply.disposition, InferenceDisposition::Completed);
        settings.paused = true;
        assert_eq!(
            dispatch_inference(&settings, None, &assets, &request)
                .await
                .disposition,
            InferenceDisposition::NotSent
        );
        settings.paused = false;
        settings.max_embedding_calls = 0;
        assert_eq!(
            dispatch_inference(&settings, None, &assets, &request)
                .await
                .disposition,
            InferenceDisposition::NotSent
        );
    }
}
