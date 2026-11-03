//! Opt-in live check that followed assets reach a Loupist's Map through "What
//! you follow" Sources and ordinary World jobs, at creation and when followed later.
use super::*;

/// A mixed list: a US stock, a Korean stock under its Korean name, an ETF, a
/// crypto asset, a commodity and a manual entry without a name or market.
const WATCHING: [(&str, &str, Option<&str>); 6] = [
    ("AAPL", "NASDAQ", Some("Apple")),
    ("005930", "KRX", Some("삼성전자")),
    ("SPY", "NYSEARCA", Some("SPDR S&P 500 ETF Trust")),
    ("BTC", "CRYPTO", Some("Bitcoin")),
    ("GC", "COMEX", Some("Gold")),
    ("NVDA", "UNKNOWN", None),
];
/// Held, under its Korean name.
const HOLDINGS_CSV: &str =
    "symbol,market,name,quantity,cost,value,currency\n000660,KRX,SK하이닉스,10,150000,1800000,KRW\n";
/// Followed after creation: a stock, a crypto asset and two fictional manual
/// entries like the native journey's.
const FOLLOWED_LATER: [(&str, &str, Option<&str>); 4] = [
    ("TSLA", "NASDAQ", Some("Tesla")),
    ("ETH", "CRYPTO", Some("Ethereum")),
    ("QUAYX", "UNKNOWN", None),
    ("LOOMX", "UNKNOWN", None),
];

async fn drain(state: &AppState, loupist: &str) -> Result<Value> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist)?;
    let result = tokio::time::timeout(Duration::from_secs(900), async {
        loop {
            let jobs = store.graph_jobs(loupist)?;
            if !jobs
                .iter()
                .any(|job| matches!(job.status.as_str(), "queued" | "running"))
            {
                let statuses: Vec<_> = jobs.iter().map(|job| job.status.clone()).collect();
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    json!({ "jobs": statuses }),
                );
            }
            match state.graph.pump_once().await {
                Ok(Some(_)) => (),
                Ok(None) => tokio::time::sleep(Duration::from_millis(100)).await,
                Err(error) => return Ok(json!({ "error": error.to_string() })),
            }
        }
    })
    .await;
    result.unwrap_or_else(|_| Ok(json!({ "error": "timeout" })))
}

/// Each followed asset with the Map node it matched (the Map's own matching,
/// by symbol and then name) and whether an `entity_added` event names it.
fn landed(state: &AppState, loupist: &str) -> Result<Value> {
    let map = crate::commands::knowledge::map(
        state,
        KnowledgeMapQuery {
            loupist_id: loupist.into(),
            max_nodes: Some(1000),
        },
    )?;
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist)?;
    let events = store
        .growth_events(&GrowthEventsQuery {
            loupist_id: loupist.into(),
            since_ms: None,
            until_ms: None,
            limit: Some(500),
            cursor: None,
        })?
        .events;
    let context = state.store.map_context()?;
    let snapshot: Option<String> = {
        let connection = state.store.lock_loupist(loupist)?;
        let mut statement = connection.prepare(
            "SELECT snapshot.snapshot_json FROM knowledge_records record
             JOIN knowledge_record_heads head ON head.record_id = record.record_id
             JOIN knowledge_world_snapshots snapshot ON snapshot.revision_id = head.revision_id
             WHERE record.owner_loupist_id = ?1 AND record.kind = 'world' LIMIT 1",
        )?;
        let mut rows = statement.query_map([loupist], |row| row.get(0))?;
        rows.next().transpose()?
    };
    let body: Option<WorldSnapshotBody> = snapshot
        .map(|json| serde_json::from_str(&json))
        .transpose()?;
    let world = body
        .as_ref()
        .map(|body| body.nodes.clone())
        .unwrap_or_default();
    let name = |id: &str| {
        world
            .iter()
            .find(|node| node.id == id)
            .map_or_else(|| id.to_owned(), |node| node.name.clone())
    };
    let claims: Vec<Value> = body
        .as_ref()
        .map(|body| body.claims.as_slice())
        .unwrap_or_default()
        .iter()
        .map(|claim| {
            json!({
                "relation": claim.relation,
                "endpoints": claim.endpoints.iter().map(|end| name(&end.node_id)).collect::<Vec<_>>(),
            })
        })
        .collect();
    let mut assets = Vec::new();
    for item in context.watchlist.iter().chain(&context.holdings) {
        let (matched, _) = crate::knowledge::match_items(std::slice::from_ref(item), &world);
        let nodes: Vec<Value> = matched
            .iter()
            .filter_map(|id| map.nodes.iter().find(|node| &node.id == id))
            .map(|node| {
                json!({
                    "label": node.label,
                    "entityKind": node.entity_kind,
                    "localNames": node.local_names,
                    "entityAdded": events.iter().any(|event| event.kind == "entity_added" && event.subject_id.as_deref() == Some(node.id.as_str())),
                })
            })
            .collect();
        assets.push(json!({
            "symbol": item.symbol,
            "market": item.market,
            "name": item.display_name,
            "nodes": nodes,
        }));
    }
    let entities: Vec<Value> = map
        .nodes
        .iter()
        .filter(|node| node.kind == "entity")
        .map(|node| json!({ "label": node.label, "entityKind": node.entity_kind, "watched": node.watched, "held": node.held }))
        .collect();
    Ok(json!({
        "assets": assets,
        "unmatchedWatchlist": map.unmatched_watchlist.iter().map(|item| &item.symbol).collect::<Vec<_>>(),
        "unmatchedHoldings": map.unmatched_holdings.iter().map(|item| &item.symbol).collect::<Vec<_>>(),
        "entities": entities,
        "claims": claims,
        "entityAdded": events.iter().filter(|event| event.kind == "entity_added").count(),
        "followedSources": store.list_records(loupist)?.iter().filter(|record| record.kind == RecordKind::Source).count(),
    }))
}

#[tokio::test]
#[ignore = "Paid live World run; explicit Pi directory, resources and output required"]
async fn live_followed_assets_reach_the_map() -> Result<()> {
    let out = PathBuf::from(std::env::var("LOUPIST_FOLLOWED_OUT")?);
    let receipts = out.with_extension("receipts.json");
    if out.exists() || receipts.exists() {
        return Err("Refusing to overwrite followed-assets evidence".into());
    }
    let pi = PathBuf::from(std::env::var("LOUPIST_FOLLOWED_PI_AGENT_DIR")?);
    if !pi.is_absolute() || !pi.is_dir() {
        return Err("Pi directory must exist and be absolute".into());
    }
    let resources = PathBuf::from(std::env::var("LOUPIST_FOLLOWED_RESOURCES")?);
    let profile = std::env::var("LOUPIST_FOLLOWED_MODEL")
        .unwrap_or_else(|_| "openai-codex/gpt-6-luna".into());
    let thinking = std::env::var("LOUPIST_FOLLOWED_THINKING").unwrap_or_else(|_| "high".into());
    let (provider, model_id) = profile.split_once('/').ok_or("model is provider/model")?;
    let temp = tempfile::Builder::new()
        .prefix("loupist-followed-")
        .tempdir_in("/tmp")?;
    let mut state = AppState::for_eval(temp.path().join("app"));
    fs::write(
        state.artifacts.app_data_dir.join("config.toml"),
        "[appearance]\nlanguage = 'en'\n[pi.retry]\nenabled = false\nmax_retries = 0\n",
    )?;
    state.eval_pi_agent_dir = Some(pi);
    let frozen = artifacts::FrozenArtifacts::new(artifacts()?)?;
    Arc::make_mut(&mut state.artifacts).pi = Some(frozen.pi.clone());
    let model = crate::providers::connection::eval_models(&state, provider)
        .await?
        .into_iter()
        .find(|model| model.model == model_id)
        .ok_or("World model unavailable")?;
    model.validate_thinking_level(&thinking)?;
    state.set_model_catalog(crate::providers::catalog::ModelCatalog {
        models: vec![model.clone()],
    })?;
    let inference = crate::providers::inference::ProviderInferenceService::from_state(&state)?;
    state.graph.shutdown().await?;
    state.graph = GraphSupervisor::with_inference(
        state.store.clone(),
        state.artifacts.app_data_dir.clone(),
        Some(resources),
        state.maintenance.clone(),
        Some(inference.clone()),
    );
    if !state.graph.runtime_available() {
        return Err("Graph runtime unavailable".into());
    }
    for (symbol, market, name) in WATCHING {
        state.store.watchlist_add(symbol, market, name)?;
    }
    let preview = state.store.preview_csv_bytes(HOLDINGS_CSV.as_bytes())?;
    state.store.holdings_commit_csv(&preview.preview_id)?;
    let loupist = crate::commands::loupist::create_managed_loupist(
        &state,
        crate::commands::LoupistCreateRequest {
            name: "Followed".into(),
            focus: None,
            avatar: None,
            template: None,
            tone: None,
        },
    )
    .await?
    .id;
    // The eval catalog has no World default, so the Source waits for a model like
    // a new Loupist created before connecting; choosing one backfills it.
    let config = {
        let mut conn = state.store.lock_loupist(&loupist)?;
        let tx = conn.transaction()?;
        let mut config = load_settings(&tx, &loupist)?;
        config.enrichment_enabled = true;
        config.awaiting_default_model = false;
        config.helper_provider = Some(provider.into());
        config.helper_model = Some(model_id.into());
        config.helper_model_profile_id = Some(profile.clone());
        config.thinking_level = thinking.clone();
        config.timeout_ms = 120_000;
        config.run_options = model
            .run_controls
            .iter()
            .map(|control| (control.id.clone(), control.default_choice.clone()))
            .collect();
        store_settings(&tx, &loupist, &config)?;
        tx.commit()?;
        config
    };
    KnowledgeStore::for_loupist(state.store.clone(), &loupist)?.transact(|write| {
        write.enqueue_existing_sources(&loupist, &config, crate::commands::now_ms())
    })?;
    let execution = async {
        let mut bytes = vec![];
        let mut phase = |name: &str, processing: Value, elapsed: Duration| -> Result<Value> {
            let observations = inference.eval_observations();
            let provider_failed = observations.iter().any(|call| {
                matches!(call["disposition"].as_str(), Some("not_sent" | "uncertain"))
                    || call["diagnostic"]["helper"]["http_status"]
                        .as_u64()
                        .is_some_and(|status| status >= 400)
            });
            let tokens = |field: &str| {
                observations
                    .iter()
                    .filter_map(|row| row["usage"][field].as_u64())
                    .sum::<u64>()
            };
            let record = json!({
                "schema": "loupist-followed-assets/1",
                "phase": name,
                "model": profile,
                "thinking": thinking,
                "processing": processing,
                "processingMs": elapsed.as_millis() as u64,
                "map": landed(&state, &loupist)?,
                "helperInference": observations.len(),
                "helperTokens": {"input": tokens("input"), "output": tokens("output"), "cacheRead": tokens("cacheRead")},
                "worldInference": observations,
            });
            append_atomic(&out, &mut bytes, &record)?;
            eprintln!(
                "followed {name}: {}",
                serde_json::to_string_pretty(&record["map"]["assets"])?
            );
            if provider_failed {
                return Err("World provider call failed; stopping the paid followed-assets check".into());
            }
            Ok(record)
        };
        let started = Instant::now();
        let processing = drain(&state, &loupist).await?;
        phase("created", processing, started.elapsed())?;
        // Following later goes through the production follow path; the burst
        // settles into one more "What you follow" Source and one World job.
        for (symbol, market, name) in FOLLOWED_LATER {
            crate::commands::user_context::follow(&state, symbol, market, name, Some(&loupist))?;
        }
        tokio::time::sleep(Duration::from_secs(6)).await;
        let started = Instant::now();
        let processing = drain(&state, &loupist).await?;
        phase("followed_later", processing, started.elapsed())?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let shutdown = state.shutdown().await;
    // A final ledger includes calls still settling when the last phase was read.
    fs::write(
        receipts,
        serde_json::to_vec_pretty(&inference.eval_observations())?,
    )?;
    shutdown?;
    execution
}
