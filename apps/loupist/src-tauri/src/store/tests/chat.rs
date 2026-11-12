use super::support::*;
use super::*;
use crate::domain::ChatMessageStatus;
use crate::test_fixtures::{chat_message, chat_session};

#[test]
fn an_old_failure_kind_reads_as_other_and_one_unreadable_chat_never_hides_the_rest() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let failed = ChatSession {
        messages: vec![ChatMessage {
            status: ChatMessageStatus::Error,
            failure_kind: Some(crate::domain::ChatFailureKind::ProviderError),
            ..chat_message(
                "message-1",
                ChatRole::Assistant,
                "Response could not be completed.",
            )
        }],
        ..chat_session("chat-old", "loupist-1")
    };
    for chat in [
        failed,
        chat_session("chat-kept", "loupist-1"),
        chat_session("chat-broken", "loupist-1"),
    ] {
        store.create_chat(&chat).unwrap();
    }
    {
        let connection = store.lock_loupist("loupist-1").unwrap();
        // A kind an older build stored and this build no longer has.
        connection
            .execute(
                r#"UPDATE chat_sessions SET data_json = replace(data_json, '"provider_error"', '"auth_expired"') WHERE id = 'chat-old'"#,
                [],
            )
            .unwrap();
        connection
            .execute(
                r#"UPDATE chat_sessions SET data_json = '{"unreadable":true}' WHERE id = 'chat-broken'"#,
                [],
            )
            .unwrap();
    }
    let old = store.get_chat("chat-old").unwrap().unwrap();
    assert_eq!(
        old.messages[0].failure_kind,
        Some(crate::domain::ChatFailureKind::Other)
    );
    let listed = store
        .list_chats_for_loupist("loupist-1")
        .unwrap()
        .into_iter()
        .map(|chat| chat.id)
        .collect::<Vec<_>>();
    assert_eq!(listed, ["chat-kept", "chat-old"]);
}

fn stored_chat_json(store: &SqliteStore, id: &str) -> String {
    let connection = store.lock_loupist("loupist-1").unwrap();
    connection
        .query_row(
            "SELECT data_json FROM chat_sessions WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
        .unwrap()
}

fn refusal(reason: crate::domain::ChatToolRefusalReason) -> crate::domain::ChatToolRefusal {
    crate::domain::ChatToolRefusal {
        connection_id: "custom-filings".into(),
        connection_name: "Filings".into(),
        tool_name: "read_filing".into(),
        reason,
    }
}

#[test]
fn a_chat_another_version_wrote_opens_and_this_build_never_overwrites_it() {
    use crate::domain::ChatToolRefusalReason;
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let chat = ChatSession {
        messages: vec![
            chat_message("question", ChatRole::User, "Compare margins"),
            ChatMessage {
                tool_refusals: vec![refusal(ChatToolRefusalReason::NeedsApproval)],
                ..chat_message("answer", ChatRole::Assistant, "I need your OK for Filings.")
            },
        ],
        ..chat_session("chat-newer", "loupist-1")
    };
    store.create_chat(&chat).unwrap();
    {
        // Fields, a refusal reason and a Pi cache shape from a newer build.
        let connection = store.lock_loupist("loupist-1").unwrap();
        connection
            .execute(
                r#"UPDATE chat_sessions SET data_json = replace(json_set(data_json,
                    '$.pinnedNote', 'kept',
                    '$.messages[0].translation', json('{"ko":"마진 비교"}'),
                    '$.piSessionCache', json('{"newerAuthority":true}')),
                    '"needs_approval"', '"needs_new_grant"') WHERE id = 'chat-newer'"#,
                [],
            )
            .unwrap();
    }
    let stored = stored_chat_json(&store, "chat-newer");
    let read = store.get_chat("chat-newer").unwrap().unwrap();
    assert_eq!(read.messages.len(), 2);
    assert_eq!(read.messages[0].content, "Compare margins");
    assert_eq!(
        read.messages[1].tool_refusals,
        [refusal(ChatToolRefusalReason::Other)]
    );
    // A cache it cannot read exactly is dropped; the Chat rebuilds cold.
    assert!(read.pi_session_cache.is_none());
    assert_eq!(
        store
            .list_chats_for_loupist("loupist-1")
            .unwrap()
            .into_iter()
            .map(|chat| chat.id)
            .collect::<Vec<_>>(),
        ["chat-newer"]
    );
    // Chat writes compare the stored text, so a write from this build that
    // could drop what it did not read conflicts instead.
    let renamed = ChatSession {
        title: "Renamed".into(),
        updated_at_ms: read.updated_at_ms + 1,
        ..read.clone()
    };
    assert!(matches!(
        store.replace_chat(&read, &renamed),
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.delete_chat(&read),
        Err(StoreError::Conflict(_))
    ));
    assert_eq!(stored_chat_json(&store, "chat-newer"), stored);
}

#[test]
fn dropping_pi_caches_on_reconnect_or_import_keeps_every_chat_writable() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    for id in ["chat-a", "chat-b"] {
        store.create_chat(&chat_session(id, "loupist-1")).unwrap();
    }
    for scope in [Some("loupist-1"), None] {
        {
            let connection = store.lock_loupist("loupist-1").unwrap();
            crate::store::folder::drop_pi_session_caches(&connection, scope).unwrap();
        }
        for id in ["chat-a", "chat-b"] {
            assert!(stored_chat_json(&store, id).contains(r#""piSessionCache":null"#));
            let read = store.get_chat(id).unwrap().unwrap();
            let renamed = ChatSession {
                title: format!("{} again", read.title),
                updated_at_ms: read.updated_at_ms + 1,
                ..read.clone()
            };
            store.replace_chat(&read, &renamed).unwrap();
        }
    }
}

/// Chat reads in `store.rs` address the stored JSON by serde name
/// (`chats_ending_in_refusals`, `chats_ending_in_answer`, the artifact
/// listing's title) and reconnection clears `$.piSessionCache` in place. A
/// serde rename must fail here rather than silently empty those queries.
#[test]
fn sql_json_paths_name_fields_a_chat_serializes() {
    use crate::domain::{ChatToolRefusalReason, HomeAlert, HomeDelivery, HomeEarnings, HomeWatch};
    let answer = ChatMessage {
        tool_refusals: vec![refusal(ChatToolRefusalReason::NeedsApproval)],
        host_composed: true,
        delivery: Some(HomeDelivery {
            run_id: "run-1".into(),
            task_id: "task-1".into(),
            task_title: "Morning notes".into(),
            task_kind: crate::scheduled_tasks::types::TaskKind::Custom,
            schedule: None,
            run_thread_id: None,
            status: crate::scheduled_tasks::types::RunStatus::Succeeded,
            finished_at_ms: 1,
            learned: None,
        }),
        alert: Some(HomeAlert {
            asset_id: "asset-1".into(),
            move_id: "move-1".into(),
            name: "Example".into(),
            symbol: "EXM".into(),
            market: "US".into(),
            claim: "Margin above 20%".into(),
            value: "27%".into(),
            set_at_ms: 1,
            fact_date: "2026-10-11".into(),
            source_url: None,
            source_title: "Filing".into(),
        }),
        watch: Some(HomeWatch {
            event_ids: vec!["event-1".into()],
            events: Vec::new(),
            more: 0,
        }),
        earnings: Some(HomeEarnings {
            asset_id: "asset-1".into(),
            event_id: "event-1".into(),
            name: "Example".into(),
            symbol: "EXM".into(),
            market: "US".into(),
            period: "Q3".into(),
            outcome: "reported".into(),
            side: None,
            measures: Vec::new(),
        }),
        ..chat_message("answer", ChatRole::Assistant, "Margins rose.")
    };
    let chat = ChatSession {
        messages: vec![chat_message("question", ChatRole::User, "Margins?"), answer],
        ..chat_session("chat-paths", "loupist-1")
    };
    let json = serde_json::to_string(&chat).unwrap();
    let sqlite = rusqlite::Connection::open_in_memory().unwrap();
    let extract = |path: &str| -> (Option<String>, rusqlite::types::Value) {
        sqlite
            .query_row(
                "SELECT json_type(?1, ?2), json_extract(?1, ?2)",
                [json.as_str(), path],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };
    for path in [
        "$.title",
        "$.piSessionCache",
        "$.messages[#-1]",
        "$.messages[#-1].content",
        "$.messages[#-1].toolRefusals",
        "$.messages[#-1].hostComposed",
        "$.messages[#-1].delivery",
        "$.messages[#-1].alert",
        "$.messages[#-1].watch",
        "$.messages[#-1].earnings",
    ] {
        assert!(extract(path).0.is_some(), "{path}");
    }
    for (path, value) in [
        ("$.messages[#-1].role", "assistant"),
        ("$.messages[#-1].status", "complete"),
    ] {
        assert_eq!(
            extract(path).1,
            rusqlite::types::Value::Text(value.into()),
            "{path}"
        );
    }
    // The answer query relies on absent markers for an ordinary answer.
    let plain = serde_json::to_value(chat_message("plain", ChatRole::Assistant, "Done.")).unwrap();
    for key in [
        "toolRefusals",
        "hostComposed",
        "delivery",
        "alert",
        "watch",
        "earnings",
    ] {
        assert!(plain.get(key).is_none(), "{key}");
    }
}

#[test]
fn chat_messages_round_trip_memory_provenance_status_and_progress_without_loss() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let update = MemoryUpdateResult {
        status: MemoryUpdateStatus::Applied,
        commit_id: Some("commit-1".into()),
        changes: vec![MemoryUpdateChange {
            outcomes: Vec::new(),
            world_job_id: None,
            world_status: None,
            knowledge: None,
            record_id: "lens:quality".into(),
            kind: "Lens".into(),
            operation: "create".into(),
            title: "Quality".into(),
            lesson: "Prefer durable earnings quality.".into(),
            basis: "Exact filing".into(),
            future_use: "Changes later earnings checks.".into(),
        }],
        skipped_kinds: Vec::new(),
        setup: Vec::new(),
    };
    let memory_ref = MemoryRefSnapshot {
        read_ref: "lens:source".into(),
        kind: "lens".into(),
        title: "Source".into(),
        excerpt: "Exact section".into(),
        as_of: Some("2026-08-01T00:00:00Z".into()),
        section: Some("Boundary".into()),
        access: MemoryAccess::ExactRead,
        full_record_digest: None,
        record_id: "rec_00000000000000000000000000000001".into(),
        revision_id: "rev_00000000000000000000000000000001".into(),
        object_id: "object".into(),
        span_id: None,
        receipt_id: Some("rdg_test".into()),
    };
    let progress = crate::chat::progress::ChatProgress {
        started_at_ms: 10,
        finished_at_ms: Some(20),
        items: vec![crate::chat::progress::ChatProgressItem::Commentary {
            id: "commentary-1".into(),
            text: "Reading the filing.".into(),
        }],
        plan: None,
        approvals: Vec::new(),
        questions: Vec::new(),
    };
    let chat = ChatSession {
        // An untitled chat keeps its empty title.
        title: String::new(),
        set_aside_memory: Vec::new(),
        versions: Vec::new(),
        messages: vec![
            ChatMessage {
                tool_refusals: vec![crate::domain::ChatToolRefusal {
                    connection_id: "custom-test".into(),
                    connection_name: "Company research".into(),
                    tool_name: "read_filings".into(),
                    reason: crate::domain::ChatToolRefusalReason::NeedsApproval,
                }],
                observed_exact_count: 1,
                refs_digest: memory_refs_digest(std::slice::from_ref(&memory_ref)).unwrap(),
                lens_receipts: Vec::new(),
                memory_refs: vec![memory_ref],
                memory_update: Some(update),
                memory_revision: Some("tree-revision".into()),
                ..chat_message("message-1", ChatRole::Assistant, "Answer")
            },
            ChatMessage {
                status: ChatMessageStatus::Aborted,
                ..chat_message("message-2", ChatRole::Assistant, "Response stopped.")
            },
            ChatMessage {
                progress: Some(progress),
                ..chat_message("message-3", ChatRole::Assistant, "Done")
            },
        ],
        ..chat_session("chat-1", "loupist-1")
    };
    store.create_chat(&chat).unwrap();
    assert_eq!(store.get_chat(&chat.id).unwrap(), Some(chat.clone()));
    assert_eq!(
        store.list_chats_for_loupist(&chat.loupist_id).unwrap(),
        vec![chat]
    );
}

fn markdown_revision(
    id: &str,
    revision: u32,
    markdown: &str,
    message: &str,
) -> ChatArtifactRevision {
    ChatArtifactRevision::new(
        id.into(),
        revision,
        ChatArtifactPayload::Markdown {
            schema: "loupist-markdown/1".into(),
            markdown: markdown.into(),
        },
        message.into(),
        i64::from(revision) + 1,
    )
    .unwrap()
}

fn artifact(id: &str, chat: &ChatSession, revision: u32) -> ChatArtifact {
    ChatArtifact {
        id: id.into(),
        chat_session_id: chat.id.clone(),
        kind: ChatArtifactKind::Markdown,
        title: format!("{id} title"),
        current_revision: revision,
        created_at_ms: 2,
        updated_at_ms: i64::from(revision) + 1,
    }
}

/// Appends one assistant message that publishes `commits`, through the
/// transaction the chat runtime uses.
fn publish(
    store: &SqliteStore,
    chat: &mut ChatSession,
    message: &str,
    commits: Vec<ArtifactCommit>,
) {
    let expected = chat.clone();
    chat.messages.push(ChatMessage {
        artifact_refs: commits
            .iter()
            .map(|commit| commit.revision.artifact_ref(commit.artifact.title.clone()))
            .collect(),
        ..chat_message(message, ChatRole::Assistant, "Published.")
    });
    store
        .save_chat_with_artifacts_and(&expected, chat, &commits, |_| Ok(()))
        .unwrap();
}

#[test]
fn chat_artifact_replaces_current_content_and_deletes_with_its_session() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let mut chat = chat_session("chat-artifacts", "loupist-1");
    store.create_chat(&chat).unwrap();
    for revision in [1, 2] {
        let message = format!("assistant-{revision}");
        let commit = ArtifactCommit {
            artifact: artifact("artifact-1", &chat, revision),
            revision: markdown_revision(
                "artifact-1",
                revision,
                &format!("# Rev {revision}"),
                &message,
            ),
            datasets: vec![],
        };
        publish(&store, &mut chat, &message, vec![commit]);
    }

    let loaded = store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(loaded.messages[0].artifact_refs[0].revision, 1);
    assert_eq!(loaded.messages[1].artifact_refs[0].revision, 2);
    let current = store
        .get_chat_artifact_current("artifact-1")
        .unwrap()
        .unwrap();
    assert_eq!(current.revision, 2);
    assert!(matches!(
        current.payload,
        ChatArtifactPayload::Markdown { ref markdown, .. } if markdown == "# Rev 2"
    ));

    let mut wrong_loupist = chat.clone();
    wrong_loupist.loupist_id = "another-loupist".into();
    assert!(store.delete_chat(&wrong_loupist).is_err());
    assert!(store.get_chat(&chat.id).unwrap().is_some());
    store.delete_chat(&chat).unwrap();
    assert_eq!(store.get_chat(&chat.id).unwrap(), None);
    assert_eq!(store.get_chat_artifact("artifact-1").unwrap(), None);
    assert_eq!(store.get_chat_artifact_current("artifact-1").unwrap(), None);
}

#[test]
fn one_assistant_message_can_persist_distinct_artifact_commits() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let mut chat = chat_session("chat-multi-artifacts", "loupist-1");
    store.create_chat(&chat).unwrap();
    let commits = ["artifact-note", "artifact-chart-note"]
        .map(|id| ArtifactCommit {
            artifact: artifact(id, &chat, 1),
            revision: markdown_revision(id, 1, "# Note", "assistant-1"),
            datasets: vec![],
        })
        .to_vec();
    publish(&store, &mut chat, "assistant-1", commits);

    let loaded = store.get_chat(&chat.id).unwrap().unwrap();
    assert_eq!(loaded.messages[0].artifact_refs.len(), 2);
    for id in ["artifact-note", "artifact-chart-note"] {
        assert_eq!(
            store
                .get_chat_artifact_current(id)
                .unwrap()
                .unwrap()
                .revision,
            1
        );
    }
}

#[test]
fn artifact_history_lists_and_prunes_oldest_revisions_without_reusing_ids() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let mut chat = chat_session("chat-history", "loupist-1");
    store.create_chat(&chat).unwrap();
    for revision in 1..=25 {
        let message = format!("assistant-{revision}");
        let commit = ArtifactCommit {
            artifact: artifact("history", &chat, revision),
            revision: markdown_revision(
                "history",
                revision,
                &format!("Version {revision}"),
                &message,
            ),
            datasets: vec![],
        };
        publish(&store, &mut chat, &message, vec![commit]);
    }
    let versions = store.list_chat_artifact_versions("history").unwrap();
    assert_eq!(
        versions.iter().map(|v| v.revision_id).collect::<Vec<_>>(),
        (6..=25).rev().collect::<Vec<_>>()
    );
    assert!(store
        .read_chat_artifact("history", Some(5))
        .unwrap()
        .is_none());
    let retained = store
        .read_chat_artifact("history", Some(6))
        .unwrap()
        .unwrap();
    assert_eq!(retained.revision.source_message_id, "assistant-6");
    assert_eq!(retained.revision.created_at_ms, 7);
    assert_eq!(retained.artifact.current_revision, 25);
    assert!(
        matches!(retained.revision.payload, ChatArtifactPayload::Markdown { markdown, .. } if markdown == "Version 6")
    );
    store.delete_chat(&chat).unwrap();
    let connection = store.connection.lock().unwrap();
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM chat_artifact_revisions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
}

fn chart_commit(chat: &ChatSession, revision: u32, text_bytes: usize) -> ArtifactCommit {
    use crate::chat::chart::{
        ChartColumn, ChartColumnKind, ChartDataset, ChartDatasetLineage, ChartDocument,
    };
    let dataset = ChartDataset::new(
        "chart-dataset".into(),
        vec![
            ChartColumn {
                id: "x".into(),
                label: "Name".into(),
                kind: ChartColumnKind::String,
            },
            ChartColumn {
                id: "y".into(),
                label: "Value".into(),
                kind: ChartColumnKind::Number,
            },
        ],
        (0..text_bytes.max(1).div_ceil(8192))
            .map(|_| {
                vec![
                    serde_json::json!("a".repeat(text_bytes.min(8192))),
                    serde_json::json!(revision),
                ]
            })
            .collect(),
        ChartDatasetLineage::AgentAuthored {
            upstream_receipts: vec![],
        },
    )
    .unwrap();
    let chart: ChartDocument = serde_json::from_value(serde_json::json!({
        "dataset_id": dataset.id, "dataset_digest": dataset.digest,
        "view": { "kind": "bar", "x": "x", "y": ["y"], "semantic_types": {} },
        "studies": [], "drawings": []
    }))
    .unwrap();
    let mut artifact = artifact("chart-history", chat, revision);
    artifact.kind = ChatArtifactKind::Chart;
    artifact.title = format!("Chart {revision}");
    ArtifactCommit {
        artifact,
        revision: ChatArtifactRevision::new(
            "chart-history".into(),
            revision,
            ChatArtifactPayload::Chart {
                schema: "loupist-chart/1".into(),
                chart: Box::new(chart),
            },
            format!("answer-{revision}"),
            i64::from(revision) + 1,
        )
        .unwrap(),
        datasets: vec![dataset],
    }
}

#[test]
fn older_answer_reads_its_own_chart_data_and_restore_appends_a_new_current_revision() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let mut chat = chat_session("chat-chart-history", "loupist-1");
    store.create_chat(&chat).unwrap();
    for revision in [1, 2] {
        let commit = chart_commit(&chat, revision, 1);
        publish(
            &store,
            &mut chat,
            &format!("answer-{revision}"),
            vec![commit],
        );
    }
    let original_ref = &chat.messages[0].artifact_refs[0];
    let original = store
        .read_chat_artifact(&original_ref.artifact_id, Some(original_ref.revision))
        .unwrap()
        .unwrap();
    assert_eq!(original.artifact.title, "Chart 1");
    assert_eq!(original.revision.digest, original_ref.digest);
    assert_eq!(
        original.chart_dataset.as_ref().unwrap().rows[0][1],
        serde_json::json!(1)
    );
    assert_eq!(original.revision.source_message_id, chat.messages[0].id);
    let restored = store
        .restore_chat_artifact(&chat, "chart-history", 1, 2, 10)
        .unwrap();
    assert_eq!(restored.revision.revision, 3);
    assert_eq!(restored.revision.created_at_ms, 10);
    assert_eq!(restored.revision.source_message_id, "answer-1");
    assert_eq!(restored.chart_dataset, original.chart_dataset);
    assert_eq!(restored.restored_from_revision_id, Some(1));
    assert_eq!(
        store
            .get_chat_artifact_current("chart-history")
            .unwrap()
            .unwrap(),
        restored.revision
    );
    assert_eq!(
        store.get_chart_dataset("chart-dataset").unwrap(),
        original.chart_dataset
    );
    assert_eq!(
        store
            .read_chat_artifact("chart-history", Some(2))
            .unwrap()
            .unwrap()
            .chart_dataset
            .unwrap()
            .rows[0][1],
        serde_json::json!(2)
    );
    assert_eq!(
        store
            .list_chat_artifact_versions("chart-history")
            .unwrap()
            .iter()
            .map(|v| v.revision_id)
            .collect::<Vec<_>>(),
        vec![3, 2, 1]
    );
    assert!(store
        .restore_chat_artifact(&chat, "chart-history", 1, 2, 11)
        .is_err());
    let other = chat_session("other-chat", "loupist-1");
    store.create_chat(&other).unwrap();
    assert!(store
        .restore_chat_artifact(&other, "chart-history", 1, 3, 11)
        .is_err());
    assert_eq!(store.get_chat(&chat.id).unwrap().unwrap(), chat);
}

#[test]
fn artifact_history_byte_cap_prunes_snapshots_before_the_count_limit() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let mut chat = chat_session("chat-byte-cap", "loupist-1");
    store.create_chat(&chat).unwrap();
    for revision in 1..=5 {
        let commit = chart_commit(&chat, revision, 7 * 1024 * 1024);
        publish(
            &store,
            &mut chat,
            &format!("answer-{revision}"),
            vec![commit],
        );
    }
    let versions = store.list_chat_artifact_versions("chart-history").unwrap();
    assert_eq!(
        versions.iter().map(|v| v.revision_id).collect::<Vec<_>>(),
        vec![5, 4, 3, 2]
    );
    assert!(store
        .read_chat_artifact("chart-history", Some(1))
        .unwrap()
        .is_none());
    let bytes: i64 = store
        .connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT SUM(byte_size) FROM chat_artifact_revisions",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(bytes <= crate::chat::artifacts::MAX_ARTIFACT_HISTORY_BYTES as i64);
}

#[test]
fn artifact_history_seeds_existing_current_content_and_rolls_back_with_the_answer() {
    let store = SqliteStore::open_in_memory().unwrap();
    seed_loupist(&store, &loupist());
    let mut chat = chat_session("chat-existing-artifact", "loupist-1");
    store.create_chat(&chat).unwrap();
    let commit = ArtifactCommit {
        artifact: artifact("existing", &chat, 1),
        revision: markdown_revision("existing", 1, "Before history", "answer-1"),
        datasets: vec![],
    };
    publish(&store, &mut chat, "answer-1", vec![commit]);
    // This is the on-disk shape of an artifact published before history was enabled.
    store
        .connection
        .lock()
        .unwrap()
        .execute_batch("DROP TABLE chat_artifact_revisions")
        .unwrap();
    let revision = markdown_revision("existing", 2, "Replacement", "answer-2");
    let next = ArtifactCommit {
        artifact: artifact("existing", &chat, 2),
        revision,
        datasets: vec![],
    };
    let mut updated = chat.clone();
    updated.messages.push(ChatMessage {
        artifact_refs: vec![next.revision.artifact_ref(next.artifact.title.clone())],
        ..chat_message("answer-2", ChatRole::Assistant, "Published.")
    });
    assert!(store
        .save_chat_with_artifacts_and(&chat, &updated, std::slice::from_ref(&next), |_| {
            Err(StoreError::Conflict("simulated turn failure"))
        })
        .is_err());
    assert_eq!(
        store
            .get_chat_artifact_current("existing")
            .unwrap()
            .unwrap()
            .revision,
        1
    );
    publish(&store, &mut chat, "answer-2", vec![next]);
    assert_eq!(
        store.list_chat_artifact_versions("existing").unwrap().len(),
        2
    );
    let original = store
        .read_chat_artifact("existing", Some(1))
        .unwrap()
        .unwrap();
    assert!(
        matches!(original.revision.payload, ChatArtifactPayload::Markdown { markdown, .. } if markdown == "Before history")
    );
}

#[test]
fn review_storage_unavailable_bound_folder_does_not_break_other_records() {
    let root = tempfile::tempdir().unwrap();
    let store = SqliteStore::open_catalog(root.path().join("catalog.sqlite3")).unwrap();
    for id in ["first", "second"] {
        let workspace = root.path().join(id);
        std::fs::create_dir(&workspace).unwrap();
        let mut profile = crate::test_fixtures::loupist_profile(id);
        profile.workspace_root = workspace.to_str().unwrap().into();
        profile.root_filesystem_identity =
            crate::fs::loupist_root::BoundLoupistRoot::open_unbound(workspace)
                .unwrap()
                .identity();
        store.create_loupist(&profile).unwrap();
        drop(store.lock_loupist(id).unwrap());
    }
    // Choose the first bound entry to break, rather than depending on a
    // randomized HashMap order. No binding is added or removed afterwards.
    let order: Vec<_> = store.folders.lock().unwrap().keys().cloned().collect();
    let (unavailable_id, owner) = (&order[0], &order[1]);
    let mut chat = chat_session("reachable-chat", owner);
    store.create_chat(&chat).unwrap();
    let commit = chart_commit(&chat, 1, 1);
    let artifact = commit.artifact.clone();
    let revision = commit.revision.clone();
    let dataset = commit.datasets[0].clone();
    publish(&store, &mut chat, "answer-1", vec![commit]);

    // A second SQLite handle holds an exclusive lock on the unrelated folder.
    // A zero busy timeout makes its bound handle fail deterministically, with
    // no process, sleep or timing-dependent failure injection.
    store
        .lock_loupist(unavailable_id)
        .unwrap()
        .busy_timeout(std::time::Duration::ZERO)
        .unwrap();
    let unavailable = rusqlite::Connection::open(
        store
            .loupist_data_dir(unavailable_id)
            .unwrap()
            .join("loupist.sqlite3"),
    )
    .unwrap();
    unavailable.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let error = store
        .lock_loupist(unavailable_id)
        .unwrap()
        .query_row("SELECT count(*) FROM chat_sessions", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_err();
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    assert_eq!(
        store.folders.lock().unwrap().keys().next(),
        Some(unavailable_id)
    );

    assert_eq!(store.get_chat(&chat.id).unwrap(), Some(chat.clone()));
    assert_eq!(
        store.get_chat_artifact(&artifact.id).unwrap(),
        Some(artifact.clone())
    );
    assert_eq!(
        store.list_chat_artifacts(&chat.id).unwrap().as_slice(),
        std::slice::from_ref(&artifact)
    );
    assert_eq!(
        store.get_chat_artifact_current(&artifact.id).unwrap(),
        Some(revision.clone())
    );
    assert_eq!(
        store.get_chart_dataset(&dataset.id).unwrap(),
        Some(dataset.clone())
    );
    let snapshot = store
        .read_chat_artifact(&artifact.id, Some(1))
        .unwrap()
        .unwrap();
    assert_eq!(snapshot.revision, revision);
    assert_eq!(snapshot.chart_dataset, Some(dataset));
    assert_eq!(
        store
            .list_chat_artifact_versions(&artifact.id)
            .unwrap()
            .len(),
        1
    );
    unavailable.execute_batch("ROLLBACK").unwrap();
}
