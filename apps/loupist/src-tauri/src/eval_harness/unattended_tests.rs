use super::*;

const SCHEDULED: &str = include_str!("../../../evals/agent/suites/scheduled-language-v1.json");

#[test]
fn scheduled_fixture_rejects_conflicting_flags_and_identity() {
    let raw: Value = serde_json::from_str(SCHEDULED).unwrap();
    let scenario = &raw["scenarios"][0];
    let validate = |input: Value| -> Result<()> {
        let parsed: Scenario = serde_json::from_value(input)?;
        let session = &parsed.sessions[0];
        scheduled::validate(&parsed, session, &session.turns[0])
    };
    assert!(validate(scenario.clone()).is_ok());
    for flags in [
        json!({"briefing":true}),
        json!({"as_of":"2026-11-18"}),
        json!({"attachments":[{"filename":"sample.txt","media_type":"text/plain","text":"Text"}]}),
        json!({"scheduled":null}),
        json!({"scheduled":{"kind":"earnings"}}),
        json!({"scheduled":{"kind":"briefing","run_existing":true}}),
        json!({"scheduled":{"kind":"custom","manual":true}}),
    ] {
        let mut invalid = scenario.clone();
        invalid["sessions"][0]["turns"][0]
            .as_object_mut()
            .unwrap()
            .extend(flags.as_object().unwrap().clone());
        assert!(validate(invalid).is_err(), "{flags}");
    }
    for (field, value) in [
        ("id", "research"),
        ("memory", "read_only"),
        ("role", "train"),
    ] {
        let mut invalid = scenario.clone();
        invalid["sessions"][0][field] = json!(value);
        assert!(validate(invalid).is_err(), "{field}");
    }
    let mut invalid = scenario.clone();
    invalid["seed"] = json!({"watchlist":[{"name":"Test","reference":{"exchange":"KRX"}}]});
    assert!(validate(invalid).is_err());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
mod runtime {
    use super::*;
    use std::io::BufWriter;

    async fn scheduled_case(scenario: &Scenario, stall: bool) -> Value {
        let session = &scenario.sessions[0];
        let turn = &session.turns[0];
        scheduled::validate(scenario, session, turn).unwrap();
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        super::super::tests::scripted(&mut state, root.path());
        let language = scenario.language.as_deref().unwrap();
        fs::write(
            state.artifacts.app_data_dir.join("config.toml"),
            format!("[appearance]\nlanguage = {language:?}\ndisplay_currency = \"USD\"\n"),
        )
        .unwrap();
        let en = "Three million credits remained after June equipment spending.";
        let ko = "6월 설비 지출 후 삼백만 크레디트가 남았습니다.";
        let brief = scenario.id.contains("-BRIEF-");
        let first = match (brief, language, scenario.id.contains("-OVERRIDE-")) {
            (true, "en", _) => "There are no new findings in the workspace.",
            (true, _, _) => "작업 공간에서 새로 확인한 내용은 없습니다.",
            (false, "en", false) | (false, "ko", true) => en,
            _ => ko,
        };
        let full = format!("{first}\n\nFixture detail stays in the scheduled session.");
        let mut script = json!({"prompt":turn.user,"answer":full,"stall":stall});
        if brief {
            script["prompt"] = json!(crate::briefings::PROMPT);
            script["briefing_language"] = json!(language);
        }
        fs::write(
            root.path().join("scheduled-script.json"),
            script.to_string(),
        )
        .unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let home = crate::chat::home::ensure_home_thread(&state, &loupist.id).unwrap();
        let mut record = run_turn(
            &state,
            &home,
            session,
            turn,
            "fixture",
            "medium",
            if stall { 3_000 } else { 60_000 },
        )
        .await
        .unwrap();
        state.shutdown().await.unwrap();
        let received: Vec<String> = fs::read_to_string(root.path().join("scheduled-prompts.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0], record["admission"]["prompt"]);
        assert_eq!(record["admission"]["surface"], "scheduled", "{record}");
        assert_eq!(record["resolved_appearance"], language);
        assert_eq!(record["display_currency"], "USD");
        assert_eq!(record["runs"].as_array().unwrap().len(), 1);
        if stall {
            assert_eq!(record["status"], "failed", "{record}");
            assert_eq!(record["error"], "timeout", "{record}");
            assert_eq!(record["terminal_event"]["status"], "aborted", "{record}");
            assert_eq!(record["runs"][0]["status"], "interrupted", "{record}");
            assert_eq!(record["deliveries"], json!([]));
        } else {
            assert_eq!(record["status"], "completed", "{record}");
            assert_eq!(record["interface_label_calls"], 0);
            assert_eq!(record["runs"][0]["status"], "succeeded", "{record}");
            assert_eq!(record["deliveries"].as_array().unwrap().len(), 1);
            assert_eq!(record["answer"], first);
            assert_eq!(record["session_answer"]["content"], full);
            assert_eq!(record["deliveries"][0]["content"], first);
            assert_eq!(
                record["deliveries"][0]["run_id"],
                record["admission"]["run_id"]
            );
            if !brief {
                assert_eq!(received[0], turn.user);
            }
        }
        for (key, value) in json!({
            "schema":"loupist-agent-trace/1","run_id":"scripted-scheduled-fixture","suite":"scheduled-language",
            "scenario_id":scenario.id,"session_id":session.id,"session_role":session.role,
            "memory_mode":session.memory,"user":turn.user,"turn_index":0,"trial":0
        }).as_object().unwrap() {
            record[key] = value.clone();
        }
        record
    }

    #[tokio::test]
    async fn scheduled_language_reaches_real_bot_delivery_in_both_appearances() {
        let suite: Suite = serde_json::from_str(SCHEDULED).unwrap();
        let mut records = Vec::new();
        for scenario in &suite.scenarios {
            records.push(scheduled_case(scenario, false).await);
        }
        if let Some(path) = std::env::var_os("LOUPIST_SCHEDULED_PROOF") {
            let mut writer = BufWriter::new(fs::File::create_new(path).unwrap());
            for record in records {
                serde_json::to_writer(&mut writer, &record).unwrap();
                writeln!(writer).unwrap();
            }
            writer.flush().unwrap();
        }
    }

    #[tokio::test]
    async fn scripted_presets_report_first_then_only_when_met() {
        use crate::scheduled_tasks::{
            presets,
            types::{
                TaskDefinition, TaskDraft, TaskKind, TaskStatus, TaskSubject, TaskTarget,
                WatchPreset,
            },
        };
        let suite: Suite = serde_json::from_str(SCHEDULED).unwrap();
        let session = &suite.scenarios[1].sessions[0];
        let turn = &session.turns[0];
        let existing =
            serde_json::from_value(json!({"kind":"custom","run_existing":true})).unwrap();
        for preset in [
            WatchPreset::FilingsAndResults,
            WatchPreset::ThesisLines,
            WatchPreset::ResultsFollowUp,
        ] {
            let root = tempfile::tempdir_in("/tmp").unwrap();
            let workspace = root.path().join("workspace");
            fs::create_dir(&workspace).unwrap();
            let mut state = AppState::for_eval(root.path().join("app"));
            super::super::tests::scripted(&mut state, root.path());
            let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
                .await
                .unwrap();
            let home = crate::chat::home::ensure_home_thread(&state, &loupist.id).unwrap();
            let once = preset == WatchPreset::ResultsFollowUp;
            let filled = presets::fill(
                &state,
                &loupist.id,
                preset,
                Some(vec![TaskSubject::Name {
                    name: "Northwind Loom".into(),
                }]),
                presets::PresetParams {
                    language: Some("en".into()),
                    results_date: once.then(|| "2099-11-15".into()),
                },
                None,
            )
            .unwrap();
            let prompt = filled.prompt.clone();
            state
                .store
                .scheduled_task_create_definition(
                    &loupist.id,
                    "fixture-watch",
                    TaskDraft {
                        title: filled.title,
                        prompt: filled.prompt,
                        schedule: filled.schedule,
                        target: TaskTarget::NewChat,
                        model_profile_id: "fixture".into(),
                        thinking_level: "medium".into(),
                        run_options: Default::default(),
                        use_memory: true,
                        update_memory: true,
                        status: TaskStatus::Active,
                    },
                    TaskKind::Custom,
                    TaskDefinition {
                        watch: filled.watch,
                        ..Default::default()
                    },
                    crate::commands::now_ms(),
                )
                .unwrap();
            let cases = if once { 1 } else { 3 };
            for i in 0..cases {
                let quiet = i == 1;
                let mut script = json!({
                    "prompt": prompt,
                    "answer": if quiet { "" } else { "Checked the available reports for Northwind Loom." },
                    "previous_run": i > 0,
                });
                if i < 2 {
                    script["quiet_result"] = json!(quiet);
                }
                fs::write(
                    root.path().join("scheduled-script.json"),
                    script.to_string(),
                )
                .unwrap();
                let record =
                    scheduled::run(&state, &home, turn, &existing, "fixture", "medium", 60_000)
                        .await
                        .unwrap();
                assert_eq!(record["status"], "completed", "{record}");
                assert_eq!(record["runs"][0]["quiet"], quiet, "{record}");
                assert_eq!(
                    record["deliveries"].as_array().unwrap().len(),
                    usize::from(!quiet),
                    "{record}"
                );
                let saved = state.store.get_chat(&home.id).unwrap().unwrap();
                assert!(
                    saved
                        .messages
                        .iter()
                        .filter_map(|message| message.delivery.as_ref())
                        .next()
                        .unwrap()
                        .first_run
                );
            }
            state.shutdown().await.unwrap();
        }
    }

    #[tokio::test]
    async fn scheduled_timeout_aborts_one_run_without_a_delivery() {
        let suite: Suite = serde_json::from_str(SCHEDULED).unwrap();
        scheduled_case(&suite.scenarios[1], true).await;
    }

    /// Without an admission there is nothing to compare: the record names the
    /// startup failure and its kind, so selection can drop it and `--resume`
    /// reruns it, instead of reporting an identity mismatch.
    #[tokio::test]
    async fn a_scheduled_run_whose_pi_never_started_reports_that_failure() {
        let suite: Suite = serde_json::from_str(SCHEDULED).unwrap();
        let scenario = &suite.scenarios[1];
        let session = &scenario.sessions[0];
        let turn = &session.turns[0];
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        super::super::tests::scripted(&mut state, root.path());
        fs::write(root.path().join("exit-pi-startup"), "exit").unwrap();
        fs::write(
            state.artifacts.app_data_dir.join("config.toml"),
            format!(
                "[appearance]\nlanguage = {:?}\n",
                scenario.language.as_deref().unwrap()
            ),
        )
        .unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let home = crate::chat::home::ensure_home_thread(&state, &loupist.id).unwrap();
        let record = run_turn(&state, &home, session, turn, "fixture", "medium", 60_000)
            .await
            .unwrap();
        state.shutdown().await.unwrap();
        assert_eq!(record["status"], "failed", "{record}");
        assert_eq!(record["runs"][0]["status"], "failed", "{record}");
        assert!(record["admission"].is_null(), "{record}");
        assert_eq!(record["error"], "process_stopped_before_prompt", "{record}");
        assert_eq!(record["failureKind"], "pi_exited", "{record}");
        assert_eq!(record["startup_failed"], true, "{record}");
    }

    #[tokio::test]
    async fn scripted_handoff_conclusion_is_read_in_bot_with_memory_off() {
        use crate::chat::handoff::{
            self, HandoffRequest, HandoffRun, HandoffStatus, StagedHandoff,
        };

        use crate::commands::types::ChatStreamEvent;

        let suite: Suite =
            serde_json::from_str(include_str!("../../../evals/agent/suites/handoff-v1.json"))
                .unwrap();
        let scenario = &suite.scenarios[0];
        let session = &scenario.sessions[0];
        let turn = &session.turns[0];
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        super::super::tests::scripted(&mut state, root.path());
        fs::write(
            root.path().join("fixture.py"),
            include_str!("handoff_read_pi.py"),
        )
        .unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let staged = StagedHandoff {
            request: HandoffRequest::parse(&json!({
                "title":"Alder Rail capacity",
                "question":"Can the revised timetable cover the maintenance window?",
                "requestKey":"alder-capacity"
            }))
            .unwrap(),
            run: HandoffRun {
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: Default::default(),
                use_memory: false,
                update_memory: false,
            },
        };
        let mut handoff = handoff::commit_standalone(&state, &loupist.id, &staged).unwrap();
        let card_id = "handoff-fixture-alder";
        let original = std::mem::replace(&mut handoff.id, card_id.into());
        state
            .store
            .lock_loupist(&loupist.id)
            .unwrap()
            .execute(
                "UPDATE chat_handoffs SET id=?1, data_json=?2 WHERE loupist_id=?3 AND id=?4",
                rusqlite::params![
                    card_id,
                    serde_json::to_string(&handoff).unwrap(),
                    loupist.id,
                    original
                ],
            )
            .unwrap();
        let run_id = "eval-handoff-child";
        let (admission, prompt) = handoff::claim_fixture(&state, &handoff, run_id);
        let (lifetime, mut settled) = tokio::sync::watch::channel(());
        let (finish_state, owner) = (state.clone(), loupist.id.clone());
        let sink = ChatEventSink::new(move |event| {
            let _ = &lifetime;
            if let ChatStreamEvent::Completed { message_id, .. } = event {
                handoff::finish_done_fixture(&finish_state, &owner, card_id, Some(message_id))
                    .unwrap();
            }
            Ok(())
        });
        send_chat(
            ChatSendRequest {
                run_id: run_id.into(),
                loupist_id: loupist.id.clone(),
                thread_id: handoff.chat_id.clone(),
                prompt,
                use_memory: false,
                update_memory: false,
                as_of: None,
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: Default::default(),
                mentioned_connection_ids: vec![],
                attachments: vec![],
                pins: vec![],
                selected_artifact: None,
            },
            sink,
            &state,
            Some(admission),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(20), settled.changed())
            .await
            .unwrap()
            .unwrap_err();
        let child = state.store.get_chat(&handoff.chat_id).unwrap().unwrap();
        let answer = child.messages.last().unwrap();
        assert_eq!(answer.status, crate::domain::ChatMessageStatus::Complete);
        assert!(answer.content.contains("Only the northern spur"));
        let finished = handoff::list(&state, &loupist.id)
            .unwrap()
            .into_iter()
            .find(|row| row.id == card_id)
            .unwrap();
        assert_eq!(finished.status, HandoffStatus::Done);
        assert_eq!(finished.message_id.as_deref(), Some(answer.id.as_str()));
        let home = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                crate::chat::home::deliver_finished_runs(&state, &loupist.id).unwrap();
                let home = crate::chat::home::ensure_home_thread(&state, &loupist.id).unwrap();
                if let Some(message) = home.messages.iter().find(|message| {
                    message.handoff.as_ref().is_some_and(|card| {
                        card.handoff_id == card_id && card.status == HandoffStatus::Done
                    })
                }) {
                    assert!(!message.content.contains("northern spur"));
                    break home;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("finished hand-off did not reach Bot history");
        let mut record = run_turn(&state, &home, session, turn, "fixture", "medium", 20_000)
            .await
            .unwrap();
        state.shutdown().await.unwrap();
        assert_eq!(record["status"], "completed", "{record}");
        assert_eq!(record["admission"]["surface"], "bot");
        assert_eq!(session.memory, "off");
        assert!(record["answer"]
            .as_str()
            .unwrap()
            .contains("Alder Rail capacity research concluded: Only the northern spur"));
        assert!(
            record["tool_calls"].as_array().unwrap().iter().any(|call| {
                call["name"] == "chat_history"
                    && call["args"]["card_id"] == card_id
                    && call["outcome"] == "ok"
            }),
            "{record}"
        );
        for (key, value) in json!({
            "schema":"loupist-agent-trace/1","run_id":"scripted-handoff-fixture","suite":"handoff",
            "scenario_id":scenario.id,"session_id":session.id,"session_role":session.role,
            "memory_mode":session.memory,"user":turn.user,"turn_index":0,"trial":0
        })
        .as_object()
        .unwrap()
        {
            record[key] = value.clone();
        }
        if let Some(path) = std::env::var_os("LOUPIST_HANDOFF_PROOF") {
            fs::write(path, format!("{record}\n")).unwrap();
        }
    }

    #[tokio::test]
    async fn delegated_pre_admission_projects_the_handoff_surface() {
        use crate::chat::handoff::{self, HandoffRequest, HandoffRun, StagedHandoff};

        let root = tempfile::tempdir_in("/tmp").unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        let mut state = AppState::for_eval(root.path().join("app"));
        super::super::tests::scripted(&mut state, root.path());
        fs::write(
            root.path().join("fixture.py"),
            include_str!("../commands/tests/fixtures/research_open_pi.py"),
        )
        .unwrap();
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        let staged = StagedHandoff {
            request: HandoffRequest::parse(&json!({
                "title":"Eval hand-off","question":"Who else can supply HBM4 in 2027?",
                "brief":"Check capacity announcements. Write the update in Korean.",
                "requestKey":"eval-handoff",
            }))
            .unwrap(),
            run: HandoffRun {
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: Default::default(),
                use_memory: false,
                update_memory: false,
            },
        };
        let handoff = handoff::commit_standalone(&state, &loupist.id, &staged).unwrap();
        let run_id = "eval-delegated";
        let (admission, prompt) = handoff::claim_fixture(&state, &handoff, run_id);
        let capture = Arc::new(Mutex::new(observation::TurnObservation::default()));
        let observer = capture.clone();
        let (lifetime, mut settled) = tokio::sync::watch::channel(());
        let mut sink = ChatEventSink::new(|_| Ok(()));
        sink.eval_pi_event = Some(Arc::new(move |event| {
            let _ = &lifetime;
            observer.lock().unwrap().observe(event);
        }));
        send_chat(
            ChatSendRequest {
                run_id: run_id.into(),
                loupist_id: loupist.id.clone(),
                thread_id: handoff.chat_id.clone(),
                prompt,
                use_memory: false,
                update_memory: false,
                as_of: None,
                model_profile_id: "fixture".into(),
                thinking_level: "medium".into(),
                run_options: Default::default(),
                mentioned_connection_ids: vec![],
                attachments: vec![],
                pins: vec![],
                selected_artifact: None,
            },
            sink,
            &state,
            Some(admission),
        )
        .await
        .unwrap();
        tokio::time::timeout(Duration::from_secs(20), settled.changed())
            .await
            .unwrap()
            .unwrap_err();
        state.shutdown().await.unwrap();

        let mut record = json!({});
        capture.lock().unwrap().write_projection(&mut record);
        assert_eq!(record["admission"]["surface"], "delegated", "{record}");
        assert_eq!(record["admission"]["prompt"], staged.request.question);
        assert_eq!(record["terminal_event"]["status"], "completed");
        let call: Value = serde_json::from_str(
            fs::read_to_string(root.path().join("research-open-calls.jsonl"))
                .unwrap()
                .trim(),
        )
        .unwrap();
        assert!(call["prompt"]
            .as_str()
            .unwrap()
            .contains(&staged.request.question));
        assert_eq!(call["result"]["ok"], false);
        assert!(call["result"]["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unattended"));
    }
}
