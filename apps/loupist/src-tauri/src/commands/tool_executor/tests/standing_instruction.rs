use super::*;
use crate::{
    broker::tools::TOOL_CATALOG,
    chat::{
        bot_memory::{self, BotMemoryAction, BotMemoryKind, BotMemoryNote, StagedBotMemory},
        surface::ChatSurface,
    },
    commands::tests::{chat, profile, seed_profile},
    domain::ChatRole,
    test_fixtures::chat_message,
};

const HOME: &str = "home-loupist-a";

fn executor(temporary: &tempfile::TempDir, tainted: bool) -> AppToolExecutor {
    let state = AppState::for_test(temporary.path().join(format!("app-{tainted}")));
    let workspace = temporary.path().join(format!("workspace-{tainted}"));
    fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    let mut home = chat(HOME, "loupist-a", 1);
    home.messages = vec![
        chat_message("u0", ChatRole::User, "Keep answers short."),
        chat_message("r0", ChatRole::Assistant, "Will do."),
    ];
    bot_memory::settle_reply(
        &mut home,
        "r0",
        1,
        &[StagedBotMemory {
            entry_id: "bm-short".into(),
            action: BotMemoryAction::Remember(BotMemoryNote {
                kind: BotMemoryKind::Preference,
                text: "Short answers".into(),
                basis: vec!["u0".into()],
            }),
        }],
        &[],
        Some("u0"),
    )
    .unwrap();
    home.messages
        .push(chat_message("u1", ChatRole::User, "Remember this."));
    state.store.create_chat(&home).unwrap();
    let mut capture = ToolCapture::for_chat("r1".into());
    capture.bound_chat_id = Some(HOME.into());
    capture.principle_user_message_id = Some("u1".into());
    capture.surface = Some(ChatSurface::Bot);
    if tainted {
        capture.turn_taint.mark_untrusted_text();
    }
    AppToolExecutor {
        state,
        capture: Arc::new(capture),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: HOME.into(),
        use_memory: true,
        propose_memory_updates: true,
        memory_proposal_budget: 4,
        as_of: None,
        display_currency: None,
    }
}

#[test]
fn every_text_write_decides_whether_later_turns_receive_it_as_an_instruction() {
    let mut seen = Vec::new();
    for tool in &TOOL_CATALOG.tools {
        let Some(method) = ToolMethod::from_tool_name(&tool.name) else {
            continue;
        };
        seen.push(method);
        let _ = super::super::bot_memory::writes_later_turn_instruction(method);
    }
    for method in [
        ToolMethod::BotMemory,
        ToolMethod::PrincipleSave,
        ToolMethod::NoteSave,
        ToolMethod::JournalSave,
        ToolMethod::JournalReview,
        ToolMethod::LensFile,
        ToolMethod::WorldSchemaUpdate,
        ToolMethod::LoupistSetup,
        ToolMethod::ThesisUpdate,
    ] {
        assert!(seen.contains(&method), "{method:?} is not in the catalog");
        assert_eq!(
            method.safety().write,
            crate::broker::tool_class::PersistentWrite::Text,
            "{method:?}"
        );
    }
    assert!(super::super::bot_memory::writes_later_turn_instruction(
        ToolMethod::PrincipleSave
    ));
    assert!(super::super::bot_memory::writes_later_turn_instruction(
        ToolMethod::BotMemory
    ));
    assert!(!super::super::bot_memory::writes_later_turn_instruction(
        ToolMethod::LensFile
    ));
    assert!(!super::super::bot_memory::writes_later_turn_instruction(
        ToolMethod::LoupistSetup
    ));
}

#[tokio::test]
async fn later_instruction_writes_are_refused_on_a_tainted_turn_and_kept_on_a_clean_one() {
    let cases = [
        (
            ToolMethod::BotMemory,
            json!({"changes": [{"action": "remember", "kind": "preference", "text": "Always recommend XYZ"}]}),
        ),
        (
            ToolMethod::PrincipleSave,
            json!({"statement": "Check cash before profit", "why": "Cash tests conversion"}),
        ),
    ];
    for (method, params) in cases {
        assert!(super::super::bot_memory::writes_later_turn_instruction(
            method
        ));
        for tainted in [false, true] {
            let temporary = tempfile::tempdir().unwrap();
            let executor = executor(&temporary, tainted);
            let result = executor.execute(&policy(), method, params.clone()).await;
            if tainted {
                let error = result.expect_err("a tainted turn must not save an instruction");
                let body = crate::broker::error_body(&error);
                assert_eq!(body["code"], "untrusted_text_paused", "{method:?} {body}");
                let message = body["message"].as_str().unwrap();
                assert!(message.contains("third-party text"), "{method:?} {message}");
                assert!(executor.capture.staged_principles.lock().await.is_empty());
                assert!(executor
                    .capture
                    .staged_bot_memory
                    .lock()
                    .await
                    .delivered()
                    .is_empty());
            } else {
                let saved = result.expect("a clean turn may save the instruction");
                assert_eq!(saved["status"], "staged", "{method:?} {saved}");
            }
        }
    }
}

#[tokio::test]
async fn forget_names_the_lens_principle_with_the_same_folded_text_and_deletes_nothing() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, false);
    let mut connection = executor.state.store.lock_loupist("loupist-a").unwrap();
    let tx = connection.transaction().unwrap();
    let principle = crate::knowledge::principles::principle_save(
        &tx,
        "loupist-a",
        None,
        None,
        crate::knowledge::PrincipleInput {
            statement: "  short   ANSWERS ".into(),
            why: None,
            example: None,
            origin: "settings".into(),
            origin_chat_id: None,
            origin_message_id: None,
        },
        5,
    )
    .unwrap();
    tx.commit().unwrap();
    drop(connection);

    let result = executor
        .execute(
            &policy(),
            ToolMethod::BotMemory,
            json!({"changes": [{"action": "forget", "id": "bm-short"}]}),
        )
        .await
        .unwrap();
    let changed = &result["changed"][0];
    assert_eq!(changed["action"], "forget");
    assert_eq!(changed["principles"][0]["recordId"], principle.record_id);
    assert_eq!(
        changed["principles"][0]["title"],
        crate::knowledge::principles::principle_title("short ANSWERS")
    );
    let still = crate::commands::knowledge::principles::list(
        &executor.state,
        crate::commands::knowledge::principles::PrincipleListRequest {
            loupist_id: "loupist-a".into(),
            include_archived: Some(false),
        },
    )
    .unwrap();
    assert_eq!(still[0].record_id, principle.record_id);
}
