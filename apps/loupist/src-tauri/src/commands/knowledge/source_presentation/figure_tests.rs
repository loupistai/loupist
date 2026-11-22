use super::super::{
    source_read, with_loupist_knowledge, KnowledgeSourceReadRequest, SourceDetailDto,
    SourceFigureRequest,
};
use crate::app::AppState;
use crate::commands::tests::{profile, seed_profile};
use crate::commands::tool_executor::{
    evidence_id,
    number_checks::{check_link, EvidenceLink},
    RunResult, RunResultOrigin, RunResultProducer, RunResultRegistry,
};
use crate::domain::{ChatMessage, ChatRole, MemoryAccess, MemoryRefSnapshot, NumberCheckStatus};
use crate::knowledge::{
    Authorship, Coverage, KnowledgeWrite, RepresentationRole, RetainMaterial, RetainNewSource,
    RetainedSource, Selector, SourceClass,
};
use crate::test_fixtures::{chat_message, chat_session};
use serde_json::{json, Value};

const THREAD: &str = "filing";
const LOUPIST: &str = "loupist-a";
const REVENUE: &str = "96,169";
const MARGIN: &str = "73.9%";

fn retain(state: &AppState, envelope: &str, url: &str, origin: Option<&str>) -> RetainedSource {
    with_loupist_knowledge(state, LOUPIST, |tx, blobs| {
        let request = RetainNewSource {
            origin: None,
            loupist_id: LOUPIST.into(),
            at_ms: 2,
            title: "Annual filing".into(),
            source_class: SourceClass::ToolResult,
            origin_locator: url.into(),
            provider_identity: "example.com".into(),
            authorship: Authorship::Tool,
            coverage: Coverage::IncludedExcerpt,
            origin_chat_id: origin.map(|_| THREAD.to_owned()),
            request_ref: origin.map(|_| "original-fetch".into()),
            declared_digest: None,
            publication_time: None,
            material: Some(RetainMaterial {
                media_type: "application/json".into(),
                role: RepresentationRole::Excerpt,
                document_id: "filing".into(),
                parser_version: None,
                parent_representation_id: None,
                input_span_ids: Vec::new(),
                bytes: envelope.as_bytes().into(),
                selector: Selector::ByteRange {
                    start: 0,
                    end: envelope.len() as u64,
                },
                context_note: "Exact delivered web capture".into(),
            }),
        };
        let write = KnowledgeWrite::bind(tx, blobs);
        match origin {
            Some(answer) => write.retain_new_source_with_origin(request, answer, None),
            None => write.retain_new_source(request),
        }
    })
    .unwrap()
}

fn answer(
    id: &str,
    figure: &str,
    excerpt: &str,
    envelope: &str,
    source: Option<&RetainedSource>,
) -> ChatMessage {
    let saved = source.is_some();
    let tool = if saved { "world_read" } else { "web_fetch" };
    let origin = if saved {
        RunResultOrigin::SavedContext
    } else {
        RunResultOrigin::AcquiredOriginal
    };
    let result_ref = if saved { id } else { "original-fetch" };
    let id_for_evidence = evidence_id(result_ref);
    let read_ref = saved.then(|| format!("rdg_{id}"));
    let result = RunResult {
        result_ref: result_ref.into(),
        producer: RunResultProducer {
            origin,
            runtime_id: if saved { "memory" } else { "native-web" }.into(),
            tool_name: tool.into(),
            provider: None,
        },
        origin: None,
        request_digest: "a".repeat(64),
        response_digest: crate::util::sha256(envelope.as_bytes()),
        retrieved_at: "2026-11-21T00:00:00Z".into(),
        payload: if saved {
            json!({"text": envelope, "scope": "source", "readRef": read_ref})
        } else {
            serde_json::from_str(envelope).unwrap()
        },
        model_delivery: None,
        warnings: vec![],
        upstream_result_refs: vec![],
    };
    let evidence = serde_json::from_value(json!({
        "id": id_for_evidence,
        "title": "Annual filing",
        "kind": if saved { "saved_context" } else { "original" },
        "tool_name": tool,
        "read_ref": read_ref,
        "retrieved_at": result.retrieved_at,
        "excerpt": excerpt,
        "excerpt_truncated": false,
        "content_sha256": result.response_digest,
        "warnings": []
    }))
    .unwrap();
    let mut answer = chat_message(
        id,
        ChatRole::Assistant,
        &format!("[{figure}](#evidence-{id_for_evidence})"),
    );
    let check = check_link(
        &EvidenceLink {
            index: 0,
            text: figure.into(),
            evidence_id: id_for_evidence,
        },
        &result,
        Some(&evidence),
        &RunResultRegistry::default(),
    )
    .unwrap();
    assert_eq!(check.status, NumberCheckStatus::Matched);
    if saved {
        // The check must use offsets in decoded content, not the JSON excerpt.
        assert!(check.context.is_some() && check.span.is_none());
    } else {
        assert!(check.span.is_some());
    }
    answer.number_checks.push(check);
    answer.evidence_refs.push(evidence);
    if let (Some(source), Some(read_ref)) = (source, read_ref) {
        answer.memory_refs.push(MemoryRefSnapshot {
            read_ref: read_ref.clone(),
            kind: "source".into(),
            title: "Annual filing".into(),
            excerpt: excerpt.chars().take(180).collect(),
            as_of: None,
            section: None,
            access: MemoryAccess::ExactRead,
            full_record_digest: None,
            record_id: source.record_id.clone(),
            revision_id: source.revision_id.clone(),
            object_id: source.source_id.clone(),
            span_id: source.span_id.clone(),
            receipt_id: Some(read_ref),
        });
        answer.observed_exact_count = 1;
        answer.refs_digest = crate::domain::memory_refs_digest(&answer.memory_refs).unwrap();
        answer.created_at_ms = 3;
    }
    answer
}

fn request(
    source: &RetainedSource,
    figure: Option<SourceFigureRequest>,
) -> KnowledgeSourceReadRequest {
    KnowledgeSourceReadRequest {
        loupist_id: LOUPIST.into(),
        record_id: source.record_id.clone(),
        revision_id: source.revision_id.clone(),
        source_id: Some(source.source_id.clone()),
        span_id: source.span_id.clone(),
        purpose: "renderer_inspection".into(),
        world_record_id: None,
        world_revision_id: None,
        figure,
    }
}

fn figure(thread: &str, answer: &ChatMessage) -> SourceFigureRequest {
    SourceFigureRequest {
        thread_id: thread.into(),
        message_id: answer.id.clone(),
        evidence_id: answer.evidence_refs[0].id.clone(),
        link_index: 0,
    }
}

fn projected(detail: &SourceDetailDto) -> Value {
    let view = detail.spans[0].presentation.as_ref().unwrap();
    serde_json::to_value(view).unwrap()
}

fn capture_text() -> String {
    let gap = "This paragraph is part of the retained report. ".repeat(26);
    format!("# Net sales\n\n| Services | {REVENUE} |\n\n{gap}\n\n# Gross margin\n\nサービスの粗利益率は {MARGIN} でした。")
}

fn envelope(content: &str) -> String {
    json!({
        "content": content, "content_bytes": content.len(), "content_offset": 12745,
        "next_offset": null, "extraction_truncated": false,
        "extraction_method": "readability", "content_class": "untrusted_web",
        "untrusted_content": true, "url": "https://example.com/filing"
    })
    .to_string()
}

#[test]
fn saved_filing_projects_both_answers_and_focuses_the_figure_clicked() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::for_test(dir.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile(LOUPIST, &dir.path().join(LOUPIST), 1),
    );
    seed_profile(
        state.store.as_ref(),
        &profile("loupist-b", &dir.path().join("loupist-b"), 1),
    );

    let content = capture_text();
    let raw = envelope(&content);
    let saved = retain(&state, &raw, "https://example.com/filing", Some("revenue"));
    // Same bytes, different retained Source.
    let other = retain(&state, &raw, "https://example.com/another-filing", None);
    assert_ne!(saved.record_id, other.record_id);
    assert_eq!(saved.selected_digest, other.selected_digest);

    let mut revenue = answer("revenue", REVENUE, &content, &raw, None);
    revenue.memory_update = Some(
        serde_json::from_value(json!({
            "status": "applied", "commitId": "revenue",
            "changes": [{
                "recordId": saved.source_id, "kind": "Source", "operation": "create",
                "title": "Annual filing", "lesson": "Retained the filing.",
                "basis": "Web capture", "futureUse": "Filing research",
                "knowledge": {
                    "kind": "source", "recordId": saved.record_id, "objectId": saved.source_id,
                    "revisionId": saved.revision_id, "spanId": saved.span_id
                }
            }]
        }))
        .unwrap(),
    );
    let margin = answer("margin", MARGIN, &raw, &raw, Some(&saved));
    let other_source = answer("other-source", MARGIN, &raw, &raw, Some(&other));
    let mut older_revision = saved.clone();
    older_revision.revision_id = format!("rev_{}", "f".repeat(32));
    let other_revision = answer("other-revision", MARGIN, &raw, &raw, Some(&older_revision));
    let mut wrong_span = saved.clone();
    wrong_span.span_id = Some(format!("spn_{}", "f".repeat(32)));
    let other_span = answer("other-span", MARGIN, &raw, &raw, Some(&wrong_span));

    let mut outside = answer("outside", MARGIN, &raw, &raw, Some(&saved));
    let context = outside.number_checks[0].context.as_mut().unwrap();
    context.text = format!("{MARGIN} outside the retained window");
    context.start = 0;
    context.end = MARGIN.len();

    let mut chat = chat_session(THREAD, LOUPIST);
    chat.messages.extend(
        [
            &revenue,
            &margin,
            &other_source,
            &other_revision,
            &other_span,
            &outside,
        ]
        .into_iter()
        .cloned(),
    );
    state.store.create_chat(&chat).unwrap();

    let mut foreign = chat_session("other-thread", "loupist-b");
    foreign.messages.push(margin.clone());
    state.store.create_chat(&foreign).unwrap();

    with_loupist_knowledge(&state, LOUPIST, |tx, _| {
        tx.execute(
            "INSERT INTO chat_sessions (id,loupist_id,updated_at_ms,data_json) VALUES (?1,?2,?3,?4)",
            rusqlite::params!["unreadable", LOUPIST, 4, json!({"messages": [&margin]}).to_string()],
        )?;
        Ok(())
    })
    .unwrap();

    let ordinary = source_read(&state, request(&saved, None)).unwrap();
    let unselected = projected(&ordinary);
    assert!(ordinary.figure_failure.is_none());
    assert!(unselected.get("focusedMark").is_none());
    assert_eq!(unselected["citedPassages"].as_array().unwrap().len(), 2);

    for (i, (answer, value)) in [(&revenue, REVENUE), (&margin, MARGIN)]
        .into_iter()
        .enumerate()
    {
        let detail = source_read(&state, request(&saved, Some(figure(THREAD, answer)))).unwrap();
        let view = projected(&detail);
        assert!(detail.figure_failure.is_none());
        assert_eq!(view["text"], content);
        assert_eq!(view["citedPassages"], unselected["citedPassages"]);
        let start = content.find(value).unwrap();
        let end = start + value.len();
        assert_eq!(view["focusedMark"], json!({"start": start, "end": end}));
        let passage = &view["citedPassages"][i];
        assert!(passage["start"].as_u64().unwrap() <= start as u64);
        assert!(passage["end"].as_u64().unwrap() >= end as u64);
        assert_eq!(
            passage["marks"],
            json!([{
                "start": start, "end": end, "evidenceId": answer.evidence_refs[0].id
            }])
        );
    }
    assert!(
        content.find(MARGIN).unwrap() > content[..content.find(MARGIN).unwrap()].chars().count()
    );

    let mut wrong_link = figure(THREAD, &margin);
    wrong_link.link_index = 1;
    let mut wrong_original = figure(THREAD, &revenue);
    wrong_original.evidence_id = margin.evidence_refs[0].id.clone();
    for (input, reason) in [
        (figure("other-thread", &margin), "answer_unavailable"),
        (figure("unreadable", &margin), "answer_unavailable"),
        (figure(THREAD, &other_source), "evidence_mismatch"),
        (figure(THREAD, &other_revision), "evidence_mismatch"),
        (figure(THREAD, &other_span), "evidence_mismatch"),
        (wrong_original, "evidence_mismatch"),
        (figure(THREAD, &outside), "mark_unavailable"),
        (wrong_link, "mark_unavailable"),
    ] {
        let refused = source_read(&state, request(&saved, Some(input))).unwrap();
        assert_eq!(projected(&refused), unselected);
        assert_eq!(
            serde_json::to_value(&refused).unwrap()["figureFailure"],
            reason
        );
    }
}

#[test]
fn older_requested_answer_is_included_in_the_bounded_projection() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::for_test(dir.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile(LOUPIST, &dir.path().join(LOUPIST), 1),
    );
    let content = capture_text();
    let raw = envelope(&content);
    let saved = retain(&state, &raw, "https://example.com/filing", None);
    let mut oldest = answer("oldest", REVENUE, &raw, &raw, Some(&saved));
    oldest.created_at_ms = 2;
    let mut chat = chat_session(THREAD, LOUPIST);
    chat.messages.push(oldest.clone());
    for n in 0..33 {
        let mut newer = answer(&format!("recent-{n}"), MARGIN, &raw, &raw, Some(&saved));
        newer.created_at_ms += n;
        chat.messages.push(newer);
    }
    chat.updated_at_ms = 40;
    state.store.create_chat(&chat).unwrap();

    let ordinary = source_read(&state, request(&saved, None)).unwrap();
    let ordinary = projected(&ordinary);
    let passages = ordinary["citedPassages"].as_array().unwrap();
    assert_eq!(passages.len(), 1);
    assert_eq!(passages[0]["marks"].as_array().unwrap().len(), 32);

    let focused = source_read(&state, request(&saved, Some(figure(THREAD, &oldest)))).unwrap();
    let focused = projected(&focused);
    let start = content.find(REVENUE).unwrap();
    assert_eq!(
        focused["focusedMark"],
        json!({"start": start, "end": start + REVENUE.len()})
    );
    let passages = focused["citedPassages"].as_array().unwrap();
    assert_eq!(passages.len(), 2);
    assert_eq!(passages[1]["marks"].as_array().unwrap().len(), 31);
}

#[test]
fn saved_population_figures_in_chat_and_reader_use_the_verified_observation() {
    let dir = tempfile::tempdir().unwrap();
    let state = AppState::for_test(dir.path().join("app"));
    seed_profile(
        state.store.as_ref(),
        &profile(LOUPIST, &dir.path().join(LOUPIST), 1),
    );
    let raw = json!({
        "schema_version":"loupist-finance-result/1",
        "dataset_ref":{"dataset_id":"hidden-identifier","digest":"hidden-digest","revision":1},
        "data":{
            "series":{"indicator_name":"Population, total","economy_name":"Japan"},
            "observations":[
                {"date":"2020","period":"2020","value":"126261000","decimal":0},
                {"date":"2024","period":"2024","value":"123975371","decimal":0}
            ]
        }
    })
    .to_string();
    let saved = retain(&state, &raw, "https://example.com/population", None);
    let mut legacy = answer("legacy", "123,975,371", &raw, &raw, Some(&saved));
    let start = raw.find("123975371").unwrap();
    legacy.number_checks[0].span = Some(crate::domain::ChatEvidenceMatch {
        start,
        end: start + 9,
    });
    legacy.number_checks[0].context = None;
    let mut english = answer("english", "126,261,000", &raw, &raw, Some(&saved));
    let mut korean = answer("korean", "126,261,000", &raw, &raw, Some(&saved));
    for (message, text) in [
        (&mut english, "126,261,000 people"),
        (&mut korean, "126,261,000명"),
    ] {
        message.content = message.content.replace("126,261,000", text);
        message.number_checks.clear();
    }
    let mut chat = chat_session(THREAD, LOUPIST);
    chat.messages.extend([legacy, english, korean]);
    state.store.create_chat(&chat).unwrap();
    let dto = crate::commands::types::chat_dto_with_memory(&state, &chat).unwrap();

    for (index, message) in dto.messages.iter().enumerate() {
        let check = &message.number_checks[0];
        let (period, value, row) = if index == 0 {
            ("2024", "123,975,371", 1)
        } else {
            ("2020", "126,261,000", 0)
        };
        assert_eq!(check.status, NumberCheckStatus::Matched);
        assert!(check.span.is_none());
        let context = check.context.as_ref().unwrap();
        assert_eq!(
            context.text,
            format!("Population, total · Japan · {period} · {value}")
        );
        assert_eq!(&context.text[context.start..context.end], value);
        let detail = source_read(
            &state,
            request(&saved, Some(figure(THREAD, &chat.messages[index]))),
        )
        .unwrap();
        assert!(detail.figure_failure.is_none());
        let view = projected(&detail);
        assert_eq!(view["text"], raw);
        assert_eq!(view["table"]["focusedCell"], json!({"row":row, "column":2}));
        assert_eq!(
            view["table"]["columns"],
            json!(["economy_name", "period", "Population, total"])
        );
        assert_eq!(
            view["table"]["rows"][1][2],
            json!({"kind":"number","value":"123975371"})
        );
        assert!(!view["table"].to_string().contains("hidden-"));
    }

    let retained = state.store.get_chat(THREAD).unwrap().unwrap();
    assert!(retained.messages[0].number_checks[0].span.is_some());
    assert!(retained.messages[1].number_checks.is_empty());
    assert!(retained.messages[2].number_checks.is_empty());
}

#[test]
fn saved_data_table_preserves_labels_selected_cells_and_retained_row_count() {
    let rows = (0..60)
        .map(|index| {
            json!({"value":index,"date":format!("2024-{index}"),"featured":index == 59,
                           "next_offset":index,"dataset_id":"hidden","responseDigest":"hidden"})
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "dataset_ref":{"dataset_id":"hidden"},
        "data":{"rows":rows,"columns":{"value":{"label":"Verified amount"}}}
    });
    let saved = crate::datasets::saved_rows::SavedRows::parse(&payload).unwrap();
    let table = super::data::table(&saved, Some("/data/rows/59/value")).unwrap();
    let view = serde_json::to_value(table).unwrap();
    assert_eq!(
        view["columns"],
        json!(["date", "featured", "Verified amount"])
    );
    assert_eq!(view["totalRows"], 60);
    assert_eq!(view["rowOffset"], 20);
    assert_eq!(view["rows"].as_array().unwrap().len(), 40);
    assert_eq!(view["focusedCell"], json!({"row":39, "column":2}));
    assert_eq!(
        view["rows"][39],
        json!([{"kind":"text","value":"2024-59"},{"kind":"boolean","value":true},{"kind":"number","value":"59"}])
    );
    assert!(!view.to_string().contains("hidden"));
}
