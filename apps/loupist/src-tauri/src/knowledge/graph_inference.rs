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

/// Local English embeddings are a CPU operation. They do not require paid
/// enrichment or provider credentials.
pub fn admit_local_embedding(settings: &GraphProcessingSettings) -> Result<(), &'static str> {
    if settings.paused {
        return Err("graph processing is paused");
    }
    if settings.embedding_profile != "local-en-128" {
        return Err("graph embedding profile is not the local english model");
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
    if kind == "embedding" && settings.embedding_profile == "local-en-128" {
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
    let vectors = match embed_local_english_batch(assets, &texts) {
        Ok(vectors) => vectors,
        Err(_) => return uncertain("local english embedding failed"),
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
    if kind == "embedding" && settings.embedding_profile == "local-en-128" {
        return embed_locally(settings, assets, request);
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
        "certainty":if input.is_some() && output.is_some() {"accounted"} else {"uncertain"}})
}

pub fn embed_local_english(assets: &Path, text: &str) -> Result<Vec<f32>, String> {
    embed_local_english_batch(assets, &[text.to_owned()])?
        .pop()
        .ok_or_else(|| "local english embedding is missing".to_owned())
}

/// Bounded tokenization batches preserve the single-text model's exact pooling.
pub(crate) fn embed_local_english_batch(
    assets: &Path,
    texts: &[String],
) -> Result<Vec<Vec<f32>>, String> {
    let vectors = cached_english_model(assets)?
        .0
        .encode_with_args(texts, Some(512), 128);
    if vectors.len() != texts.len() || vectors.iter().any(|vector| vector.len() != 128) {
        return Err("local english embedding dimensions are invalid".to_owned());
    }
    Ok(vectors)
}

/// Fingerprint of the exact loaded model plus encoding settings. Changing
/// bundled assets across app runs cannot reuse another model's document vectors.
pub(super) fn local_english_model_fingerprint(assets: &Path) -> Result<String, String> {
    Ok(cached_english_model(assets)?.1.clone())
}

type CachedEnglishModel = std::sync::Arc<(model2vec_rs::model::StaticModel, String)>;

const ENGLISH_ASSET_NAMES: [&str; 3] = ["tokenizer.json", "model.safetensors", "config.json"];

#[derive(PartialEq, Eq)]
struct EnglishAssetStamp {
    len: u64,
    modified: std::time::SystemTime,
    // Also detect same-size replacements or writes whose mtime was restored.
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

fn english_asset_stamps(assets: &Path) -> Result<[EnglishAssetStamp; 3], String> {
    let stamps = ENGLISH_ASSET_NAMES.map(|name| -> Result<EnglishAssetStamp, String> {
        let metadata = std::fs::metadata(assets.join(name)).map_err(|error| error.to_string())?;
        Ok(EnglishAssetStamp {
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

struct VerifiedEnglishAssets {
    stamps: [EnglishAssetStamp; 3],
    model: CachedEnglishModel,
}

#[derive(Default)]
struct EnglishModelCache {
    assets: std::collections::HashMap<std::path::PathBuf, VerifiedEnglishAssets>,
    models: std::collections::HashMap<String, CachedEnglishModel>,
    #[cfg(test)]
    verifications: usize,
}

impl EnglishModelCache {
    fn get(&mut self, assets: &Path) -> Result<CachedEnglishModel, String> {
        let assets = assets.canonicalize().map_err(|error| error.to_string())?;
        let stamps = english_asset_stamps(&assets)?;
        if let Some(verified) = self.assets.get(&assets) {
            if verified.stamps == stamps {
                return Ok(std::sync::Arc::clone(&verified.model));
            }
        }
        #[cfg(test)]
        {
            self.verifications += 1;
        }
        // Hash the exact bytes passed to the loader. Metadata only skips work
        // for assets already verified in this process; it never identifies
        // persisted vectors or authorizes a different model's bytes.
        let tokenizer =
            std::fs::read(assets.join("tokenizer.json")).map_err(|error| error.to_string())?;
        let weights =
            std::fs::read(assets.join("model.safetensors")).map_err(|error| error.to_string())?;
        let config =
            std::fs::read(assets.join("config.json")).map_err(|error| error.to_string())?;
        if english_asset_stamps(&assets)? != stamps {
            return Err("embedding assets changed during initialization".to_owned());
        }
        let fingerprint = crate::util::sha256(
            format!(
                "local-en-128/normalize/512-tokens/8000-chars/v1\n{}\n{}\n{}",
                crate::util::sha256(&tokenizer),
                crate::util::sha256(&weights),
                crate::util::sha256(&config),
            )
            .as_bytes(),
        );
        let model = if let Some(model) = self.models.get(&fingerprint) {
            std::sync::Arc::clone(model)
        } else {
            let model = model2vec_rs::model::StaticModel::from_bytes(
                &tokenizer,
                &weights,
                &config,
                Some(true),
            )
            .map_err(|error| error.to_string())?;
            let model = std::sync::Arc::new((model, fingerprint.clone()));
            self.models
                .insert(fingerprint, std::sync::Arc::clone(&model));
            model
        };
        self.assets.insert(
            assets,
            VerifiedEnglishAssets {
                stamps,
                model: std::sync::Arc::clone(&model),
            },
        );
        Ok(model)
    }
}

fn cached_english_model(assets: &Path) -> Result<CachedEnglishModel, String> {
    use std::sync::{Mutex, OnceLock};
    static MODELS: OnceLock<Mutex<EnglishModelCache>> = OnceLock::new();
    // The lock also makes concurrent first use verify and load only once.
    MODELS
        .get_or_init(|| Mutex::new(EnglishModelCache::default()))
        .lock()
        .map_err(|_| "embedding initialization failed")?
        .get(assets)
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
        let assets =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
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
        let model = cached_english_model(&assets).unwrap();
        println!(
            "embedding_timing cold_model_ms={:.3}",
            started.elapsed().as_secs_f64() * 1000.0
        );
        let expected = model.0.encode_with_args(&texts, Some(512), 128);
        for sample in 0..3 {
            let started = std::time::Instant::now();
            for _ in 0..10 {
                assert_eq!(local_english_model_fingerprint(&assets).unwrap(), model.1);
            }
            let fingerprint_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = std::time::Instant::now();
            let reply = embed_locally(&settings, &assets, &request);
            let callback_ms = started.elapsed().as_secs_f64() * 1000.0;
            assert_eq!(reply.disposition, InferenceDisposition::Completed);
            assert_eq!(reply.body["vectors"], json!(expected));
            let started = std::time::Instant::now();
            let vectors = embed_local_english_batch(&assets, &texts).unwrap();
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

    #[test]
    fn unchanged_embedding_assets_are_verified_once_even_with_concurrent_use() {
        let assets =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
        let cache = std::sync::Mutex::new(EnglishModelCache::default());
        let models = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| cache.lock().unwrap().get(&assets).unwrap()))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        let mut cache = cache.into_inner().unwrap();
        for model in models {
            assert!(std::sync::Arc::ptr_eq(&model, &cache.get(&assets).unwrap()));
        }
        assert_eq!(cache.verifications, 1);
    }

    #[test]
    fn embedding_asset_changes_are_reverified_and_failed_loads_can_recover() {
        let bundled =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
        let directory = tempfile::tempdir().unwrap();
        for name in ENGLISH_ASSET_NAMES {
            std::fs::copy(bundled.join(name), directory.path().join(name)).unwrap();
        }
        let mut cache = EnglishModelCache::default();
        let first = cache.get(directory.path()).unwrap();
        let path = directory.path().join("config.json");
        let config = std::fs::read_to_string(&path).unwrap();
        // Same-size edits must invalidate the verification too.
        let changed = config.replacen("true", "null", 1);
        assert_eq!(config.len(), changed.len());
        std::fs::write(&path, changed).unwrap();
        let second = cache.get(directory.path()).unwrap();
        assert_ne!(first.1, second.1);
        assert_eq!(cache.verifications, 2);
        std::fs::write(&path, b"{").unwrap();
        assert!(cache.get(directory.path()).is_err());
        std::fs::write(&path, config).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &cache.get(directory.path()).unwrap()
        ));
        assert_eq!(cache.verifications, 4);
        std::fs::remove_file(path).unwrap();
        assert!(cache.get(directory.path()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn same_size_asset_replacement_with_restored_mtime_is_reverified() {
        let bundled =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
        let directory = tempfile::tempdir().unwrap();
        for name in ENGLISH_ASSET_NAMES {
            std::fs::copy(bundled.join(name), directory.path().join(name)).unwrap();
        }
        let mut cache = EnglishModelCache::default();
        let first = cache.get(directory.path()).unwrap();
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
        let second = cache.get(directory.path()).unwrap();
        assert_ne!(first.1, second.1);
        assert_eq!(cache.verifications, 2);
    }

    #[test]
    fn local_embedding_callback_matches_batched_vectors_and_checks_every_input() {
        let assets =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
        let texts: Vec<String> = (0..131)
            .map(|index| format!("Memory packaging and bandwidth {index}"))
            .collect();
        let expected = embed_local_english_batch(&assets, &texts).unwrap();
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
        let bundled =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
        let first_dir = tempfile::tempdir().unwrap();
        let second_dir = tempfile::tempdir().unwrap();
        for name in ["tokenizer.json", "model.safetensors", "config.json"] {
            std::fs::copy(bundled.join(name), first_dir.path().join(name)).unwrap();
            std::fs::copy(bundled.join(name), second_dir.path().join(name)).unwrap();
        }
        let first = cached_english_model(first_dir.path()).unwrap();
        // The same bytes from another path share a model; changed assets cannot.
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &cached_english_model(second_dir.path()).unwrap()
        ));
        let mut tokenizer: serde_json::Value = serde_json::from_slice(
            &std::fs::read(second_dir.path().join("tokenizer.json")).unwrap(),
        )
        .unwrap();
        let memory = tokenizer["model"]["vocab"]["memory"].clone();
        let other = tokenizer["model"]["vocab"]["the"].clone();
        tokenizer["model"]["vocab"]["memory"] = other;
        tokenizer["model"]["vocab"]["the"] = memory;
        std::fs::write(
            second_dir.path().join("tokenizer.json"),
            serde_json::to_vec(&tokenizer).unwrap(),
        )
        .unwrap();
        let second = cached_english_model(second_dir.path()).unwrap();
        assert_ne!(first.1, second.1);
        assert!(!std::sync::Arc::ptr_eq(&first, &second));
        assert!(std::sync::Arc::ptr_eq(
            &first,
            &cached_english_model(first_dir.path()).unwrap()
        ));
        assert_eq!(second.0.encode_single("memory").len(), 128);
        assert_ne!(
            first.0.encode_single("memory"),
            second.0.encode_single("memory")
        );
        std::fs::remove_file(second_dir.path().join("config.json")).unwrap();
        assert!(cached_english_model(second_dir.path()).is_err());
    }

    #[test]
    fn batched_local_embeddings_match_single_texts_across_batch_boundaries() {
        let assets =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
        let texts: Vec<String> = (0..131)
            .map(|index| match index % 4 {
                0 => String::new(),
                1 => format!("HBM4 bandwidth {index} Gbps versus TB/s"),
                2 => "반도체 세정 식각 포토트랙 🧪".repeat(600),
                _ => "memory and packaging equipment ".repeat(1000),
            })
            .collect();
        let model = cached_english_model(&assets).unwrap();
        let expected: Vec<_> = texts
            .iter()
            .map(|text| model.0.encode_single(text))
            .collect();
        assert!(expected.iter().all(|vector| vector.len() == 128));
        assert_eq!(
            embed_local_english_batch(&assets, &texts).unwrap(),
            expected
        );
        assert!(embed_local_english_batch(&assets, &[]).unwrap().is_empty());
    }

    #[tokio::test]
    async fn local_embedding_needs_no_provider_and_obeys_pause_and_budget() {
        let assets =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../src/knowledge/embed_assets");
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
