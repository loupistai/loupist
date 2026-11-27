//! Compare matched inputs at Pi's provider callback, using the real host and
//! extension with a scripted SDK provider.
use super::*;

#[tokio::test]
async fn learning_script_exercises_context_omission_quiet_commit_and_budget_stop() {
    use crate::scheduled_tasks::types::*;

    let root = tempfile::tempdir_in("/tmp").unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    super::tests::scripted(&mut state, root.path());
    let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("node"))
        .find(|path| path.is_absolute() && path.is_file())
        .expect("Node from the worktree");
    let module =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/eval_harness/session_language_capture.mjs");
    fs::write(
        root.path().join("session-language-capture.json"),
        json!([node, module]).to_string(),
    )
    .unwrap();
    let mut catalog = state.model_catalog().unwrap();
    catalog.models[0].provider = "custom-faux".into();
    state.set_model_catalog(catalog).unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let owner = &loupist.id;
    let suite: Suite =
        serde_json::from_str(include_str!("../../../evals/agent/suites/language-v1.json")).unwrap();
    let fixture = suite
        .scenarios
        .iter()
        .find(|scenario| scenario.id == "LG-003")
        .unwrap();
    seed(&state, owner, &fixture.seed).unwrap();
    let previous = crate::commands::now_ms() - 400;
    let task = state
        .store
        .scheduled_task_create_definition(
            owner,
            "learning-fixture",
            TaskDraft {
                title: "Fictional learning program".into(),
                prompt: "Review the Alder Rail quarterly note.".into(),
                schedule: Schedule::Interval { every_minutes: 60 },
                target: TaskTarget::NewChat,
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: Default::default(),
                use_memory: true,
                update_memory: true,
                status: TaskStatus::Active,
            },
            TaskKind::Learning,
            TaskDefinition {
                program: Some(LearningProgram {
                    depth: LearningDepth::Standard,
                    scope: LearningScope {
                        connections: LearningConnections::Selected(vec![]),
                        web: false,
                    },
                    keeps: LearningKeeps {
                        sources: false,
                        lens: true,
                        asset_reads: false,
                    },
                    preset: None,
                }),
                watch: TaskWatch {
                    report: Some(ReportPolicy::WhenNew),
                    ..Default::default()
                },
                ..Default::default()
            },
            previous - 1,
        )
        .unwrap();
    for (at, status) in [
        (previous, RunStatus::Succeeded),
        (previous + 10, RunStatus::Failed),
    ] {
        let (claim, ()) = state
            .store
            .scheduled_task_claim_with(owner, &task.id, at, true, |_| Ok(()))
            .unwrap()
            .unwrap();
        state
            .store
            .scheduled_task_finish(owner, &claim.run.run_id, status, None, at + 1)
            .unwrap();
    }
    let mut last_success = previous;
    for (cancel, title) in [
        (false, "Alder Rail valuation"),
        (true, "Alder Rail discarded draft"),
    ] {
        fs::write(
            root.path().join("learning-script.json"),
            json!({
                "previous": last_success, "requests": 24, "soft": 12, "hard": 16,
                "cancel": cancel, "title": title,
            })
            .to_string(),
        )
        .unwrap();
        let started = state
            .scheduled
            .dispatch(&state, owner, &task.id, true)
            .await
            .unwrap()
            .unwrap();
        let finished = tokio::time::timeout(std::time::Duration::from_secs(60), async {
            loop {
                let run = state
                    .store
                    .scheduled_task_run_get(owner, &started.run_id)
                    .unwrap()
                    .unwrap();
                if run.status != RunStatus::Running {
                    break run;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let lens =
            crate::knowledge::KnowledgeStore::for_loupist(state.store.clone(), owner).unwrap();
        let kept = lens
            .read(|store| store.lens_index(owner))
            .unwrap()
            .iter()
            .any(|page| page.title == title);
        let ledger_usage = state
            .store
            .scheduled_run_ledger_usage(owner, &started.run_id)
            .unwrap();
        assert_eq!(finished.usage, ledger_usage);
        if cancel {
            assert_eq!(finished.status, RunStatus::Interrupted, "{finished:?}");
            assert_eq!(
                finished.error.as_deref(),
                Some(crate::scheduled_tasks::BUDGET_EXCEEDED)
            );
            assert_eq!(finished.budget.unwrap().outcome, BudgetOutcome::Cancelled);
            assert_eq!(finished.usage.unwrap().requests, Some(24));
            assert!(!kept);
        } else {
            assert_eq!(
                finished.status,
                RunStatus::Succeeded,
                "{finished:?} {:?}",
                fs::read_to_string(root.path().join("learning-script-error.txt")).ok(),
            );
            assert!(finished.quiet);
            assert_eq!(finished.budget.unwrap().outcome, BudgetOutcome::Soft);
            assert_eq!(finished.usage.unwrap().requests, Some(14));
            assert!(kept);
            last_success = started.started_at_ms;
        }
    }
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn session_language_capture_lg003_at_both_appearances() {
    let ko: Suite =
        serde_json::from_str(include_str!("../../../evals/agent/suites/language-v1.json")).unwrap();
    let en: Suite = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/session-language-v1.json"
    ))
    .unwrap();
    let catalog: Value =
        serde_json::from_str(include_str!("../../../agent/tool-catalog.json")).unwrap();
    let on_demand = catalog["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "interface_labels");

    for (suite, id, lang) in [(ko, "LG-003", "ko"), (en, "LG-003-EN", "en")] {
        let scenario = suite
            .scenarios
            .iter()
            .find(|scenario| scenario.id == id)
            .unwrap();
        assert_eq!(scenario.language.as_deref(), Some(lang));
        let _clock = clock::install(scenario.clock.as_deref()).unwrap();
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        let agent_dir = state.artifacts.app_data_dir.join("pi");
        fs::create_dir_all(&agent_dir).unwrap();
        state.eval_pi_agent_dir = Some(agent_dir);
        super::tests::scripted(&mut state, root.path());
        fs::write(
            state.artifacts.app_data_dir.join("config.toml"),
            format!("[appearance]\nlanguage = {lang:?}\ndisplay_currency = \"USD\"\n"),
        )
        .unwrap();
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join("node"))
            .find(|path| path.is_absolute() && path.is_file())
            .expect("Node from the Loupist worktree environment");
        let module = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/eval_harness/session_language_capture.mjs");
        fs::write(
            root.path().join("session-language-capture.json"),
            json!([node, module]).to_string(),
        )
        .unwrap();
        let mut catalog = state.model_catalog().unwrap();
        catalog.models[0].provider = "custom-faux".into();
        state.set_model_catalog(catalog).unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        seed(&state, &loupist.id, &scenario.seed).unwrap();
        let chat = new_chat(&state, &loupist.id, "research").unwrap();
        let session = &scenario.sessions[0];
        let turn = &session.turns[0];
        let record = run_turn(&state, &chat, session, turn, "fixture", "medium", 60_000)
            .await
            .unwrap();
        state.shutdown().await.unwrap();
        assert_eq!(record["status"], "completed", "{record}");
        assert_eq!(record["model_rounds"], 3, "{record}");
        assert_eq!(record["observed_model"], "custom-faux/fixture-model");
        assert_eq!(record["resolved_appearance"], lang);
        assert_eq!(record["display_currency"], "USD");
        assert_eq!(record["interface_label_calls"], 0);
        let calls = record["tool_calls"].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["name"], "world_search");
        assert_eq!(calls[1]["name"], "world_read");
        let source = &scenario.seed.sources[0];
        assert!(record["memory_reads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|read| {
                read["kind"] == "source"
                    && read["access"] == "exact_read"
                    && read["title"] == source.title
            }));

        for phase in ["first.json", "after-read.json"] {
            let path = root.path().join(phase);
            let bytes = fs::read(&path).unwrap();
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(request["options"]["reasoning"], "medium");
            let messages = request["context"]["messages"].as_array().unwrap();
            let [user] = messages
                .iter()
                .filter(|message| message["role"] == "user")
                .collect::<Vec<_>>()[..]
            else {
                panic!("expected one user message");
            };
            let prompt = user["content"][0]["text"].as_str().unwrap();
            let (_, current) = prompt.rsplit_once("<current_user_message>\n").unwrap();
            assert_eq!(current, format!("{}\n</current_user_message>", turn.user));
            let envelope: Value = serde_json::from_str(
                prompt
                    .split_once("<turn_envelope>\n")
                    .unwrap()
                    .1
                    .split_once("\n</turn_envelope>")
                    .unwrap()
                    .0,
            )
            .unwrap();
            assert_eq!(envelope["chat_surface"]["surface"], "session");
            let interface = &envelope["interface"];
            if on_demand {
                assert_eq!(interface, &json!({"display_currency":"USD"}), "{phase}");
            } else {
                assert_eq!(interface["language"], lang);
                assert_eq!(interface["display_currency"], "USD");
                assert_eq!(interface["names"].as_object().unwrap().len(), 5);
            }
            if phase == "after-read.json" {
                let read = messages
                    .iter()
                    .find(|message| {
                        message["role"] == "toolResult" && message["toolName"] == "world_read"
                    })
                    .unwrap();
                let read: Value =
                    serde_json::from_str(read["content"][0]["text"].as_str().unwrap()).unwrap();
                assert_eq!(
                    read["text"]
                        .as_str()
                        .and_then(crate::custom_connections::taint::delimited_body),
                    Some(source.text.as_str())
                );
                assert!(read["citation_url"]
                    .as_str()
                    .is_some_and(|url| record["answer"].as_str().unwrap().contains(url)));
            }
            if let Some(output) = std::env::var_os("F3_CAPTURE_DIR").map(PathBuf::from) {
                fs::create_dir_all(&output).unwrap();
                fs::write(output.join(format!("{lang}-{phase}")), bytes).unwrap();
            }
        }
    }
}
