//! Opt-in five-session product-growth proof. Never seeds a learned method or World.
use super::*;
const MODEL: &str = "openai-codex/gpt-6.1-sol";
#[derive(Deserialize)]
struct Fixture {
    schema: String,
    loupist: String,
    model: String,
    thinking: String,
    watchlist: Vec<WatchSeed>,
    sources: Vec<DatedSource>,
    sessions: Vec<Session>,
}
#[derive(Deserialize)]
struct DatedSource {
    id: String,
    title: String,
    publication_time: String,
    origin: String,
    text: String,
}
fn retain(
    state: &AppState,
    loupist: &str,
    config: &GraphProcessingSettings,
    sources: &[DatedSource],
) -> Result<()> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist)?;
    for source in sources {
        store.transact(|w| {
            let now = crate::commands::now_ms();
            let retained = w.retain_new_source(RetainNewSource {
                origin: None,
                loupist_id: loupist.into(),
                at_ms: now,
                title: source.title.clone(),
                source_class: SourceClass::PublisherDocument,
                origin_locator: source.origin.clone(),
                provider_identity: "fictional-growth-eval".into(),
                authorship: Authorship::Publisher,
                coverage: Coverage::IncludedFull,
                origin_chat_id: None,
                request_ref: None,
                declared_digest: None,
                publication_time: Some(TimePoint {
                    precision: TimePrecision::Date,
                    value: Some(source.publication_time.clone()),
                    approximate: false,
                }),
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
                    context_note: "Fictional growth scenario; no market observations".into(),
                }),
            })?;
            w.enqueue_source_world(loupist, &retained, config, now)?;
            Ok(())
        })?;
    }
    Ok(())
}
async fn drain(state: &AppState, loupist: &str) -> Result<Value> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist)?;
    let result = tokio::time::timeout(Duration::from_secs(900), async {
        loop {
            let jobs = store.graph_jobs(loupist)?;
            let (active_reviews, deferred_reviews): (i64, i64) = state.store.lock_loupist(loupist)?.query_row(
                "SELECT COALESCE(SUM(status IN ('queued','running')),0), COALESCE(SUM(status='pending'),0) FROM knowledge_learning_reviews WHERE loupist_id=?1",
                [loupist], |r| Ok((r.get(0)?, r.get(1)?)))?;
            // A `pending` review waits for its cooldown or the next idle queue; waiting for it here
            // would exceed the timeout, so report it instead of treating the Loupist as fully drained.
            if !jobs.iter().any(|j| matches!(j.status.as_str(), "queued" | "running")) && active_reviews == 0 {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(json!({"status":if jobs.iter().all(|j|j.status=="completed") {"completed"} else {"failed"},"jobs":jobs,"learningReviewsPending":deferred_reviews}));
            }
            match state.graph.pump_once().await {
                Ok(Some(_)) => (),
                Ok(None) => tokio::time::sleep(Duration::from_millis(100)).await,
                Err(error) => return Ok(json!({"status":"failed","error":error.to_string(),"jobs":store.graph_jobs(loupist)?})),
            }
        }
    }).await;
    match result {
        Ok(value) => value,
        Err(_) => Ok(
            json!({"status":"failed","error":"growth_processing_timeout","jobs":store.graph_jobs(loupist)?}),
        ),
    }
}
fn snapshot(state: &AppState, loupist: &str) -> Result<Value> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist)?;
    let map = crate::commands::knowledge::map(
        state,
        KnowledgeMapQuery {
            loupist_id: loupist.into(),
            max_nodes: Some(1000),
        },
    )?;
    let mut nodes = BTreeMap::<String, usize>::new();
    let mut edges = BTreeMap::<String, usize>::new();
    for n in &map.nodes {
        *nodes.entry(n.kind.clone()).or_default() += 1;
    }
    for e in &map.edges {
        *edges.entry(e.kind.clone()).or_default() += 1;
    }
    let mut growth = vec![];
    let mut cursor = None;
    loop {
        let page = store.growth_events(&GrowthEventsQuery {
            loupist_id: loupist.into(),
            since_ms: None,
            until_ms: None,
            limit: Some(200),
            cursor,
        })?;
        growth.extend(page.events);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let mut conn = state.store.lock_loupist(loupist)?;
    let tx = conn.transaction()?;
    let reviews = learning::list(
        &tx,
        &learning::ReviewQuery {
            loupist_id: loupist.into(),
            limit: Some(200),
        },
    )?;
    let principles = principles::principle_list(&tx, loupist, true)?;
    let journals = journal::list(&tx, loupist)?;
    tx.commit()?;
    drop(conn);
    let records = store.list_records(loupist)?;
    let mut methods = vec![];
    for record in &records {
        if record.kind == RecordKind::Lens
            && !principles.iter().any(|p| p.record_id == record.record_id)
        {
            let page = store.read(|read| read.read_page(loupist, &record.record_id))?;
            methods.push(json!({"record":record,"page":page}));
        }
    }
    let inbox = state
        .store
        .inbox_runs(loupist, None, 100)?
        .into_iter()
        .map(crate::briefings::commands::InboxItem::from)
        .collect::<Vec<_>>();
    Ok(
        json!({"knowledgeMap":map,"nodes_by_kind":nodes,"edges_by_kind":edges,"growth_events":growth,"learning_reviews":reviews,"principles":principles,"methods":methods,"journal_count":journals.len(),"journals":journals,"inbox":inbox}),
    )
}
async fn briefing(state: &AppState, loupist: &str) -> Result<Value> {
    let task = crate::briefings::commands::settings_set(
        state,
        crate::briefings::commands::SettingsRequest {
            loupist_id: loupist.into(),
            enabled: true,
            schedule: None,
        },
    )?
    .task
    .ok_or("missing briefing task")?;
    if task.model_profile_id != MODEL || task.thinking_level != "medium" {
        return Err("briefing model mismatch".into());
    }
    let captured = Arc::new(Mutex::new(trace::PiTrace::default()));
    let observer = captured.clone();
    state.scheduled.observe_eval(Arc::new(move |event| {
        observer.lock().unwrap().observe(event)
    }));
    let run = state
        .scheduled
        .dispatch(state, loupist, &task.id, true)
        .await?
        .ok_or("briefing not admitted")?;
    let receipt = tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            let run = state
                .store
                .scheduled_task_run_get(loupist, &run.run_id)?
                .ok_or("briefing missing")?;
            if run.status != crate::scheduled_tasks::types::RunStatus::Running {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(run);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await??;
    let chat = state
        .store
        .get_chat(
            receipt
                .thread_id
                .as_deref()
                .ok_or("briefing chat missing")?,
        )?
        .ok_or("briefing chat missing")?;
    let message = chat
        .messages
        .iter()
        .rev()
        .find(|m| m.role == crate::domain::ChatRole::Assistant)
        .ok_or("briefing answer missing")?;
    let mut result = trace::persisted(message, &captured.lock().unwrap());
    trace::enrich_commits(state, &chat, message, &mut result)?;
    Ok(result)
}

#[tokio::test]
#[ignore = "Paid live five-session growth scenario; explicit Pi directory and output required"]
async fn live_growth_scenario_v1() -> Result<()> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest.join("../evals/world/growth-scenario-v1/scenario.json");
    let fixture_bytes = fs::read(&fixture_path)?;
    let fixture: Fixture = serde_json::from_slice(&fixture_bytes)?;
    assert_eq!(fixture.schema, "loupist-growth-scenario/1");
    assert_eq!(fixture.model, MODEL);
    assert_eq!(fixture.thinking, "medium");
    assert_eq!(fixture.sessions.len(), 5);
    assert_eq!(fixture.sources.len(), 5);
    let out = PathBuf::from(std::env::var("LOUPIST_GROWTH_OUT")?);
    if out.exists() {
        return Err("Refusing to overwrite growth evidence".into());
    }
    let pi = PathBuf::from(std::env::var("LOUPIST_GROWTH_PI_AGENT_DIR")?);
    if !pi.is_absolute() || !pi.is_dir() {
        return Err("Pi directory must exist and be absolute".into());
    }
    let temp = tempfile::Builder::new()
        .prefix("loupist-growth-")
        .tempdir_in("/tmp")?;
    let workspace = temp.path().join("Semiconductors");
    fs::create_dir(&workspace)?;
    let mut state = AppState::for_eval(temp.path().join("app"));
    fs::write(
        state.artifacts.app_data_dir.join("config.toml"),
        "[appearance]\nlanguage = 'en'\n[pi.retry]\nenabled = false\nmax_retries = 0\n",
    )?;
    state.eval_pi_agent_dir = Some(pi);
    let frozen = artifacts::FrozenArtifacts::new(artifacts()?)?;
    Arc::make_mut(&mut state.artifacts).pi = Some(frozen.pi.clone());
    let model = crate::providers::connection::eval_models(&state, "openai-codex")
        .await?
        .into_iter()
        .find(|m| m.id == MODEL)
        .ok_or("sol unavailable")?;
    model.validate_thinking_level("medium")?;
    state.set_model_catalog(crate::providers::catalog::ModelCatalog {
        models: vec![model.clone()],
    })?;
    let inference = crate::providers::inference::ProviderInferenceService::from_state(&state)?;
    state.graph.shutdown().await?;
    state.graph = GraphSupervisor::with_inference(
        state.store.clone(),
        state.artifacts.app_data_dir.clone(),
        Some(manifest.join("resources")),
        state.maintenance.clone(),
        Some(inference.clone()),
    );
    if !state.graph.runtime_available() {
        return Err("Graph runtime unavailable".into());
    }
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace).await?;
    state.store.update_loupist_profile(&loupist.id, crate::store::LoupistProfileEdit { name: Some(&fixture.loupist), focus: Some("Fictional semiconductor capacity ramps, margins, inventory and cash conversion"), instructions: Some("Research only the retained fictional Sources; no external data. Apply saved principles and relevant learned methods. Provide information, not investment advice."), ..Default::default() }, crate::commands::now_ms())?;
    enable_connections(&state, &loupist.id, &[])?;
    for watch in &fixture.watchlist {
        state
            .store
            .watchlist_add(&watch.symbol, &watch.market, watch.display_name.as_deref())?;
    }
    let config = GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some("openai-codex".into()),
        helper_model: Some("gpt-6.1-sol".into()),
        helper_model_profile_id: Some(MODEL.into()),
        thinking_level: "medium".into(),
        timeout_ms: 120_000,
        run_options: model
            .run_controls
            .iter()
            .map(|c| (c.id.clone(), c.default_choice.clone()))
            .collect(),
        ..Default::default()
    };
    {
        let mut conn = state.store.lock_loupist(&loupist.id)?;
        let tx = conn.transaction()?;
        store_settings(&tx, &loupist.id, &config)?;
        learning::save_settings(&tx, &loupist.id, false)?;
        tx.commit()?;
    }
    let mut bytes = vec![];
    let mut usage_start = 0;
    let execution = async {
        for (index, session) in fixture.sessions.iter().enumerate() {
            eprintln!("growth: starting {}",session.id);
            let mut processing = Value::Null;
            if index == 0 { retain(&state,&loupist.id,&config,&fixture.sources[..2])?; processing = drain(&state,&loupist.id).await?; }
            if index == 2 {
                let mut conn = state.store.lock_loupist(&loupist.id)?; let tx = conn.transaction()?;
                learning::save_settings(&tx,&loupist.id,true)?; tx.commit()?; drop(conn);
                retain(&state,&loupist.id,&config,&fixture.sources[2..])?;
                processing = drain(&state,&loupist.id).await?;
            }
            let mut turns = vec![];
            if index == 3 { turns.push(briefing(&state,&loupist.id).await?); }
            else if !session.turns.is_empty() {
                let chat = new_chat(&state,&loupist.id,&session.id)?;
                for turn in &session.turns { turns.push(run_turn(&state,&chat,session,turn,MODEL,"medium",600_000).await?); }
            }
            let settled = drain(&state,&loupist.id).await?;
            let ledger = inference.eval_observations();
            let snapshot = snapshot(&state,&loupist.id)?;
            let record = json!({"schema":"loupist-growth-session/1","session":session.id,"model":MODEL,"thinking":"medium","scenario_sha256":crate::util::sha256(&fixture_bytes),"agent_snapshot_sha256":frozen.sha256,"snapshot":snapshot,"processing":processing,"post_processing":settled,"turns":turns,"helper_inference":&ledger[usage_start..],"final_answer":turns.last().and_then(|t|t.get("answer")).cloned().unwrap_or(Value::Null)});
            usage_start=ledger.len();
            append_atomic(&out,&mut bytes,&record)?;
            eprintln!("growth: saved {} nodes={} reviews={}",session.id,record["snapshot"]["knowledgeMap"]["nodes"].as_array().map_or(0,Vec::len),record["snapshot"]["learning_reviews"].as_array().map_or(0,Vec::len));
            if turns.iter().any(|t|t["status"]!="completed") { return Err("Chat/briefing failed; snapshot preserved".into()); }
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    state.shutdown().await?;
    execution
}
