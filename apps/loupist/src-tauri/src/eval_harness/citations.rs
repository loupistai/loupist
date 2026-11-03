use super::*;

#[test]
fn text_attachment_fixtures_use_production_intake_limits() {
    let mut turn: Turn = serde_json::from_value(json!({
        "user":"Cite the attached note.",
        "attachments":[{"filename":"note.md", "media_type":"text/markdown", "text":"A supplied fact."}]
    }))
    .unwrap();
    let inputs = turn.attachment_inputs().unwrap();
    assert_eq!(inputs[0].filename, "note.md");
    assert_eq!(
        BASE64.decode(&inputs[0].data_base64).unwrap(),
        b"A supplied fact."
    );
    turn.attachments[0].filename = "../note.md".into();
    assert!(turn.attachment_inputs().is_err());
    turn.attachments[0].filename = "note.md".into();
    turn.attachments[0].media_type = "image/png".into();
    assert!(turn.attachment_inputs().is_err());
}

/// The live definitions also run without a model, through Chat attachment
/// intake, broker delivery acknowledgment, Source staging and answer commit.
#[cfg(all(target_os = "macos", target_arch = "aarch64", debug_assertions))]
#[tokio::test]
async fn attached_and_workspace_notes_keep_their_citation_through_source_save() {
    let suite: Suite = serde_json::from_str(include_str!(
        "../../../evals/agent/suites/citations-v1.json"
    ))
    .unwrap();
    for scenario in suite.scenarios {
        let root = tempfile::Builder::new()
            .prefix("eval-citations-")
            .tempdir_in("/tmp")
            .unwrap();
        let workspace = root.path().join("workspace");
        fs::create_dir(&workspace).unwrap();
        for (path, text) in &scenario.workspace_files {
            fs::write(workspace.join(path), text).unwrap();
        }
        let mut state = AppState::for_eval(root.path().join("app"));
        super::tests::scripted(&mut state, root.path());
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &workspace)
            .await
            .unwrap();
        enable_connections(&state, &loupist.id, &[]).unwrap();
        let chat = new_chat(&state, &loupist.id, &scenario.title).unwrap();
        let mut session = scenario.sessions.into_iter().next().unwrap();
        let mut turn = session.turns.remove(0);
        turn.user.push_str(" CITE_NOTE_EVAL");
        let result = run_turn(&state, &chat, &session, &turn, "fixture", "medium", 30_000)
            .await
            .unwrap();
        assert_eq!(result["status"], "completed", "{}: {result}", scenario.id);
        let calls = result["tool_calls"].as_array().unwrap();
        let capture = &calls
            .iter()
            .find(|call| call["name"] == "workspace_capture")
            .unwrap()["result"];
        let saved = &calls
            .iter()
            .find(|call| call["name"] == "source_save")
            .unwrap()["result"];
        let citation = capture["citation_url"].as_str().unwrap();
        assert!(citation.starts_with("#evidence-ev_"), "{capture}");
        assert!(!citation.contains("<<untrusted"));
        assert_eq!(saved["result_ref"], capture["result_ref"]);
        assert_eq!(saved["status"], "staged");
        assert_eq!(saved["durable"], false);
        assert!(result["answer"]
            .as_str()
            .unwrap()
            .contains(&format!("[18420]({citation})")));
        assert_eq!(result["number_checks"][0]["status"], "matched", "{result}");
        let commits = result["memory_commits"].as_array().unwrap();
        assert_eq!(
            commits
                .iter()
                .filter(|commit| commit["kind"] == "source")
                .count(),
            1
        );
        let persisted = state.store.get_chat(&chat.id).unwrap().unwrap();
        assert_eq!(
            persisted.messages[0].attachments.len(),
            turn.attachments.len()
        );
        assert_eq!(persisted.messages.last().unwrap().evidence_refs.len(), 1);
        state.shutdown().await.unwrap();
    }
}
