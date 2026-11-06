use super::*;
use crate::agent_harness::{attach_relevant_memory, turn_envelope_block};
use crate::broker::{ToolExecutor, ToolMethod, ToolPolicy};
use crate::commands::tests::{chat, profile, seed_profile};
use crate::commands::tool_executor::{AppToolExecutor, ToolCapture};
use crate::knowledge::{JournalDraft, KnowledgeWrite, OpenTurn, SaveResearch, WorkingNoteDraft};
use serde_json::{json, Value};
use std::sync::Arc;

fn fixture() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::for_test(dir.path().join("app"));
    for loupist in ["loupist-a", "loupist-b"] {
        let workspace = dir.path().join(loupist);
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile(loupist, &workspace, 1));
    }
    state
        .store
        .create_chat(&chat("probe", "loupist-a", 1))
        .unwrap();
    (dir, state)
}

fn method(state: &AppState, title: &str, body: &str, at_ms: i64) -> crate::knowledge::PageRead {
    with_loupist_knowledge(state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).file_section(
            "loupist-a",
            crate::knowledge::lens_page::PageKind::Method,
            title,
            "steps",
            body,
            &crate::knowledge::test_support::user_origin(),
            at_ms,
        )
    })
    .unwrap()
}

fn note(state: &AppState, loupist: &str, chat_id: &str, title: &str, body: &str) -> String {
    state.store.create_chat(&chat(chat_id, loupist, 1)).unwrap();
    with_loupist_knowledge(state, loupist, |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        write.open_turn(OpenTurn {
            loupist_id: loupist.into(),
            chat_session_id: chat_id.into(),
            turn_id: chat_id.into(),
            at_ms: 10,
        })?;
        Ok(write
            .save_research(SaveResearch {
                loupist_id: loupist.into(),
                turn_id: chat_id.into(),
                at_ms: 11,
                observations: String::new(),
                interpretation: String::new(),
                assumptions: String::new(),
                evaluation: String::new(),
                journal: None,
                note_inputs: None,
                working_note: Some(WorkingNoteDraft {
                    title: title.into(),
                    markdown: body.into(),
                    change_summary: "Saved user premise".into(),
                    expected_research_id: None,
                }),
            })?
            .research_id)
    })
    .unwrap()
}

fn journal(state: &AppState, chat_id: &str, conclusion: &str, at_ms: i64) {
    state
        .store
        .create_chat(&chat(chat_id, "loupist-a", 1))
        .unwrap();
    with_loupist_knowledge(state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        write.open_turn(OpenTurn {
            loupist_id: "loupist-a".into(),
            chat_session_id: chat_id.into(),
            turn_id: chat_id.into(),
            at_ms,
        })?;
        write.save_research(SaveResearch {
            loupist_id: "loupist-a".into(),
            turn_id: chat_id.into(),
            at_ms,
            observations: String::new(),
            interpretation: String::new(),
            assumptions: String::new(),
            evaluation: String::new(),
            journal: Some(JournalDraft {
                thesis: None,
                check: Some(crate::knowledge::journal::fixture_prediction()),
                outcome: None,
                judgment: conclusion.into(),
                reasoning: "Recorded so the page cannot hide a note.".into(),

                source_receipt_ids: Vec::new(),
                world_delivery_ids: Vec::new(),
                lenses: Vec::new(),
                earlier: Vec::new(),

                entities: Vec::new(),
            }),
            note_inputs: None,
            working_note: None,
        })?;
        Ok(())
    })
    .unwrap();
}

fn legacy_research(state: &AppState, chat_id: &str, observations: &str, at_ms: i64) -> String {
    state
        .store
        .create_chat(&chat(chat_id, "loupist-a", 1))
        .unwrap();
    with_loupist_knowledge(state, "loupist-a", |tx, blobs| {
        let write = KnowledgeWrite::bind(tx, blobs);
        write.open_turn(OpenTurn {
            loupist_id: "loupist-a".into(),
            chat_session_id: chat_id.into(),
            turn_id: chat_id.into(),
            at_ms,
        })?;
        Ok(write
            .save_research(SaveResearch {
                loupist_id: "loupist-a".into(),
                turn_id: chat_id.into(),
                at_ms,
                observations: observations.into(),
                interpretation: String::new(),
                assumptions: String::new(),
                evaluation: String::new(),
                journal: None,
                note_inputs: None,
                working_note: None,
            })?
            .research_id)
    })
    .unwrap()
}

fn envelope(context: &HostMemoryContext, enabled: bool) -> Value {
    let base = turn_envelope_block(
        chrono::Utc::now(),
        enabled,
        &[],
        None,
        None,
        None,
        None,
        &[],
    )
    .unwrap();
    serde_json::from_str(&attach_relevant_memory(&base, &context.relevant)).unwrap()
}

#[tokio::test]
async fn host_memory_relevant_old_method_and_cross_chat_note_are_executable_not_receipts() {
    let (_dir, state) = fixture();
    let saved = method(
        &state,
        "Acme liquidity covenant",
        "Stress liquidity against the Acme covenant before assessing solvency.",
        1,
    );
    for i in 0..LENS_INDEX_LINES {
        method(
            &state,
            &format!("Unrelated gardening {i}"),
            "Prune roses in spring.",
            (i as i64 + 2) * 1000,
        );
    }
    let note_id = note(
        &state,
        "loupist-a",
        "prior-chat",
        "Acme liquidity",
        "User supplied Acme liquidity covenant headroom of USD 250 million.",
    );
    let foreign_id = note(
        &state,
        "loupist-b",
        "foreign-chat",
        "Acme liquidity covenant",
        "Private Acme liquidity covenant headroom.",
    );
    let context =
        host_memory_context(&state, "loupist-a", "Evaluate Acme liquidity covenant").unwrap();
    assert!(!context
        .entries
        .iter()
        .any(|entry| entry.id == saved.record_id));
    let value = envelope(&context, true);
    let pointers = value["memory_protocol"]["relevant"].as_array().unwrap();
    assert_eq!(pointers.len(), 2);
    assert!(pointers.iter().any(|p| p["readArgs"]["noteId"] == note_id));
    assert!(!pointers
        .iter()
        .any(|p| p["readArgs"]["noteId"] == foreign_id));
    // Pointers say what to read; they are never citation receipts themselves.
    assert!(pointers
        .iter()
        .all(|pointer| pointer.get("readRef").is_none()));
    let capture = Arc::new(ToolCapture::default());
    let executor = AppToolExecutor {
        state: state.clone(),
        capture: capture.clone(),
        loupist_id: "loupist-a".into(),
        capability_ids: Default::default(),
    };
    let policy = ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "probe".into(),
        use_memory: true,
        propose_memory_updates: true,
        memory_proposal_budget: 1,
        as_of: None,
    };
    let mut save = json!({
        "workingNote": {"title":"Acme assessment", "markdown":"Apply the saved covenant check.", "changeSummary":"Assessment", "expectedResearchId":null},
        "lensReads":[saved.record_id], "previousNoteIds":[note_id]
    });
    assert!(executor
        .execute(&policy, ToolMethod::NoteSave, save.clone())
        .await
        .is_err());
    assert!(capture.staged_research.lock().await.is_none());
    for pointer in pointers {
        let method = match pointer["tool"].as_str().unwrap() {
            "lens_read" => ToolMethod::LensRead,
            "conversation_memory" => ToolMethod::ConversationMemory,
            other => panic!("unexpected tool {other}"),
        };
        let read = executor
            .execute(&policy, method, pointer["readArgs"].clone())
            .await
            .unwrap();
        if method == ToolMethod::LensRead {
            assert_eq!(read["recordId"], saved.record_id);
            save["lensReads"] = json!([read["readRef"]]);
            // The previous note is still only a pointer until it is read.
            assert!(executor
                .execute(&policy, ToolMethod::NoteSave, save.clone())
                .await
                .is_err());
        } else {
            assert_eq!(read["note"]["researchId"], note_id);
            assert_eq!(read["note"]["chatId"], "prior-chat");
        }
    }
    executor
        .execute(&policy, ToolMethod::NoteSave, save)
        .await
        .unwrap();
    let acknowledged = capture.staged_research.lock().await.clone().unwrap();
    let blobs = knowledge_blobs_for_loupist(&state, "loupist-a").unwrap();
    // Independently prove the persistence authority boundary still rejects a
    // pointer even if it bypasses tool validation, with no delivered reads.
    let mut pointer_only = acknowledged.clone();
    pointer_only.lens_reads.clear();
    pointer_only.previous_note_ids.clear();
    pointer_only.lens_reads = vec![saved.record_id.clone()];
    {
        let mut connection = state.store.lock().unwrap();
        let tx = connection.transaction().unwrap();
        assert!(persist_saved_turn(
            &tx,
            &blobs,
            "loupist-a",
            "probe",
            "pointer-only",
            100_000,
            &[],
            true,
            &[],
            &[],
            Some(&pointer_only),
        )
        .is_err());
    }
    let reads = capture.staged_reads.lock().await.clone();
    {
        let mut connection = state.store.lock().unwrap();
        let tx = connection.transaction().unwrap();
        persist_saved_turn(
            &tx,
            &blobs,
            "loupist-a",
            "probe",
            "exact-read",
            100_000,
            &[],
            true,
            &[],
            &reads,
            Some(&acknowledged),
        )
        .unwrap();
        tx.commit().unwrap();
    }
    assert!(capture
        .staged_research
        .lock()
        .await
        .as_ref()
        .unwrap()
        .working_note
        .is_some());
}

#[test]
fn host_memory_relevant_excludes_archived_standing_weak_and_empty_and_caps_each_kind() {
    let (_dir, state) = fixture();
    assert!(
        host_memory_context(&state, "loupist-a", "Acme liquidity covenant")
            .unwrap()
            .relevant
            .is_empty()
    );
    let standing = ensure_perspective(&state, "loupist-a", "Acme liquidity covenant")
        .unwrap()
        .unwrap();
    let archived = method(
        &state,
        "Archived Acme liquidity covenant",
        "Acme liquidity covenant",
        1,
    );
    with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).set_lens_archived(
            "loupist-a",
            &archived.record_id,
            true,
            "Retired",
            2,
            "user",
        )
    })
    .unwrap();
    for i in 0..5 {
        method(
            &state,
            &format!("Acme liquidity covenant {i}"),
            "Stress Acme liquidity covenant.",
            3 + i,
        );
        note(
            &state,
            "loupist-a",
            &format!("saved-{i}"),
            &format!("Acme liquidity covenant {i}"),
            "User supplied Acme liquidity covenant headroom.",
        );
    }
    let context = host_memory_context(&state, "loupist-a", "Acme liquidity covenant").unwrap();
    assert_eq!(
        context.relevant.iter().filter(|p| p.kind == "Lens").count(),
        4
    );
    assert_eq!(
        context
            .relevant
            .iter()
            .filter(|p| p.kind == "WorkingNote")
            .count(),
        3
    );
    assert!(!context
        .relevant
        .iter()
        .any(|p| p.read_args["recordId"] == standing
            || p.read_args["recordId"] == archived.record_id));
    assert!(envelope(&context, false).get("memory_protocol").is_none());
    for message in ["", "*", "Please tell me", "Acme orchid watering"] {
        let context = host_memory_context(&state, "loupist-a", message).unwrap();
        assert!(
            context.relevant.is_empty(),
            "unexpected match for {message}"
        );
        assert!(envelope(&context, true)["memory_protocol"]
            .get("relevant")
            .is_none());
    }
}

#[test]
fn host_memory_relevant_keeps_financial_phrase_aliases() {
    let (_dir, state) = fixture();
    let saved = method(
        &state,
        "Profit durability",
        "Check operating profit across cycles.",
        1,
    );
    let context = host_memory_context(&state, "loupist-a", "Explain operating income").unwrap();
    assert_eq!(context.relevant.len(), 1);
    assert_eq!(context.relevant[0].read_args["recordId"], saved.record_id);
}

#[test]
fn memory_query_terms_stay_bounded_and_keep_the_subject_after_a_long_paste() {
    let (_dir, state) = fixture();
    let korean = note(
        &state,
        "loupist-a",
        "korean",
        "유동성 covenant",
        "사용자가 알려준 유동성 covenant 기준",
    );
    let acme = note(
        &state,
        "loupist-a",
        "acme-subject",
        "Acme covenant",
        "Acme covenant headroom supplied by the user.",
    );
    let paste = (0..80)
        .map(|index| format!("uniquetoken{index:04}extralong"))
        .collect::<Vec<_>>()
        .join(" ");
    for (message, expected) in [
        (
            format!(
                "{} 유동성 covenant",
                "Please consider the following analysis. ".repeat(80)
            ),
            &korean,
        ),
        (format!("{paste}. Acme covenant?"), &acme),
    ] {
        let terms = memory_query_terms(&message);
        assert!(terms.join(" ").len() <= 200, "{terms:?}");
        let context = host_memory_context(&state, "loupist-a", &message).unwrap();
        assert!(
            context.relevant.iter().any(|pointer| {
                pointer.kind == "WorkingNote" && pointer.read_args["noteId"] == *expected
            }),
            "{terms:?}"
        );
    }
    let crowded = memory_query_terms(&format!("{} acme covenant", "z".repeat(200)));
    assert!(crowded.iter().any(|term| term == "acme"), "{crowded:?}");
    assert!(crowded.iter().any(|term| term == "covenant"), "{crowded:?}");
    assert!(crowded.iter().all(|term| term.len() <= 32));
    assert!(memory_query_terms(&"재무제표 ".repeat(100)).join(" ").len() <= 200);
}

#[test]
fn note_pointers_keep_a_working_note_when_journals_and_legacy_rows_match() {
    let (_dir, state) = fixture();
    let note_id = note(
        &state,
        "loupist-a",
        "acme-note",
        "Acme liquidity covenant",
        "User supplied Acme liquidity covenant headroom.",
    );
    for index in 0..21 {
        journal(
            &state,
            &format!("crowd-{index}"),
            "Acme liquidity covenant remains binding.",
            100 + index,
        );
    }
    let legacy_id = legacy_research(
        &state,
        "legacy-acme",
        "Acme liquidity covenant in the old research row.",
        200,
    );
    let context = host_memory_context(&state, "loupist-a", "Acme liquidity covenant").unwrap();
    let note_ids: Vec<_> = context
        .relevant
        .iter()
        .filter(|pointer| pointer.kind == "WorkingNote")
        .map(|pointer| pointer.read_args["noteId"].as_str().unwrap_or_default())
        .collect();
    assert!(note_ids.contains(&note_id.as_str()), "{note_ids:?}");
    assert!(!note_ids.contains(&legacy_id.as_str()), "{note_ids:?}");
}

#[test]
fn the_seeded_perspective_holds_the_focus_in_a_focus_section() {
    let (_dir, state) = fixture();
    let record_id = ensure_perspective(&state, "loupist-a", "  Semiconductor supply chain ")
        .unwrap()
        .unwrap();
    let page = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).read_page("loupist-a", &record_id)
    })
    .unwrap();
    assert_eq!(page.title, "Outlook");
    let sections: Vec<_> = page
        .body
        .sections()
        .iter()
        .map(|section| {
            (
                section.key.as_str().to_owned(),
                section.heading.clone(),
                section.markdown.clone(),
            )
        })
        .collect();
    assert_eq!(
        sections,
        [(
            "focus".to_owned(),
            "Focus".to_owned(),
            "Semiconductor supply chain".to_owned()
        )]
    );
}

#[test]
fn a_new_focus_rewrites_only_the_perspective_focus_section() {
    let (_dir, state) = fixture();
    let record_id = ensure_perspective(&state, "loupist-a", "Semiconductor supply chain")
        .unwrap()
        .unwrap();
    let sections = || {
        with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
            KnowledgeWrite::bind(tx, blobs).read_page("loupist-a", &record_id)
        })
        .unwrap()
        .body
        .sections()
        .iter()
        .map(|section| (section.key.as_str().to_owned(), section.markdown.clone()))
        .collect::<Vec<_>>()
    };
    with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        use crate::knowledge::lens_page::{
            Basis, FilingOrigin, LensOp, PageDelta, PageTarget, SectionKey,
        };
        KnowledgeWrite::bind(tx, blobs).file(
            "loupist-a",
            &FilingOrigin::User {
                chat_id: None,
                message_id: None,
            },
            vec![PageDelta {
                target: PageTarget::Existing {
                    record_id: record_id.clone(),
                },
                ops: vec![LensOp::Put {
                    key: SectionKey::new("avoid")?,
                    heading: "Avoid".into(),
                    markdown: "Short-term price calls".into(),
                    basis: Basis::default(),
                    expected: None,
                }],
            }],
            2,
        )
    })
    .unwrap();
    assert_eq!(
        ensure_perspective(&state, "loupist-a", " HBM packaging equipment ").unwrap(),
        Some(record_id.clone())
    );
    let expected = vec![
        ("focus".to_owned(), "HBM packaging equipment".to_owned()),
        ("avoid".to_owned(), "Short-term price calls".to_owned()),
    ];
    assert_eq!(sections(), expected);
    ensure_perspective(&state, "loupist-a", "HBM packaging equipment").unwrap();
    ensure_perspective(&state, "loupist-a", "").unwrap();
    assert_eq!(
        sections(),
        expected,
        "the same or an empty focus changes nothing"
    );
}
