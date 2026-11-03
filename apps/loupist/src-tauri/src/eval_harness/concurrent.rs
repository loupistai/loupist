//! Real Chat concurrency qualification, with canonical state and overlap receipts.
use super::*;
use futures_util::future::join_all;
const RELEASE_FILE: &str = "release-gate.txt";

#[derive(Deserialize)]
struct ConcurrentSuite {
    schema: String,
    scenarios: Vec<ConcurrentScenario>,
}
#[derive(Deserialize)]
struct ConcurrentScenario {
    id: String,
    members: Vec<Member>,
}
#[derive(Deserialize)]
struct Member {
    id: String,
    loupist: String,
    memory: String,
    user: String,
    files: BTreeMap<String, String>,
    ready_file: String,
    #[serde(default)]
    cancel: bool,
    #[serde(default)]
    answer_contains: Vec<String>,
    #[serde(default)]
    answer_excludes: Vec<String>,
    #[serde(default)]
    memory_contains: Vec<String>,
    #[serde(default)]
    memory_excludes: Vec<String>,
    min_commits: usize,
    #[serde(default)]
    required_commit_kind: Option<String>,
}

pub struct Options {
    pub suite: PathBuf,
    pub out: PathBuf,
    pub pi_agent_dir: PathBuf,
    pub model: String,
    pub thinking: String,
    pub trials: usize,
    pub trial_index: Option<usize>,
}

pub async fn run(options: Options) -> Result<()> {
    let pending_path = options.out.with_extension("pending.jsonl");
    if options
        .trial_index
        .is_some_and(|trial| trial >= options.trials)
    {
        return Err("trial index must be smaller than trials".into());
    }
    if options.out.exists()
        || pending_path.exists()
        || options.trials == 0
        || !options.pi_agent_dir.is_absolute()
        || !options.pi_agent_dir.is_dir()
    {
        return Err(
            "require a new output, positive trials, and an existing absolute Pi profile".into(),
        );
    }
    let input = fs::read(&options.suite)?;
    let suite: ConcurrentSuite = serde_json::from_slice(&input)?;
    if suite.schema != "loupist-concurrent-suite/1" {
        return Err("unsupported concurrent suite".into());
    }
    for scenario in &suite.scenarios {
        let mut markers = std::collections::BTreeSet::new();
        if !(2..=4).contains(&scenario.members.len()) {
            return Err("each group needs 2–4 Chats".into());
        }
        for member in &scenario.members {
            if member.ready_file == RELEASE_FILE
                || member.files.contains_key(RELEASE_FILE)
                || !markers.insert((&member.loupist, &member.ready_file))
                || scenario.members.iter().any(|other| {
                    other.loupist == member.loupist && other.files.contains_key(&member.ready_file)
                })
            {
                return Err(
                    "each Chat needs its own readiness marker outside the input files".into(),
                );
            }
            memory_policy(&member.memory)?;
            for path in member
                .files
                .keys()
                .chain(std::iter::once(&member.ready_file))
            {
                relative(Path::new(path))?;
            }
        }
    }
    let pi = artifacts()?;
    let candidate = json!({"suite_sha256":crate::util::sha256(&input),
        "system_sha256":crate::util::sha256(&fs::read(&pi.system_prompt)?),
        "extension_sha256":crate::util::sha256(&fs::read(&pi.extension)?),
        "catalog_sha256":crate::util::sha256(include_bytes!("../../../agent/tool-catalog.json"))});
    let mut output = vec![];
    let mut pending = vec![];
    let mut all_pass = true;
    for scenario in &suite.scenarios {
        for trial in 0..options.trials {
            if options
                .trial_index
                .is_some_and(|selected| selected != trial)
            {
                continue;
            }
            let root = tempfile::Builder::new()
                .prefix("loupist-concurrent-")
                .tempdir_in(if cfg!(unix) {
                    PathBuf::from("/tmp")
                } else {
                    std::env::temp_dir()
                })?
                .keep();
            append_atomic(
                &pending_path,
                &mut pending,
                &json!({
                    "schema":"loupist-concurrent-attempt/1", "scenario":scenario.id,
                    "trial":trial, "state":"started", "root":root, "candidate":candidate,
                    "model":options.model, "thinking":options.thinking,
                    "at":chrono::Utc::now().to_rfc3339(),
                }),
            )?;
            let app_dir = root.join("app");
            let mut state = AppState::for_eval(app_dir.clone());
            state.eval_pi_agent_dir = Some(options.pi_agent_dir.clone());
            Arc::make_mut(&mut state.artifacts).pi = Some(pi.clone());
            fs::write(
                app_dir.join("config.toml"),
                "[pi.retry]\nenabled = false\nmax_retries = 0\n",
            )?;
            let provider = options
                .model
                .split_once('/')
                .ok_or("model must be provider/model")?
                .0;
            let models = crate::providers::connection::eval_models(&state, provider).await?;
            if !models
                .iter()
                .any(|m| m.id == options.model && m.thinking_levels.contains(&options.thinking))
            {
                return Err("requested model/effort not available".into());
            }
            state.set_model_catalog(crate::providers::catalog::ModelCatalog { models })?;
            let mut loupists = BTreeMap::new();
            let mut chats = vec![];
            let mut sessions = vec![];
            let mut turns = vec![];
            for member in &scenario.members {
                if !loupists.contains_key(&member.loupist) {
                    let workspace = root.join(format!("workspace-{}", loupists.len()));
                    fs::create_dir(&workspace)?;
                    fs::write(workspace.join(RELEASE_FILE), "pending")?;
                    let loupist =
                        crate::commands::loupist::create_folder_loupist(&state, &workspace).await?;
                    loupists.insert(member.loupist.clone(), (loupist.id, workspace));
                }
                let (loupist, workspace) = &loupists[&member.loupist];
                let marker = workspace.join(&member.ready_file);
                fs::create_dir_all(marker.parent().unwrap())?;
                fs::write(marker, "pending")?;
                for (name, text) in &member.files {
                    let file = workspace.join(name);
                    fs::create_dir_all(file.parent().unwrap())?;
                    fs::write(file, text)?;
                }
                chats.push(new_chat(&state, loupist, &member.id)?);
                sessions.push(Session {
                    id: member.id.clone(),
                    role: "probe".into(),
                    memory: member.memory.clone(),
                    turns: vec![],
                });
                turns.push(Turn {
                    user: member.user.clone(),
                    as_of: None,
                    ..Default::default()
                });
            }
            let started = chrono::Utc::now().to_rfc3339();
            let clock = Instant::now();
            let mut observations = vec![];
            let mut max_active = 0;
            let mut max_ready_active = 0;
            let mut cancelled = vec![false; chats.len()];
            let mut released = false;
            let mut previous = Value::Null;
            let results = {
                let runs = join_all(chats.iter().zip(&sessions).zip(&turns).map(
                    |((chat, session), turn)| {
                        run_turn(
                            &state,
                            chat,
                            session,
                            turn,
                            &options.model,
                            &options.thinking,
                            180_000,
                        )
                    },
                ));
                tokio::pin!(runs);
                loop {
                    tokio::select! {
                        results = &mut runs => break results,
                        _ = tokio::time::sleep(Duration::from_millis(25)) => {
                            let activities = state.run_coordinator.activities()?;
                            let active = activities.iter().filter(|a| a.kind == "chat").count();
                            let ready = scenario.members.iter().enumerate().filter(|(i, member)|
                                fs::read_to_string(loupists[&member.loupist].1.join(&member.ready_file)).is_ok_and(|text| text == "ready") &&
                                activities.iter().any(|a| a.kind == "chat" && a.target == chats[*i].id)).count();
                            max_active = max_active.max(active);
                            max_ready_active = max_ready_active.max(ready);
                            let current = json!({"activities":activities,"ready_active":ready});
                            if current != previous {
                                observations.push(json!({"elapsed_ms":clock.elapsed().as_millis(),"state":current}));
                                previous = current;
                            }
                            for (i, member) in scenario.members.iter().enumerate() {
                                if member.cancel && !cancelled[i] && ready == scenario.members.len() {
                                    if let Some(run) = activities.iter().find(|a| a.kind == "chat" && a.target == chats[i].id) {
                                        state.cancel_run(&run.run_id, crate::runs::coordinator::RunKind::Chat).await?;
                                        cancelled[i] = true;
                                    }
                                }
                            }
                            if !released && ready == scenario.members.len() {
                                for (_, workspace) in loupists.values() {
                                    fs::write(workspace.join(RELEASE_FILE), "go")?;
                                }
                                released = true;
                            }
                        }
                    }
                }
            };
            let mut snapshots = BTreeMap::new();
            for (label, (loupist, _)) in &loupists {
                snapshots.insert(label.clone(), world::snapshot(&state, loupist, None).await?);
            }
            let mut checks = BTreeMap::from([
                ("native_barrier_released".to_string(), released),
                (
                    "registered_overlap".to_string(),
                    max_active >= scenario.members.len(),
                ),
                (
                    "native_workspace_overlap".to_string(),
                    max_ready_active >= scenario.members.len(),
                ),
                (
                    "all_owned_runs_released".to_string(),
                    state.run_coordinator.active_count() == 0,
                ),
                (
                    "private_process_leases_released".to_string(),
                    fs::read_dir(&state.artifacts.process_lease_dir)?
                        .next()
                        .is_none(),
                ),
            ]);
            let mut records = vec![];
            let mut saved_chats = vec![];
            for (i, result) in results.into_iter().enumerate() {
                let member = &scenario.members[i];
                let result = result?;
                let answer = result["answer"].as_str().unwrap_or_default();
                let memory = serde_json::to_string(&snapshots[&member.loupist])?;
                let commits = result["memory_commits"].as_array().map_or(0, Vec::len);
                checks.insert(
                    format!("{}:terminal", member.id),
                    result["status"]
                        == if member.cancel {
                            "aborted"
                        } else {
                            "completed"
                        },
                );
                checks.insert(
                    format!("{}:answer", member.id),
                    member.answer_contains.iter().all(|s| answer.contains(s))
                        && member.answer_excludes.iter().all(|s| !answer.contains(s)),
                );
                checks.insert(
                    format!("{}:memory", member.id),
                    commits >= member.min_commits
                        && (member.memory != "off" || commits == 0)
                        && member.memory_contains.iter().all(|s| memory.contains(s))
                        && member.memory_excludes.iter().all(|s| !memory.contains(s)),
                );
                checks.insert(
                    format!("{}:commit_kind", member.id),
                    member.required_commit_kind.as_ref().is_none_or(|kind| {
                        result["memory_commits"].as_array().is_some_and(|commits| {
                            commits.iter().any(|commit| commit["kind"] == *kind)
                        })
                    }),
                );
                checks.insert(
                    format!("{}:cancel", member.id),
                    cancelled[i] == member.cancel,
                );
                let calls = result["tool_calls"].as_array().ok_or("missing tools")?;
                checks.insert(
                    format!("{}:tool_contracts", member.id),
                    calls.iter().all(|c| {
                        c["error_code"] != "tool_validation_failed"
                            && (member.cancel || c["outcome"] == "ok")
                    }),
                );
                saved_chats.push(
                    state
                        .store
                        .get_chat(&chats[i].id)?
                        .ok_or("missing canonical Chat")?,
                );
                records.push(json!({"member":member.id,"loupist_id":chats[i].loupist_id,"thread_id":chats[i].id,"memory":member.memory,"result":result}));
            }
            state.shutdown().await?;
            drop(state);
            let reopened = AppState::for_eval(app_dir);
            for chat in &saved_chats {
                checks.insert(
                    format!("{}:reopen", chat.id),
                    serde_json::to_value(reopened.store.get_chat(&chat.id)?)?
                        == serde_json::to_value(Some(chat))?,
                );
            }
            for (label, (loupist, _)) in &loupists {
                checks.insert(
                    format!("{label}:memory_reopen"),
                    world::snapshot(&reopened, loupist, None).await? == snapshots[label],
                );
            }
            reopened.shutdown().await?;
            let passed = checks.values().all(|v| *v);
            all_pass &= passed;
            let record = json!({"schema":"loupist-concurrent-result/1","scenario":scenario.id,"trial":trial,
                "model":options.model,"thinking":options.thinking,"candidate":candidate,"root":root,"started_at":started,
                "duration_ms":clock.elapsed().as_millis(),"max_active":max_active,"max_ready_active":max_ready_active,
                "observations":observations,"members":records,"snapshots":snapshots,"checks":checks,"passed":passed});
            append_atomic(&options.out, &mut output, &record)?;
            append_atomic(
                &pending_path,
                &mut pending,
                &json!({
                    "schema":"loupist-concurrent-attempt/1", "scenario":scenario.id,
                    "trial":trial, "state":"completed", "passed":passed,
                    "at":chrono::Utc::now().to_rfc3339(),
                }),
            )?;
            eprintln!(
                "{} trial={trial} passed={passed} overlap={max_active}/{max_ready_active}",
                scenario.id
            );
        }
    }
    if all_pass {
        Ok(())
    } else {
        Err("concurrent qualification has failed checks; see output".into())
    }
}
