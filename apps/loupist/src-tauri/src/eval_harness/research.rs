use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    schema: String,
    training: Scenario,
    probes: Vec<Scenario>,
}

fn disposable_state(
    root: &Path,
    pi_dir: &Path,
    frozen: &artifacts::FrozenArtifacts,
) -> Result<AppState> {
    let mut state = AppState::for_eval(root.join("app"));
    fs::write(
        state.artifacts.app_data_dir.join("config.toml"),
        frozen.config_toml(),
    )?;
    state.eval_pi_agent_dir = Some(pi_dir.to_path_buf());
    Arc::make_mut(&mut state.artifacts).pi = Some(frozen.pi.clone());
    Ok(state)
}

fn learning_snapshot(state: &AppState, loupist: &str) -> Result<Value> {
    let mut connection = state.store.lock_loupist(loupist)?;
    let tx = connection.transaction()?;
    let counts: (i64, i64, i64) = tx.query_row(
        "SELECT (SELECT COUNT(*) FROM knowledge_research_experiences WHERE loupist_id=?1),
                (SELECT COUNT(*) FROM knowledge_growth_events WHERE loupist_id=?1 AND kind IN ('claim_added','claim_disputed')),
                (SELECT COUNT(*) FROM knowledge_journals WHERE loupist_id=?1)",
        [loupist], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    let reviews = learning::list(
        &tx,
        &learning::ReviewQuery {
            loupist_id: loupist.into(),
            limit: Some(200),
        },
    )?;
    Ok(
        json!({"experiences":counts.0,"worldClaimEvents":counts.1,"journals":counts.2,"reviews":reviews}),
    )
}

fn learned_methods(state: &AppState, loupist: &str) -> Result<Vec<LensSeed>> {
    let store = KnowledgeStore::for_loupist(state.store.clone(), loupist)?;
    let ids = {
        let connection = state.store.lock_loupist(loupist)?;
        let mut query = connection.prepare(
            "SELECT record_id FROM knowledge_lens_metadata WHERE loupist_id=?1 AND learned=1 AND lens_kind='method' AND archived_at_ms IS NULL ORDER BY record_id",
        )?;
        let ids = query
            .query_map([loupist], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids
    };
    let records = store.list_records(loupist)?;
    ids.iter()
        .map(|id| {
            Ok(LensSeed {
                title: records
                    .iter()
                    .find(|record| &record.record_id == id)
                    .ok_or("learned method record missing")?
                    .title
                    .clone(),
                document: store.read_lens(loupist, id)?.document,
            })
        })
        .collect()
}

async fn observe_session(
    state: &AppState,
    loupist: &str,
    session: &Session,
    model: &str,
    thinking: &str,
) -> Result<Vec<Value>> {
    let chat = new_chat(state, loupist, &session.id)?;
    let mut turns = Vec::new();
    for turn in &session.turns {
        let mut trace = run_turn(state, &chat, session, turn, model, thinking, 180_000).await?;
        Budget {
            max_input_tokens_per_turn: 150_000,
            max_turns: 8,
        }
        .enforce(&mut trace);
        turns.push(trace);
        if turns
            .last()
            .is_some_and(|trace| trace["status"] != "completed")
        {
            break;
        }
    }
    Ok(turns)
}

#[tokio::test]
#[ignore = "Paid production research and learning comparison; requires explicit model, Pi directory and output"]
async fn live_autonomous_research_v1() -> Result<()> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let bytes = fs::read(manifest.join("../evals/lens/autonomous-research-v1.json"))?;
    let fixture: Fixture = serde_json::from_slice(&bytes)?;
    if fixture.schema != "loupist-autonomous-research/1"
        || fixture.training.sessions.len() != 2
        || fixture
            .training
            .sessions
            .iter()
            .any(|session| session.turns.len() != 1 || session.memory != "remember")
        || !fixture.training.seed.lens.is_empty()
        || !fixture.training.seed.principles.is_empty()
        || fixture.probes.len() != 2
        || fixture.probes.iter().any(|probe| {
            probe.sessions.len() != 1
                || probe.sessions[0].turns.len() != 1
                || probe.sessions[0].memory != "read_only"
        })
    {
        return Err(
            "research fixture must have two ordinary investigations and two read-only probes"
                .into(),
        );
    }
    let out = PathBuf::from(std::env::var("LOUPIST_RESEARCH_OUT")?);
    if out.exists() {
        return Err("refusing to overwrite a research evaluation attempt".into());
    }
    let pi_dir = PathBuf::from(std::env::var("LOUPIST_RESEARCH_PI_AGENT_DIR")?);
    if !pi_dir.is_absolute() || !pi_dir.is_dir() {
        return Err("Pi directory must exist and be absolute".into());
    }
    let model_id = std::env::var("LOUPIST_RESEARCH_MODEL")?;
    let thinking = std::env::var("LOUPIST_RESEARCH_THINKING").unwrap_or_else(|_| "medium".into());
    let provider = model_id
        .split_once('/')
        .ok_or("model must be provider/model")?
        .0;
    let frozen = artifacts::FrozenArtifacts::new(artifacts()?)?;
    let root = tempfile::Builder::new()
        .prefix("loupist-research-")
        .tempdir_in("/tmp")?;
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace)?;
    let mut state = disposable_state(root.path(), &pi_dir, &frozen)?;
    let model = crate::providers::connection::eval_models(&state, provider)
        .await?
        .into_iter()
        .find(|model| model.id == model_id)
        .ok_or("requested research model unavailable")?;
    model.validate_thinking_level(&thinking)?;
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
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace).await?;
    seed(&state, &loupist.id, &fixture.training.seed)?;
    enable_connections(&state, &loupist.id, &[])?;
    let config = GraphProcessingSettings {
        enrichment_enabled: true,
        helper_provider: Some(model.provider.clone()),
        helper_model: Some(model.model.clone()),
        helper_model_profile_id: Some(model_id.clone()),
        thinking_level: thinking.clone(),
        timeout_ms: 120_000,
        max_llm_calls: 1,
        run_options: model
            .run_controls
            .iter()
            .map(|control| (control.id.clone(), control.default_choice.clone()))
            .collect(),
        ..Default::default()
    };
    {
        let connection = state.store.lock_loupist(&loupist.id)?;
        store_settings(&connection, &loupist.id, &config)?;
    }
    let mut output = Vec::new();
    let stamp = json!({"schema":"loupist-autonomous-research-run/1","fixtureSha256":crate::util::sha256(&bytes),"agentSnapshotSha256":frozen.sha256,"model":model_id,"thinking":thinking});
    let training = async {
        for session in &fixture.training.sessions {
            eprintln!("research: {}", session.id);
            let turns = observe_session(&state, &loupist.id, session, &model_id, &thinking).await?;
            let settled = tokio::time::timeout(Duration::from_secs(180), growth::drain(&state, &loupist.id)).await??;
            let snapshot = learning_snapshot(&state, &loupist.id)?;
            append_atomic(&out, &mut output, &json!({"harness":stamp,"phase":"train","session":session.id,"turns":turns,"settled":settled,"snapshot":snapshot,"helperInference":inference.eval_observations()}))?;
            if turns.iter().any(|turn| turn["status"] != "completed") {
                return Err("research training failed; attempt was preserved".into());
            }
        }
        let snapshot = learning_snapshot(&state, &loupist.id)?;
        let learned = learned_methods(&state, &loupist.id)?;
        append_atomic(&out, &mut output, &json!({"harness":stamp,"phase":"learned","snapshot":snapshot,"methods":learned}))?;
        if snapshot["experiences"] != 2 || snapshot["worldClaimEvents"] != 0
            || snapshot["journals"] != 0 || learned.is_empty()
        {
            return Err("two research experiences did not create a candidate independently of World claim growth".into());
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(learned)
    }.await;
    state.shutdown().await?;
    let learned = training?;

    for (index, probe) in fixture.probes.iter().enumerate() {
        for condition in if index % 2 == 0 {
            ["B", "D"]
        } else {
            ["D", "B"]
        } {
            eprintln!("research: {} condition {condition}", probe.id);
            let root = tempfile::Builder::new()
                .prefix("loupist-research-probe-")
                .tempdir_in("/tmp")?;
            let workspace = root.path().join("workspace");
            fs::create_dir(&workspace)?;
            let state = disposable_state(root.path(), &pi_dir, &frozen)?;
            state.set_model_catalog(crate::providers::catalog::ModelCatalog {
                models: vec![model.clone()],
            })?;
            let loupist =
                crate::commands::loupist::create_folder_loupist(&state, &workspace).await?;
            let mut sources: Vec<SourceSeed> = fixture
                .training
                .seed
                .sources
                .iter()
                .chain(&probe.seed.sources)
                .map(|source| SourceSeed {
                    title: source.title.clone(),
                    text: source.text.clone(),
                    origin: source.origin.clone(),
                })
                .collect();
            sources.sort_by(|a, b| a.title.cmp(&b.title));
            seed(
                &state,
                &loupist.id,
                &Seed {
                    instructions: Some(
                        "Use only the retained fictional originals. Do not fetch external data."
                            .into(),
                    ),
                    sources,
                    lens: if condition == "D" {
                        learned
                            .iter()
                            .map(|method| LensSeed {
                                title: method.title.clone(),
                                document: method.document.clone(),
                            })
                            .collect()
                    } else {
                        Vec::new()
                    },
                    ..Default::default()
                },
            )?;
            enable_connections(&state, &loupist.id, &[])?;
            let method_ids: Vec<_> = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id)?
                .list_records(&loupist.id)?
                .into_iter()
                .filter(|record| {
                    record.kind == RecordKind::Lens
                        && learned.iter().any(|method| method.title == record.title)
                })
                .map(|record| record.record_id)
                .collect();
            let execution = observe_session(
                &state,
                &loupist.id,
                &probe.sessions[0],
                &model_id,
                &thinking,
            )
            .await;
            let snapshot = learning_snapshot(&state, &loupist.id)?;
            state.shutdown().await?;
            let turns = execution?;
            let exact_read = turns.iter().any(|turn| {
                turn["memory_reads"].as_array().is_some_and(|reads| {
                    reads.iter().any(|read| {
                        read["access"] == "exact_read"
                            && method_ids.iter().any(|id| read["record_id"] == *id)
                    })
                })
            });
            let marked_applied = turns.iter().any(|turn| {
                turn["tool_calls"].as_array().is_some_and(|calls| {
                    calls.iter().any(|call| {
                        call["name"] == "lens_read"
                            && call["args"]["status"] == "applied"
                            && method_ids.iter().any(|id| call["args"]["recordId"] == *id)
                    })
                })
            });
            append_atomic(
                &out,
                &mut output,
                &json!({"harness":stamp,"phase":"probe","scenario":probe.id,"condition":condition,"turns":turns,"snapshot":snapshot,"learnedMethodIds":method_ids,"exactMethodRead":exact_read,"markedApplied":marked_applied}),
            )?;
            if turns.iter().any(|turn| {
                turn["status"] != "completed"
                    || turn["memory_commits"]
                        .as_array()
                        .is_none_or(|commits| !commits.is_empty())
            }) || snapshot["experiences"] != 0
                || snapshot["journals"] != 0
            {
                return Err("fresh read-only probe failed or wrote Memory".into());
            }
            if condition == "D" && ((index == 0 && !exact_read) || (index == 1 && marked_applied)) {
                return Err(
                    "method discovery or out-of-scope application needs review; evidence preserved"
                        .into(),
                );
            }
        }
    }
    Ok(())
}
