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
        payload["content"].as_str()
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
