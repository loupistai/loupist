use super::*;
use crate::knowledge::{principles as storage, Principle, PrincipleInput};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleListRequest {
    pub loupist_id: String,
    pub include_archived: Option<bool>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleCreateRequest {
    pub loupist_id: String,
    pub statement: String,
    pub why: Option<String>,
    pub example: Option<String>,
    pub origin: String,
    pub origin_chat_id: Option<String>,
    pub origin_message_id: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleUpdateRequest {
    pub loupist_id: String,
    pub record_id: String,
    pub expected_revision_id: String,
    pub statement: Option<String>,
    pub why: Option<String>,
    pub example: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrincipleArchiveRequest {
    pub loupist_id: String,
    pub record_id: String,
    pub archived: bool,
}

#[tauri::command]
pub fn principle_list(
    state: State<'_, AppState>,
    request: PrincipleListRequest,
) -> Result<Vec<Principle>, CommandError> {
    list(&state, request)
}
#[tauri::command]
pub fn principle_create(
    state: State<'_, AppState>,
    request: PrincipleCreateRequest,
) -> Result<Principle, CommandError> {
    create(&state, request)
}
#[tauri::command]
pub fn principle_update(
    state: State<'_, AppState>,
    request: PrincipleUpdateRequest,
) -> Result<Principle, CommandError> {
    update(&state, request)
}
#[tauri::command]
pub fn principle_archive(
    state: State<'_, AppState>,
    request: PrincipleArchiveRequest,
) -> Result<Principle, CommandError> {
    archive(&state, request)
}

pub(in crate::commands) fn list(
    state: &AppState,
    request: PrincipleListRequest,
) -> Result<Vec<Principle>, CommandError> {
    admit(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id, |tx, _| {
        storage::principle_list(
            tx,
            &request.loupist_id,
            request.include_archived.unwrap_or(false),
        )
    })
}
pub(super) fn create(
    state: &AppState,
    request: PrincipleCreateRequest,
) -> Result<Principle, CommandError> {
    let _activity = admit_mutation(state, &request.loupist_id)?;
    if !matches!(request.origin.as_str(), "settings" | "feedback") {
        return Err(CommandError::invalid(
            "direct principle creation requires settings or feedback origin",
        ));
    }
    if request.origin == "feedback" {
        let chat = state
            .store
            .get_chat(request.origin_chat_id.as_deref().unwrap_or(""))
            .map_err(super::super::map_store)?
            .ok_or_else(|| CommandError::not_found("origin Chat"))?;
        if chat.loupist_id != request.loupist_id
            || !chat.messages.iter().any(|m| {
                Some(&m.id) == request.origin_message_id.as_ref()
                    && m.role == crate::domain::ChatRole::Assistant
            })
        {
            return Err(CommandError::invalid(
                "feedback must name an answer in this Loupist",
            ));
        }
    }
    with_loupist_knowledge(state, &request.loupist_id, |tx, _| {
        storage::principle_save(
            tx,
            &request.loupist_id,
            None,
            None,
            PrincipleInput {
                statement: request.statement,
                why: request.why,
                example: request.example,
                origin: request.origin,
                origin_chat_id: request.origin_chat_id,
                origin_message_id: request.origin_message_id,
            },
            crate::commands::now_ms(),
        )
    })
}
pub(super) fn update(
    state: &AppState,
    request: PrincipleUpdateRequest,
) -> Result<Principle, CommandError> {
    let _activity = admit_mutation(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id, |tx, _| {
        let old = storage::principle_read(tx, &request.loupist_id, &request.record_id)?;
        storage::principle_save(
            tx,
            &request.loupist_id,
            Some(&request.record_id),
            Some(&request.expected_revision_id),
            PrincipleInput {
                statement: request.statement.unwrap_or(old.statement),
                why: request.why.or(old.why),
                example: request.example.or(old.example),
                origin: "settings".into(),
                origin_chat_id: None,
                origin_message_id: None,
            },
            crate::commands::now_ms(),
        )
    })
}
pub(super) fn archive(
    state: &AppState,
    request: PrincipleArchiveRequest,
) -> Result<Principle, CommandError> {
    let _activity = admit_mutation(state, &request.loupist_id)?;
    with_loupist_knowledge(state, &request.loupist_id, |tx, _| {
        storage::principle_archive(
            tx,
            &request.loupist_id,
            &request.record_id,
            request.archived,
            crate::commands::now_ms(),
        )
    })
}

pub(in crate::commands) fn attach_context(
    envelope: &str,
    block: &str,
    principles: &[Principle],
) -> String {
    if block.is_empty() {
        return envelope.into();
    }
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(envelope) else {
        return envelope.into();
    };
    value["User principles"] = serde_json::json!(block);
    value["user_principle_revisions"] = serde_json::json!(principles
        .iter()
        .map(|p| serde_json::json!({"recordId":p.record_id,"revisionId":p.revision_id}))
        .collect::<Vec<_>>());
    serde_json::to_string(&value).unwrap_or_else(|_| envelope.into())
}

pub(in crate::commands) fn persist_principles(
    tx: &rusqlite::Transaction<'_>,
    loupist: &str,
    staged: &[crate::commands::tool_executor::StagedPrinciple],
    at_ms: i64,
) -> Result<Vec<crate::domain::MemoryUpdateChange>, CommandError> {
    let mut changes = Vec::new();
    for p in staged {
        let saved = storage::principle_save(
            tx,
            loupist,
            Some(&p.record_id),
            p.expected_revision_id.as_deref(),
            p.input.clone(),
            at_ms,
        )
        .map_err(map_knowledge)?;
        changes.push(crate::domain::MemoryUpdateChange {
            outcomes: Vec::new(),
            world_job_id: None,
            world_status: None,
            record_id: saved.record_id.clone(),
            kind: "principle".into(),
            operation: if p.expected_revision_id.is_some() {
                "revise"
            } else {
                "create"
            }
            .into(),
            title: saved.statement.clone(),
            lesson: saved.statement,
            basis: "The user's explicitly stated research rule".into(),
            future_use: "Checked against dated facts in later research".into(),
            knowledge: Some(crate::domain::KnowledgeReference {
                kind: "lens".into(),
                record_id: saved.record_id.clone(),
                revision_id: saved.revision_id,
                object_id: saved.record_id,
                span_id: None,
                receipt_id: None,
            }),
        });
    }
    Ok(changes)
}

pub(in crate::commands) fn persist_citations(
    tx: &rusqlite::Transaction<'_>,
    blobs: &BlobStore,
    loupist: &str,
    turn: &str,
    answer: &str,
    reads: &[crate::commands::tool_executor::StagedKnowledgeRead],
    at_ms: i64,
) -> crate::knowledge::KnowledgeResult<()> {
    use pulldown_cmark::{Event, Parser, Tag};
    let ids = Parser::new(answer)
        .filter_map(|event| match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                dest_url.strip_prefix("lens:").map(str::to_owned)
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut entities = std::collections::BTreeSet::new();
    for read in reads {
        if let crate::commands::tool_executor::AckedPayload::World { view_json, .. } = &read.payload
        {
            let value: serde_json::Value = serde_json::from_str(view_json)?;
            for node in value["nodes"].as_array().into_iter().flatten() {
                if let Some(id) = node["id"].as_str() {
                    entities.insert(id.to_owned());
                }
            }
        }
    }
    let write = crate::knowledge::KnowledgeWrite::bind(tx, blobs);
    for id in ids {
        let Ok(page) = write.read_page(loupist, &id) else {
            continue;
        };
        if page.archived {
            continue;
        }
        // A stale explicitly delivered page cannot count as a current-head citation.
        if reads
            .iter()
            .any(|r| r.scope == "lens" && r.record_id == id && r.revision_id != page.revision_id)
        {
            continue;
        }
        write.record_applied(loupist, turn, &id, &entities, at_ms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::tests::{profile, seed_profile};
    fn state() -> (tempfile::TempDir, AppState) {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::for_test(temp.path().join("app"));
        let workspace = temp.path().join("loupist");
        std::fs::create_dir_all(&workspace).unwrap();
        seed_profile(state.store.as_ref(), &profile("loupist-a", &workspace, 1));
        (temp, state)
    }
    fn request(statement: &str) -> PrincipleCreateRequest {
        PrincipleCreateRequest {
            loupist_id: "loupist-a".into(),
            statement: statement.into(),
            why: None,
            example: None,
            origin: "settings".into(),
            origin_chat_id: None,
            origin_message_id: None,
        }
    }
    #[test]
    fn principle_commands_preserve_revisions_and_reject_forged_feedback() {
        let (_temp, state) = state();
        let p = create(&state, request("Check cash flow")).unwrap();
        let mut bad = request("Check revenue");
        bad.origin = "chat".into();
        assert!(create(&state, bad).is_err());
        let mut bad = request("Check revenue");
        bad.origin = "feedback".into();
        bad.origin_chat_id = Some("foreign".into());
        bad.origin_message_id = Some("foreign".into());
        assert!(create(&state, bad).is_err());
        let updated = update(
            &state,
            PrincipleUpdateRequest {
                loupist_id: p.loupist_id.clone(),
                record_id: p.record_id.clone(),
                expected_revision_id: p.revision_id.clone(),
                statement: Some("Check operating cash flow".into()),
                why: Some("Before trusting profit".into()),
                example: None,
            },
        )
        .unwrap();
        let error = update(
            &state,
            PrincipleUpdateRequest {
                loupist_id: p.loupist_id.clone(),
                record_id: p.record_id.clone(),
                expected_revision_id: p.revision_id,
                statement: Some("Stale rule".into()),
                why: None,
                example: None,
            },
        )
        .unwrap_err();
        assert_eq!(error.code, "conflict");
        assert!(
            archive(
                &state,
                PrincipleArchiveRequest {
                    loupist_id: p.loupist_id.clone(),
                    record_id: p.record_id.clone(),
                    archived: true
                }
            )
            .unwrap()
            .archived
        );
        assert!(list(
            &state,
            PrincipleListRequest {
                loupist_id: p.loupist_id.clone(),
                include_archived: None
            }
        )
        .unwrap()
        .is_empty());
        let all = list(
            &state,
            PrincipleListRequest {
                loupist_id: p.loupist_id,
                include_archived: Some(true),
            },
        )
        .unwrap();
        assert_eq!(all[0].revision_id, updated.revision_id);
    }
    #[test]
    fn principle_context_precedes_perspective_and_citations_need_final_links() {
        let (_temp, state) = state();
        let p = create(&state, request("Check cash flow")).unwrap();
        assert!(
            host_memory_context_for_turn(&state, "loupist-a", "", false, &[], None)
                .unwrap()
                .is_none()
        );
        // "Not this time" in one chat leaves the principle out of that chat only.
        let set_aside = host_memory_context_for_turn(
            &state,
            "loupist-a",
            "",
            true,
            std::slice::from_ref(&p.record_id),
            None,
        )
        .unwrap()
        .unwrap();
        assert!(set_aside.principles.is_empty());
        assert!(!set_aside.principles_block.contains(&p.revision_id));
        let context = host_memory_context_for_turn(&state, "loupist-a", "", true, &[], None)
            .unwrap()
            .unwrap();
        let envelope = attach_context(
            "{\"default_lens\":\"Perspective\"}",
            &context.principles_block,
            &context.principles,
        );
        assert!(envelope.find("User principles").unwrap() < envelope.find("default_lens").unwrap());
        assert!(envelope.contains(&p.revision_id));
        let read = crate::commands::tool_executor::StagedKnowledgeRead {
            read_ref: "world-read".into(),
            scope: "world".into(),
            record_id: "world".into(),
            span_id: None,
            revision_id: "rev-world".into(),
            object_id: "world".into(),
            title: "World".into(),
            payload: crate::commands::tool_executor::AckedPayload::World {
                revision_digest: "digest".into(),
                view_json: serde_json::json!({"nodes":[{"id":"entity-a"},{"id":"entity-b"}]})
                    .to_string(),
                view_digest: "digest".into(),
            },
        };
        with_loupist_knowledge(&state, "loupist-a", |tx, blobs| {
            persist_citations(
                tx,
                blobs,
                "loupist-a",
                "turn-1",
                "Plain answer",
                std::slice::from_ref(&read),
                10,
            )?;
            let n: i64 = tx.query_row(
                "SELECT count(*) FROM knowledge_lens_anchors WHERE role = 'applied'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(n, 0);
            persist_citations(
                tx,
                blobs,
                "loupist-a",
                "turn-2",
                &format!("[Cash flow](lens:{})", p.record_id),
                std::slice::from_ref(&read),
                11,
            )?;
            persist_citations(
                tx,
                blobs,
                "loupist-a",
                "turn-2",
                &format!("[Cash flow](lens:{})", p.record_id),
                std::slice::from_ref(&read),
                11,
            )?;
            let n: i64 = tx.query_row(
                "SELECT count(*) FROM knowledge_lens_anchors WHERE role = 'applied'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(n, 2);
            persist_citations(
                tx,
                blobs,
                "other",
                "turn-3",
                &format!("[Cash flow](lens:{})", p.record_id),
                &[read],
                12,
            )?;
            let n: i64 = tx.query_row(
                "SELECT count(*) FROM knowledge_lens_anchors WHERE role = 'applied'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(n, 2);
            Ok(())
        })
        .unwrap();
    }
    #[tokio::test]
    async fn principle_folder_portability_preserves_identity_origins_and_archive() {
        fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
            std::fs::create_dir(to).unwrap();
            for entry in std::fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let target = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_dir(&entry.path(), &target);
                } else {
                    std::fs::copy(entry.path(), target).unwrap();
                }
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("original");
        std::fs::create_dir(&folder).unwrap();
        let state = AppState::for_folder_test(temp.path().join("app-a"));
        let loupist = crate::commands::loupist::create_folder_loupist(&state, &folder)
            .await
            .unwrap();
        let mut req = request("Check cash flow before profit");
        req.loupist_id = loupist.id.clone();
        let saved = create(&state, req).unwrap();
        archive(
            &state,
            PrincipleArchiveRequest {
                loupist_id: loupist.id.clone(),
                record_id: saved.record_id.clone(),
                archived: true,
            },
        )
        .unwrap();
        let before = list(
            &state,
            PrincipleListRequest {
                loupist_id: loupist.id.clone(),
                include_archived: Some(true),
            },
        )
        .unwrap();
        drop(state);
        let copied = temp.path().join("copied");
        copy_dir(&folder, &copied);
        let reopened = AppState::for_folder_test(temp.path().join("app-b"));
        let opened = crate::commands::loupist::create_folder_loupist(&reopened, &copied)
            .await
            .unwrap();
        assert_eq!(opened.id, loupist.id);
        let after = list(
            &reopened,
            PrincipleListRequest {
                loupist_id: opened.id,
                include_archived: Some(true),
            },
        )
        .unwrap();
        assert_eq!(after, before);
    }
}
