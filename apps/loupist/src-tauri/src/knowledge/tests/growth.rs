use std::collections::BTreeSet;

use super::*;

fn query(loupist_id: &str) -> GrowthEventsQuery {
    GrowthEventsQuery {
        loupist_id: loupist_id.into(),
        since_ms: None,
        until_ms: None,
        limit: Some(5000),
        cursor: None,
    }
}

fn events_since(store: &KnowledgeStore, since_ms: Option<i64>) -> Vec<GrowthEvent> {
    store
        .growth_events(&GrowthEventsQuery {
            since_ms,
            ..query("loupist-a")
        })
        .unwrap()
        .events
}

fn world_request(store: &KnowledgeStore) -> SealWorld {
    let record = create(store, "loupist-a", RecordKind::World, "World");
    SealWorld {
        loupist_id: "loupist-a".into(),
        record_id: record.record_id,
        expected_revision_id: None,
        at_ms: 2,
        nodes: vec![
            node("a", "First", "entity", BTreeMap::new()),
            node("b", "Second", "entity", BTreeMap::new()),
        ],
        claims: vec![
            claim("one", "Original claim", "a", "b"),
            claim("two", "Other claim", "a", "b"),
        ],
        annotations: vec![],
        dependencies: vec![],
    }
}

#[test]
fn knowledge_growth_world_diff_commit_rollback_and_identical_content() {
    let (_directory, store) = open_store();
    let mut request = world_request(&store);
    let sealed = store.seal_world(request.clone()).unwrap();
    let initial = events_since(&store, None);
    let count = |kind: &str| initial.iter().filter(|event| event.kind == kind).count();
    assert_eq!(
        (initial.len(), count("entity_added"), count("claim_added")),
        (4, 2, 2)
    );
    request.expected_revision_id = Some(sealed.revision_id);
    request.at_ms = 3;
    let same = store.seal_world(request.clone()).unwrap();
    assert_eq!(events_since(&store, None), initial);

    request.expected_revision_id = Some(same.revision_id.clone());
    request.at_ms = 4;
    request.claims[0].status = ClaimStatus::Disputed;
    request.claims[1].status = ClaimStatus::Withdrawn;
    request
        .nodes
        .push(node("c", "Third", "entity", BTreeMap::new()));
    request.claims.push(claim("three", "New claim", "b", "c"));
    let rollback = store.transact(|write| -> KnowledgeResult<()> {
        write.seal_world(request.clone())?;
        Err(KnowledgeError::Invalid("rollback test"))
    });
    assert!(rollback.is_err());
    assert_eq!(events_since(&store, None), initial);
    let head = store
        .read(|write| write.owned_record("loupist-a", &request.record_id))
        .unwrap();
    assert_eq!(head.revision_id, Some(same.revision_id));

    let revised = store.seal_world(request.clone()).unwrap();
    let changed: BTreeSet<_> = events_since(&store, Some(4))
        .into_iter()
        .map(|event| (event.kind, event.subject_id))
        .collect();
    let expected = [
        ("entity_added", "c"),
        ("claim_added", "three"),
        ("claim_disputed", "one"),
        ("claim_withdrawn", "two"),
    ];
    assert_eq!(
        changed,
        expected
            .into_iter()
            .map(|(kind, subject)| (kind.to_string(), Some(subject.to_string())))
            .collect()
    );
    request.expected_revision_id = Some(revised.revision_id);
    request.at_ms = 5;
    request.claims.retain(|claim| claim.id != "three");
    store.seal_world(request).unwrap();
    let removed = events_since(&store, Some(5));
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].kind, "claim_withdrawn");
}

#[test]
fn knowledge_growth_lens_changes_keep_origin_and_skip_noops() {
    let (_directory, store) = open_store();
    let record = create(&store, "loupist-a", RecordKind::Lens, "Method");
    let mut request = ReviseLens {
        loupist_id: "loupist-a".into(),
        record_id: record.record_id.clone(),
        expected_revision_id: None,
        at_ms: 2,
        document: lens_document("First body"),
        dependencies: vec![],
        change: empty_change("Created"),
    };
    request.document.applicability = "First use\nSecond line".into();
    let created = store
        .transact(|write| {
            write.revise_lens_with_origin(
                request.clone(),
                "loupist",
                Some("chat-a"),
                Some("message-a"),
            )
        })
        .unwrap();
    request.expected_revision_id = Some(created.revision_id);
    request.at_ms = 3;
    request.document.applicability = "New use".into();
    let revised = store.revise_lens(request.clone()).unwrap();
    request.expected_revision_id = Some(revised.revision_id);
    request.at_ms = 4;
    store.revise_lens(request).unwrap();
    store
        .transact(|write| {
            let id = &record.record_id;
            write.rename_lens("loupist-a", id, "Renamed", "Clearer", 5, "user", None, None)?;
            write.rename_lens("loupist-a", id, "Renamed", "Same", 6, "user", None, None)?;
            write.set_lens_archived("loupist-a", id, true, "Retire", 7, "user")?;
            write.set_lens_archived("loupist-a", id, true, "Same state", 8, "user")?;
            write.set_lens_archived("loupist-a", id, false, "Restore", 9, "user")
        })
        .unwrap();
    let events = events_since(&store, None);
    let kinds: Vec<_> = events.iter().map(|event| event.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "lens_created",
            "lens_revised",
            "lens_renamed",
            "lens_archived",
            "lens_restored"
        ]
    );
    assert_eq!(events[0].actor, "loupist");
    assert_eq!(events[0].origin_chat_id.as_deref(), Some("chat-a"));
    assert_eq!(events[0].origin_message_id.as_deref(), Some("message-a"));
    assert_eq!(events[1].after_summary.as_deref(), Some("New use"));
    let rollback = store.transact(|write| -> KnowledgeResult<()> {
        let id = &record.record_id;
        write.rename_lens(
            "loupist-a",
            id,
            "Rolled back",
            "Undo",
            10,
            "user",
            None,
            None,
        )?;
        Err(KnowledgeError::Invalid("rollback test"))
    });
    assert!(rollback.is_err());
    assert_eq!(events_since(&store, None), events);
}

#[test]
fn knowledge_growth_source_retention_deduplication_and_rollback() {
    let (_directory, store) = open_store();
    let mut request = source_request("loupist-a", b"Retained source");
    request.origin_chat_id = Some("chat-a".into());
    let rollback = store.transact(|write| -> KnowledgeResult<()> {
        write.retain_new_source_with_origin(request.clone(), "message-a")?;
        assert_eq!(write.growth_events(&query("loupist-a"))?.events.len(), 1);
        Err(KnowledgeError::Invalid("rollback test"))
    });
    assert!(rollback.is_err());
    assert!(events_since(&store, None).is_empty());
    let saved = store
        .transact(|write| write.retain_new_source_with_origin(request.clone(), "message-a"))
        .unwrap();
    store.retain_new_source(request).unwrap();
    let events = events_since(&store, None);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, "source_retained");
    assert_eq!(
        events[0].record_id.as_deref(),
        Some(saved.record_id.as_str())
    );
    assert_eq!(events[0].origin_message_id.as_deref(), Some("message-a"));
}

#[test]
fn knowledge_growth_pagination_is_deterministic_and_scoped() {
    let (_directory, store) = open_store();
    store.seal_world(world_request(&store)).unwrap();
    let all = events_since(&store, None);
    let request = GrowthEventsQuery {
        limit: Some(1),
        since_ms: Some(2),
        until_ms: Some(2),
        ..query("loupist-a")
    };
    let mut page_request = request.clone();
    let mut collected = Vec::new();
    loop {
        let page = store.growth_events(&page_request).unwrap();
        collected.extend(page.events);
        if page.next_cursor.is_none() {
            break;
        }
        page_request.cursor = page.next_cursor;
    }
    assert_eq!(collected, all);
    let cursor = store.growth_events(&request).unwrap().next_cursor;
    for invalid in [
        GrowthEventsQuery {
            loupist_id: "loupist-b".into(),
            cursor: cursor.clone(),
            ..request.clone()
        },
        GrowthEventsQuery {
            since_ms: None,
            cursor,
            ..request.clone()
        },
        GrowthEventsQuery {
            limit: Some(5001),
            ..query("loupist-a")
        },
        GrowthEventsQuery {
            limit: Some(0),
            ..query("loupist-a")
        },
    ] {
        assert!(store.growth_events(&invalid).is_err());
    }
    assert!(store
        .growth_events(&query("loupist-b"))
        .unwrap()
        .events
        .is_empty());
}

#[test]
fn knowledge_growth_caps_publications_and_keeps_events_immutable() {
    let (_directory, store) = open_store();
    let mut request = world_request(&store);
    request.claims.clear();
    request.nodes = (0..2005)
        .map(|index| {
            node(
                &format!("node-{index:04}"),
                "Entity",
                "entity",
                BTreeMap::new(),
            )
        })
        .collect();
    store.seal_world(request).unwrap();
    let events = events_since(&store, None);
    assert_eq!(events.len(), 2001);
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind == "growth_truncated")
            .count(),
        1
    );
    store
        .transact(|write| {
            let update = "UPDATE knowledge_growth_events SET summary = 'changed'";
            assert!(write.tx.execute(update, []).is_err());
            assert!(write
                .tx
                .execute("DELETE FROM knowledge_growth_events", [])
                .is_err());
            erase_loupist_private(write.tx, "loupist-a")?;
            assert!(write.growth_events(&query("loupist-a"))?.events.is_empty());
            Ok(())
        })
        .unwrap();
}
