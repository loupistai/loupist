//! One trial: a disposable app profile, its Loupist and every session of one
//! scenario. The runner and the scripted-Pi tests share this path.
use super::*;
use crate::providers::catalog::{ConfiguredModel, ModelCatalog};
use std::collections::BTreeSet;

/// The AppState of one trial. A runner process runs its trials one after
/// another, and each ends with the app's exit path, which closes this
/// process's spawn admission; without readmitting it here, every Pi after the
/// first trial's would be refused before its prompt.
pub(super) fn trial_state(app_data_dir: PathBuf) -> AppState {
    #[cfg(unix)]
    crate::process_lease::ProcessGroups::host().readmit();
    AppState::for_eval(app_data_dir)
}

/// What every trial of a batch shares.
pub(super) struct TrialHost {
    pub pi: PiArtifacts,
    /// The live Pi authentication directory, used in place. A scripted Pi has none.
    pub pi_agent_dir: Option<PathBuf>,
    /// The frozen app configuration each profile starts from; `None` keeps the
    /// test profile's.
    pub config_toml: Option<String>,
    /// The `run_python` module cache every trial shares.
    pub python_cache: Option<PathBuf>,
    /// The model every turn asks for, by catalog id.
    pub model: String,
    pub thinking: String,
    pub turn_timeout_ms: u64,
    /// Models discovered with the first trial's Pi, for a live provider.
    pub discovery: Option<Discovery>,
    pub keep: bool,
}

pub(super) struct Discovery {
    pub provider: String,
    pub models: Option<Vec<ConfiguredModel>>,
}

/// Where a trial's records go. The runner writes and budgets them; a test keeps them.
pub(super) trait Recorder {
    /// Whether the batch admits another turn. `false` stops the batch.
    fn admit_turn(&mut self) -> bool;
    fn record(
        &mut self,
        session: &Session,
        turn_index: usize,
        turn: &Turn,
        model: &ConfiguredModel,
        record: Value,
    ) -> Result<AfterTurn>;
}

/// The scenario's interface language: its `language`, else an `en` or `ko` tag.
fn interface_language(scenario: &Scenario) -> Result<Option<&str>> {
    let language = scenario.language.as_deref().or_else(|| {
        if scenario.tags.iter().any(|tag| tag == "en") {
            Some("en")
        } else if scenario.tags.iter().any(|tag| tag == "ko") {
            Some("ko")
        } else {
            None
        }
    });
    if language.is_some_and(|language| !matches!(language, "en" | "ko")) {
        return Err("scenario language must be en or ko".into());
    }
    Ok(language)
}

/// Each World publication's recorded instant, by Source id: its
/// `recorded_time`, else its Source's `publication_time`. A publication
/// placed after a turn is recorded later than that turn's clock.
pub(super) fn world_times(scenario: &Scenario) -> Result<BTreeMap<String, i64>> {
    use world::script::{recorded_ms, After};
    let Some(block) = &scenario.world else {
        return Ok(BTreeMap::new());
    };
    let sources: BTreeMap<&str, &SourceSeed> = scenario
        .seed
        .sources
        .iter()
        .filter_map(|source| Some((source.id.as_deref()?, source)))
        .collect();
    let turns: BTreeMap<&str, usize> = scenario
        .sessions
        .iter()
        .map(|session| (session.id.as_str(), session.turns.len()))
        .collect();
    world::script::validate(block, &sources.keys().copied().collect(), &turns)?;
    let scenario_clock = scenario
        .clock
        .as_deref()
        .map(chrono::DateTime::parse_from_rfc3339)
        .transpose()
        .map_err(|_| "scenario clock must be RFC 3339")?;
    let offset = scenario_clock.map_or(chrono::FixedOffset::east_opt(0).unwrap(), |clock| {
        *clock.offset()
    });
    let mut times = BTreeMap::new();
    for publication in &block.publications {
        let source = sources[publication.source.as_str()];
        let stated = publication
            .recorded_time
            .as_deref()
            .or(source.publication_time.as_deref())
            .ok_or_else(|| {
                format!(
                    "a published Source states its publication_time: {}",
                    publication.source
                )
            })?;
        let at = recorded_ms(stated, offset)?;
        if let After::Turn { session, turn } = &publication.after {
            let turn_clock = scenario
                .sessions
                .iter()
                .find(|candidate| &candidate.id == session)
                .and_then(|session| session.turns.get(*turn))
                .and_then(|turn| turn.scheduled.as_ref())
                .and_then(|fixture| fixture.clock())
                .map(chrono::DateTime::parse_from_rfc3339)
                .transpose()
                .map_err(|_| "a scheduled clock must be RFC 3339")?
                .or(scenario_clock);
            if turn_clock.is_some_and(|clock| at <= clock.timestamp_millis()) {
                return Err(format!(
                    "{} is published after {session}#{turn}, so it is recorded later than that turn's clock",
                    publication.source
                )
                .into());
            }
        }
        times.insert(publication.source.clone(), at);
    }
    Ok(times)
}

/// The bundled embedding model a scripted World answers embedding stages with.
fn embed_assets() -> Result<PathBuf> {
    Ok(staged_resources("embed_assets")?.join("embed_assets"))
}

/// A trial's Loupist with every seed staged, before its first turn.
pub(super) struct Prepared {
    pub loupist_id: String,
    pub retained: seeds::Retained,
    pub staging: Option<world::script::Staging>,
    /// The asset links already made; the rest wait for their company's publication.
    pub linked: BTreeSet<usize>,
    /// The research pages `lens_pages` filed: `{title, record_id}`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub lens_pages: Vec<Value>,
}

/// Create the trial's Loupist in `workspace` and stage its seeds: Sources and
/// the other seed records, the World publications placed at seed, the
/// research pages that cite them, then programs and the Learning allowance.
pub(super) async fn prepare(
    state: &AppState,
    workspace: &Path,
    scenario: &Scenario,
    model: &ConfiguredModel,
    thinking: &str,
    timeout_ms: u64,
) -> Result<Prepared> {
    let loupist = crate::commands::loupist::create_folder_loupist(state, workspace).await?;
    let recorded = world_times(scenario)?;
    let retained = seed_at(state, &loupist.id, &scenario.seed, &recorded)?;
    enable_connections(state, &loupist.id, &scenario.connections)?;
    let staging = match &scenario.world {
        Some(block) => {
            let mut staging = world::script::Staging::new(block, &recorded, embed_assets()?)?;
            {
                let mut connection = state.store.lock_loupist(&loupist.id)?;
                let tx = connection.transaction()?;
                crate::knowledge::store_settings(
                    &tx,
                    &loupist.id,
                    &staging.settings(model, thinking),
                )?;
                tx.commit()?;
            }
            staging.queue(state, &loupist.id, &retained)?;
            staging
                .publish(
                    state,
                    &loupist.id,
                    &retained,
                    &world::script::After::seed(),
                    timeout_ms,
                )
                .await?;
            Some(staging)
        }
        None => None,
    };
    let mut linked = BTreeSet::new();
    seeds::asset_links(state, &loupist.id, &scenario.seed.asset_links, &mut linked)?;
    let lens_pages = seeds::lens_pages(
        state,
        &loupist.id,
        &scenario.seed.lens_pages,
        &retained,
        staging.as_ref(),
    )?;
    scheduled::seed_programs(
        state,
        &loupist.id,
        &scenario.seed.programs,
        &model.id,
        thinking,
    )?;
    Box::pin(scheduled::seed_checks(
        state,
        &loupist.id,
        &scenario.seed.checks,
        &model.id,
        thinking,
    ))
    .await?;
    if let Some(left) = scenario.seed.learning_allowance {
        scheduled::spend_learning_allowance(state, &loupist.id, &model.id, left)?;
    }
    Ok(Prepared {
        loupist_id: loupist.id,
        retained,
        staging,
        linked,
        lens_pages,
    })
}

/// Run every session of one trial. Returns whether the batch must stop.
pub(super) async fn run_trial(
    host: &mut TrialHost,
    scenario: &Scenario,
    replay: Option<&replay::Replay>,
    recorder: &mut dyn Recorder,
) -> Result<bool> {
    let temporary = tempfile::Builder::new()
        .prefix("loupist-eval-")
        .tempdir_in(eval_temp_root())?;
    let workspace = temporary.path().join("workspace");
    fs::create_dir(&workspace)?;
    for (name, contents) in &scenario.workspace_files {
        relative(Path::new(name))?;
        let path = workspace.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, contents)?;
    }
    let mut state = trial_state(temporary.path().join("app"));
    configure_market_runtime(&mut state, scenario, &host.pi.runtime_dir)?;
    if let Some(cache) = &host.python_cache {
        Arc::make_mut(&mut state.artifacts).python_cache_root = cache.clone();
    }
    let language = interface_language(scenario)?;
    // Only this disposable app profile is changed; paid eval attempts never retry.
    let config = state.artifacts.app_data_dir.join("config.toml");
    match (&host.config_toml, language) {
        (Some(frozen), Some(language)) => {
            let mut document = frozen.parse::<toml_edit::Document>()?;
            document["appearance"]["language"] = toml_edit::value(language);
            fs::write(config, document.to_string())?;
        }
        (Some(frozen), None) => fs::write(config, frozen)?,
        (None, Some(language)) => fs::write(
            config,
            format!("[appearance]\nlanguage = {}\n", toml_edit::value(language)),
        )?,
        (None, None) => {}
    }
    state.eval_pi_agent_dir = host.pi_agent_dir.clone();
    Arc::make_mut(&mut state.artifacts).pi = Some(host.pi.clone());
    if let Some(discovery) = &mut host.discovery {
        if discovery.models.is_none() {
            let available =
                crate::providers::connection::eval_models(&state, &discovery.provider).await?;
            eprintln!(
                "discovered models: {}",
                available
                    .iter()
                    .map(|m| m.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            discovery.models = Some(available);
        }
        let models = discovery.models.clone().unwrap_or_default();
        if !models.iter().any(|model| model.id == host.model) {
            return Err("requested model not found in Pi model discovery".into());
        }
        state.set_model_catalog(ModelCatalog { models })?;
    }
    let selected = state.model_catalog()?.resolve(&host.model)?;
    eprintln!(
        "eval model {}/{} thinking={}",
        selected.provider, selected.model, host.thinking
    );
    let replay = replay
        .map(|replay| replay.clone().install(&state.credential_environment))
        .transpose()?;
    if scenario
        .world
        .as_ref()
        .is_some_and(|block| block.mode == world::script::Mode::Live)
    {
        // Live extraction needs the provider service of the installed Pi
        // profile, not the one the test constructor captured.
        state.graph.shutdown().await?;
        state.graph = crate::knowledge::GraphSupervisor::with_inference(
            state.store.clone(),
            state.artifacts.app_data_dir.clone(),
            None,
            state.maintenance.clone(),
            Some(crate::providers::inference::ProviderInferenceService::from_state(&state)?),
        );
    }
    let Prepared {
        loupist_id,
        retained,
        mut staging,
        mut linked,
        ..
    } = Box::pin(prepare(
        &state,
        &workspace,
        scenario,
        &selected,
        &host.thinking,
        host.turn_timeout_ms,
    ))
    .await?;
    let loupist = state
        .store
        .get_loupist(&loupist_id)?
        .ok_or("the trial's Loupist is missing")?;
    let mut stop_batch = false;
    'sessions: for session in &scenario.sessions {
        memory_policy(&session.memory)?;
        let chat = if session.id == "bot" {
            crate::chat::home::ensure_home_thread(&state, &loupist.id)
                .map_err(|error| error.message)?
        } else {
            new_chat(&state, &loupist.id, &session.id)?
        };
        for (turn_index, turn) in session.turns.iter().enumerate() {
            if !recorder.admit_turn() {
                stop_batch = true;
                break 'sessions;
            }
            let mut record = run_turn(
                &state,
                &chat,
                session,
                turn,
                &selected.id,
                &host.thinking,
                host.turn_timeout_ms,
            )
            .await?;
            if let Some(staging) = &mut staging {
                staging.settle_unscripted(&state, &loupist.id).await?;
                record["world_staged"] = staging.trace(&state, &loupist.id)?;
            }
            let next = recorder.record(session, turn_index, turn, &selected, record)?;
            if let (Some(staging), AfterTurn::Continue) = (&mut staging, &next) {
                staging
                    .publish(
                        &state,
                        &loupist.id,
                        &retained,
                        &world::script::After::turn(&session.id, turn_index),
                        host.turn_timeout_ms,
                    )
                    .await?;
                seeds::asset_links(&state, &loupist.id, &scenario.seed.asset_links, &mut linked)?;
            }
            match next {
                AfterTurn::Continue => {}
                // A timeout or a Pi that never started aborts this Loupist's
                // dependent sessions only. The next trial starts in a fresh
                // Loupist and must still be measured.
                AfterTurn::NextTrial => break 'sessions,
                AfterTurn::StopBatch => {
                    stop_batch = true;
                    break 'sessions;
                }
            }
        }
    }
    state.shutdown().await?;
    drop(state);
    drop(replay);
    if host.keep {
        eprintln!("kept {}", temporary.keep().display());
    }
    Ok(stop_batch)
}
