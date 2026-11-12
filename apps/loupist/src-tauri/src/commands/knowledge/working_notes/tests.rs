use super::*;
use crate::commands::tests::{chat, profile, seed_profile};
use crate::commands::tool_executor::StagedResearch;
use crate::knowledge::WorkingNoteDraft;

fn fixture() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::for_persistent_test(dir.path().join("app"));
    for (loupist, id) in [("loupist-a", "chat-a"), ("loupist-b", "chat-b")] {
        let workspace = dir.path().join(loupist);
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile(loupist, &workspace, 1));
        state.store.create_chat(&chat(id, loupist, 1)).unwrap();
    }
    (dir, state)
}

fn staged(markdown: &str, expected: Option<&str>) -> StagedResearch {
    StagedResearch {
        source_reads: Vec::new(),
        world_reads: Vec::new(),
        lens_reads: Vec::new(),
        previous_note_ids: Vec::new(),
        working_note: Some(WorkingNoteDraft {
            title: "Savings plan".into(),
            markdown: markdown.into(),
            change_summary: "Updated the monthly contribution assumption.".into(),
            expected_research_id: expected.map(str::to_string),
        }),
        observations: String::new(),
        interpretation: String::new(),
        assumptions: String::new(),
        evaluation: String::new(),
        journal: None,
    }
}

fn persist(
    state: &AppState,
    item: &StagedResearch,
    commit: bool,
    learning: bool,
) -> Result<(), CommandError> {
    let blobs = knowledge_blobs(state)?;
    let mut connection = state.store.lock().unwrap();
    let tx = connection.transaction().unwrap();
    crate::commands::knowledge::persist_saved_turn(
        &tx,
        &blobs,
        "loupist-a",
        "chat-a",
        &format!("turn_{}", uuid::Uuid::new_v4().simple()),
        crate::commands::now_ms(),
        &[],
        learning,
        &[],
        &[],
        Some(item),
    )?;
    if commit {
        tx.commit().unwrap();
    }
    Ok(())
}

fn page(state: &AppState) -> WorkingNotesPage {
    working_notes_page(
        state,
        WorkingNotesRequest {
            loupist_id: "loupist-a".into(),
            chat_id: "chat-a".into(),
        },
    )
    .unwrap()
}

#[test]
fn working_notes_replace_the_body_atomically_and_refuse_stale_or_foreign_access() {
    let (dir, state) = fixture();
    persist(
        &state,
        &staged("Monthly contribution: 500.", None),
        true,
        true,
    )
    .unwrap();
    let first = page(&state).current.unwrap();
    assert!(persist(&state, &staged("Stale change", None), true, true).is_err());
    let mut self_link = staged("Updated plan", Some(&first.research_id));
    self_link.previous_note_ids = vec![first.research_id.clone()];
    assert!(persist(&state, &self_link, true, true).is_err());
    let rolled_back = staged("Discard this attempt.", Some(&first.research_id));
    persist(&state, &rolled_back, false, true).unwrap();
    assert_eq!(page(&state).current.as_ref(), Some(&first));
    let revised = staged("Monthly contribution: 750.", Some(&first.research_id));
    persist(&state, &revised, true, true).unwrap();
    let second = page(&state).current.unwrap();
    assert_eq!(first.research_id, second.research_id);
    assert!(persist(&state, &staged("Stale creation", None), true, true).is_err());
    // Search and exact reads see only the current body, and only for its Loupist.
    let hits = crate::commands::knowledge::host_search(&state, "loupist-a", "Savings", "research")
        .unwrap();
    let hits = hits.as_array().unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["researchId"], second.research_id);
    assert_eq!(
        read_working_context(&state, "loupist-a", &first.research_id)
            .unwrap()
            .markdown,
        second.markdown
    );
    assert!(read_working_context(&state, "loupist-b", &second.research_id).is_err());
    assert!(working_notes_page(
        &state,
        WorkingNotesRequest {
            loupist_id: "loupist-b".into(),
            chat_id: "chat-a".into(),
        }
    )
    .is_err());

    state.close_for_restart_test();
    let state = AppState::for_persistent_test(dir.path().join("app"));
    assert_eq!(page(&state).current.as_ref(), Some(&second));
    state
        .store
        .delete_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    let retained = read_working_context(&state, "loupist-a", &second.research_id).unwrap();
    assert_eq!(retained.markdown, "Monthly contribution: 750.");
}

#[test]
fn working_notes_abort_and_memory_off_publish_nothing_and_context_is_gated() {
    let (_dir, state) = fixture();
    let item = staged("A plan that must not leak", None);
    persist(&state, &item, false, true).unwrap();
    persist(&state, &item, true, false).unwrap();
    assert!(page(&state).current.is_none());
    persist(&state, &item, true, true).unwrap();
    let note = page(&state).current.unwrap();
    let off = r#"{"live_time":{},"memory_protocol":{"active":false}}"#;
    assert_eq!(attach_working_context(off, Some(&note)), off);
    let absent = r#"{"live_time":{}}"#;
    assert_eq!(attach_working_context(absent, Some(&note)), absent);
    let enabled: serde_json::Value = serde_json::from_str(&attach_working_context(
        r#"{"memory_protocol":{"active":true}}"#,
        Some(&note),
    ))
    .unwrap();
    assert_eq!(
        enabled["memory_protocol"]["workingNote"]["current"]["researchId"],
        note.research_id
    );
}

#[test]
fn selected_private_notes_are_resolved_only_for_the_owning_loupist() {
    let (_dir, state) = fixture();
    let saved =
        crate::commands::knowledge::with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
            let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
            write.open_turn(crate::knowledge::OpenTurn {
                loupist_id: "loupist-a".into(),
                chat_session_id: "chat-a".into(),
                turn_id: "memory-selection-turn".into(),
                at_ms: 1,
            })?;
            write.save_research(crate::knowledge::SaveResearch {
                note_inputs: None,
                loupist_id: "loupist-a".into(),
                turn_id: "memory-selection-turn".into(),
                at_ms: 2,
                observations: "The user supplied a savings assumption.".into(),
                interpretation: String::new(),
                assumptions: String::new(),
                evaluation: String::new(),
                working_note: staged("Private plan", None).working_note,
                journal: Some(crate::knowledge::JournalDraft {
                    thesis: None,
                    check: Some(crate::knowledge::journal::fixture_prediction()),
                    outcome: None,
                    judgment: "Review the assumption later.".into(),
                    reasoning: "User plan".into(),

                    source_receipt_ids: vec![],
                    world_delivery_ids: vec![],
                    lenses: vec![],
                    earlier: vec![],

                    entities: Vec::new(),
                }),
            })
        })
        .unwrap();
    {
        let connection = state.store.lock().unwrap();
        connection.execute_batch("PRAGMA query_only=ON;").unwrap();
    }
    let active = r#"{"memory_protocol":{"active":true}}"#;
    let context: serde_json::Value =
        serde_json::from_str(&attach_journal_context(&state, "loupist-a", active).unwrap())
            .unwrap();
    assert_eq!(
        context["memory_protocol"]["dueJournals"][0]["journalId"],
        saved.journal.as_ref().unwrap().journal_id
    );
    let other: serde_json::Value =
        serde_json::from_str(&attach_journal_context(&state, "loupist-b", active).unwrap())
            .unwrap();
    assert_eq!(
        other["memory_protocol"]["dueJournals"],
        serde_json::json!([])
    );
    let off = r#"{"memory_protocol":{"active":false}}"#;
    assert_eq!(
        attach_journal_context(&state, "loupist-a", off).unwrap(),
        off
    );
    let absent = r#"{"live_time":{}}"#;
    assert_eq!(
        attach_journal_context(&state, "loupist-a", absent).unwrap(),
        absent
    );
    {
        let connection = state.store.lock().unwrap();
        connection.execute_batch("PRAGMA query_only=OFF;").unwrap();
    }

    let pins = vec![
        crate::commands::ChatKnowledgePin {
            selection: None,
            kind: "research".into(),
            record_id: None,
            revision_id: None,
            object_id: saved.research_id.clone(),
            span_id: None,
            label: "UNTRUSTED_RENDERER_LABEL".into(),
        },
        crate::commands::ChatKnowledgePin {
            selection: None,
            kind: "journal".into(),
            record_id: None,
            revision_id: None,
            object_id: saved.journal.as_ref().unwrap().journal_id.clone(),
            span_id: None,
            label: "UNTRUSTED_RENDERER_LABEL".into(),
        },
    ];
    let hints = crate::commands::knowledge::selected_reference_hints(&state, "loupist-a", &pins)
        .unwrap()
        .unwrap();
    assert_eq!(hints["items"][0]["read"]["noteId"], saved.research_id);
    assert_eq!(
        hints["items"][1]["read"]["journalId"],
        saved.journal.unwrap().journal_id
    );
    assert!(!hints.to_string().contains("UNTRUSTED_RENDERER_LABEL"));
    assert!(
        crate::commands::knowledge::selected_reference_hints(&state, "loupist-b", &pins)
            .unwrap()
            .is_none()
    );
}

#[test]
fn note_and_journal_preserve_distinct_evidence_at_their_combined_limit() {
    use crate::commands::tool_executor::{StagedResearchJournal, StagedSource};
    use std::collections::BTreeSet;

    let (_dir, state) = fixture();
    let sources: Vec<_> = (0..64)
        .map(|index| StagedSource {
            custom_origin: None,
            source_class: crate::knowledge::SourceClass::ToolResult,
            authorship: crate::knowledge::Authorship::Tool,
            title: format!("Filing {index}"),
            bytes: format!("Original filing evidence {index}").into_bytes(),
            media_type: "text/plain".into(),
            origin: format!("https://example.test/filing/{index}"),
            coverage: "included_full".into(),
            chat_id: "chat-a".into(),
            request_ref: format!("save-{index}"),
            result_ref: format!("read-{index}"),
            publication_time: None,
            provider_identity: "example-publisher".into(),
            document_id: format!("filing-{index}"),
            material_role: "original".into(),
        })
        .collect();
    let mut research = staged(
        "Working note with its own evidence and a specific judgment.",
        None,
    );
    research.source_reads = sources[..32]
        .iter()
        .map(|source| source.request_ref.clone())
        .collect();
    let journal_at_ms = chrono::DateTime::parse_from_rfc3339("2024-02-29T12:00:00Z")
        .unwrap()
        .timestamp_millis();
    let mut prediction = crate::knowledge::journal::fixture_prediction();
    prediction.by_date = Some("2034-02-28".into());
    research.journal = Some(StagedResearchJournal {
        thesis: None,
        at_ms: journal_at_ms,
        check: Some(prediction),
        outcome: None,
        judgment: "A judgment from the second group of sources.".into(),
        reasoning: "These sources address the journal.".into(),

        earlier: Vec::new(),
        source_reads: sources[32..]
            .iter()
            .map(|source| source.request_ref.clone())
            .collect(),
        world_reads: Vec::new(),
        lens_reads: Vec::new(),

        entities: Vec::new(),
    });
    let changes = {
        let blobs = knowledge_blobs(&state).unwrap();
        let mut connection = state.store.lock().unwrap();
        let tx = connection.transaction().unwrap();
        let changes = crate::commands::knowledge::persist_saved_turn(
            &tx,
            &blobs,
            "loupist-a",
            "chat-a",
            "distinct-note-journal-inputs",
            10,
            &[],
            true,
            &sources,
            &[],
            Some(&research),
        )
        .unwrap();
        tx.commit().unwrap();
        changes
    };
    let source_record = |index: usize| {
        changes
            .iter()
            .find(|change| change.title == format!("Filing {index}"))
            .unwrap()
            .knowledge
            .as_ref()
            .unwrap()
            .record_id
            .clone()
    };
    let expected_all: BTreeSet<_> = (0..64).map(source_record).collect();
    let expected_journal: BTreeSet<_> = (32..64).map(source_record).collect();
    let note = page(&state).current.unwrap();
    let frozen = note.inputs.unwrap();
    assert_eq!(
        frozen
            .sources
            .iter()
            .map(|source| source.record_id.clone())
            .collect::<BTreeSet<_>>(),
        expected_all
    );
    let journal_id = &changes
        .iter()
        .find(|change| change.kind == "Journal")
        .unwrap()
        .knowledge
        .as_ref()
        .unwrap()
        .object_id;
    let journal = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).get_journal("loupist-a", journal_id)
    })
    .unwrap();
    assert_eq!(
        journal
            .inputs
            .sources
            .iter()
            .map(|source| source.record_id.clone())
            .collect::<BTreeSet<_>>(),
        expected_journal
    );
}
