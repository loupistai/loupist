//! The Memory-affecting calls from judgment-v1 B-EN-008, with host-issued ids
//! substituted at runtime. Trace section keys were redacted; only their slugs
//! are reconstructed. No model or external connection participates.
use super::*;

async fn fixture() -> (tempfile::TempDir, AppState, ChatSession, Value, Session) {
    let root = tempfile::Builder::new()
        .prefix("eval-journal-")
        .tempdir_in("/tmp")
        .unwrap();
    let workspace = root.path().join("workspace");
    fs::create_dir(&workspace).unwrap();
    let mut state = AppState::for_eval(root.path().join("app"));
    tests::scripted(&mut state, root.path());
    let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
        .await
        .unwrap();
    let suite: Suite =
        serde_json::from_str(include_str!("../../../evals/agent/suites/judgment-v1.json")).unwrap();
    let scenario = suite
        .scenarios
        .into_iter()
        .find(|s| s.id == "B-EN-008")
        .unwrap();
    seed(&state, &loupist.id, &scenario.seed).unwrap();
    let store = KnowledgeStore::for_loupist(state.store.clone(), &loupist.id).unwrap();
    let lens = store
        .list_records(&loupist.id)
        .unwrap()
        .into_iter()
        .find(|r| r.title == "My research principles")
        .unwrap();
    let page = store
        .read(|r| r.read_page(&loupist.id, &lens.record_id))
        .unwrap();
    assert_eq!(page.body.to_body().page().unwrap().sections.len(), 6);
    let mut case: Value = serde_json::from_str(include_str!("judgment_commit_cases.json")).unwrap();
    case["calls"][0]["args"] = json!({"recordId": page.record_id, "revisionId":page.revision_id});
    let session = scenario.sessions.into_iter().next().unwrap();
    let chat = new_chat(&state, &loupist.id, "journal reproduction").unwrap();
    (root, state, chat, case, session)
}

#[tokio::test]
async fn traced_judgment_combination_refuses_bad_links_then_commits_journal_lens_and_note() {
    let (root, state, chat, mut case, session) = fixture().await;
    case["repair_entities"] = json!(true);
    fs::write(root.path().join("judgment-case.json"), case.to_string()).unwrap();
    let mut turn = session.turns.into_iter().next().unwrap();
    turn.user.push_str("\nJUDGMENT_COMMIT_EVAL");
    let result = run_turn(
        &state,
        &chat,
        &session_without_turns(),
        &turn,
        "fixture",
        "medium",
        30_000,
    )
    .await
    .unwrap();
    assert_eq!(result["status"], "completed", "{result}");
    assert!(result.get("memory_commit_failure").is_none(), "{result}");
    let calls = result["tool_calls"].as_array().unwrap();
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["name"] == "journal_save" && call["outcome"] == "error")
            .count(),
        1,
        "{result}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| call["name"] == "journal_save" && call["outcome"] == "ok")
            .count(),
        1,
        "{result}"
    );
    let store = KnowledgeStore::for_loupist(state.store.clone(), &chat.loupist_id).unwrap();
    let journal_id: String = state
        .store
        .lock_loupist(&chat.loupist_id)
        .unwrap()
        .query_row(
            "SELECT journal_id FROM knowledge_journals WHERE loupist_id=?1",
            [&chat.loupist_id],
            |row| row.get(0),
        )
        .unwrap();
    let journal = store.get_journal(&chat.loupist_id, &journal_id).unwrap();
    assert!(journal.entities.is_empty());
    assert_eq!(journal.inputs.lenses.len(), 1);
    assert!(journal.check.is_none());
    let notes = store
        .read(|r| r.working_notes(&chat.loupist_id, &chat.id, 10))
        .unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].inputs.as_ref().unwrap().lenses.len(), 1);
    let subject = store
        .list_records(&chat.loupist_id)
        .unwrap()
        .into_iter()
        .find(|r| r.title == "Hush Orchard")
        .unwrap();
    let page = store
        .read(|r| r.read_page(&chat.loupist_id, &subject.record_id))
        .unwrap();
    let body = page.body.to_body();
    assert_eq!(body.page().unwrap().sections.len(), 2);
    assert!(body
        .page()
        .unwrap()
        .sections
        .iter()
        .all(|section| section.basis.refs.len() == 1));
    // A later Journal+note save keeps the note's identity too: this rules out
    // stable research ids as the cause of the original new-note failure.
    let note_id = notes[0].research_id.clone();
    case["repair_entities"] = json!(false);
    case["calls"]
        .as_array_mut()
        .unwrap()
        .retain(|call| call["name"] != "lens_file");
    for call in case["calls"].as_array_mut().unwrap() {
        if call["name"] == "journal_save" {
            call["args"].as_object_mut().unwrap().remove("entities");
        }
        if call["name"] == "note_save" {
            call["args"]["workingNote"]["expectedResearchId"] = json!(note_id);
            call["args"]["workingNote"]["changeSummary"] =
                json!("Revisited the same user judgment.");
        }
    }
    fs::write(root.path().join("judgment-case.json"), case.to_string()).unwrap();
    let next = run_turn(
        &state,
        &chat,
        &session_without_turns(),
        &turn,
        "fixture",
        "medium",
        30_000,
    )
    .await
    .unwrap();
    assert_eq!(next["status"], "completed", "{next}");
    let updated = store
        .read(|r| r.working_notes(&chat.loupist_id, &chat.id, 10))
        .unwrap();
    assert_eq!(updated.len(), 1);
    assert_eq!(updated[0].research_id, note_id);
    assert_eq!(
        state
            .store
            .lock_loupist(&chat.loupist_id)
            .unwrap()
            .query_row("SELECT count(*) FROM knowledge_journals", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    state.shutdown().await.unwrap();
}

#[tokio::test]
async fn traced_journal_check_alone_commits_both_percent_unit_spellings() {
    for unit in ["%", "% of sales"] {
        let (root, state, chat, mut case, session) = fixture().await;
        let mut journal = case["check"].clone();
        journal["check"]["unit"] = json!(unit);
        // The original failing check had labels; it is refused and repaired.
        // The successful trace omitted them and uses '% of sales'.
        if unit == "%" {
            case["repair_entities"] = json!(true);
        } else {
            journal.as_object_mut().unwrap().remove("entities");
        }
        case["calls"] = json!([case["calls"][0], case["calls"][2], case["calls"][3],
            {"name":"journal_save", "method":"journal.save", "args":journal}]);
        fs::write(root.path().join("judgment-case.json"), case.to_string()).unwrap();
        let mut turn = session.turns.into_iter().nth(1).unwrap();
        turn.user.push_str("\nJUDGMENT_COMMIT_EVAL");
        let result = run_turn(
            &state,
            &chat,
            &session_without_turns(),
            &turn,
            "fixture",
            "medium",
            30_000,
        )
        .await
        .unwrap();
        assert_eq!(result["status"], "completed", "{result}");
        let journal = result["memory_commits"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "journal")
            .unwrap();
        assert_eq!(journal["check"]["threshold"], 60.0);
        assert_eq!(journal["check"]["confidence"], 70);
        assert_eq!(journal["check"]["unit"], unit);
        assert_eq!(journal["check"]["direction"], "below");
        assert_eq!(journal["check"]["byDate"], "2026-12-15");
        state.shutdown().await.unwrap();
    }
}

fn session_without_turns() -> Session {
    Session {
        id: "probe".into(),
        role: "probe".into(),
        memory: "remember".into(),
        turns: Vec::new(),
    }
}
