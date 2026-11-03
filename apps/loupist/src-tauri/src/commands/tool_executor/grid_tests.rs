//! `grid_publish` without a model (finance-grid-answers.md §8,
//! finance-korea-retail-disagreement.md §8): validation, per-cell checks,
//! staging only for a completed turn, revision, conflict rules and as-of.
use super::*;
use crate::chat::grid::{GridCell, GridDocument};
use crate::commands::tests::{chat, profile, seed_profile};

fn policy() -> ToolPolicy {
    ToolPolicy {
        loupist_id: "loupist-a".into(),
        session_id: "chat-a".into(),
        use_memory: false,
        propose_memory_updates: false,
        memory_proposal_budget: 0,
        as_of: None,
    }
}

fn as_of(day: &str) -> ToolPolicy {
    ToolPolicy {
        as_of: Some(day.into()),
        ..policy()
    }
}

fn executor(temporary: &tempfile::TempDir, message: &str) -> AppToolExecutor {
    let state = AppState::for_persistent_test(temporary.path().join("app"));
    let workspace = temporary.path().join("loupist-a");
    std::fs::create_dir_all(&workspace).unwrap();
    seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
    state
        .store
        .create_chat(&chat("chat-a", "loupist-a", 1))
        .unwrap();
    AppToolExecutor {
        state,
        capture: Arc::new(ToolCapture::for_chat(message.into())),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    }
}

/// Acknowledges one result and returns its citation_url.
async fn cite(
    executor: &AppToolExecutor,
    tool: &str,
    origin: RunResultOrigin,
    payload: Value,
) -> String {
    let delivery = new_id("grid-test");
    let result_ref = executor
        .capture
        .stage_run_result_with_delivery(
            &delivery,
            RunResultProducer {
                origin,
                runtime_id: "native-finance".into(),
                tool_name: tool.into(),
                provider: Some("OpenDART".into()),
            },
            &json!({"company": tool}),
            payload,
            vec![],
            None,
        )
        .await
        .unwrap();
    executor.capture.commit_delivery(&delivery).await;
    evidence::citation_url(&result_ref)
}

async fn statements(executor: &AppToolExecutor, name: &str, revenue: &str, income: &str) -> String {
    cite(
        executor,
        "dart_financials",
        RunResultOrigin::AcquiredOriginal,
        json!({"dataset_ref": {"dataset_id": format!("ds_{name}"), "revision": 1}, "facts": [
            {"corp_name": name, "canonical_concept": "revenue", "period": "FY2025", "fs_div": "CFS",
             "value": revenue, "unit": "KRW", "scale": "1", "filed_at": "2026-03-18"},
            {"corp_name": name, "canonical_concept": "operating_income", "period": "FY2025", "fs_div": "CFS",
             "value": income, "unit": "KRW", "scale": "1", "filed_at": "2026-03-18"}
        ]}),
    )
    .await
}

async fn margin(executor: &AppToolExecutor, source: &str) -> String {
    let output = executor
        .execute(
            &policy(),
            ToolMethod::Calculate,
            json!({"operation": "margin", "unit": "KRW bn",
                "inputs": {"profit": "38.1", "revenue": "452.3"},
                "input_sources": {"profit": source, "revenue": source}}),
        )
        .await
        .unwrap();
    output["citation_url"].as_str().unwrap().to_owned()
}

fn rows(count: usize) -> Value {
    json!((0..count)
        .map(|index| json!({"symbol": format!("99{index:04}"), "market": "KRX", "label": format!("Company {index}")}))
        .collect::<Vec<_>>())
}

fn grid(rows: Value, columns: Value, cells: Value) -> Value {
    json!({"mode": "create", "layout": "assets", "title": "FY2025 revenue and margin",
        "rows": rows, "columns": columns, "cells": cells})
}

fn two_columns() -> Value {
    json!([
        {"id": "revenue", "label": "Revenue", "period": "FY2025", "basis": "Consolidated", "unit": "amount"},
        {"id": "margin", "label": "Operating margin", "period": "FY2025", "unit": "percent"}
    ])
}

async fn refused(executor: &AppToolExecutor, policy: &ToolPolicy, params: Value) -> String {
    executor
        .execute(policy, ToolMethod::GridPublish, params)
        .await
        .unwrap_err()
        .to_string()
}

#[tokio::test]
async fn every_value_cell_is_checked_against_the_evidence_it_cites() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, "assistant-grid");
    let solsup = statements(&executor, "Solsup Foods", "452300000000", "38100000000").await;
    let iseul = statements(&executor, "Iseul Logistics", "128400000000", "7600000000").await;
    let margin = margin(&executor, &solsup).await;
    let output = executor
        .execute(
            &policy(),
            ToolMethod::GridPublish,
            grid(
                json!([
                    {"symbol": "990221", "market": "KRX", "label": "Solsup Foods"},
                    {"symbol": "990552", "market": "KRX", "label": "Iseul Logistics"}
                ]),
                two_columns(),
                json!([
                    [{"value": "₩452.3bn", "citation_url": solsup}, {"value": "8.42%", "citation_url": margin}],
                    [{"value": "₩131.0bn", "citation_url": iseul}, {"empty": "not_reported", "note": "Separate statements only."}]
                ]),
            ),
        )
        .await
        .unwrap();
    assert_eq!(output["status"], "staged");
    assert_eq!(output["artifact"]["kind"], "grid");
    let statuses: Vec<_> = output["cell_checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|check| {
            (
                check["cell"].as_str().unwrap().to_owned(),
                check["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        statuses,
        [
            ("cells[0][0]".to_owned(), "rounded".to_owned()),
            ("cells[0][1]".to_owned(), "matched".to_owned()),
            ("cells[1][0]".to_owned(), "not_found".to_owned()),
        ]
    );
    assert_eq!(
        output["empty_cells"],
        json!([{"cell": "cells[1][1]", "reason": "not_reported"}])
    );
    // A cell that does not match its source asks for a fix; the grid still stages.
    assert!(output["next"]
        .as_str()
        .unwrap()
        .contains("differs or not_found"));

    let commit = executor.capture.artifacts.lock().await[0].clone();
    let ChatArtifactPayload::Grid { grid, .. } = &commit.revision.payload else {
        panic!("not a grid");
    };
    let GridDocument::Assets(grid) = grid.as_ref() else {
        panic!("not an assets grid");
    };
    let GridCell::Value(calculated) = &grid.cells[0][1] else {
        panic!("not a value");
    };
    // The calculated cell's popover carries the formula and its cited inputs.
    let calc = calculated.check.calc.as_ref().unwrap();
    assert_eq!(calc.formulas.len(), 1);
    assert!(calc.inputs.iter().all(|input| input.evidence_id.is_some()));
    // Rows resolve to an asset page; holdings never reach the document.
    assert!(grid.rows[0].asset_id.is_some());
    let stored = serde_json::to_string(&commit.revision.payload).unwrap();
    assert!(!stored.contains("quantity"));
}

#[tokio::test]
async fn malformed_grids_are_refused_with_the_field_to_fix() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, "assistant-grid");
    let solsup = statements(&executor, "Solsup Foods", "452300000000", "38100000000").await;
    let value = json!({"value": "₩452.3bn", "citation_url": solsup});
    let one_column = json!([{"id": "revenue", "label": "Revenue", "unit": "amount"}]);
    let cells = |count: usize| json!((0..count).map(|_| json!([value])).collect::<Vec<_>>());
    let cases = [
        (
            grid(
                rows(2),
                one_column.clone(),
                json!([[value], [{"value": "₩1bn"}]]),
            ),
            "cells[1][0].citation_url: required field is missing",
        ),
        (
            grid(
                rows(2),
                one_column.clone(),
                json!([[value], [{"value": "₩1bn", "citation_url": "#evidence-ev_0000"}]]),
            ),
            "cells[1][0].citation_url: not a citation_url from this turn",
        ),
        (
            grid(
                rows(2),
                one_column.clone(),
                json!([[value], [{"value": "₩1bn", "citation_url": "https://example.com/report"}]]),
            ),
            "cells[1][0].citation_url: not a citation_url from this turn",
        ),
        (
            grid(
                rows(2),
                one_column.clone(),
                json!([[value, value], [value]]),
            ),
            "cells: expected 2 rows of 1 cells",
        ),
        (
            grid(rows(13), one_column.clone(), cells(13)),
            "rows: expected 2–12 rows",
        ),
        (
            grid(
                rows(2),
                one_column.clone(),
                json!([[value], [{"note": "Not filed yet."}]]),
            ),
            "cells[1][0]: an empty cell needs a reason",
        ),
        (
            grid(
                rows(2),
                one_column.clone(),
                json!([[value], [{"empty": "zero"}]]),
            ),
            "cells[1][0].empty: expected not_reported",
        ),
        (
            grid(
                json!([
                    {"symbol": "990221", "market": "KRX", "label": "Solsup Foods"},
                    {"symbol": "990221", "market": "krx", "label": "Solsup again"}
                ]),
                one_column.clone(),
                cells(2),
            ),
            "rows[1]: duplicate row",
        ),
    ];
    for (params, expected) in cases {
        let error = refused(&executor, &policy(), params).await;
        assert!(
            error.contains(expected),
            "{error} does not contain {expected}"
        );
    }
    // Nothing was staged by a refused call.
    assert!(executor.capture.artifacts.lock().await.is_empty());
}

#[tokio::test]
async fn a_grid_is_saved_only_with_a_completed_turn_and_revising_keeps_its_id() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, "assistant-grid-1");
    let solsup = statements(&executor, "Solsup Foods", "452300000000", "38100000000").await;
    let params = grid(
        rows(2),
        json!([{"id": "revenue", "label": "Revenue", "unit": "amount"}]),
        json!([[{"value": "₩452.3bn", "citation_url": solsup}], [{"empty": "unavailable"}]]),
    );
    // A cancelled or failed delivery leaves no grid behind.
    let delivery = new_id("cancelled");
    executor
        .execute_for_delivery(
            &policy(),
            ToolMethod::GridPublish,
            params.clone(),
            &delivery,
        )
        .await
        .unwrap();
    executor.discard_delivery(&policy(), &delivery).await;
    assert!(executor.capture.artifacts.lock().await.is_empty());

    executor
        .execute(&policy(), ToolMethod::GridPublish, params.clone())
        .await
        .unwrap();
    let commit = executor.capture.artifacts.lock().await[0].clone();
    assert_eq!(commit.artifact.kind, ChatArtifactKind::Grid);
    let evidence = executor.capture.evidence_refs.lock().await.clone();
    assert!(!evidence.is_empty());
    let expected = executor.state.store.get_chat("chat-a").unwrap().unwrap();
    let mut saved = expected.clone();
    saved.messages.push(crate::domain::ChatMessage {
        created_at_ms: commit.artifact.updated_at_ms,
        artifact_refs: vec![commit.revision.artifact_ref(commit.artifact.title.clone())],
        evidence_refs: evidence.clone(),
        ..crate::test_fixtures::chat_message(
            &commit.revision.source_message_id,
            crate::domain::ChatRole::Assistant,
            "Published.",
        )
    });
    saved.updated_at_ms = commit.artifact.updated_at_ms;
    executor
        .state
        .store
        .save_chat_with_artifacts(&expected, &saved, std::slice::from_ref(&commit))
        .unwrap();

    let next = AppToolExecutor {
        state: executor.state.clone(),
        capture: Arc::new(ToolCapture::for_chat("assistant-grid-2".into())),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    };
    let read = next
        .execute(
            &policy(),
            ToolMethod::ArtifactRead,
            json!({"artifact_id": commit.artifact.id}),
        )
        .await
        .unwrap();
    assert_eq!(read["artifact"]["kind"], "grid");
    assert_eq!(read["grid"]["layout"], "assets");
    // The old turn's citation cannot be reused; the revision cites this turn's evidence.
    let mut revise = params.clone();
    revise["mode"] = json!("revise");
    revise["artifact_id"] = read["artifact"]["artifact_id"].clone();
    revise["edit_token"] = read["artifact"]["edit_token"].clone();
    let stale = refused(&next, &policy(), revise.clone()).await;
    assert!(
        stale.contains("not a citation_url from this turn"),
        "{stale}"
    );
    let fresh = statements(&next, "Solsup Foods", "452300000000", "38100000000").await;
    revise["title"] = json!("Revenue, revised");
    revise["cells"][0][0]["citation_url"] = json!(fresh);
    next.execute(&policy(), ToolMethod::GridPublish, revise.clone())
        .await
        .unwrap();
    let revised = next.capture.artifacts.lock().await[0].clone();
    assert_eq!(revised.artifact.id, commit.artifact.id);
    assert_eq!(revised.revision.revision, 2);
    super::tests::save_artifact_turn(&executor.state, &revised);
    let original = executor
        .state
        .store
        .read_chat_artifact(&commit.artifact.id, Some(1))
        .unwrap()
        .unwrap();
    assert_eq!(original.revision, commit.revision);
    assert_eq!(original.evidence_refs, evidence);
    let chat = executor.state.store.get_chat("chat-a").unwrap().unwrap();
    let restored = executor
        .state
        .store
        .restore_chat_artifact(
            &chat,
            &commit.artifact.id,
            1,
            2,
            revised.artifact.updated_at_ms + 1,
        )
        .unwrap();
    assert_eq!(restored.evidence_refs, original.evidence_refs);
    assert_eq!(restored.revision.payload, original.revision.payload);
    // A forged token is refused.
    revise["edit_token"] = json!("0".repeat(64));
    let forged = refused(&next, &policy(), revise).await;
    assert!(forged.contains("edit_token does not match"), "{forged}");
}

#[tokio::test]
async fn a_conflict_card_needs_disagreeing_sources_and_a_world_entity_it_read() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, "assistant-conflict");
    let filed = statements(&executor, "Solsup Foods", "452300000000", "38100000000").await;
    let news = cite(
        &executor,
        "web_fetch",
        RunResultOrigin::AcquiredOriginal,
        json!({"title": "Solsup preliminary results", "url": "https://fictional.example/solsup",
            "published_at": "2026-02-05", "content": "Solsup Foods reported preliminary FY2025 revenue of ₩455.0bn."}),
    )
    .await;
    let conflict = |reports: Value| {
        json!({"mode": "create", "layout": "conflict", "title": "Solsup FY2025 revenue",
            "measure": {"label": "FY2025 revenue", "entity_label": "Solsup Foods", "period": "FY2025"},
            "reports": reports, "reason": "preliminary_vs_final",
            "explanation": "The news page reports the preliminary figure; the filing is final.",
            "used": 0, "used_reason": "It is the filed figure."})
    };
    let reports = json!([
        {"citation_url": filed, "says": "₩452.3bn", "basis": "Consolidated, final"},
        {"citation_url": news, "says": "₩455.0bn", "basis": "Preliminary"}
    ]);
    let output = executor
        .execute(
            &policy(),
            ToolMethod::GridPublish,
            conflict(reports.clone()),
        )
        .await
        .unwrap();
    assert_eq!(output["layout"], "conflict");
    assert_eq!(
        output["cell_checks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|check| check["status"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["rounded", "matched"]
    );
    let commit = executor.capture.artifacts.lock().await[0].clone();
    let ChatArtifactPayload::Grid { grid, .. } = &commit.revision.payload else {
        panic!("not a grid");
    };
    let GridDocument::Conflict(card) = grid.as_ref() else {
        panic!("not a conflict");
    };
    // Dates come from the evidence: the filing row and the page's publication.
    assert_eq!(card.reports[0].dated.as_deref(), Some("2026-03-18"));
    assert_eq!(card.reports[1].dated.as_deref(), Some("2026-02-05"));
    assert!(!card.map_link);

    // Reports that agree after normalization are not a conflict.
    let agree = refused(
        &executor,
        &policy(),
        conflict(json!([
            {"citation_url": filed, "says": "₩452.3bn"},
            {"citation_url": news, "says": "4,523억 원"}
        ])),
    )
    .await;
    assert!(agree.contains("these reports agree"), "{agree}");
    // One source cannot disagree with itself.
    let same = refused(
        &executor,
        &policy(),
        conflict(json!([
            {"citation_url": filed, "says": "₩452.3bn"},
            {"citation_url": filed, "says": "₩455.0bn"}
        ])),
    )
    .await;
    assert!(
        same.contains("each report cites a different source"),
        "{same}"
    );
    // A World entity this turn did not read is refused.
    let mut foreign = conflict(reports.clone());
    foreign["world_entity_id"] = json!("ent:solsup");
    let error = refused(&executor, &policy(), foreign.clone()).await;
    assert!(
        error.contains("world_entity_id: not a World entity"),
        "{error}"
    );

    // After a World read that holds a disputed claim about the entity, the card links the Map.
    let world = |status: &str| {
        StagedKnowledgeRead {
        read_ref: "rdg_00000000000000000000000000000001".into(),
        scope: "world".into(),
        record_id: "rec_world".into(),
        span_id: None,
        revision_id: "rev_world".into(),
        object_id: "rec_world".into(),
        title: "World".into(),
        payload: AckedPayload::World {
            revision_digest: "digest".into(),
            view_json: json!({"nodes": [{"id": "ent:solsup"}, {"id": "ent:metric"}], "edges": [
                {"id": "c1", "relation": "REPORTS", "status": status,
                 "endpoints": [{"nodeId": "ent:solsup", "role": "subject"}, {"nodeId": "ent:metric", "role": "object"}]}
            ]})
            .to_string(),
            view_digest: "view".into(),
        },
    }
    };
    executor
        .capture
        .staged_reads
        .lock()
        .await
        .push(world("active"));
    executor
        .execute(&policy(), ToolMethod::GridPublish, foreign.clone())
        .await
        .unwrap();
    executor
        .capture
        .staged_reads
        .lock()
        .await
        .push(world("disputed"));
    executor
        .execute(&policy(), ToolMethod::GridPublish, foreign)
        .await
        .unwrap();
    let links: Vec<bool> = executor
        .capture
        .artifacts
        .lock()
        .await
        .iter()
        .filter_map(|commit| match &commit.revision.payload {
            ChatArtifactPayload::Grid { grid, .. } => match grid.as_ref() {
                GridDocument::Conflict(card) => Some(card.map_link),
                GridDocument::Assets(_) => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(links, [false, false, true]);
    // The World read itself says the sources disagree.
    let note = grid::sources_disagree(
        &json!({"edges": [{"status": "disputed",
        "endpoints": [{"nodeId": "ent:solsup"}]}]})
        .to_string(),
    )
    .unwrap();
    assert_eq!(note["claims"], 1);
    assert_eq!(note["entity_ids"], json!(["ent:solsup"]));
    assert!(grid::sources_disagree(&json!({"edges": []}).to_string()).is_none());
}

#[tokio::test]
async fn under_an_as_of_date_every_cell_was_known_by_then() {
    let temporary = tempfile::tempdir().unwrap();
    let executor = executor(&temporary, "assistant-as-of");
    let solsup = statements(&executor, "Solsup Foods", "452300000000", "38100000000").await;
    let file = cite(
        &executor,
        "workspace_capture",
        RunResultOrigin::WorkspaceFile,
        json!({"content": "Revenue 452.3"}),
    )
    .await;
    let params = |url: &str| {
        grid(
            rows(2),
            json!([{"id": "revenue", "label": "Revenue", "unit": "amount"}]),
            json!([[{"value": "₩452.3bn", "citation_url": url}], [{"empty": "not_checked"}]]),
        )
    };
    // The filing came after the date: the cell is left out.
    let later = refused(&executor, &as_of("2026-03-03"), params(&solsup)).await;
    assert!(later.contains("as_of_later: cells[0][0]"), "{later}");
    // A local file has no publication date to hold against the cutoff.
    let local = refused(&executor, &as_of("2026-04-01"), params(&file)).await;
    assert!(local.contains("as_of_unsupported: cells[0][0]"), "{local}");
    executor
        .execute(
            &as_of("2026-04-01"),
            ToolMethod::GridPublish,
            params(&solsup),
        )
        .await
        .unwrap();
    let commit = executor.capture.artifacts.lock().await[0].clone();
    let ChatArtifactPayload::Grid { grid, .. } = &commit.revision.payload else {
        panic!("not a grid");
    };
    assert_eq!(grid.as_of(), Some("2026-04-01"));
    super::tests::save_artifact_turn(&executor.state, &commit);

    // Read back under an earlier date it cannot show what was known then.
    let next = AppToolExecutor {
        state: executor.state.clone(),
        capture: Arc::new(ToolCapture::for_chat("assistant-as-of-2".into())),
        loupist_id: "loupist-a".into(),
        capability_ids: BTreeSet::new(),
    };
    let read = |policy: ToolPolicy| {
        let next = &next;
        let id = commit.artifact.id.clone();
        async move {
            next.execute(
                &policy,
                ToolMethod::ArtifactRead,
                json!({"artifact_id": id}),
            )
            .await
        }
    };
    let error = read(as_of("2026-03-31")).await.unwrap_err().to_string();
    assert!(error.contains("as_of_unsupported"), "{error}");
    assert!(read(as_of("2026-04-02")).await.is_ok());
    assert!(read(policy()).await.is_ok());
}
