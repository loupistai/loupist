use super::*;
use crate::commands::knowledge::{selected_reference_hints, with_loupist_knowledge};
use crate::commands::tests::{profile, seed_profile};
use crate::knowledge::{
    Authorship, Coverage, RepresentationRole, RetainMaterial, RetainNewSource, Selector,
    SourceClass,
};

#[test]
fn selection_offsets_use_utf16_and_reject_split_surrogates_or_stale_text() {
    let selected = |start_utf16, end_utf16, text: &str| SourceTextSelection {
        start_utf16,
        end_utf16,
        text: text.into(),
    };
    let text = "A😀 한글\nrepeated repeated";
    assert!(matches_text(text, &selected(1, 3, "😀")));
    assert!(matches_text(text, &selected(4, 6, "한글")));
    assert!(matches_text(text, &selected(16, 24, "repeated")));
    for selection in [
        selected(1, 2, "😀"),
        selected(2, 3, "😀"),
        selected(0, 99, text),
        selected(4, 6, "other"),
        selected(4, 4, ""),
    ] {
        assert!(!matches_text(text, &selection));
    }
}

#[test]
fn resolver_returns_existing_span_and_rechecks_typed_selection_at_chat_admission_without_writes() {
    let dir = tempfile::tempdir().unwrap();
    let state = crate::app::AppState::for_test(dir.path().join("app"));
    for owner in ["loupist-a", "loupist-b"] {
        seed_profile(
            state.store.as_ref(),
            &profile(owner, &dir.path().join(owner), 1),
        );
    }
    let text = "Revenue 😀 grew.\n매출은 늘었어요. Revenue 😀 grew.";
    let saved = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: "loupist-a".into(),
            at_ms: 2,
            title: "Retained filing".into(),
            source_class: SourceClass::PublisherDocument,
            origin_locator: "https://example.test".into(),
            provider_identity: "example".into(),
            authorship: Authorship::Publisher,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            publication_time: None,
            material: Some(RetainMaterial {
                media_type: "text/plain".into(),
                role: RepresentationRole::Original,
                document_id: "filing".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: vec![],
                bytes: text.as_bytes().to_vec(),
                selector: Selector::ByteRange {
                    start: 0,
                    end: text.len() as u64,
                },
                context_note: "Retained".into(),
            }),
        })
    })
    .unwrap();
    let request = || SourceSelectionRequest {
        source: KnowledgeSourceReadRequest {
            loupist_id: "loupist-a".into(),
            record_id: saved.record_id.clone(),
            revision_id: saved.revision_id.clone(),
            source_id: Some(saved.source_id.clone()),
            span_id: saved.span_id.clone(),
            purpose: "renderer_inspection".into(),
            world_record_id: None,
            world_revision_id: None,
        },
        selection: SourceTextSelection {
            start_utf16: 0,
            end_utf16: 16,
            text: "Revenue 😀 grew.".into(),
        },
    };
    let counts = || {
        with_loupist_knowledge(&state, "loupist-a", |tx, _| {
            Ok([
                "knowledge_source_spans",
                "knowledge_record_revisions",
                "knowledge_read_receipts",
            ]
            .map(|table| {
                tx.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
            }))
        })
        .unwrap()
    };
    let before = counts();
    let resolved = resolve(&state, request()).unwrap();
    assert_eq!(resolved.pin.span_id, saved.span_id);
    assert_eq!(resolved.pin.label, "Retained filing");
    assert_eq!(
        counts(),
        before,
        "inspection must not persist a span or issue a model receipt"
    );
    let hints = selected_reference_hints(&state, "loupist-a", std::slice::from_ref(&resolved.pin))
        .unwrap()
        .unwrap();
    assert_eq!(hints["items"][0]["selection"]["text"], "Revenue 😀 grew.");
    assert_eq!(hints["items"][0]["selection"]["endUtf16"], 16);
    assert_eq!(
        hints["items"][0]["read"]["spanId"],
        saved.span_id.as_ref().unwrap().as_str()
    );
    let mut forged = resolved.pin;
    forged.selection.as_mut().unwrap().text = "Forged text".into();
    assert!(selected_reference_hints(&state, "loupist-a", &[forged])
        .unwrap()
        .is_none());
    let mut foreign = request();
    foreign.source.loupist_id = "loupist-b".into();
    assert!(resolve(&state, foreign).is_err());
    let mut stale = request();
    stale.selection.text = "Changed".into();
    assert!(resolve(&state, stale).is_err());
    let mut wrong = request();
    wrong.source.source_id = Some("wrong-source".into());
    assert!(resolve(&state, wrong).is_err());
    let mut missing = request();
    missing.source.span_id = None;
    assert!(resolve(&state, missing).is_err());
    assert_eq!(counts(), before);
}
