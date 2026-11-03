//! Controlled retrieval scaling through canonical import and the real local index.
//! Prepared claims are fixture input, never a measured extraction or a hidden oracle.
use super::*;

#[derive(Deserialize, Serialize)]
struct ScaleSuite {
    schema: String,
    version: u32,
    sources: Vec<ScaleSource>,
    cases: Vec<ScaleCase>,
}

#[derive(Deserialize, Serialize)]
struct ScaleSource {
    #[serde(flatten)]
    source: Source,
    canonical_import: PreparedClaim,
}

#[derive(Deserialize, Serialize)]
struct PreparedClaim {
    node_id: String,
    name: String,
    entity_identifier: String,
    #[serde(default)]
    aliases: Vec<String>,
    claim_id: String,
    proposition: String,
    relation: String,
    value: Value,
    unit: String,
    #[serde(default)]
    currency: Option<String>,
    period: String,
    basis: String,
    quote: String,
    utf8_start: usize,
    utf8_end_exclusive: usize,
}

#[derive(Deserialize, Serialize)]
struct ScaleCase {
    id: String,
    distractor_count: usize,
    #[serde(default)]
    focal_source_ids: Vec<String>,
    source_ids: Vec<String>,
    queries: Vec<String>,
    browse_page_size: u32,
    user: String,
}

fn entity_node(prepared: &PreparedClaim) -> GraphNode {
    let mut aliases = prepared.aliases.clone();
    aliases.push(prepared.entity_identifier.clone());
    aliases.sort();
    aliases.dedup();
    GraphNode {
        id: prepared.node_id.clone(),
        name: prepared.name.clone(),
        aliases,
        description: format!("Fictional company {}.", prepared.entity_identifier),
        kind: "company".into(),
        source_role: "source".into(),
        properties: BTreeMap::from([(
            "entity_identifier".into(),
            json!(prepared.entity_identifier),
        )]),
    }
}

fn validate(suite: &ScaleSuite, options: &Options) -> Result<()> {
    if suite.schema != "loupist-world-retrieval-scale/1" || suite.version != 1 {
        return Err("unsupported World retrieval-scale schema/version".into());
    }
    if options.trials == 0
        || options
            .trial_index
            .is_some_and(|trial| trial >= options.trials)
        || options.turn_timeout_ms == 0
        || options.stage_timeout_ms == 0
        || options.world_timeout_ms < 1_000
        || options.world_timeout_ms > 120_000
    {
        return Err("invalid scale trial count or timeout".into());
    }
    if options
        .condition
        .as_deref()
        .is_some_and(|value| value != "scale")
    {
        return Err("retrieval-scale supports only --condition scale".into());
    }
    let mut sources = BTreeSet::new();
    let mut claims = BTreeSet::new();
    let mut nodes = BTreeMap::new();
    let mut value_nodes = BTreeSet::new();
    for item in &suite.sources {
        let source = &item.source;
        let claim = &item.canonical_import;
        if source.id.is_empty()
            || source.text.is_empty()
            || !sources.insert(source.id.as_str())
            || !claims.insert(claim.claim_id.as_str())
            || claim.node_id.trim().is_empty()
            || claim.name.trim().is_empty()
            || claim.entity_identifier.trim().is_empty()
            || claim.proposition.trim().is_empty()
            || claim.quote.is_empty()
            || !claim.value.is_number()
        {
            return Err("invalid or duplicate scale source/claim identity".into());
        }
        let entity = entity_node(claim);
        if let Some(previous) = nodes.get(&claim.node_id) {
            if previous != &entity {
                return Err("conflicting prepared entity definition for the same node_id".into());
            }
        } else {
            nodes.insert(claim.node_id.clone(), entity);
        }
        value_nodes.insert(format!("{}-value", claim.claim_id));
        if source
            .text
            .as_bytes()
            .get(claim.utf8_start..claim.utf8_end_exclusive)
            != Some(claim.quote.as_bytes())
        {
            return Err("prepared scale claim quote does not match retained UTF-8 bytes".into());
        }
        if let Some(date) = &source.publication_time {
            time_point(date)?;
        }
        acquired(source)?;
        if crate::util::credential_text::contains_credential_text(&source.text)
            || crate::util::credential_text::contains_credential_text(&source.origin)
        {
            return Err("credential-shaped scale source refused".into());
        }
    }
    if nodes.keys().any(|id| value_nodes.contains(id)) {
        return Err("prepared entity identity collides with a claim value node".into());
    }
    let mut cases = BTreeSet::new();
    for case in &suite.cases {
        if case.id.is_empty()
            || !cases.insert(case.id.as_str())
            || case.source_ids.is_empty()
            || case.source_ids.len() != case.distractor_count + case.focal_source_ids.len().max(1)
            || case.focal_source_ids.iter().collect::<BTreeSet<_>>().len()
                != case.focal_source_ids.len()
            || case
                .focal_source_ids
                .iter()
                .any(|id| !case.source_ids.contains(id))
            || case.source_ids.iter().collect::<BTreeSet<_>>().len() != case.source_ids.len()
            || case
                .source_ids
                .iter()
                .any(|id| !sources.contains(id.as_str()))
            || case.queries.is_empty()
            || case
                .queries
                .iter()
                .any(|query| query.is_empty() || query.len() > 500)
            || !(1..=100).contains(&case.browse_page_size)
            || case.user.trim().is_empty()
        {
            return Err("invalid scale case, source references, or query bounds".into());
        }
    }
    if options
        .scenarios
        .iter()
        .any(|id| !cases.contains(id.as_str()))
    {
        return Err("unknown retrieval-scale scenario".into());
    }
    Ok(())
}

fn scenario(case: &ScaleCase) -> Scenario {
    Scenario {
        id: case.id.clone(),
        title: format!(
            "World retrieval scale: {} distractors",
            case.distractor_count
        ),
        split: "retrieval_scale".into(),
        condition: "scale".into(),
        // Import and local embedding are explicit; automatic paid extraction is off.
        world_enabled: false,
        connections: vec![],
        replay: None,
        workspace_files: BTreeMap::new(),
        seed_sources: vec![],
        seed_lenses: vec![],
        research_focus: String::new(),
        seed: String::new(),
        template: String::new(),
        steps: vec![],
    }
}

fn canonical_import(
    state: &AppState,
    loupist_id: &str,
    config: &GraphProcessingSettings,
    suite: &ScaleSuite,
    case: &ScaleCase,
) -> Result<(WorldRevision, Value)> {
    if config.enrichment_enabled {
        return Err("scale import requires automatic World extraction disabled".into());
    }
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?;
    if !store.graph_jobs(loupist_id)?.is_empty() {
        return Err("scale import requires a fresh Loupist without graph jobs".into());
    }
    let mut nodes = vec![];
    let mut entity_ids = BTreeSet::new();
    let mut claims = vec![];
    let mut dependencies = vec![];
    let mut ingested = vec![];
    let mut retained_time = 0_i64;
    for id in &case.source_ids {
        let item = suite
            .sources
            .iter()
            .find(|source| &source.source.id == id)
            .ok_or("scale source reference disappeared")?;
        let source = &item.source;
        let prepared = &item.canonical_import;
        let records = ingest(state, loupist_id, config, std::slice::from_ref(source))?;
        let saved: RetainedSource = serde_json::from_value(records[0]["saved"].clone())?;
        let span_id = saved
            .span_id
            .as_ref()
            .ok_or("retained scale source has no span")?;
        let at_ms = acquired(source)?;
        retained_time = retained_time.max(at_ms);
        let target_id = format!("{}-value", prepared.claim_id);
        if entity_ids.insert(prepared.node_id.clone()) {
            nodes.push(entity_node(prepared));
        }
        let amount = format!(
            "{} {}{}",
            prepared.value,
            prepared
                .currency
                .as_ref()
                .map(|currency| format!("{currency} "))
                .unwrap_or_default(),
            prepared.unit
        );
        nodes.push(GraphNode {
            id: target_id.clone(),
            name: format!("{amount} ({})", prepared.period),
            aliases: vec![],
            description: prepared.proposition.clone(),
            kind: "reported_value".into(),
            source_role: "source".into(),
            properties: BTreeMap::new(),
        });
        let mut qualifiers = vec![
            Qualifier {
                key: "value".into(),
                value: prepared.value.to_string(),
            },
            Qualifier {
                key: "unit".into(),
                value: prepared.unit.clone(),
            },
            Qualifier {
                key: "period".into(),
                value: prepared.period.clone(),
            },
            Qualifier {
                key: "basis".into(),
                value: prepared.basis.clone(),
            },
        ];
        if let Some(currency) = &prepared.currency {
            qualifiers.push(Qualifier {
                key: "currency".into(),
                value: currency.clone(),
            });
        }
        claims.push(GraphClaim {
            id: prepared.claim_id.clone(),
            proposition: prepared.proposition.clone(),
            attribution: source.origin.clone(),
            relation: prepared.relation.clone(),
            endpoints: vec![
                ClaimEndpoint {
                    node_id: prepared.node_id.clone(),
                    role: "source".into(),
                },
                ClaimEndpoint {
                    node_id: target_id,
                    role: "target".into(),
                },
            ],
            qualifiers,
            evidence: vec![EvidenceBinding {
                span_id: span_id.clone(),
                role: BindingRole::ExtractedFrom,
                attribution: source.origin.clone(),
            }],
            status: ClaimStatus::Active,
            recorded_at_ms: Some(at_ms),
            publication_time: source
                .publication_time
                .as_deref()
                .map(time_point)
                .transpose()?,
            effective_time: None,
            valid_at: None,
            invalid_at: None,
            expired_at: None,
        });
        dependencies.push(EvidenceRevision {
            record_id: saved.record_id.clone(),
            revision_digest: saved.revision_digest.clone(),
            role: DependencyRole::WorldSource,
        });
        ingested.push(json!({"fixture_id":source.id,"retained":records[0],
            "prepared_claim_id":prepared.claim_id,"evidence_span_id":span_id,
            "quote":prepared.quote,"quote_utf8_start":prepared.utf8_start,
            "quote_utf8_end_exclusive":prepared.utf8_end_exclusive}));
    }
    let world = store.create_record(CreateRecord {
        loupist_id: loupist_id.into(),
        kind: RecordKind::World,
        title: "World".into(),
        at_ms: retained_time,
    })?;
    let revision = store.seal_world(SealWorld {
        loupist_id: loupist_id.into(),
        record_id: world.record_id,
        expected_revision_id: None,
        at_ms: retained_time,
        nodes,
        claims,
        annotations: vec![],
        dependencies,
    })?;
    if !store.graph_jobs(loupist_id)?.is_empty() {
        return Err("canonical scale import unexpectedly scheduled extraction".into());
    }
    Ok((
        revision,
        json!({"source_count":case.source_ids.len(),"claim_count":case.source_ids.len(),
        "sources":ingested,"import_kind":"prepared_canonical_claims_not_live_extraction"}),
    ))
}

fn browse_page(
    state: &AppState,
    loupist_id: &str,
    world: &WorldRevision,
    page_size: u32,
) -> Result<Value> {
    let view = KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?.query_world(
        WorldGraphQuery {
            loupist_id: loupist_id.into(),
            record_id: world.record_id.clone(),
            revision_id: world.revision_id.clone(),
            search: String::new(),
            center_node_id: None,
            center_claim_id: None,
            kinds: vec![],
            relations: vec![],
            binding_roles: vec![],
            statuses: vec![],
            publication_times: vec![],
            published_by: None,
            cursor: None,
            node_limit: Some(page_size * 2),
            edge_limit: Some(page_size),
            byte_limit: Some(262_144),
        },
    )?;
    Ok(serde_json::to_value(view)?)
}

fn assert_no_paid_extraction(
    state: &AppState,
    loupist_id: &str,
    inference: &crate::providers::inference::ProviderInferenceService,
) -> Result<Value> {
    let observations = inference.eval_observations();
    let jobs =
        KnowledgeStore::for_loupist(state.store.clone(), loupist_id)?.graph_jobs(loupist_id)?;
    // Local embedding callbacks also set inference_attempted. The paid-call
    // ledger and the admitted LLM budget distinguish extraction from indexing.
    if !observations.is_empty()
        || jobs
            .iter()
            .any(|job| job.document.extraction.max_llm_calls != 0)
    {
        return Err("retrieval-scale unexpectedly attempted paid World extraction".into());
    }
    Ok(json!({"provider_calls":observations,"jobs":jobs,"paid_extraction_attempts":0}))
}

pub async fn run(options: Options) -> Result<()> {
    if !options.pi_agent_dir.is_absolute() || !options.pi_agent_dir.is_dir() {
        return Err("--pi-agent-dir must be an existing absolute directory, used in place".into());
    }
    if options.out.as_os_str().is_empty() || options.out.exists() {
        return Err("--out must be new; retries require a distinct output".into());
    }
    let bytes = fs::read(&options.suite)?;
    let suite: ScaleSuite = serde_json::from_slice(&bytes)?;
    validate(&suite, &options)?;
    let selected = suite
        .cases
        .iter()
        .filter(|case| options.scenarios.is_empty() || options.scenarios.contains(&case.id))
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return Err("no scale cases selected".into());
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    let pi = artifacts()?;
    let harness = json!({
        "suite_sha256":crate::util::sha256(&bytes),
        "runner_sha256":crate::util::sha256(&fs::read(std::env::current_exe()?)?),
        "system_prompt_sha256":crate::util::sha256(&fs::read(&pi.system_prompt)?),
        "tool_catalog_sha256":crate::util::sha256(include_bytes!("../../../../agent/tool-catalog.json")),
        "dirty":batch::dirty(std::path::Path::new(env!("CARGO_MANIFEST_DIR")))?,
        "chat_model":options.model,"chat_thinking":options.thinking,
        "world_model":options.world_model,"world_thinking":options.world_thinking,
        "automatic_paid_extraction":false,"index_method":"canonical_projection_and_local_embeddings",
        "turn_timeout_ms":options.turn_timeout_ms,"stage_timeout_ms":options.stage_timeout_ms,
        "world_timeout_ms":options.world_timeout_ms,
    });
    let journal = PathBuf::from(format!("{}.pending.jsonl", options.out.display()));
    if journal.exists() {
        return Err("scale attempt journal already exists".into());
    }
    let mut output_bytes = vec![];
    let mut pending_bytes = vec![];
    let mut failures = 0_usize;
    'cases: for case in selected {
        for trial in 0..options.trials {
            if options
                .trial_index
                .is_some_and(|selected| selected != trial)
            {
                continue;
            }
            let temporary = tempfile::Builder::new()
                .prefix("loupist-world-scale-")
                .tempdir_in(if cfg!(unix) {
                    PathBuf::from("/tmp")
                } else {
                    std::env::temp_dir()
                })?;
            let workspace = temporary.path().join("workspace");
            fs::create_dir(&workspace)?;
            let mut state = AppState::for_eval(temporary.path().join("app"));
            fs::write(
                state.artifacts.app_data_dir.join("config.toml"),
                "[pi.retry]\nenabled = false\nmax_retries = 0\n",
            )?;
            let mut record = json!({"schema":"loupist-world-trace/1","experiment":"retrieval_scale",
                "run_id":run_id,"attempt_id":uuid::Uuid::new_v4().to_string(),"suite":"world-scale-v1",
                "scenario_id":case.id,"condition":"scale","split":"retrieval_scale","trial":trial,
                "step_id":"final-probe","harness":harness,"trial_root":temporary.path(),"workspace":workspace,
                "scenario_input_sha256":crate::util::sha256(&serde_json::to_vec(case)?),
                "started_at":chrono::Utc::now().to_rfc3339(),"status":"interrupted",
                "distractor_count":case.distractor_count,"source_count":case.source_ids.len(),
                "focal_source_ids":case.focal_source_ids,
                "cost_usd":null,"embedding_duration_ms":null,
                "embedding_duration_note":"Combined index rebuild duration includes startup and local embeddings; callbacks do not expose separate duration."});
            append_atomic(&journal, &mut pending_bytes, &record)?;
            let started = Instant::now();
            let mut stop_batch = false;
            let mut initialized_inference = None;
            let mut initialized_loupist = None;
            let execution: Result<()> = async {
                let (loupist_id, evidence, inference, config) = setup(&mut state, &options, &scenario(case), &workspace).await?;
                initialized_inference = Some(inference.clone());
                initialized_loupist = Some(loupist_id.clone());
                record["loupist_id"] = json!(loupist_id);
                record["setup"] = evidence;
                if !state.graph.runtime_available() { return Err("staged graph runtime unavailable for scale index".into()); }
                let import_start = Instant::now();
                let (world, imported) = canonical_import(&state, &loupist_id, &config, &suite, case)?;
                record["canonical_import"] = imported;
                record["canonical_import_duration_ms"] = json!(import_start.elapsed().as_millis() as u64);
                record["world_revision"] = serde_json::to_value(&world)?;
                record["first_browse_page"] = browse_page(&state, &loupist_id, &world, case.browse_page_size)?;
                record["before_index"] = snapshot(&state, &loupist_id, None).await?;
                let index_start = Instant::now();
                let accepted = state.graph.index_sealed_world(&loupist_id, &world.record_id, &world.revision_id)?;
                record["index_admission"] = serde_json::to_value(&accepted)?;
                if accepted.document.extraction.max_llm_calls != 0 {
                    return Err("sealed World index admitted nonzero LLM budget".into());
                }
                record["processing"] = drain(&state, &loupist_id, true, options.stage_timeout_ms).await?;
                record["index_rebuild_duration_ms"] = json!(index_start.elapsed().as_millis() as u64);
                record["world_inference"] = json!(inference.eval_observations());
                record["no_paid_extraction"] = assert_no_paid_extraction(&state, &loupist_id, &inference)?;
                if record["processing"]["status"] != "completed" { return Err("scale index rebuild did not complete".into()); }
                let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist_id)?;
                let after_index = store.get_revision(&loupist_id, &world.revision_id)?;
                if after_index.revision_digest != world.revision_digest {
                    return Err("index rebuild changed canonical World revision".into());
                }
                let mut queries = vec![];
                for query in &case.queries {
                    let query_start = Instant::now();
                    let result = tokio::time::timeout(Duration::from_millis(options.stage_timeout_ms),
                        state.graph.query_indexed(&loupist_id, &world.record_id, &world.revision_id, query)).await;
                    let query_record = match result {
                        Ok(Ok(result)) => json!({"query":query,"status":"completed",
                            "duration_ms":query_start.elapsed().as_millis() as u64,
                            "result_bytes":serde_json::to_vec(&result)?.len(),"result":result}),
                        Ok(Err(error)) => json!({"query":query,"status":"failed","error":error.to_string(),
                            "duration_ms":query_start.elapsed().as_millis() as u64,"result":null}),
                        Err(_) => json!({"query":query,"status":"failed","error":"scale_query_timeout",
                            "duration_ms":query_start.elapsed().as_millis() as u64,"result":null}),
                    };
                    let indexed = query_record["status"] == "completed"
                        && query_record["result"]["indexed"] == true
                        && query_record["result"]["mode"] == "hybrid";
                    queries.push(query_record);
                    record["queries"] = json!(queries);
                    if !indexed { return Err("scale query lacks successful real hybrid index evidence".into()); }
                }
                record["before_chat"] = snapshot(&state, &loupist_id, None).await?;
                let chat = new_chat(&state, &loupist_id, "scale-probe")?;
                let session = Session { id:"scale-probe".into(),role:"probe".into(),memory:"read_only".into(),turns:vec![] };
                let turn = Turn { user:case.user.clone(), as_of:None, ..Default::default() };
                let mut chat_trace = run_turn(&state, &chat, &session, &turn, &options.model,
                    &options.thinking, options.turn_timeout_ms).await?;
                let (provider, model) = options.model.split_once('/').ok_or("Chat model must be provider/model")?;
                for (key,value) in json!({"schema":"loupist-agent-trace/1","run_id":run_id,
                    "suite":"world-scale-v1","scenario_id":case.id,"trial":trial,"session_id":"scale-probe",
                    "session_role":"probe","turn_index":0,"user":case.user,"memory_mode":"read_only",
                    "model":{"provider":provider,"model":model,"thinking":options.thinking}}).as_object().unwrap() {
                    chat_trace[key] = value.clone();
                }
                stop_batch = batch::provider_rejection(&chat_trace);
                let success = chat_trace["status"] == "completed";
                record["chat_trace"] = chat_trace;
                record["after_chat"] = snapshot(&state, &loupist_id, None).await?;
                record["world_inference"] = json!(inference.eval_observations());
                record["no_paid_extraction"] = assert_no_paid_extraction(&state, &loupist_id, &inference)?;
                if !success { return Err("scale Chat did not complete".into()); }
                Ok(())
            }.await;
            if let Some(inference) = &initialized_inference {
                record["world_inference"] = json!(inference.eval_observations());
            }
            if let (Some(loupist_id), Some(inference)) =
                (&initialized_loupist, &initialized_inference)
            {
                record["final_extraction_audit"] =
                    match assert_no_paid_extraction(&state, loupist_id, inference) {
                        Ok(evidence) => evidence,
                        Err(error) => {
                            stop_batch = true;
                            json!({"status":"failed","error":error.to_string()})
                        }
                    };
            }
            record["duration_ms"] = json!(started.elapsed().as_millis() as u64);
            match execution {
                Ok(()) => record["status"] = json!("completed"),
                Err(error) => {
                    record["status"] = json!("failed");
                    record["error"] = super::super::trace::bounded_json(&json!(error.to_string()));
                    stop_batch |= error.to_string().starts_with("World provider unavailable:");
                    failures += 1;
                }
            }
            if record["final_extraction_audit"]["status"] == "failed" {
                if record["status"] == "completed" {
                    failures += 1;
                }
                record["status"] = json!("failed");
                record["error"] = json!("retrieval-scale failed the no-paid-extraction audit");
            }
            if let Err(error) = state.shutdown().await {
                record["shutdown_error"] =
                    super::super::trace::bounded_json(&json!(error.to_string()));
                if record["status"] == "completed" {
                    failures += 1;
                }
                record["status"] = json!("failed");
            }
            record["completed_at"] = json!(chrono::Utc::now().to_rfc3339());
            append_atomic(&options.out, &mut output_bytes, &record)?;
            eprintln!(
                "World scale {} trial={} status={} duration={}ms",
                case.id,
                trial,
                record["status"].as_str().unwrap_or("unknown"),
                started.elapsed().as_millis()
            );
            drop(state);
            if options.keep {
                eprintln!("kept {}", temporary.keep().display());
            }
            if stop_batch {
                break 'cases;
            }
        }
    }
    if failures > 0 {
        return Err(format!("{failures} scale attempts failed; all records preserved").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn suite() -> ScaleSuite {
        let text = "Fictional Company declared 9 USD million operating profit in Q2 2026.";
        let mut sources = vec![];
        for index in 0..21 {
            let id = format!("s-{index:02}");
            let name = if index == 20 {
                "Focal Company".into()
            } else {
                format!("Unrelated Company {index}")
            };
            sources.push(ScaleSource {
                source: Source {
                    id: id.clone(),
                    title: name.clone(),
                    text: text.into(),
                    origin: format!("https://fixtures.invalid/{id}"),
                    publication_time: Some("2026-08-05".into()),
                    acquired_at: Some("2026-08-05T12:00:00Z".into()),
                    authorship: Authorship::Publisher,
                },
                canonical_import: PreparedClaim {
                    node_id: format!("n-{index:02}"),
                    name,
                    entity_identifier: format!("FICT-{index:02}"),
                    aliases: vec![],
                    claim_id: format!("c-{index:02}"),
                    proposition: format!(
                        "Company {index} reported 9 USD million operating profit."
                    ),
                    relation: "operating_profit".into(),
                    value: json!(9),
                    unit: "million".into(),
                    currency: Some("USD".into()),
                    period: "2026-Q2".into(),
                    basis: "consolidated".into(),
                    quote: text.into(),
                    utf8_start: 0,
                    utf8_end_exclusive: text.len(),
                },
            });
        }
        ScaleSuite {
            schema: "loupist-world-retrieval-scale/1".into(),
            version: 1,
            cases: vec![ScaleCase {
                id: "page-boundary".into(),
                distractor_count: 20,
                focal_source_ids: vec![],
                source_ids: sources
                    .iter()
                    .map(|source| source.source.id.clone())
                    .collect(),
                queries: vec!["Focal Company".into()],
                browse_page_size: 20,
                user: "Find the focal company result.".into(),
            }],
            sources,
        }
    }

    fn shared_entity_suite() -> ScaleSuite {
        let mut fixture = suite();
        fixture.sources.truncate(3);
        for (index, source) in fixture.sources.iter_mut().enumerate() {
            let (value, period, unit) = match index {
                0 => (9, "2026-Q1", "million"),
                1 => (14, "2026-Q2", "million"),
                _ => (23, "2026-Q2", "thousand"),
            };
            let text = format!("Fictional Company reported {value} USD {unit} in {period}.");
            source.source.text = text.clone();
            source.canonical_import.node_id = "shared-company".into();
            source.canonical_import.name = "Fictional Company".into();
            source.canonical_import.entity_identifier = "FICT-SHARED".into();
            source.canonical_import.aliases = vec!["Copper Harbor".into()];
            source.canonical_import.value = json!(value);
            source.canonical_import.period = period.into();
            source.canonical_import.unit = unit.into();
            source.canonical_import.proposition = text.clone();
            source.canonical_import.quote = text.clone();
            source.canonical_import.utf8_end_exclusive = text.len();
        }
        fixture.cases[0].distractor_count = 0;
        fixture.cases[0].source_ids = fixture
            .sources
            .iter()
            .map(|source| source.source.id.clone())
            .collect();
        fixture.cases[0].focal_source_ids = fixture.cases[0].source_ids.clone();
        fixture
    }

    #[test]
    fn scale_validation_rejects_inconsistent_fixtures_before_model_work() {
        let existing: ScaleSuite =
            serde_json::from_str(include_str!("../../../../evals/world/scale-corpus-v1.json"))
                .unwrap();
        validate(&existing, &Options::default()).unwrap();
        validate(&shared_entity_suite(), &Options::default()).unwrap();
        for field in ["name", "entity_identifier", "aliases"] {
            let mut fixture = shared_entity_suite();
            let other = &mut fixture.sources[1].canonical_import;
            match field {
                "name" => other.name = "Different Company".into(),
                "entity_identifier" => other.entity_identifier = "FICT-OTHER".into(),
                _ => other.aliases.push("Different Alias".into()),
            }
            assert!(validate(&fixture, &Options::default()).is_err(), "{field}");
        }
        let mut fixture = suite();
        fixture.sources[1].canonical_import.node_id =
            format!("{}-value", fixture.sources[0].canonical_import.claim_id);
        assert!(validate(&fixture, &Options::default()).is_err());
        let mut fixture = suite();
        fixture.sources[0].canonical_import.utf8_end_exclusive -= 1;
        assert!(validate(&fixture, &Options::default()).is_err());
    }

    #[tokio::test]
    async fn scale_shared_entity_claims_keep_distinct_values_periods_units_and_sources() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_eval(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let fixture = shared_entity_suite();
        validate(&fixture, &Options::default()).unwrap();
        let config = GraphProcessingSettings::default();
        save_settings(&state, &loupist.id, &config).unwrap();
        let (world, evidence) =
            canonical_import(&state, &loupist.id, &config, &fixture, &fixture.cases[0]).unwrap();
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
        assert!(store.graph_jobs(&loupist.id).unwrap().is_empty());
        let snapshot = store
            .record_snapshot(&loupist.id, &world.revision_id)
            .unwrap();
        let body: WorldSnapshotBody = serde_json::from_value(snapshot.payload).unwrap();
        assert_eq!(body.nodes.len(), 4);
        assert_eq!(body.claims.len(), 3);
        let mut targets = BTreeSet::new();
        let mut spans = BTreeSet::new();
        for (index, source) in fixture.sources.iter().enumerate() {
            let prepared = &source.canonical_import;
            let claim = body
                .claims
                .iter()
                .find(|claim| claim.id == prepared.claim_id)
                .unwrap();
            assert_eq!(claim.endpoints[0].node_id, "shared-company");
            assert_eq!(claim.endpoints[0].role, "source");
            assert_eq!(claim.endpoints[1].role, "target");
            assert!(targets.insert(claim.endpoints[1].node_id.clone()));
            assert!(spans.insert(claim.evidence[0].span_id.clone()));
            for (key, expected) in [
                ("value", prepared.value.to_string()),
                ("period", prepared.period.clone()),
                ("unit", prepared.unit.clone()),
            ] {
                assert_eq!(
                    claim
                        .qualifiers
                        .iter()
                        .find(|item| item.key == key)
                        .unwrap()
                        .value,
                    expected
                );
            }
            assert_eq!(
                claim.evidence[0].span_id,
                evidence["sources"][index]["evidence_span_id"]
                    .as_str()
                    .unwrap()
            );
            assert_eq!(
                evidence["sources"][index]["retained"]["canonical_text"],
                source.source.text
            );
        }
        state.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn scale_canonical_import_preserves_bytes_dates_isolation_and_browse_boundary() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_eval(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let fixture = suite();
        validate(&fixture, &Options::default()).unwrap();
        let config = GraphProcessingSettings::default();
        save_settings(&state, &loupist.id, &config).unwrap();
        let (world, evidence) =
            canonical_import(&state, &loupist.id, &config, &fixture, &fixture.cases[0]).unwrap();
        assert_eq!(
            evidence["sources"][20]["retained"]["canonical_text"],
            fixture.sources[20].source.text
        );
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
        assert!(store.graph_jobs(&loupist.id).unwrap().is_empty());
        let snapshot = store
            .record_snapshot(&loupist.id, &world.revision_id)
            .unwrap();
        assert_eq!(
            snapshot.payload["claims"][20]["publicationTime"]["value"],
            "2026-08-05"
        );
        let page = browse_page(&state, &loupist.id, &world, 20).unwrap();
        assert_eq!(page["edges"].as_array().unwrap().len(), 20);
        assert!(!page["edges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edge| edge["id"] == "c-20"));
        assert!(page["nextCursor"].is_string());
        let exact: WorldGraphQuery = serde_json::from_value(json!({
            "loupistId":loupist.id,"recordId":world.record_id,"revisionId":world.revision_id,
            "centerClaimId":"c-20","nodeLimit":2,"edgeLimit":1
        }))
        .unwrap();
        let exact = store.query_world(exact).unwrap();
        assert_eq!(exact.edges.len(), 1);
        assert_eq!(exact.edges[0].id, "c-20");
        assert_eq!(
            exact.edges[0].claim.evidence[0].span_id,
            evidence["sources"][20]["evidence_span_id"]
                .as_str()
                .unwrap()
        );
        let other_workspace = temp.path().join("other");
        fs::create_dir(&other_workspace).unwrap();
        let other = crate::commands::loupist::create_folder_loupist(&state, &other_workspace)
            .await
            .unwrap();
        assert!(KnowledgeStore::for_loupist(state.store.clone(), &other.id)
            .unwrap()
            .record_snapshot(&other.id, &world.revision_id)
            .is_err());
        state.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn scale_paid_extraction_audit_allows_local_embedding_callbacks() {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_eval(temp.path().join("app"));
        let workspace = temp.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let fixture = suite();
        let config = GraphProcessingSettings::default();
        save_settings(&state, &loupist.id, &config).unwrap();
        let (world, _) =
            canonical_import(&state, &loupist.id, &config, &fixture, &fixture.cases[0]).unwrap();
        let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
        let accepted = store
            .graph_accept_sealed_world(
                &config,
                crate::knowledge::SealedWorldIndex {
                    loupist_id: loupist.id.clone(),
                    world_record_id: world.record_id,
                    world_revision_id: world.revision_id,
                    at_ms: 1_800_000_000_000,
                },
            )
            .unwrap();
        let running = store
            .graph_mark_running(&config, &accepted.job_id, 1_800_000_000_001)
            .unwrap();
        store
            .graph_note_inference(
                &running.job_id,
                running.document.attempt_id.as_deref().unwrap(),
            )
            .unwrap();
        let inference =
            crate::providers::inference::ProviderInferenceService::from_state(&state).unwrap();
        assert!(inference.eval_observations().is_empty());
        assert_eq!(running.document.extraction.max_llm_calls, 0);
        assert_no_paid_extraction(&state, &loupist.id, &inference)
            .expect("local embedding inference is not paid World extraction");
        state.shutdown().await.unwrap();
    }
}
