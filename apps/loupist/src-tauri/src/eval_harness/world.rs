//! Opt-in live World evaluations. All writable state, including the derived World index, belongs
//! to a disposable trial; only the existing Pi authentication directory is reused.
mod checkpoint;
mod lens_seed;
pub mod scale;
use super::{append_atomic, artifacts, batch, new_chat, run_turn, Result, Session, Turn};
use crate::{app::AppState, domain::ChatSession, knowledge::*, store::LoupistStore};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

pub struct Options {
    pub suite: PathBuf,
    pub out: PathBuf,
    pub pi_agent_dir: PathBuf,
    pub trials: usize,
    pub trial_index: Option<usize>,
    pub scenarios: Vec<String>,
    pub condition: Option<String>,
    pub model: String,
    pub thinking: String,
    pub world_model: String,
    pub world_thinking: String,
    pub turn_timeout_ms: u64,
    pub world_timeout_ms: u64,
    pub stage_timeout_ms: u64,
    pub resources: Option<PathBuf>,
    pub keep: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            suite: PathBuf::new(),
            out: PathBuf::new(),
            pi_agent_dir: PathBuf::new(),
            trials: 3,
            trial_index: None,
            scenarios: vec![],
            condition: None,
            model: "openai-codex/gpt-6-luna".into(),
            thinking: "medium".into(),
            world_model: "openai-codex/gpt-6-luna".into(),
            world_thinking: "high".into(),
            turn_timeout_ms: 600_000,
            world_timeout_ms: 120_000,
            stage_timeout_ms: 900_000,
            resources: None,
            keep: false,
        }
    }
}

#[derive(Deserialize)]
struct Suite {
    schema: String,
    suite: String,
    version: u32,
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize, Serialize)]
struct Scenario {
    id: String,
    title: String,
    #[serde(default)]
    split: String,
    condition: String,
    world_enabled: bool,
    #[serde(default)]
    connections: Vec<String>,
    #[serde(default)]
    replay: Option<PathBuf>,
    #[serde(default)]
    workspace_files: BTreeMap<String, String>,
    #[serde(default)]
    seed_sources: Vec<Source>,
    #[serde(default)]
    seed_lenses: Vec<lens_seed::SeedLens>,
    /// Custom research focus, the same text the product stores on a new Loupist.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    research_focus: String,
    /// `template` applies the curated preset a new Loupist gets from `template`.
    /// `none` leaves the World extension empty. Absent matches `none`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    seed: String,
    /// The template whose curated seed `seed: "template"` applies.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    template: String,
    steps: Vec<Step>,
}

#[derive(Deserialize, Serialize)]
struct Source {
    id: String,
    title: String,
    text: String,
    origin: String,
    #[serde(default)]
    publication_time: Option<String>,
    #[serde(default)]
    acquired_at: Option<String>,
    #[serde(default = "publisher")]
    authorship: Authorship,
}
fn publisher() -> Authorship {
    Authorship::Publisher
}

#[derive(Deserialize, Serialize)]
struct Step {
    id: String,
    #[serde(default)]
    workspace_files: BTreeMap<String, String>,
    #[serde(default)]
    sources: Vec<Source>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    chat: Option<String>,
    #[serde(default = "read_only")]
    memory: String,
    #[serde(default = "probe")]
    role: String,
}
fn read_only() -> String {
    "read_only".into()
}
fn probe() -> String {
    "probe".into()
}

fn time_point(value: &str) -> Result<TimePoint> {
    let precision = if value.len() == 10 {
        chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")?;
        TimePrecision::Date
    } else {
        chrono::DateTime::parse_from_rfc3339(value)?;
        TimePrecision::Instant
    };
    Ok(TimePoint {
        precision,
        value: Some(value.into()),
        approximate: false,
    })
}

fn acquired(source: &Source) -> Result<i64> {
    // A fixed acquisition timestamp makes independent trials equivalent; this
    // is fixture metadata, never an assertion that the live clock is frozen.
    Ok(chrono::DateTime::parse_from_rfc3339(
        source
            .acquired_at
            .as_deref()
            .unwrap_or("2026-10-01T00:00:00Z"),
    )?
    .timestamp_millis())
}

fn validate(suite: &Suite, options: &Options) -> Result<()> {
    if suite.schema != "loupist-world-suite/1" || suite.version != 1 || suite.suite.is_empty() {
        return Err("unsupported World suite schema/version".into());
    }
    if options.trials == 0
        || options
            .trial_index
            .is_some_and(|trial| trial >= options.trials)
        || options.turn_timeout_ms == 0
        || options.stage_timeout_ms == 0
        || options.world_timeout_ms == 0
        || options.world_timeout_ms > 120_000
    {
        return Err("invalid trial count or timeout".into());
    }
    let mut ids = BTreeSet::new();
    for scenario in &suite.scenarios {
        if scenario.id.is_empty() || !ids.insert(&scenario.id) || scenario.steps.is_empty() {
            return Err("World scenarios require unique IDs and at least one step".into());
        }
        let mut steps = BTreeSet::new();
        let mut fixture_paths = BTreeSet::new();
        for path in scenario.workspace_files.keys() {
            fixture_paths.insert(path);
            super::relative(std::path::Path::new(path))?;
        }
        if let Some(path) = &scenario.replay {
            super::relative(path)?;
        }
        if !scenario.connections.is_empty() && scenario.replay.is_none() {
            return Err("World eval connections require a fail-closed replay fixture".into());
        }
        let mut source_ids: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
        for source in scenario
            .seed_sources
            .iter()
            .chain(scenario.steps.iter().flat_map(|s| &s.sources))
        {
            if source.id.is_empty() || source.text.is_empty() {
                return Err("World source ID/text is empty".into());
            }
            let bytes = serde_json::to_vec(source)?;
            if source_ids
                .insert(&source.id, bytes.clone())
                .is_some_and(|old| old != bytes)
            {
                return Err("a source ID identifies different fixture material".into());
            }
            if let Some(value) = &source.publication_time {
                time_point(value)?;
            }
            acquired(source)?;
            if crate::util::credential_text::contains_credential_text(&source.text)
                || crate::util::credential_text::contains_credential_text(&source.origin)
            {
                return Err("credential-shaped World fixture refused".into());
            }
        }
        lens_seed::validate(&scenario.seed_lenses, &scenario.seed_sources)?;
        custom_focus(scenario)?;
        for (index, step) in scenario.steps.iter().enumerate() {
            for path in step.workspace_files.keys() {
                super::relative(std::path::Path::new(path))?;
                if !fixture_paths.insert(path) {
                    return Err("workspace fixture paths must be introduced only once".into());
                }
            }
            if step.id.is_empty() || !steps.insert(&step.id) {
                return Err("World step IDs must be unique".into());
            }
            super::memory_policy(&step.memory)?;
            if !matches!(step.role.as_str(), "train" | "probe") {
                return Err("World step role must be train or probe".into());
            }
            if step
                .user
                .as_ref()
                .is_some_and(|text| text.trim().is_empty())
            {
                return Err("World Chat prompt is empty".into());
            }
            // The runner ingests scenario-level Sources at the first step.
            let ingests_seed_sources = index == 0 && !scenario.seed_sources.is_empty();
            if !ingests_seed_sources
                && step.sources.is_empty()
                && step.query.is_none()
                && step.user.is_none()
            {
                return Err("World step has no source, query, or Chat operation".into());
            }
            if step
                .query
                .as_ref()
                .is_some_and(|q| q.is_empty() || q.len() > 500)
            {
                return Err("World query must contain 1..500 bytes".into());
            }
        }
    }
    for id in &options.scenarios {
        if !ids.contains(id) {
            return Err(format!("unknown scenario {id}").into());
        }
    }
    Ok(())
}

// Fixtures appear at the declared step, so later evidence cannot leak into earlier turns.
// Never replace files written by Chat or follow a symlink outside this isolated workspace.
fn introduce_workspace_files(
    workspace: &std::path::Path,
    files: &BTreeMap<String, String>,
) -> Result<Value> {
    use std::io::Write;
    let root = workspace.canonicalize()?;
    let mut manifest = serde_json::Map::new();
    for (name, contents) in files {
        super::relative(std::path::Path::new(name))?;
        let path = root.join(name);
        let mut parent = root.clone();
        for component in std::path::Path::new(name)
            .parent()
            .into_iter()
            .flat_map(|p| p.components())
        {
            parent.push(component);
            if parent
                .symlink_metadata()
                .is_ok_and(|meta| meta.file_type().is_symlink())
            {
                return Err("workspace fixture parent is a symlink".into());
            }
            fs::create_dir_all(&parent)?;
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        file.write_all(contents.as_bytes())?;
        manifest.insert(
            name.clone(),
            json!(crate::util::sha256(contents.as_bytes())),
        );
    }
    Ok(Value::Object(manifest))
}

fn settings(options: &Options, scenario: &Scenario) -> Result<GraphProcessingSettings> {
    let (provider, model) = options
        .world_model
        .split_once('/')
        .ok_or("World model must be provider/model")?;
    Ok(GraphProcessingSettings {
        enrichment_enabled: scenario.world_enabled,
        helper_provider: Some(provider.into()),
        helper_model: Some(model.into()),
        helper_model_profile_id: Some(options.world_model.clone()),
        thinking_level: options.world_thinking.clone(),
        timeout_ms: options.world_timeout_ms,
        research_focus: scenario.research_focus.trim().to_string(),
        ..Default::default()
    })
}

fn custom_focus(scenario: &Scenario) -> Result<()> {
    let focus = scenario.research_focus.trim();
    if focus.chars().count() > 280 {
        return Err("research focus must be 280 characters or fewer".into());
    }
    match scenario.seed.as_str() {
        "" | "none" => Ok(()),
        "template" if template_seed(scenario, 0).is_none() => {
            Err("template seed requires a curated template".into())
        }
        "template" => Ok(()),
        _ => Err("World seed must be template or none".into()),
    }
}

/// The product's curated preset for this scenario's template.
fn template_seed(scenario: &Scenario, now_ms: i64) -> Option<crate::knowledge::WorldExtension> {
    crate::knowledge::curated_seed(Some(scenario.template.as_str()), now_ms)
}

fn remember_focus(state: &AppState, loupist_id: &str, focus: &str) -> Result<()> {
    if focus.is_empty() {
        return Ok(());
    }
    // The product stores the same sentence on the Loupist profile and on World settings.
    // The seed prompt prefers the profile; extraction reads the settings.
    state
        .store
        .update_loupist_profile(
            loupist_id,
            crate::store::LoupistProfileEdit {
                focus: Some(focus),
                ..Default::default()
            },
            crate::commands::now_ms(),
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn schema_trace(state: &AppState, loupist_id: &str, mode: &str) -> Result<Value> {
    let connection = state.store.lock_loupist(loupist_id)?;
    let view = crate::knowledge::schema_view(&connection, loupist_id)?;
    Ok(json!({"mode": mode, "extension": view.extension, "changes": view.changes}))
}

fn save_settings(
    state: &AppState,
    loupist_id: &str,
    settings: &GraphProcessingSettings,
) -> Result<()> {
    let mut connection = state.store.lock_loupist(loupist_id)?;
    let tx = connection.transaction()?;
    store_settings(&tx, loupist_id, settings)?;
    tx.commit()?;
    Ok(())
}

fn ingest(
    state: &AppState,
    loupist_id: &str,
    config: &GraphProcessingSettings,
    sources: &[Source],
) -> Result<Value> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    let mut records = vec![];
    for source in sources {
        let at_ms = acquired(source)?;
        let source_class = match source.authorship {
            Authorship::Analysis => SourceClass::Analysis,
            Authorship::User => SourceClass::UserStatement,
            Authorship::Tool => SourceClass::ToolResult,
            Authorship::ImportedContributor => SourceClass::ImportedContributor,
            Authorship::Publisher => SourceClass::PublisherDocument,
        };
        let publication_time = source
            .publication_time
            .as_deref()
            .map(time_point)
            .transpose()?;
        let saved = store.transact(|write| {
            let retained = write.retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: loupist_id.into(),
                at_ms,
                title: source.title.clone(),
                source_class,
                origin_locator: source.origin.clone(),
                provider_identity: "eval-fixture".into(),
                authorship: source.authorship,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time,
                material: Some(RetainMaterial {
                    media_type: "text/plain".into(),
                    role: RepresentationRole::Original,
                    document_id: source.id.clone(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: vec![],
                    bytes: source.text.as_bytes().to_vec(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: source.text.len() as u64,
                    },
                    context_note: "Frozen fictional World evaluation source".into(),
                }),
            })?;
            write.enqueue_source_world(loupist_id, &retained, config, at_ms)?;
            Ok(retained)
        })?;
        let span = store.read_capture_material(ReadCapture {
            loupist_id: loupist_id.into(),
            record_id: saved.record_id.clone(),
            capture_id: saved.capture_id.clone(),
            at_ms,
        })?;
        let actual_text = String::from_utf8(span.bytes)?;
        if actual_text != source.text {
            return Err("canonical retained bytes differ from fixture".into());
        }
        records.push(json!({"fixture_id":source.id,"saved":saved,"canonical_text":actual_text,
            "selected_digest":span.selected_digest,"coverage":span.coverage,"publication_time":source.publication_time,
            "acquired_at_ms":at_ms,"authorship":source.authorship}));
    }
    Ok(json!(records))
}

async fn drain(
    state: &AppState,
    loupist_id: &str,
    enabled: bool,
    timeout_ms: u64,
) -> Result<Value> {
    let started = Instant::now();
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    loop {
        let jobs = store.graph_jobs(loupist_id)?;
        let pending = jobs
            .iter()
            .any(|job| matches!(job.status.as_str(), "queued" | "running"));
        if !enabled || !pending {
            let failures = jobs
                .iter()
                .filter(|job| job.status != "completed")
                .map(|job| json!({"job_id":job.job_id,"status":job.status}))
                .collect::<Vec<_>>();
            return Ok(
                json!({"status":if failures.is_empty(){"completed"}else{"failed"},
                "duration_ms":started.elapsed().as_millis() as u64,"jobs":jobs,"failures":failures,
                "usage":null,"cost_usd":null,"usage_note":"See the stage's cumulative world_inference ledger for actual callback usage."}),
            );
        }
        if started.elapsed() >= Duration::from_millis(timeout_ms) {
            return Ok(json!({"status":"failed","error":"world_stage_timeout",
                "duration_ms":started.elapsed().as_millis() as u64,"jobs":jobs,"usage":null,"cost_usd":null}));
        }
        // Drive the production supervisor explicitly so its first runtime or
        // worker failure is preserved rather than hidden by the background loop.
        // A Chat may already have woken it; transactional claiming still owns jobs.
        let remaining = Duration::from_millis(timeout_ms).saturating_sub(started.elapsed());
        let driven = tokio::time::timeout(remaining, state.graph.pump_once()).await;
        match driven {
            Ok(Ok(Some(_))) => {}
            Ok(Ok(None)) => {
                let current = store.graph_jobs(loupist_id)?;
                if !current.iter().any(|job| job.status == "running")
                    && current.iter().any(|job| job.status == "queued")
                {
                    return Ok(json!({"status":"failed","error":"world_jobs_not_runnable",
                        "duration_ms":started.elapsed().as_millis() as u64,"jobs":current}));
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Ok(Err(error)) => {
                return Ok(json!({"status":"failed","error":error.to_string(),
                "duration_ms":started.elapsed().as_millis() as u64,"jobs":store.graph_jobs(loupist_id)?}))
            }
            Err(_) => {
                return Ok(json!({"status":"failed","error":"world_stage_timeout",
                "duration_ms":started.elapsed().as_millis() as u64,"jobs":store.graph_jobs(loupist_id)?}))
            }
        }
    }
}

pub(super) async fn snapshot(
    state: &AppState,
    loupist_id: &str,
    query: Option<&str>,
) -> Result<Value> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    let mut snapshots = vec![];
    let mut queries = vec![];
    for record in store.list_records(loupist_id)? {
        if let Some(revision) = &record.revision_id {
            let saved = store.record_snapshot(loupist_id, revision)?;
            let mut materials = vec![];
            if record.kind == RecordKind::Source {
                let source: SourceRecordPayload = serde_json::from_value(saved.payload.clone())?;
                for capture in source.source.captures {
                    for representation in capture.representations {
                        for span in representation.spans {
                            let retained = store.read_span(ReadSpan {
                                loupist_id: loupist_id.into(),
                                record_id: record.record_id.clone(),
                                span_id: span.span_id.clone(),
                                at_ms: chrono::Utc::now().timestamp_millis(),
                            })?;
                            materials.push(json!({"capture_id":capture.capture_id,"representation_id":representation.representation_id,
                                "span_id":span.span_id,"media_type":retained.media_type,"coverage":retained.coverage,
                                "byte_length":retained.bytes.len(),"bytes_sha256":crate::util::sha256(&retained.bytes),
                                "selected_digest":retained.selected_digest,"canonical_text":std::str::from_utf8(&retained.bytes).ok()}));
                        }
                    }
                }
            }
            if record.kind == RecordKind::World {
                if let Some(search) = query {
                    let started = Instant::now();
                    let result = state
                        .graph
                        .query_indexed(loupist_id, &record.record_id, revision, search)
                        .await;
                    queries.push(match result {
                        Ok(result) => json!({"record_id":record.record_id,"revision_id":revision,
                            "query":search,"status":"completed","duration_ms":started.elapsed().as_millis() as u64,"result":result}),
                        Err(error) => json!({"record_id":record.record_id,"revision_id":revision,
                            "query":search,"status":"failed","error":error.to_string(),"result":null}),
                    });
                }
            }
            snapshots
                .push(json!({"record":record,"snapshot":saved,"canonical_materials":materials}));
        }
    }
    let query_status = if query.is_none() {
        "not_requested"
    } else if queries.is_empty() {
        "no_world"
    } else if queries.iter().any(|result| result["status"] != "completed") {
        "failed"
    } else {
        "completed"
    };
    Ok(json!({"records":snapshots,"queries":queries,"query_status":query_status}))
}

fn selected(scenario: &Scenario, options: &Options) -> bool {
    (options.scenarios.is_empty() || options.scenarios.contains(&scenario.id))
        && options
            .condition
            .as_ref()
            .is_none_or(|condition| condition == &scenario.condition)
}

async fn setup(
    state: &mut AppState,
    options: &Options,
    scenario: &Scenario,
    workspace: &std::path::Path,
) -> Result<(
    String,
    Value,
    crate::providers::inference::ProviderInferenceService,
    GraphProcessingSettings,
)> {
    state.eval_pi_agent_dir = Some(options.pi_agent_dir.clone());
    Arc::make_mut(&mut state.artifacts).pi = Some(artifacts()?);
    let provider = options
        .model
        .split_once('/')
        .ok_or("Chat model must be provider/model")?
        .0;
    let world_provider = options
        .world_model
        .split_once('/')
        .ok_or("World model must be provider/model")?
        .0;
    let mut models = crate::providers::connection::eval_models(state, provider).await?;
    if world_provider != provider {
        models.extend(crate::providers::connection::eval_models(state, world_provider).await?);
    }
    for (id, effort) in [
        (&options.model, &options.thinking),
        (&options.world_model, &options.world_thinking),
    ] {
        if !models.iter().any(|model| {
            &model.id == id
                && model.thinking_levels.contains(effort)
                && model.input.iter().any(|input| input == "text")
        }) {
            return Err(format!(
                "requested model/effort not present in live catalog: {id}/{effort}"
            )
            .into());
        }
    }
    state
        .set_model_catalog(crate::providers::catalog::ModelCatalog {
            models: models.clone(),
        })
        .map_err(|e| e.message)?;
    let inference = crate::providers::inference::ProviderInferenceService::from_state(state)
        .map_err(|e| e.message)?;
    let (provider, model) = options.world_model.split_once('/').unwrap();
    let readiness = inference
        .readiness(provider, model)
        .map_err(|e| e.message)?;
    if scenario.world_enabled && !readiness.available {
        return Err(format!(
            "World provider unavailable: {}",
            readiness.reason.unwrap_or_default()
        )
        .into());
    }
    // Replace the test constructor's service, which captured empty artifacts
    // and a temporary auth directory, after the live Pi profile is installed.
    state.graph.shutdown().await?;
    state.graph = GraphSupervisor::with_inference(
        state.store.clone(),
        state.artifacts.app_data_dir.clone(),
        options.resources.clone(),
        state.maintenance.clone(),
        Some(inference.clone()),
    );
    if scenario.world_enabled && !state.graph.runtime_available() {
        return Err("staged Graph worker/runtime not available".into());
    }
    let loupist = crate::commands::loupist::create_folder_loupist(state, workspace)
        .await
        .map_err(|e| e.message)?;
    let mut config = settings(options, scenario)?;
    let world_model = models
        .iter()
        .find(|model| model.id == options.world_model)
        .unwrap();
    config.run_options = world_model
        .run_controls
        .iter()
        .map(|control| (control.id.clone(), control.default_choice.clone()))
        .collect();
    world_model
        .validate_run_options(&config.run_options)
        .map_err(|error| error.message)?;
    if scenario.seed == "template" {
        // As `loupist_create` does for a template Loupist that keeps its default focus.
        config.extension = template_seed(scenario, crate::commands::now_ms())
            .ok_or("template seed requires a curated template")?;
        let mut connection = state.store.lock_loupist(&loupist.id)?;
        let tx = connection.transaction()?;
        crate::knowledge::apply_preset(
            &tx,
            &loupist.id,
            Some(scenario.template.as_str()),
            crate::commands::now_ms(),
        )?;
        tx.commit()?;
    }
    save_settings(state, &loupist.id, &config)?;
    remember_focus(state, &loupist.id, scenario.research_focus.trim())?;
    // Connections are exhaustive, and nonempty lists require replay. Native Pi
    // host permissions remain the production permissions.
    super::enable_connections(state, &loupist.id, &scenario.connections)?;
    let model_evidence = models
        .iter()
        .filter(|m| m.id == options.model || m.id == options.world_model)
        .collect::<Vec<_>>();
    Ok((
        loupist.id,
        json!({"settings":config,"model_catalog":model_evidence,
        "world_provider_readiness":readiness,"runtime_available":state.graph.runtime_available(),
        "resource_dir":options.resources,
        "data_access":{"fixture":"canonical_sources_and_optional_replay","marketplace_connections":scenario.connections,"native_tools":"production_host_permissions"}}),
        inference,
        config,
    ))
}

pub async fn run(options: Options) -> Result<()> {
    if !options.pi_agent_dir.is_absolute() || !options.pi_agent_dir.is_dir() {
        return Err("--pi-agent-dir must be an existing absolute directory, used in place".into());
    }
    if options.out.as_os_str().is_empty() || options.out.exists() {
        return Err("--out must name a new output; retries require a distinct file".into());
    }
    let suite_bytes = fs::read(&options.suite)?;
    let suite: Suite = serde_json::from_slice(&suite_bytes)?;
    validate(&suite, &options)?;
    if !suite.scenarios.iter().any(|s| selected(s, &options)) {
        return Err("no scenarios selected".into());
    }
    // Freeze external data before the first model call. No hidden/oracle fields
    // are deserialized, and replay bytes are included in execution identity.
    let inputs = suite.scenarios.iter().filter(|s| selected(s, &options)).map(|scenario| {
        let bytes = scenario.replay.as_ref().map(|path| fs::read(options.suite.parent()
            .unwrap_or(std::path::Path::new(".")).join(path))).transpose()?;
        let replay: Option<super::replay::Replay> = bytes.as_deref().map(serde_json::from_slice).transpose()?;
        let replay_sha256 = bytes.as_deref().map(crate::util::sha256);
        let fingerprint = crate::util::sha256(&serde_json::to_vec(&json!({"scenario":scenario,
            "replay_sha256":replay_sha256,"turn_timeout_ms":options.turn_timeout_ms,
            "world_timeout_ms":options.world_timeout_ms,"stage_timeout_ms":options.stage_timeout_ms}))?);
        Ok((scenario, replay, replay_sha256, fingerprint))
    }).collect::<Result<Vec<_>>>()?;
    let run_id = uuid::Uuid::new_v4().to_string();
    let executable = std::env::current_exe()?;
    let binary_sha256 = crate::util::sha256(&fs::read(&executable)?);
    let pi = artifacts()?;
    let harness = json!({"suite_sha256":crate::util::sha256(&suite_bytes),"runner_sha256":binary_sha256,
        "system_prompt_sha256":crate::util::sha256(&fs::read(&pi.system_prompt)?),
        "tool_catalog_sha256":crate::util::sha256(include_bytes!("../../../agent/tool-catalog.json")),
        "dirty":batch::dirty(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))?,
        "trial_count":options.trials,"selected_trial_index":options.trial_index,
        "chat_model":options.model,"chat_thinking":options.thinking,"world_model":options.world_model,
        "world_thinking":options.world_thinking,"turn_timeout_ms":options.turn_timeout_ms,
        "world_timeout_ms":options.world_timeout_ms,"stage_timeout_ms":options.stage_timeout_ms});
    let journal = PathBuf::from(format!("{}.pending.jsonl", options.out.display()));
    if journal.exists() {
        return Err("attempt journal already exists".into());
    }
    let mut output_bytes = vec![];
    let mut pending_bytes = vec![];
    let mut failed = 0_usize;
    'scenarios: for (scenario, frozen_replay, replay_sha256, input_fingerprint) in inputs {
        for trial in 0..options.trials {
            if options
                .trial_index
                .is_some_and(|selected| selected != trial)
            {
                continue;
            }
            let temporary = tempfile::Builder::new()
                .prefix("loupist-world-eval-")
                .tempdir_in(if cfg!(unix) {
                    PathBuf::from("/tmp")
                } else {
                    std::env::temp_dir()
                })?;
            let workspace = temporary.path().join("workspace");
            fs::create_dir(&workspace)?;
            let initial_files = introduce_workspace_files(&workspace, &scenario.workspace_files)?;
            let mut state = AppState::for_eval(temporary.path().join("app"));
            let replay_guard = frozen_replay
                .clone()
                .map(|replay| replay.install(&state.credential_environment))
                .transpose()?;
            fs::write(
                state.artifacts.app_data_dir.join("config.toml"),
                "[pi.retry]\nenabled = false\nmax_retries = 0\n",
            )?;
            let attempt_id = uuid::Uuid::new_v4().to_string();
            let base = json!({"schema":"loupist-world-trace/1","run_id":run_id,"attempt_id":attempt_id,
                "suite":suite.suite,"scenario_id":scenario.id,"condition":scenario.condition,"split":scenario.split,
                "trial":trial,"harness":harness,"scenario_input_sha256":input_fingerprint,"replay_sha256":replay_sha256,
                "trial_root":temporary.path(),"workspace":workspace});
            let mut setup_record = base.clone();
            setup_record["step_id"] = json!("__setup");
            setup_record["introduced_workspace_files"] = initial_files;
            setup_record["status"] = json!("interrupted");
            setup_record["started_at"] = json!(chrono::Utc::now().to_rfc3339());
            append_atomic(&journal, &mut pending_bytes, &setup_record)?;
            let initialized = setup(&mut state, &options, scenario, &workspace).await;
            let mut blocked = None;
            let mut loupist_id = String::new();
            let mut inference = None;
            let mut stop_batch = false;
            let mut config = settings(&options, scenario)?;
            match initialized {
                Ok((id, evidence, service, configured)) => {
                    loupist_id = id;
                    inference = Some(service);
                    config = configured;
                    setup_record["status"] = json!("completed");
                    setup_record["evidence"] = evidence;
                    setup_record["loupist_id"] = json!(loupist_id);
                }
                Err(error) => {
                    setup_record["status"] = json!("failed");
                    setup_record["error"] = super::trace::bounded_json(&json!(error.to_string()));
                    blocked = Some("setup_failed");
                    stop_batch = error.to_string().starts_with("World provider unavailable:");
                    failed += 1;
                }
            }
            append_atomic(&options.out, &mut output_bytes, &setup_record)?;
            if blocked.is_none() {
                let mode = match scenario.seed.as_str() {
                    "template" => "template",
                    _ => "none",
                };
                let mut seed_record = base.clone();
                seed_record["step_id"] = json!("__seed");
                seed_record["loupist_id"] = json!(loupist_id);
                seed_record["started_at"] = json!(chrono::Utc::now().to_rfc3339());
                seed_record["status"] = json!("interrupted");
                append_atomic(&journal, &mut pending_bytes, &seed_record)?;
                let started = Instant::now();
                let service = inference
                    .as_ref()
                    .expect("inference follows a completed setup");
                let settled = schema_trace(&state, &loupist_id, mode).map(|mut trace| {
                    trace["seed_inference"] = json!([]);
                    trace
                });
                match settled {
                    Ok(trace) => {
                        seed_record["world_inference"] = trace["seed_inference"].clone();
                        seed_record["world_schema"] = trace;
                        seed_record["status"] = json!("completed");
                    }
                    Err(error) => {
                        let message = error.to_string();
                        let mut trace =
                            schema_trace(&state, &loupist_id, mode).unwrap_or(Value::Null);
                        if let Some(object) = trace.as_object_mut() {
                            object.insert(
                                "seed_inference".into(),
                                json!(service.eval_observations()),
                            );
                        }
                        seed_record["world_inference"] =
                            trace.get("seed_inference").cloned().unwrap_or(Value::Null);
                        seed_record["world_schema"] = trace;
                        seed_record["status"] = json!("failed");
                        seed_record["error"] = super::trace::bounded_json(&json!(message.clone()));
                        blocked = Some("seed_failed");
                        failed += 1;
                        let quota = message.contains("rate_limit") || message.contains("quota");
                        if quota || message.contains("seed inference unconfirmed") {
                            stop_batch = true;
                            seed_record["batch_stop_reason"] = json!(if quota {
                                "rate_limit"
                            } else {
                                "world_inference_unconfirmed_no_automatic_retry"
                            });
                        }
                    }
                }
                seed_record["duration_ms"] = json!(started.elapsed().as_millis() as u64);
                eprintln!(
                    "World {} trial={} step=__seed status={} duration={}ms",
                    scenario.id,
                    trial,
                    seed_record["status"].as_str().unwrap_or("unknown"),
                    started.elapsed().as_millis()
                );
                append_atomic(&options.out, &mut output_bytes, &seed_record)?;
            }
            let mut chats: BTreeMap<String, ChatSession> = BTreeMap::new();
            let mut chat_turns: BTreeMap<String, usize> = BTreeMap::new();
            for (index, step) in scenario.steps.iter().enumerate() {
                let mut record = base.clone();
                record["step_id"] = json!(step.id);
                record["step_index"] = json!(index);
                record["loupist_id"] = json!(loupist_id);
                record["started_at"] = json!(chrono::Utc::now().to_rfc3339());
                record["status"] = json!("interrupted");
                append_atomic(&journal, &mut pending_bytes, &record)?;
                if let Some(reason) = blocked {
                    record["status"] = json!("skipped");
                    record["error"] = json!(reason);
                    append_atomic(&options.out, &mut output_bytes, &record)?;
                    continue;
                }
                let started = Instant::now();
                let execution: Result<()> = async {
                    record["introduced_workspace_files"] = introduce_workspace_files(&workspace, &step.workspace_files)?;
                    if index == 0 {
                        record["seed_ingestion"] = ingest(&state, &loupist_id, &config, &scenario.seed_sources)?;
                        if !scenario.seed_lenses.is_empty() {
                            let seeded = lens_seed::seed(&state, &loupist_id, &scenario.seed_lenses, &record["seed_ingestion"])?;
                            record["seed_lenses"] = seeded;
                        }
                    }
                    record["source_ingestion"] = ingest(&state, &loupist_id, &config, &step.sources)?;
                    record["processing"] = drain(&state, &loupist_id, scenario.world_enabled, options.stage_timeout_ms).await?;
                    record["before_chat"] = snapshot(&state, &loupist_id, step.query.as_deref()).await?;
                    if record["processing"]["status"] != "completed" { return Err("World processing did not complete".into()); }
                    if record["before_chat"]["query_status"] == "failed" { return Err("World retrieval did not complete".into()); }
                    if let Some(user) = &step.user {
                        let chat_name = step.chat.as_deref().unwrap_or(&step.id).to_string();
                        if !chats.contains_key(&chat_name) { chats.insert(chat_name.clone(), new_chat(&state, &loupist_id, &chat_name)?); }
                        let turn_index = *chat_turns.get(&chat_name).unwrap_or(&0);
                        let session = Session { id:chat_name.clone(), role:step.role.clone(), memory:step.memory.clone(), turns:vec![] };
                        let turn = Turn { user:user.clone(), as_of:None, ..Default::default() };
                        let mut chat_trace = run_turn(&state, &chats[&chat_name], &session, &turn, &options.model,
                            &options.thinking, options.turn_timeout_ms).await?;
                        let (provider, model) = options.model.split_once('/').unwrap();
                        for (key,value) in json!({"schema":"loupist-agent-trace/1","run_id":run_id,"suite":suite.suite,
                            "scenario_id":scenario.id,"trial":trial,"session_id":chat_name,"session_role":step.role,
                            "turn_index":turn_index,"user":user,"memory_mode":step.memory,
                            "model":{"provider":provider,"model":model,"thinking":options.thinking}}).as_object().unwrap() {
                            chat_trace[key] = value.clone();
                        }
                        chat_turns.insert(chat_name, turn_index + 1);
                        let success = chat_trace["status"] == "completed";
                        stop_batch = batch::provider_rejection(&chat_trace);
                        record["chat_trace"] = chat_trace;
                        record["post_chat_processing"] = drain(&state, &loupist_id, scenario.world_enabled, options.stage_timeout_ms).await?;
                        record["after_chat"] = snapshot(&state, &loupist_id, step.query.as_deref()).await?;
                        if !success { return Err("Chat did not complete".into()); }
                        if record["post_chat_processing"]["status"] != "completed" { return Err("post-Chat World processing did not complete".into()); }
                        if record["after_chat"]["query_status"] == "failed" { return Err("post-Chat World retrieval did not complete".into()); }
                    }
                    Ok(())
                }.await;
                // Retain this step's files before the next Chat can overwrite them.
                // Capture failure limits evidence; it does not change the Chat outcome.
                let checkpoint_dir = options
                    .out
                    .with_extension("workspace-checkpoints")
                    .join(&attempt_id)
                    .join(format!("step-{index:04}"));
                record["workspace_checkpoint"] = match checkpoint::capture(
                    &workspace,
                    &checkpoint_dir,
                ) {
                    Ok(checkpoint) => checkpoint,
                    Err(error) => {
                        json!({"status":"failed","directory":checkpoint_dir,"error":error.to_string()})
                    }
                };
                record["world_inference"] = inference
                    .as_ref()
                    .map_or(Value::Null, |service| json!(service.eval_observations()));
                record["duration_ms"] = json!(started.elapsed().as_millis() as u64);
                match execution {
                    Ok(()) => record["status"] = json!("completed"),
                    Err(error) => {
                        record["status"] = json!("failed");
                        record["error"] = super::trace::bounded_json(&json!(error.to_string()));
                        blocked = Some("prior_step_failed");
                        failed += 1;
                    }
                }
                if record["status"] == "failed"
                    && record["world_inference"].as_array().is_some_and(|calls| {
                        calls.iter().any(|call| call["disposition"] != "completed")
                    })
                {
                    stop_batch = true;
                    record["batch_stop_reason"] =
                        json!("world_inference_unconfirmed_no_automatic_retry");
                }
                eprintln!(
                    "World {} trial={} step={} status={} duration={}ms",
                    scenario.id,
                    trial,
                    step.id,
                    record["status"].as_str().unwrap_or("unknown"),
                    started.elapsed().as_millis()
                );
                append_atomic(&options.out, &mut output_bytes, &record)?;
            }
            let shutdown = state.shutdown().await;
            if let Err(error) = shutdown {
                let mut record = base.clone();
                record["step_id"] = json!("__shutdown");
                record["status"] = json!("failed");
                record["error"] = super::trace::bounded_json(&json!(error.to_string()));
                append_atomic(&options.out, &mut output_bytes, &record)?;
                failed += 1;
            }
            drop(state);
            drop(replay_guard);
            if options.keep {
                eprintln!("kept {}", temporary.keep().display());
            }
            if stop_batch {
                eprintln!("Stopping World batch: provider unavailable/rejected or World inference unconfirmed; no automatic retry.");
                break 'scenarios;
            }
        }
    }
    if failed > 0 {
        return Err(format!("{failed} World attempt stages failed; all records preserved").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LoupistStore;

    fn source() -> Source {
        Source {
            id: "fixture-original".into(),
            title: "Fictional original".into(),
            text: "Fictional Seorim Labs had USD 24 million consolidated revenue in 2026 Q2."
                .into(),
            origin: "https://fictional.example/seorim/q2".into(),
            publication_time: Some("2026-08-01T09:30:00Z".into()),
            acquired_at: Some("2026-08-02T12:00:00Z".into()),
            authorship: Authorship::Publisher,
        }
    }

    #[tokio::test]
    async fn world_eval_retains_exact_material_and_deduplicates_without_enabling_processing() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_eval(temp.path().join("app"));
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        let a = crate::commands::loupist::create_folder_loupist(&state, &first)
            .await
            .unwrap();
        let b = crate::commands::loupist::create_folder_loupist(&state, &second)
            .await
            .unwrap();
        let config = GraphProcessingSettings::default();
        save_settings(&state, &a.id, &config).unwrap();
        save_settings(&state, &b.id, &config).unwrap();
        let one = ingest(&state, &a.id, &config, &[source()]).unwrap();
        let two = ingest(&state, &a.id, &config, &[source()]).unwrap();
        let isolated = ingest(&state, &b.id, &config, &[source()]).unwrap();
        assert_eq!(one[0]["saved"], two[0]["saved"]);
        assert_ne!(
            one[0]["saved"]["recordId"],
            isolated[0]["saved"]["recordId"]
        );
        assert_eq!(one[0]["canonical_text"], source().text);
        assert_eq!(one[0]["acquired_at_ms"], 1785672000000_i64);
        let store = KnowledgeStore::for_loupist(state.store.clone(), &a.id).unwrap();
        let records = store.list_records(&a.id).unwrap();
        assert_eq!(
            records
                .iter()
                .filter(|record| record.kind == RecordKind::Source)
                .count(),
            1
        );
        assert!(!records
            .iter()
            .any(|record| record.kind == RecordKind::World));
        assert!(store.graph_jobs(&a.id).unwrap().is_empty());
        let record = records
            .into_iter()
            .find(|record| record.kind == RecordKind::Source)
            .unwrap();
        let snapshot = store
            .record_snapshot(&a.id, record.revision_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(
            snapshot.payload["source"]["captures"][0]["publicationTime"]["value"],
            "2026-08-01T09:30:00Z"
        );
        assert!(KnowledgeStore::for_loupist(state.store.clone(), &b.id)
            .unwrap()
            .record_snapshot(&b.id, record.revision_id.as_deref().unwrap())
            .is_err());
        state.shutdown().await.unwrap();
    }

    #[test]
    fn world_eval_delivers_fixtures_in_order_without_replacing_chat_files() {
        let temp = tempfile::tempdir().unwrap();
        let initial = BTreeMap::from([("initial.txt".into(), "initial bytes".into())]);
        let later = BTreeMap::from([("updates/correction.txt".into(), "corrected bytes".into())]);
        let manifest = introduce_workspace_files(temp.path(), &initial).unwrap();
        assert_eq!(
            manifest["initial.txt"],
            crate::util::sha256(b"initial bytes")
        );
        assert!(!temp.path().join("updates/correction.txt").exists());
        introduce_workspace_files(temp.path(), &later).unwrap();
        assert_eq!(
            fs::read_to_string(temp.path().join("updates/correction.txt")).unwrap(),
            "corrected bytes"
        );
        assert!(introduce_workspace_files(temp.path(), &initial).is_err());
        assert_eq!(
            fs::read_to_string(temp.path().join("initial.txt")).unwrap(),
            "initial bytes"
        );
        for path in ["../escape", ".loupist/canonical.txt", "/absolute"] {
            assert!(introduce_workspace_files(
                temp.path(),
                &BTreeMap::from([(path.into(), "bad".into())])
            )
            .is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn world_eval_fixture_delivery_refuses_symlink_parents() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), temp.path().join("link")).unwrap();
        let files = BTreeMap::from([("link/escape.txt".into(), "bad".into())]);
        assert!(introduce_workspace_files(temp.path(), &files).is_err());
        assert!(!outside.path().join("escape.txt").exists());
    }

    fn suite_value() -> Value {
        json!({"schema":"loupist-world-suite/1","version":1,"suite":"world-test",
            "scenarios":[{"id":"W-1","title":"Fixture","condition":"clean","world_enabled":true,
                "seed_sources":[source()],"steps":[{"id":"probe","user":"What is known?","memory":"read_only"}]}]})
    }

    #[test]
    fn template_seed_corpus_only_runs_the_template_arm_for_shipping_seeds() {
        let checked_in: Value = serde_json::from_str(include_str!(
            "../../../evals/world/template-seed-corpus-v2.json"
        ))
        .unwrap();
        let suite: Suite = serde_json::from_value(checked_in.clone()).unwrap();
        validate(&suite, &Options::default()).unwrap();
        // Keep failed class fixtures in the no-seed corpus for future measurement.
        // Their template arm must fail validation rather than silently run unseeded.
        for scenario in checked_in["scenarios"].as_array().unwrap() {
            let mut template_arm = checked_in.clone();
            template_arm["scenarios"] = json!([scenario]);
            template_arm["scenarios"][0]["seed"] = json!("template");
            let suite: Suite = serde_json::from_value(template_arm).unwrap();
            let scenario = &suite.scenarios[0];
            let ships = scenario.template == "macro";
            assert_eq!(
                validate(&suite, &Options::default()).is_ok(),
                ships,
                "{}",
                scenario.id
            );
            assert_eq!(
                template_seed(scenario, 1).is_some(),
                ships,
                "{}",
                scenario.id
            );
        }
    }

    #[test]
    fn world_eval_validation_rejects_bad_suites_before_live_work() {
        assert!(time_point("2026-02-30").is_err());
        assert!(time_point("tomorrow").is_err());
        assert_eq!(
            time_point("2026-08-01T09:30:00Z").unwrap().precision,
            TimePrecision::Instant
        );
        let check = |edit: &dyn Fn(&mut Value), options: &Options| {
            let mut value = suite_value();
            edit(&mut value);
            validate(&serde_json::from_value(value).unwrap(), options)
        };
        let options = Options::default();
        type Edit = fn(&mut Value);
        let accepted: &[(&str, Edit)] = &[
            ("seed none", |value| {
                value["scenarios"][0]["research_focus"] = json!("Clinical-stage biotech pipelines");
                value["scenarios"][0]["seed"] = json!("none");
            }),
            ("seed-only first step", |value| {
                value["scenarios"][0]["steps"] = json!([{"id":"ingest_sources"}]);
            }),
            ("seed template", |value| {
                value["scenarios"][0]["research_focus"] =
                    json!("Chips, memory, foundries and equipment");
                value["scenarios"][0]["seed"] = json!("template");
                value["scenarios"][0]["template"] = json!("semiconductors");
            }),
            ("template seed with an edited focus", |value| {
                value["scenarios"][0]["research_focus"] = json!("Only memory manufacturers");
                value["scenarios"][0]["seed"] = json!("template");
                value["scenarios"][0]["template"] = json!("semiconductors");
            }),
        ];
        for (name, edit) in accepted {
            assert!(check(edit, &options).is_ok(), "{name}");
        }
        let rejected: &[(&str, Edit)] = &[
            ("unknown seed mode", |value| {
                value["scenarios"][0]["research_focus"] = json!("Pipelines");
                value["scenarios"][0]["seed"] = json!("later");
            }),
            ("retired model seed", |value| {
                value["scenarios"][0]["research_focus"] = json!("Clinical-stage biotech pipelines");
                value["scenarios"][0]["seed"] = json!("model");
            }),
            ("template seed without a curated template", |value| {
                value["scenarios"][0]["research_focus"] = json!("Companies I hold and watch");
                value["scenarios"][0]["seed"] = json!("template");
                value["scenarios"][0]["template"] = json!("portfolio");
            }),
            ("oversized focus", |value| {
                value["scenarios"][0]["research_focus"] = json!("x".repeat(281));
                value["scenarios"][0]["seed"] = json!("none");
            }),
            ("empty first step", |value| {
                value["scenarios"][0]["seed_sources"] = json!([]);
                value["scenarios"][0]["steps"] = json!([{"id":"empty"}]);
            }),
            ("empty later step", |value| {
                value["scenarios"][0]["steps"] =
                    json!([{"id":"ingest_sources"},{"id":"empty_later"}]);
            }),
            ("blank question", |value| {
                value["scenarios"][0]["steps"] = json!([{"id":"empty","user":" "}]);
            }),
            ("unknown memory mode", |value| {
                value["scenarios"][0]["steps"] =
                    json!([{"id":"bad","user":"Question","memory":"invalid"}]);
            }),
            ("conflicting fixture", |value| {
                let mut changed = serde_json::to_value(source()).unwrap();
                changed["text"] =
                    json!("A changed document pretending to have the same fixture ID.");
                value["scenarios"][0]["steps"][0]["sources"] = json!([changed]);
            }),
            ("replaced workspace file", |value| {
                value["scenarios"][0]["workspace_files"] = json!({"initial.txt":"one"});
                value["scenarios"][0]["steps"][0]["workspace_files"] = json!({"initial.txt":"two"});
            }),
        ];
        for (name, edit) in rejected {
            assert!(check(edit, &options).is_err(), "{name}");
        }
        let unknown = Options {
            scenarios: vec!["does-not-exist".into()],
            ..Options::default()
        };
        assert!(check(&|_| {}, &unknown).is_err());
    }

    #[tokio::test]
    async fn world_eval_none_leaves_the_extension_empty() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_eval(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let focus = "Clinical-stage biotech pipelines";
        let config = GraphProcessingSettings {
            enrichment_enabled: true,
            helper_provider: Some("openai".into()),
            helper_model: Some("fixture-model".into()),
            helper_model_profile_id: Some("openai/fixture-model".into()),
            research_focus: focus.into(),
            max_llm_calls: 16,
            ..Default::default()
        };
        save_settings(&state, &loupist.id, &config).unwrap();
        remember_focus(&state, &loupist.id, focus).unwrap();
        assert_eq!(
            state.store.get_loupist(&loupist.id).unwrap().unwrap().focus,
            focus
        );
        let empty = schema_trace(&state, &loupist.id, "none").unwrap();
        assert!(empty["changes"].as_array().unwrap().is_empty());
        assert_eq!(empty["extension"]["types"].as_array().unwrap().len(), 0);
        assert_eq!(empty["extension"]["version"], 0);
        state.shutdown().await.unwrap();
    }

    #[test]
    fn world_eval_never_deserializes_hidden_oracles_into_execution_inputs() {
        let mut value = suite_value();
        value["scenarios"][0]["hidden"] = json!({"answer":"Hidden scoring secret"});
        value["scenarios"][0]["steps"][0]["expect"] = json!({"answer":"Hidden scoring secret"});
        let parsed: Suite = serde_json::from_value(value).unwrap();
        let executable = serde_json::to_string(&parsed.scenarios[0]).unwrap();
        assert!(!executable.contains("Hidden scoring secret"));
        assert_eq!(
            parsed.scenarios[0].steps[0].user.as_deref(),
            Some("What is known?")
        );
    }

    #[tokio::test]
    async fn world_eval_pending_and_cancelled_jobs_cannot_report_success() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_eval(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let config = GraphProcessingSettings {
            enrichment_enabled: true,
            helper_provider: Some("openai".into()),
            helper_model: Some("fixture-model".into()),
            ..Default::default()
        };
        save_settings(&state, &loupist.id, &config).unwrap();
        ingest(&state, &loupist.id, &config, &[source()]).unwrap();
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
        let jobs = store.graph_jobs(&loupist.id).unwrap();
        assert!(!jobs.is_empty());
        let timed_out = drain(&state, &loupist.id, true, 0).await.unwrap();
        assert_eq!(timed_out["status"], "failed");
        assert_eq!(timed_out["error"], "world_stage_timeout");
        assert_eq!(timed_out["jobs"][0]["status"], "queued");
        {
            let mut connection = state.store.lock_loupist(&loupist.id).unwrap();
            let tx = connection.transaction().unwrap();
            // A World removed mid-run is the remaining producer of this status.
            for job in jobs {
                tx.execute(
                    "UPDATE knowledge_graph_outbox SET status = ?1 WHERE job_id = ?2",
                    rusqlite::params!["cancelled_before_start", job.job_id],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }
        let cancelled = drain(&state, &loupist.id, true, 1000).await.unwrap();
        assert_eq!(cancelled["status"], "failed");
        assert_eq!(cancelled["failures"][0]["status"], "cancelled_before_start");
        assert!(store
            .graph_jobs(&loupist.id)
            .unwrap()
            .iter()
            .all(|job| !job.document.inference_attempted));
        state.shutdown().await.unwrap();
    }
}
