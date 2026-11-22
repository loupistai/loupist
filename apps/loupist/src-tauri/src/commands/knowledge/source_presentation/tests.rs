use super::*;
use crate::commands::knowledge::{
    catalog, source_read, with_loupist_knowledge, KnowledgeSourceReadRequest,
};
use crate::commands::tests::{profile, seed_profile};
use crate::knowledge::{
    Authorship, Coverage, KnowledgeWrite, RetainMaterial, RetainNewSource, Selector,
};
use serde_json::json;

fn web_payload() -> serde_json::Value {
    let content = "Example Domain\n\n문서 본문 with a \"quoted\" word.";
    json!({
        "content": content, "content_bytes": content.len(), "content_offset": 0,
        "next_offset": null, "extraction_truncated": false, "truncated": false,
        "extraction_method": "readability", "content_class": "untrusted_web",
        "untrusted_content": true, "content_sha256": "upstream-response-digest",
        "url": "https://example.com/", "status": 200
    })
}

#[test]
fn source_presentation_keeps_web_body_separate_from_exact_capture_and_export() {
    let directory = tempfile::tempdir().unwrap();
    let state = crate::app::AppState::for_test(directory.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", directory.path(), 1),
    );
    let payload = web_payload();
    let original = serde_json::to_vec(&payload).unwrap();
    let saved = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).retain_new_source(RetainNewSource {
            origin: None,
            loupist_id: "loupist-a".into(),
            at_ms: 2,
            title: "Fetched document".into(),
            source_class: SourceClass::ToolResult,
            origin_locator: "https://example.com/".into(),
            provider_identity: "example.com".into(),
            authorship: Authorship::Tool,
            coverage: Coverage::IncludedFull,
            origin_chat_id: None,
            request_ref: None,
            declared_digest: None,
            publication_time: None,
            material: Some(RetainMaterial {
                media_type: "application/json".into(),
                role: RepresentationRole::ProviderPayload,
                document_id: "example-document".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                bytes: original.clone(),
                selector: Selector::ByteRange {
                    start: 0,
                    end: original.len() as u64,
                },
                context_note: "Exact delivered provider payload".into(),
            }),
        })
    })
    .unwrap();
    let request = || KnowledgeSourceReadRequest {
        loupist_id: "loupist-a".into(),
        record_id: saved.record_id.clone(),
        revision_id: saved.revision_id.clone(),
        source_id: Some(saved.source_id.clone()),
        span_id: saved.span_id.clone(),
        purpose: "renderer_inspection".into(),
        world_record_id: None,
        world_revision_id: None,
        figure: None,
    };
    let detail = source_read(&state, request()).unwrap();
    let span = &detail.spans[0];
    assert_eq!(span.text.as_deref().unwrap().as_bytes(), original);
    assert_eq!(span.presentation.as_ref().unwrap().text, payload["content"]);
    let listed = catalog(
        &state,
        super::super::KnowledgeCatalogRequest {
            loupist_id: "loupist-a".into(),
        },
    )
    .unwrap();
    assert_eq!(
        listed.sources[0].objects[0].summary.as_deref(),
        Some("문서 본문 with a \"quoted\" word.")
    );
    let selected = super::super::source_selection::resolve(
        &state,
        super::super::source_selection::SourceSelectionRequest {
            source: request(),
            selection: crate::commands::SourceTextSelection {
                start_utf16: 16,
                end_utf16: 21,
                text: "문서 본문".into(),
            },
        },
    )
    .unwrap();
    let hints = super::super::selected_reference_hints(&state, "loupist-a", &[selected.pin])
        .unwrap()
        .unwrap();
    assert_eq!(hints["items"][0]["selection"]["text"], "문서 본문");
    let (exported, media_type, _) =
        super::super::source_export::export_bytes(&state, &request()).unwrap();
    assert_eq!(exported, original);
    assert_eq!(media_type, "application/json");
    with_loupist_knowledge(&state, "loupist-a", |tx, _| {
        let receipt_count: i64 = tx.query_row(
            "SELECT COUNT(*) FROM knowledge_read_receipts WHERE purpose = 'chat_turn'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(
            receipt_count, 0,
            "a readable preview does not grant citation authority"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn source_presentation_projects_only_consistent_web_payloads() {
    let role = RepresentationRole::ProviderPayload;
    let class = SourceClass::ToolResult;
    let payload = web_payload().to_string();
    let mut inconsistent = web_payload();
    inconsistent["content_bytes"] = json!(1);
    for (class, role, media_type, body) in [
        (
            class,
            role,
            "application/json",
            r#"{"content":"Company summary","revenue":42}"#.to_owned(),
        ),
        (
            SourceClass::UserStatement,
            role,
            "application/json",
            payload.clone(),
        ),
        (
            class,
            RepresentationRole::Original,
            "application/json",
            payload.clone(),
        ),
        (class, role, "text/plain", payload.clone()),
        (class, role, "application/json", inconsistent.to_string()),
    ] {
        assert!(source_presentation(class, role, media_type, &body).is_none());
    }

    // A partial web read keeps its coverage window.
    let mut partial = web_payload();
    partial["content_offset"] = json!(500);
    partial["next_offset"] = json!(500 + partial["content_bytes"].as_u64().unwrap());
    partial["extraction_truncated"] = json!(true);
    let view = source_presentation(
        class,
        RepresentationRole::Excerpt,
        "application/json",
        &partial.to_string(),
    )
    .unwrap();
    assert_eq!(view.text, partial["content"]);
    assert_eq!(view.content_offset, 500);
    assert_eq!(view.next_offset, Some(500 + view.text.len()));
    assert!(view.extraction_truncated);
}

fn read_cited_filing(
    content: &str,
    anchor: Option<&str>,
    figures: &[&str],
) -> SourcePresentationDto {
    let directory = tempfile::tempdir().unwrap();
    let state = crate::app::AppState::for_test(directory.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-a", directory.path(), 1),
    );
    let mut payload = web_payload();
    payload["content"] = json!(&content);
    payload["content_bytes"] = json!(content.len());
    payload["content_offset"] = json!(12745);
    let original = serde_json::to_vec(&payload).unwrap();
    let mut chat = crate::test_fixtures::chat_session("chat-a", "loupist-a");
    let mut message = crate::test_fixtures::chat_message(
        "answer",
        crate::domain::ChatRole::Assistant,
        "[report](#evidence-ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa)",
    );
    let excerpt_match = anchor.map(|text| {
        let start = content.find(text).unwrap();
        crate::domain::ChatEvidenceMatch {
            start,
            end: start + text.len(),
        }
    });
    message.evidence_refs.push(
        serde_json::from_value(json!({
            "id": "ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "title": "10-K",
            "tool_name": "web_fetch",
            "kind": "original",
            "retrieved_at": "2026-11-21T00:00:00Z",
            "excerpt": content,
            "excerpt_truncated": false,
            "excerpt_match": excerpt_match,
            "content_sha256": crate::util::sha256(&original),
            "warnings": []
        }))
        .unwrap(),
    );
    for (index, figure) in figures.iter().enumerate() {
        let start = content.find(figure).unwrap();
        message.number_checks.push(
            serde_json::from_value(json!({
                "link_index": index,
                "text": figure,
                "evidence_id": "ev_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "kind": "passage",
                "status": "matched",
                "span": { "start": start, "end": start + figure.len() },
                "fields": []
            }))
            .unwrap(),
        );
    }
    chat.messages.push(message);
    state.store.create_chat(&chat).unwrap();
    let saved = with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
        KnowledgeWrite::bind(tx, blobs).retain_new_source_with_origin(
            RetainNewSource {
                origin: None,
                loupist_id: "loupist-a".into(),
                at_ms: 2,
                title: "Fetched document".into(),
                source_class: SourceClass::ToolResult,
                origin_locator: "https://example.com/".into(),
                provider_identity: "example.com".into(),
                authorship: Authorship::Tool,
                coverage: Coverage::IncludedFull,
                origin_chat_id: Some("chat-a".into()),
                request_ref: None,
                declared_digest: None,
                publication_time: None,
                material: Some(RetainMaterial {
                    media_type: "application/json".into(),
                    role: RepresentationRole::ProviderPayload,
                    document_id: "example-document".into(),
                    parser_version: None,
                    parent_representation_id: None,
                    input_span_ids: Vec::new(),
                    bytes: original.clone(),
                    selector: Selector::ByteRange {
                        start: 0,
                        end: original.len() as u64,
                    },
                    context_note: "Exact delivered provider payload".into(),
                }),
            },
            "answer",
            None,
        )
    })
    .unwrap();
    let request = KnowledgeSourceReadRequest {
        loupist_id: "loupist-a".into(),
        record_id: saved.record_id.clone(),
        revision_id: saved.revision_id.clone(),
        source_id: Some(saved.source_id.clone()),
        span_id: saved.span_id.clone(),
        purpose: "renderer_inspection".into(),
        world_record_id: None,
        world_revision_id: None,
        figure: None,
    };
    let mut detail = source_read(&state, request).unwrap();
    detail.spans.remove(0).presentation.unwrap()
}

#[test]
fn c004_saved_filing_opens_at_its_cited_table_and_keeps_distant_citations() {
    let header = "| Category | 2024 | 2022 |";
    let figures = ["96,169", "78,129", "391,035", "394,328", "$700"];
    let note = "Deferred revenue relates to services. ".repeat(32);
    let rows = "| iPhone | 201,183 | 205,489 |\n".repeat(8);
    let content = format!("e of sale. {note}\n\n{header}\n| --- | --- | --- |\n{rows}| Services | 96,169 | 78,129 |\n| Total net sales | 391,035 | 394,328 |\n\n{note}\n\nRestricted cash was $700 million.\n\n{note}");
    let view = read_cited_filing(&content, None, &figures);
    let passages = view.cited_passages.expect("cited passages");
    assert_eq!(passages.len(), 2);
    assert!(content[passages[0].start..passages[0].end].starts_with(header));
    assert_eq!(
        &content[passages[1].start..passages[1].end],
        "Restricted cash was $700 million."
    );
    let mut marks = Vec::new();
    for passage in &passages {
        assert!(!passage.marks.is_empty());
        for mark in &passage.marks {
            assert!(passage.start <= mark.start && mark.end <= passage.end);
            marks.push(&content[mark.start..mark.end]);
        }
    }
    assert_eq!(marks, figures);
    assert_eq!(view.text, content);
}

// Older filing extraction split labels and amounts across lines and blank
// lines. These are the retained public 10-K bytes around the four citations.
const LEGACY_TABLE: &str = concat!(
    "Net sales disaggregated by significant products and services for 2024, 2023 and 2022 were as follows (in millions):\n\n",
    "iPhone $ 201,183\u{a0} $ 200,583\u{a0} $ 205,489\u{a0}\n",
    "Mac 29,984\u{a0} 29,357\u{a0} 40,177\u{a0}\n",
    "iPad 26,694\u{a0} 28,300\u{a0} 29,292\u{a0}\n",
    "Wearables, Home and Accessories\n\n",
    "Total net sales 391,035\n\n37,005\u{a0}\n\n   $ 383,285\n\n39,845\u{a0}\n\n   $ 394,328\n",
    "41,241\u{a0}\nServices (1) 96,169\u{a0} 85,200\u{a0} 78,129",
);

#[test]
fn c004_flattened_table_keeps_cited_figures_with_its_intro_without_literal_anchor() {
    let content = format!(
        "Note 2. Revenue\n\nA preceding paragraph.\n\n{LEGACY_TABLE}\n\nAn unrelated footnote."
    );
    let figures = ["391,035", "$ 394,328", "96,169", "78,129"];
    let view = read_cited_filing(&content, None, &figures);
    let passages = view.cited_passages.expect("the four retained marks");
    let [passage] = passages.as_slice() else {
        panic!("expected one cited passage")
    };
    assert_eq!(&content[passage.start..passage.end], LEGACY_TABLE);
    let marks: Vec<_> = passage
        .marks
        .iter()
        .map(|mark| {
            assert!(passage.start <= mark.start && mark.end <= passage.end);
            &content[mark.start..mark.end]
        })
        .collect();
    assert_eq!(marks, figures);
}

#[test]
fn c004_literal_anchor_alone_opens_the_whole_saved_text() {
    let view = read_cited_filing(LEGACY_TABLE, Some("Net sales disaggregated"), &[]);
    assert!(view.cited_passages.is_none());
    assert_eq!(view.text, LEGACY_TABLE);
}

#[test]
fn c004_verified_literal_keeps_an_unformatted_heading_with_its_mark() {
    let rows = "iPhone 201,183 205,489\n".repeat(12);
    let content = format!("Net sales by category\n\n{rows}Services 96,169");
    let view = read_cited_filing(&content, Some("Net sales by category"), &["96,169"]);
    let passages = view.cited_passages.expect("retained mark");
    let [passage] = passages.as_slice() else {
        panic!("expected one cited passage");
    };
    assert_eq!(&content[passage.start..passage.end], content);
    assert_eq!(passage.marks.len(), 1);
}
