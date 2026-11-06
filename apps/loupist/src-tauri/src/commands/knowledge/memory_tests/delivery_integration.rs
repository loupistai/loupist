use super::*;
use crate::commands::tool_executor::{StagedResearch, StagedResearchJournal};

#[test]
fn finalized_learning_report_opens_canonical_sources_lens_and_journal() {
    let directory = tempfile::tempdir().unwrap();
    let state = AppState::for_test(directory.path().join("app"));
    state
        .store
        .create_loupist(&loupist_profile("loupist-a"))
        .unwrap();
    let original = chat("chat-1", "loupist-a");
    state.store.create_chat(&original).unwrap();
    let lens = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).file_section(
            "loupist-a",
            crate::knowledge::lens_page::PageKind::Method,
            "Compare original accounts",
            "steps",
            "Compare original accounts",
            &crate::knowledge::test_support::user_origin(),
            3,
        )
    })
    .unwrap();
    let (_, lens_delivery) =
        host_prepare_read(&state, "loupist-a", &empty_read(&lens.record_id)).unwrap();
    let research = StagedResearch {
        source_reads: Vec::new(),
        world_reads: Vec::new(),
        lens_reads: Vec::new(),
        previous_note_ids: Vec::new(),
        working_note: None,
        observations: "The original payload was delivered.".into(),
        interpretation: String::new(),
        assumptions: "Fixture data".into(),
        evaluation: String::new(),
        journal: Some(StagedResearchJournal {
            thesis: None,
            at_ms: 1,
            check: Some(crate::knowledge::journal::fixture_prediction()),
            outcome: None,
            judgment: "Keep the exact original account.".into(),
            reasoning: "The delivered material supports this review.".into(),

            earlier: Vec::new(),
            source_reads: vec!["result-1".into()],
            world_reads: Vec::new(),
            lens_reads: Vec::new(),

            entities: Vec::new(),
        }),
    };
    let source = staged("Unmodified delivered source bytes.");
    let mut provisional = original.clone();
    provisional.messages.push(message("turn-finalized", 20));
    provisional.updated_at_ms = 20;
    let blobs = knowledge_blobs(&state).unwrap();
    let mut emitted = None;
    let performance = crate::domain::ChatPerformance {
        setup_ms: 10,
        first_text_ms: Some(50),
        generation_ms: 90,
        total_ms: 140,
        session_cache: crate::domain::ChatSessionCacheStatus::Warm,
        input_tokens: 30,
        output_tokens: 20,
        cache_read_tokens: 100,
        cache_write_tokens: 0,
    };
    state
        .store
        .replace_chat_with(&original, &provisional, |tx| {
            let changes = persist_saved_turn(
                tx,
                &blobs,
                "loupist-a",
                "chat-1",
                "turn-finalized",
                20,
                &[],
                true,
                &[source],
                &[lens_delivery],
                Some(&research),
            )
            .map_err(|error| StoreError::Invalid(error.to_string()))?;
            emitted = Some(write_chat_learning_report(
                tx,
                &provisional,
                "turn-finalized",
                true,
                changes,
                Vec::new(),
                Vec::new(),
                Some(performance.clone()),
                Vec::new(),
            )?);
            Ok(())
        })
        .unwrap();
    let emitted = emitted.unwrap();
    let persisted = state.store.get_chat("chat-1").unwrap().unwrap();
    assert_eq!(emitted.performance.as_ref(), Some(&performance));
    assert_eq!(
        serde_json::to_value(persisted.messages.last().unwrap()).unwrap(),
        serde_json::to_value(&emitted).unwrap()
    );
    let report = emitted.memory_update.unwrap();
    assert_eq!(report.status, crate::domain::MemoryUpdateStatus::Applied);
    assert_eq!(report.changes.len(), 2);
    let change = |kind: &str| {
        report
            .changes
            .iter()
            .find(|change| change.kind == kind)
            .unwrap()
    };

    let source_change = change("Source");
    let source_ref = source_change.knowledge.as_ref().unwrap();
    let read = source_read(
        &state,
        KnowledgeSourceReadRequest {
            loupist_id: "loupist-a".into(),
            record_id: source_ref.record_id.clone(),
            revision_id: source_ref.revision_id.clone(),
            source_id: Some(source_ref.object_id.clone()),
            span_id: source_ref.span_id.clone(),
            purpose: "renderer_inspection".into(),
            world_record_id: None,
            world_revision_id: None,
        },
    )
    .unwrap();
    assert_eq!(source_change.title, read.title);
    assert_eq!(
        read.spans
            .iter()
            .find(|span| Some(&span.span_id) == source_ref.span_id.as_ref())
            .unwrap()
            .text
            .as_deref(),
        Some("Unmodified delivered source bytes.")
    );

    let journal_change = change("Journal");
    let journal_ref = journal_change.knowledge.as_ref().unwrap();
    let journal = journal_read(
        &state,
        KnowledgeJournalReadRequest {
            loupist_id: "loupist-a".into(),
            journal_id: journal_ref.object_id.clone(),
        },
    )
    .unwrap();
    assert_eq!(journal_change.title, journal.judgment);
    let retained = journal_source_read(
        &state,
        KnowledgeJournalSourceReadRequest {
            loupist_id: "loupist-a".into(),
            journal_id: journal.journal_id,
            receipt_id: journal.inputs.sources[0].receipt_id.clone(),
        },
    )
    .unwrap();
    assert_eq!(
        retained.text.as_deref(),
        Some("Unmodified delivered source bytes.")
    );
}
