use super::*;

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn workspace_skill_copy_is_traced_with_host_workspace_authority() {
    let root = tempfile::Builder::new()
        .prefix("eval-skill-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    let copy = workspace.join("skills/data-analysis/SKILL.md");
    fs::create_dir_all(copy.parent().unwrap()).unwrap();
    fs::write(&copy, "This workspace file is not the bundled Skill.").unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    fs::write(
        root.path().join("replay-case.json"),
        json!({"name":"read", "method":"native.authorize",
        "args":{"name":"read", "path":copy}})
        .to_string(),
    )
    .unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let chat = new_chat(&state, &loupist.id, "workspace Skill copy").unwrap();
    let session = Session {
        id: "probe".into(),
        role: "probe".into(),
        memory: "off".into(),
        turns: vec![],
    };
    let result = run_turn(
        &state,
        &chat,
        &session,
        &Turn {
            user: "REPLAY_CASE_EVAL: read the workspace file".into(),
            ..Default::default()
        },
        "fixture",
        "medium",
        60_000,
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    let read = result["tool_calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["name"] == "read")
        .unwrap();
    assert_eq!(read["outcome"], "ok");
    assert_eq!(
        read["native_authorization"],
        json!({"class":"Workspace", "skill_id":null})
    );
    state.shutdown().await.unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn invalid_pi_agent_directory_is_rejected_before_chat_side_effects() {
    const CHILD: &str = "LOUPIST_TEST_INVALID_PI_DIRECTORY";
    if std::env::var_os(CHILD).is_none() {
        // A child test process keeps this failure injection out of every
        // other test's environment, even with parallel libtest execution.
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "eval_harness::tests::invalid_pi_agent_directory_is_rejected_before_chat_side_effects", "--nocapture"])
            .env(CHILD, "1")
            .env_remove(crate::app::LIVE_PI_AGENT_DATA_DIR_ENV)
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let chat = new_chat(&state, &loupist.id, "invalid runtime directory").unwrap();
    std::env::set_var(
        crate::app::LIVE_PI_AGENT_DATA_DIR_ENV,
        root.path().join("missing"),
    );
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = events.clone();
    let error = send_chat(
        ChatSendRequest {
            run_id: "invalid-pi-directory".into(),
            loupist_id: loupist.id.clone(),
            thread_id: chat.id.clone(),
            prompt: "TEACH_EVAL: save my method".into(),
            use_memory: true,
            update_memory: true,
            as_of: None,
            model_profile_id: "fixture".into(),
            thinking_level: "medium".into(),
            run_options: Default::default(),
            mentioned_connection_ids: vec![],
            attachments: vec![],
            pins: vec![],
            selected_artifact: None,
        },
        ChatEventSink::new(move |event| {
            observed.lock().unwrap().push(event);
            Ok(())
        }),
        &state,
        None,
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "pi_unavailable");
    assert!(state
        .store
        .get_chat(&chat.id)
        .unwrap()
        .unwrap()
        .messages
        .is_empty());
    assert!(events.lock().unwrap().is_empty());
    assert_eq!(state.run_coordinator.active_count(), 0);
    assert_eq!(
        fs::read_dir(&state.artifacts.broker_dir).unwrap().count(),
        0
    );
    assert_eq!(
        fs::read_dir(&state.artifacts.process_lease_dir)
            .unwrap()
            .count(),
        0
    );
    std::env::remove_var(crate::app::LIVE_PI_AGENT_DATA_DIR_ENV);
    state.shutdown().await.unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn completed_eval_turn_does_not_wait_for_an_unrelated_chat() {
    use crate::runs::coordinator::{RunKind, RunSpec, RunTarget};
    let root = tempfile::Builder::new()
        .prefix("eval-concurrent-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let chat = new_chat(&state, &loupist.id, "completed independently").unwrap();
    let unrelated = state
        .run_coordinator
        .register(
            RunSpec {
                run_id: "unrelated-live-chat".into(),
                loupist_id: "another-loupist".into(),
                kind: RunKind::Chat,
                target: RunTarget::ChatThread("other-thread".into()),
            },
            || Ok(()),
        )
        .unwrap();
    let session = Session {
        id: "teach".into(),
        role: "train".into(),
        memory: "remember".into(),
        turns: vec![],
    };
    let result = tokio::time::timeout(
        Duration::from_secs(20),
        run_turn(
            &state,
            &chat,
            &session,
            &Turn {
                user: "TEACH_EVAL: save my method".into(),
                as_of: None,
                ..Default::default()
            },
            "fixture",
            "medium",
            60_000,
        ),
    )
    .await;
    assert!(
        result.is_ok(),
        "a completed turn waited for another Loupist's active Chat"
    );
    let result = result.unwrap().unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert!(!result["memory_commits"].as_array().unwrap().is_empty());
    assert_eq!(
        state.run_coordinator.active_count(),
        1,
        "this turn must release its own reader and Memory writer"
    );
    drop(unrelated);
    state.shutdown().await.unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
pub(super) fn scripted(state: &mut AppState, root: &Path) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let executable = root.join("fixture-pi");
    let script = root.join("fixture.py");
    fs::write(&script, include_str!("scripted_pi.py")).unwrap();
    let compiled = std::process::Command::new("/usr/bin/cc")
        .args(["-std=c11", "-Wall", "-Werror"])
        .arg("-DFIXTURE_PYTHON=\"/usr/bin/python3\"")
        .arg(format!(
            "-DFIXTURE_SCRIPT={}",
            serde_json::to_string(&script).unwrap()
        ))
        .arg(manifest.join("src/commands/tests/fixtures/native_activity_launcher.c"))
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    let runtime = root.join("fixture-runtime");
    fs::create_dir(&runtime).unwrap();
    for (name, value) in [
        (".pi-version", crate::pi::PI_VERSION.into()),
        (
            ".pi-archive.sha256",
            crate::pi::PI_DARWIN_ARM64_ARCHIVE_SHA256.into(),
        ),
        (
            ".pi-executable.sha256",
            crate::util::sha256(&fs::read(&executable).unwrap()),
        ),
        (
            "package.json",
            json!({"version":crate::pi::PI_VERSION}).to_string(),
        ),
    ] {
        fs::write(runtime.join(name), value).unwrap();
    }
    let agent = manifest.parent().unwrap().join("agent");
    Arc::make_mut(&mut state.artifacts).pi = Some(PiArtifacts {
        executable,
        runtime_dir: runtime,
        extension: agent.join("loupist-extension.mjs"),
        provider_extension: agent.join("loupist-provider-extension.mjs"),
        system_prompt: agent.join("SYSTEM.md"),
        provider: String::new(),
        model: String::new(),
        thinking_level: String::new(),
        run_options: Default::default(),
        provider_credential: None,
    });
}
#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn eval_two_sessions_commit_and_retrieve_lens_through_broker() {
    let root = tempfile::Builder::new()
        .prefix("eval-test-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let teach = new_chat(&state, &loupist.id, "teach").unwrap();
    let session = Session {
        id: "teach".into(),
        role: "train".into(),
        memory: "remember".into(),
        turns: vec![],
    };
    let first = run_turn(
        &state,
        &teach,
        &session,
        &Turn {
            user: "TEACH_EVAL: save my fully diluted share method".into(),
            as_of: None,
            ..Default::default()
        },
        "fixture",
        "medium",
        600000,
    )
    .await
    .unwrap();
    assert_eq!(first["status"], "completed", "{first}");
    let commit = first["memory_commits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "lens" && c["title"] == "Hanbit diluted-share rule")
        .expect("first turn must commit the filed page");
    assert_eq!(commit["operation"], "create");
    assert!(first["tool_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "lens_file" && call["outcome"] == "ok"));
    let probe = new_chat(&state, &loupist.id, "probe").unwrap();
    assert_ne!(teach.id, probe.id);
    assert_ne!(teach.pi_session_id, probe.pi_session_id);
    assert!(probe.messages.is_empty());
    let session = Session {
        id: "probe".into(),
        role: "probe".into(),
        memory: "read_only".into(),
        turns: vec![],
    };
    let second = run_turn(
        &state,
        &probe,
        &session,
        &Turn {
            user: "Value fictional Hanbit: equity 720, basic shares 15, warrants 3.".into(),
            as_of: None,
            ..Default::default()
        },
        "fixture",
        "medium",
        600000,
    )
    .await
    .unwrap();
    assert_eq!(second["status"], "completed", "{second}");
    assert!(
        second["memory_reads"]
            .as_array()
            .unwrap()
            .iter()
            .any(|read| read["record_id"] == commit["record_id"] && read["access"] == "exact_read"),
        "{second}"
    );
    assert!(second["memory_commits"].as_array().unwrap().is_empty());
    assert!(second["answer"]
        .as_str()
        .unwrap()
        .contains("720 / (15 + 3) = 40"));
    state.shutdown().await.unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn eval_replay_broker_preserves_production_evidence() {
    let root = tempfile::Builder::new()
        .prefix("eval-replay-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    let replay: replay::Replay = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/replay/opendart-hanbit.json"
    ))
    .unwrap();
    let _replay = replay.install(&state.credential_environment).unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    enable_connections(&state, &loupist.id, &["opendart".into()]).unwrap();
    let chat = new_chat(&state, &loupist.id, "provider replay").unwrap();
    let session = Session {
        id: "replay".into(),
        role: "probe".into(),
        memory: "off".into(),
        turns: vec![],
    };
    let result = run_turn(
        &state,
        &chat,
        &session,
        &Turn {
            user: "DART_EVAL: retrieve the fictional filing".into(),
            as_of: None,
            ..Default::default()
        },
        "fixture",
        "medium",
        600000,
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert!(
        result["tool_calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["name"] == "dart_filing_search" && call["outcome"] == "ok"),
        "{result}"
    );
    let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
    assert!(
        !persisted.messages.last().unwrap().evidence_refs.is_empty(),
        "provider replay must retain production evidence"
    );
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn eval_seeds_use_canonical_folder_memory_and_factual_context() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_eval(root.path().join("app"));
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let input: Seed = serde_json::from_value(json!({
        "lens":[{"title":"Seeded dilution method","summary":"Use diluted shares","sections":[{"key":"use-when","heading":"Use when","markdown":"Fictional valuations"},{"key":"steps","heading":"Steps","markdown":"Divide by total shares"}]}],
        "sources":[{"title":"Fictional source","text":"Fictional shares outstanding: 18","origin":"https://fictional.example/filing"}],
        "watchlist":[{"symbol":"ZQLM","market":"NYSE","display_name":"Zircon Loom"}],
        "holdings_csv":"symbol,market,display_name,quantity,cost_basis,market_value,currency\nZQLM,NYSE,Zircon Loom,73,41,3285,USD\nQZ731,KRX,Quartz Loom,31,8200,279000,KRW\n",
        "principles":["My label for cash uncertainty is opal meadow."]
    }))
    .unwrap();
    seed(&state, &loupist.id, &input).unwrap();
    let knowledge = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    knowledge
        .read(|read| {
            let records = read.list_records(&loupist.id)?;
            let lens = records
                .iter()
                .find(|record| record.title == "Seeded dilution method")
                .unwrap();
            let page = read.read_page(&loupist.id, &lens.record_id)?;
            assert_eq!(page.body.summary(), input.lens[0].summary);
            assert_eq!(page.body.sections().len(), 2);
            assert_eq!(page.body.sections()[1].markdown, "Divide by total shares");
            assert!(records
                .iter()
                .any(|record| record.kind == RecordKind::Source
                    && record.title == "Fictional source"));
            Ok(())
        })
        .unwrap();
    let holdings = state.store.holdings_latest().unwrap().unwrap();
    let quantity = |symbol: &str| {
        holdings
            .positions
            .iter()
            .find(|position| position.symbol == symbol)
            .map(|position| position.quantity.clone())
    };
    assert_eq!(quantity("ZQLM").as_deref(), Some("73"));
    assert_eq!(quantity("QZ731").as_deref(), Some("31"));
    assert_eq!(state.store.watchlist_list().unwrap()[0].symbol, "ZQLM");
    {
        let mut connection = state.store.lock_loupist(&loupist.id).unwrap();
        let tx = connection.transaction().unwrap();
        let principles =
            crate::knowledge::principles::principle_list(&tx, &loupist.id, false).unwrap();
        assert_eq!(principles.len(), 1);
        assert_eq!(principles[0].statement, input.principles[0]);
    }
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn crypto_holdings_seed_joins_stock_holdings_through_the_product_import() {
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_eval(root.path().join("app"));
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let input: Seed = serde_json::from_value(json!({
        "holdings_csv":"symbol,market,quantity\nZQLM,NYSE,73\n",
        "holdings_crypto_csv":"asset,balance\nXXBT,0.4271\n"
    }))
    .unwrap();
    seed(&state, &loupist.id, &input).unwrap();
    let held = state
        .store
        .map_context()
        .unwrap()
        .holdings
        .into_iter()
        .map(|item| format!("{}:{}", item.symbol, item.market))
        .collect::<Vec<_>>();
    assert_eq!(held, ["BTC:CRYPTO", "ZQLM:NYSE"]);
    let refused: Seed = serde_json::from_value(
        json!({"holdings_crypto_csv":"date,pair,side,amount\n2026-10-01,BTC/USD,buy,1\n"}),
    )
    .unwrap();
    assert!(seed(&state, &loupist.id, &refused).is_err());
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn journal_seeds_tag_reasons_with_their_asset_page_and_a_refresh_without_grants_stays_local()
{
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_eval(root.path().join("app"));
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let input: Seed = serde_json::from_value(json!({
        "watchlist": [{"symbol": "990308", "market": "KRX", "display_name": "Byeolmuri Semiconductor"}],
        "journal": [{"judgment": "New packaging capacity lifts margins.", "thesis": {"symbol": "990308", "market": "KRX", "role": "reason"}, "createdAtMs": 1_772_020_800_000_i64}],
        "refresh_assets": true
    }))
    .unwrap();
    seed(&state, &loupist.id, &input).unwrap();
    // No connection is granted, so every planned read waits for one.
    crate::commands::assets::refresh_followed(
        &state,
        &loupist.id,
        std::time::Duration::from_secs(5),
        None,
    )
    .await
    .unwrap();
    let asset_id = crate::knowledge::assets::asset_id("990308", "KRX");
    let context = crate::commands::assets::view_context(&state).unwrap();
    let page = crate::commands::assets::with_assets(&state, &loupist.id, false, |tx| {
        crate::knowledge::assets::page_view(tx, &loupist.id, &asset_id, 0, &context)
    })
    .unwrap();
    assert_eq!(page.name, "Byeolmuri Semiconductor");
    assert_eq!(page.reasons.len(), 1);
    assert_eq!(page.read.as_ref().unwrap().since, "2026-02-25");
    assert!(page
        .checks
        .failures
        .iter()
        .all(|failure| failure.reason == "needs_connection"));
    assert!(page.refreshed_at_ms.is_some());
    state.shutdown().await.unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn eval_memory_write_without_reply_commits_receipts_but_provider_failure_does_not() {
    for (mode, expected, failure) in [
        ("EMPTY_REPLY_EVAL", "completed", None),
        ("PROVIDER_ERROR_EVAL", "failed", Some("provider_error")),
        ("EXIT_AFTER_WRITE_EVAL", "failed", Some("pi_exited")),
        (
            "COMMIT_FAILURE_EVAL",
            "failed",
            Some("memory_commit_failed"),
        ),
        ("STORE_FAILURE_EVAL", "failed", Some("store_failed")),
        ("EMPTY_NO_WRITE_EVAL", "failed", Some("empty_reply")),
    ] {
        let root = tempfile::Builder::new()
            .prefix("eval-completion-")
            .tempdir_in("/tmp")
            .unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        scripted(&mut state, root.path());
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        if mode.contains("COMMIT_FAILURE_EVAL") {
            state.store.lock_loupist(&loupist.id).unwrap().execute_batch(
                "CREATE TRIGGER reject_fixture_commit BEFORE INSERT ON knowledge_lens_log BEGIN SELECT RAISE(ABORT, 'private fixture commit detail'); END;"
            ).unwrap();
        }
        let chat = new_chat(&state, &loupist.id, "teach").unwrap();
        if mode == "STORE_FAILURE_EVAL" {
            state.store.lock_loupist(&loupist.id).unwrap().execute_batch(
                "CREATE TRIGGER reject_completed_chat BEFORE UPDATE ON chat_sessions WHEN json_extract(NEW.data_json, '$.messages[#-1].role') = 'assistant' AND json_extract(NEW.data_json, '$.messages[#-1].status') = 'complete' BEGIN SELECT RAISE(ABORT, 'private fixture store detail'); END;"
            ).unwrap();
        }
        let session = Session {
            id: "teach".into(),
            role: "train".into(),
            memory: "remember".into(),
            turns: vec![],
        };
        let result = run_turn(
            &state,
            &chat,
            &session,
            &Turn {
                user: format!("TEACH_EVAL {mode}"),
                as_of: None,
                ..Default::default()
            },
            "fixture",
            "medium",
            30_000,
        )
        .await
        .unwrap();
        assert_eq!(result["status"], expected, "{mode}: {result}");
        assert_eq!(result["failureKind"], json!(failure), "{result}");
        if mode == "COMMIT_FAILURE_EVAL" {
            assert_eq!(
                result["memory_commit_failure"],
                json!({"stage":"lens","reason":"storage"}),
                "{result}"
            );
        }
        let local_logs: Vec<Value> = fs::read_dir(workspace.join(".loupist/logs"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "jsonl")
            })
            .flat_map(|path| {
                fs::read_to_string(path)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect();
        let local_chat = local_logs
            .iter()
            .find(|record| record["kind"] == "chat")
            .unwrap();
        assert_eq!(
            local_chat["memoryCommitFailure"],
            result["memory_commit_failure"]
        );
        let serialized = format!("{result} {local_chat}");
        assert!(!serialized.contains("private fixture commit detail"));
        assert!(!serialized.contains("private fixture store detail"));
        if mode != "EMPTY_NO_WRITE_EVAL" {
            assert!(
                result["tool_calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|call| call["name"] == "lens_file" && call["outcome"] == "ok"),
                "{result}"
            );
        }
        let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
        let message = persisted.messages.last().unwrap();
        assert_eq!(message.host_composed, failure.is_none());
        assert_eq!(result["hostComposed"], failure.is_none());
        let records = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id)
            .unwrap()
            .read(|read| read.list_records(&loupist.id))
            .unwrap();
        assert_eq!(
            records
                .iter()
                .any(|record| record.title == "Hanbit diluted-share rule"),
            failure.is_none()
        );
        if failure.is_none() {
            assert_eq!(result["memory_commits"].as_array().unwrap().len(), 1);
            assert_eq!(message.content, result["answer"].as_str().unwrap());
            assert!(
                persisted.pi_session_cache.is_none(),
                "host-composed replies must rebuild canonical history"
            );
        } else {
            assert!(message.memory_update.is_none());
            let saved = serde_json::to_string(message).unwrap();
            assert!(!saved.contains("private provider body"));
            assert!(!saved.contains("private fixture commit detail"));
            assert!(!saved.contains("private fixture store detail"));
        }
        state.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn eval_journal_commits_include_optional_check_and_factual_outcome() {
    use crate::domain::{
        ChatMessage, ChatRole, MemoryUpdateChange, MemoryUpdateResult, MemoryUpdateStatus,
    };
    let root = tempfile::tempdir().unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let state = AppState::for_eval(root.path().join("app"));
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let chat = new_chat(&state, &loupist.id, "journal").unwrap();
    let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    store
        .open_turn(OpenTurn {
            loupist_id: loupist.id.clone(),
            chat_session_id: chat.id.clone(),
            turn_id: "prediction-turn".into(),
            at_ms: 1,
        })
        .unwrap();
    let request = SaveResearch {
        note_inputs: None,
        working_note: None,
        loupist_id: loupist.id.clone(),
        turn_id: "prediction-turn".into(),
        at_ms: 1,
        observations: "Evidence".into(),
        interpretation: String::new(),
        assumptions: String::new(),
        evaluation: String::new(),
        journal: Some(JournalDraft {
            thesis: None,
            check: Some(crate::knowledge::journal::fixture_prediction()),
            outcome: None,
            judgment: "Prediction".into(),
            reasoning: "Evidence".into(),

            source_receipt_ids: vec![],
            world_delivery_ids: vec![],
            lenses: vec![],
            earlier: vec![],

            entities: Vec::new(),
        }),
    };
    let prior = store
        .save_research(request.clone())
        .unwrap()
        .journal
        .unwrap();
    let mut review = prior.clone();
    review.outcome = Some(crate::knowledge::JournalOutcome {
        observed: 12.0,
        observed_at: "1970-01-02".into(),
        result: "came_true".into(),
        note: None,
        reviewed_as_of: None,
    });
    {
        let connection = state.store.lock_loupist(&loupist.id).unwrap();
        connection
            .execute(
                "UPDATE knowledge_journals SET body_json=?1 WHERE journal_id=?2",
                rusqlite::params![serde_json::to_string(&review).unwrap(), review.journal_id],
            )
            .unwrap();
    }
    let changes = [&prior, &review]
        .iter()
        .map(|d| MemoryUpdateChange {
            outcomes: Vec::new(),
            world_job_id: None,
            world_status: None,
            knowledge: None,
            record_id: d.journal_id.clone(),
            kind: "Journal".into(),
            operation: "create".into(),
            title: d.judgment.clone(),
            lesson: String::new(),
            basis: String::new(),
            future_use: String::new(),
        })
        .collect();
    let message = ChatMessage {
        memory_update: Some(MemoryUpdateResult {
            status: MemoryUpdateStatus::Applied,
            commit_id: None,
            changes,
            skipped_kinds: Vec::new(),
            setup: Vec::new(),
        }),
        ..crate::test_fixtures::chat_message("review-message", ChatRole::Assistant, "Reviewed")
    };
    let mut result = trace::persisted(&message, &trace::PiTrace::default());
    trace::enrich_commits(&state, &chat, &message, &mut result).unwrap();
    let commits = result["memory_commits"].as_array().unwrap();
    assert_eq!(
        commits[0]["check"],
        serde_json::to_value(&prior.check).unwrap()
    );
    assert_eq!(
        commits[1]["outcome"],
        serde_json::to_value(&review.outcome).unwrap()
    );
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn eval_timeout_during_startup_awaits_owned_cleanup_before_shutdown() {
    let root = tempfile::Builder::new()
        .prefix("eval-startup-timeout-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    fs::write(root.path().join("hold-pi-startup"), "hold").unwrap();
    fs::write(root.path().join("startup-trace.jsonl"), "").unwrap();
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let chat = new_chat(&state, &loupist.id, "timeout").unwrap();
    let session = Session {
        id: "timeout".into(),
        role: "probe".into(),
        memory: "off".into(),
        turns: vec![],
    };
    let result = run_turn(
        &state,
        &chat,
        &session,
        &Turn {
            user: "TIMEOUT_EVAL".into(),
            as_of: None,
            ..Default::default()
        },
        "fixture",
        "medium",
        1000,
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "failed");
    assert_eq!(result["error"], "timeout");
    assert_eq!(state.run_coordinator.active_count(), 0);
    // Cancellation must reach the live startup process, not just drop its future.
    let trace = fs::read_to_string(root.path().join("startup-trace.jsonl")).unwrap();
    assert!(
        trace
            .lines()
            .any(|line| serde_json::from_str::<Value>(line).unwrap()["event"] == "abort"),
        "{trace}"
    );
    assert_eq!(
        fs::read_dir(&state.artifacts.process_lease_dir)
            .unwrap()
            .count(),
        0
    );
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(
        stored.messages.last().unwrap().status,
        crate::domain::ChatMessageStatus::Aborted
    );
    state.shutdown().await.unwrap();
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn chat_runtime_stalled_provider_after_tool_fails_and_drains_before_long_tool_retry() {
    let root = tempfile::Builder::new()
        .prefix("provider-idle-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    fs::write(root.path().join("startup-trace.jsonl"), "").unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let chat = new_chat(&state, &loupist.id, "provider waits").unwrap();
    for (mode, failed) in [("STALL_AFTER_TOOL_EVAL", true), ("LONG_TOOL_EVAL", false)] {
        let mut sink = ChatEventSink::new(|_| Ok(()));
        sink.provider_idle_timeout = Some(Duration::from_millis(300));
        let run_id = format!("idle-{}", uuid::Uuid::new_v4());
        send_chat(
            ChatSendRequest {
                run_id: run_id.clone(),
                loupist_id: loupist.id.clone(),
                thread_id: chat.id.clone(),
                prompt: format!("TEACH_EVAL {mode}"),
                use_memory: true,
                update_memory: true,
                as_of: None,
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: state
                    .model_catalog()
                    .unwrap()
                    .resolve("fixture")
                    .unwrap()
                    .run_controls
                    .into_iter()
                    .map(|c| (c.id, c.default_choice))
                    .collect(),
                mentioned_connection_ids: vec![],
                attachments: vec![],
                pins: vec![],
                selected_artifact: None,
            },
            sink,
            &state,
            None,
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(15), async {
            while state.run_coordinator.active_count() != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("provider watchdog and cleanup must finish without an eval timeout");
        let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
        let message = stored.messages.last().unwrap();
        assert_eq!(
            message.failure_kind,
            if failed {
                Some(crate::domain::ChatFailureKind::ProviderTimeout)
            } else {
                None
            },
            "{mode}"
        );
        assert_eq!(
            message.status,
            if failed {
                crate::domain::ChatMessageStatus::Error
            } else {
                crate::domain::ChatMessageStatus::Complete
            },
            "{mode}"
        );
        assert_eq!(message.memory_update.is_none(), failed, "{mode}");
        assert!(
            message
                .tool_trace
                .as_ref()
                .unwrap()
                .entries
                .iter()
                .any(|call| call.name == "lens_file"),
            "tool must have executed before the provider wait"
        );
        let records = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id)
            .unwrap()
            .read(|read| read.list_records(&loupist.id))
            .unwrap();
        assert_eq!(
            records
                .iter()
                .any(|record| record.title == "Hanbit diluted-share rule"),
            !failed
        );
        assert_eq!(
            fs::read_dir(&state.artifacts.process_lease_dir)
                .unwrap()
                .count(),
            0
        );
    }
    let trace: Vec<Value> = fs::read_to_string(root.path().join("startup-trace.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        trace
            .iter()
            .filter(|event| event["event"] == "prompt")
            .count(),
        2,
        "each explicit request executes once, without a hidden replay"
    );
    assert!(trace.iter().any(|event| event["event"] == "abort"));
    state.shutdown().await.unwrap();
}

#[test]
fn product_eval_budget_counts_cached_input_and_unknown_usage() {
    let budget = Budget {
        max_input_tokens_per_turn: 120,
        max_turns: 2,
    };
    let mut record = json!({"usage":{"input":90,"cache_read":30,"cache_write":0,"output":999}});
    assert!(budget.stop_reason(&record).is_none());
    record["usage"]["cache_read"] = json!(31);
    assert_eq!(budget.stop_reason(&record).unwrap()["observed"], 121);
    record["usage"]["cache_read"] = Value::Null;
    assert!(budget.stop_reason(&record).unwrap()["observed"].is_null());
    assert_eq!(
        budget.stop_reason(&record).unwrap()["error"],
        "budget_usage_unknown"
    );
    record["error"] = json!("provider_error");
    budget.enforce(&mut record).unwrap();
    assert_eq!(record["error"], "provider_error");
    assert_eq!(record["budget_stop"]["error"], "budget_usage_unknown");
    assert_eq!(record["status"], "failed");
}

/// A `lens_file` call whose text the commit would refuse is refused when it
/// is staged, so the answer completes and Memory never fails the reply.
#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn a_filing_the_text_policy_refuses_never_fails_the_answer() {
    let root = tempfile::Builder::new()
        .prefix("eval-test-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    scripted(&mut state, root.path());
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let chat = new_chat(&state, &loupist.id, "badlink").unwrap();
    let session = Session {
        id: "badlink".into(),
        role: "train".into(),
        memory: "remember".into(),
        turns: vec![],
    };
    let result = run_turn(
        &state,
        &chat,
        &session,
        &Turn {
            user: "BADLINK_EVAL: Hanbit supplies Northgate; remember it".into(),
            as_of: None,
            ..Default::default()
        },
        "fixture",
        "medium",
        600000,
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert!(
        result["memory_commits"].as_array().unwrap().is_empty(),
        "{result}"
    );
    let stored = state.store.get_chat(&chat.id).unwrap().unwrap();
    let answer = stored.messages.last().unwrap();
    assert_eq!(answer.status, crate::domain::ChatMessageStatus::Complete);
    assert!(answer.failure_kind.is_none());
    assert_eq!(answer.content, "Noted, without saving the link.");
}

/// A question nobody can answer in an eval must not hold the turn for the
/// wait limit: the waiting call resolves at once, unanswered.
#[test]
fn an_eval_turn_settles_a_waiting_question_at_once() {
    use crate::chat::progress::ChatProgressPatch;
    use crate::chat::questions::{self, ChatQuestion, ChatQuestionOption, QuestionState};
    use crate::commands::ChatStreamEvent;
    let option = |label: &str| ChatQuestionOption {
        label: label.into(),
        description: None,
    };
    let receiver = questions::register("eval-question-1", "loupist-eval", 2);
    let question = ChatQuestion {
        request_id: "eval-question-1".into(),
        item_id: None,
        question: "Which period?".into(),
        options: vec![option("Latest quarter"), option("Full year")],
        recommended: Some(0),
        state: QuestionState::Waiting,
        answer: None,
        requested_at_ms: 0,
    };
    settle_waiting_cards(
        "loupist-eval",
        &ChatStreamEvent::ProgressPatch {
            run_id: "eval-run".into(),
            patch: ChatProgressPatch {
                sequence: 1,
                upsert_items: Vec::new(),
                append_text: Vec::new(),
                remove_item_ids: Vec::new(),
                finished_at_ms: None,
                plan: None,
                approvals: None,
                questions: Some(vec![question]),
            },
        },
    );
    let mut receiver = receiver;
    assert_eq!(
        receiver.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Closed),
        "the ask_user call resolves unanswered"
    );
}

#[test]
fn lens_seed_contract_refuses_retired_fields_and_unknown_section_fields() {
    for field in [
        "markdown",
        "applicability",
        "assumptions",
        "procedure",
        "principles",
        "limitations",
    ] {
        let mut input = json!({"title":"A method"});
        input[field] = json!("This text must not disappear");
        assert!(
            serde_json::from_value::<LensSeed>(input).is_err(),
            "{field}"
        );
    }
    assert!(serde_json::from_value::<LensSeed>(json!({
        "title":"A method", "sections":[{"key":"steps", "heading":"Steps", "markdown":"Check cash", "body":"must not disappear"}]
    })).is_err());
}

#[test]
fn repository_lens_seeds_decode_with_their_current_page_bodies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../evals/agent");
    let mut repaired = 0;
    for folder in ["suites", "heldout"] {
        for entry in fs::read_dir(root.join(folder)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let suite: Suite = serde_json::from_slice(&fs::read(&path).unwrap())
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            for scenario in suite.scenarios {
                for page in scenario.seed.lens {
                    assert!(
                        !page.sections.is_empty(),
                        "{}: {}",
                        path.display(),
                        scenario.id
                    );
                    assert!(page
                        .sections
                        .iter()
                        .all(|section| !section.markdown.trim().is_empty()));
                    if scenario.id.starts_with("B-")
                        || ["P-C02", "T-007", "T-031"].contains(&scenario.id.as_str())
                    {
                        assert_eq!(page.sections.len(), 6, "{}", scenario.id);
                        assert!(!page.summary.is_empty());
                        repaired += 1;
                    }
                }
            }
        }
    }
    assert_eq!(repaired, 19);
}
